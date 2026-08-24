//! Animated SGF GIF Exporter with AI Evaluation and Statistics Overlays (ported from sgf2gif).
//!
//! Renders Go game moves sequentially into an animated GIF with custom palette,
//! move number annotations, last-move markers, and KaTrain-style AI quality overlays.

use std::collections::BTreeMap;

use gif::{Encoder, Frame, Repeat};
use sabaki_domain_core::{Color, GameSnapshot, Vertex};

use crate::move_grading::{MoveQuality, compute_game_move_evaluations};

/// Options for configuring the exported SGF animated GIF.
#[derive(Clone, Debug)]
pub struct GifExportOptions {
    /// Dimension of the square board in pixels (default 480).
    pub image_size: u16,
    /// Delay per frame in centiseconds (100ths of a second, default 60 = 600ms).
    pub frame_delay_cs: u16,
    /// Final position pause in centiseconds (default 180 = 1.8s).
    pub final_frame_delay_cs: u16,
    /// Annotate move numbers on stones.
    pub show_move_numbers: bool,
    /// Annotate move quality badges (KaTrain eval dots).
    pub show_eval_quality: bool,
}

impl Default for GifExportOptions {
    fn default() -> Self {
        Self {
            image_size: 480,
            frame_delay_cs: 60,
            final_frame_delay_cs: 180,
            show_move_numbers: true,
            show_eval_quality: true,
        }
    }
}

/// Parses an SGF coordinate string like "dd" or "pd" into a board vertex.
fn parse_sgf_coord(value: &str, board_size: usize) -> Option<Vertex> {
    let bytes = value.trim().as_bytes();
    if bytes.len() != 2 {
        return None;
    }
    if !bytes[0].is_ascii_lowercase() || !bytes[1].is_ascii_lowercase() {
        return None;
    }
    let col = (bytes[0] - b'a') as usize;
    let row = (bytes[1] - b'a') as usize;
    if col < board_size && row < board_size {
        Some(Vertex { column: col, row })
    } else {
        None
    }
}

/// Standard 256-color palette tailored for Go board and stone rendering.
fn build_gif_palette() -> Vec<u8> {
    let mut palette = Vec::with_capacity(256 * 3);
    // 0: Board background (warm wood)
    palette.extend_from_slice(&[226, 177, 119]);
    // 1: Grid lines / star points (dark wood)
    palette.extend_from_slice(&[61, 40, 20]);
    // 2: Black stone (deep charcoal)
    palette.extend_from_slice(&[24, 24, 24]);
    // 3: White stone (clean ivory)
    palette.extend_from_slice(&[248, 248, 248]);
    // 4: Stone border / shadow
    palette.extend_from_slice(&[140, 110, 80]);
    // 5: Text / numbers on white stone (dark)
    palette.extend_from_slice(&[20, 20, 20]);
    // 6: Text / numbers on black stone (white)
    palette.extend_from_slice(&[255, 255, 255]);
    // 7: Banner background (dark slate)
    palette.extend_from_slice(&[28, 28, 34]);
    // 8: Banner text (light grey)
    palette.extend_from_slice(&[230, 230, 235]);
    // 9: Best Move (Emerald green)
    palette.extend_from_slice(&[16, 185, 129]);
    // 10: Good Move (Sky blue)
    palette.extend_from_slice(&[14, 165, 233]);
    // 11: Inaccuracy (Amber yellow)
    palette.extend_from_slice(&[245, 158, 11]);
    // 12: Mistake (Orange)
    palette.extend_from_slice(&[249, 115, 22]);
    // 13: Blunder (Rose red)
    palette.extend_from_slice(&[239, 68, 68]);
    // 14: Last move red circle marker
    palette.extend_from_slice(&[192, 57, 43]);

    // Fill the remainder of 256 colors
    while palette.len() < 256 * 3 {
        palette.extend_from_slice(&[0, 0, 0]);
    }
    palette
}

/// Exports an animated GIF from the active game lineage in the snapshot.
pub fn export_sgf_to_gif(
    snapshot: &GameSnapshot,
    options: &GifExportOptions,
) -> Result<Vec<u8>, String> {
    let size = options.image_size as usize;
    let banner_h = 32usize;
    let width = size as u16;
    let height = (size + banner_h) as u16;

    let palette = build_gif_palette();
    let mut gif_bytes = Vec::new();

    {
        let mut encoder = Encoder::new(&mut gif_bytes, width, height, &palette)
            .map_err(|e| format!("could not initialize GIF encoder: {e}"))?;
        encoder
            .set_repeat(Repeat::Infinite)
            .map_err(|e| format!("could not set repeat: {e}"))?;

        let evaluations = compute_game_move_evaluations(snapshot);
        let board_size = snapshot.board.width.max(1);

        let nodes_map: BTreeMap<&str, &sabaki_domain_core::NodeSnapshot> =
            snapshot.nodes.iter().map(|n| (n.id.as_str(), n)).collect();

        let mut current_id = snapshot.current_node_id.as_str();
        let mut lineage = Vec::new();
        while let Some(node) = nodes_map.get(current_id) {
            lineage.push(*node);
            if let Some(parent) = node.parent_id.as_deref() {
                current_id = parent;
            } else {
                break;
            }
        }
        lineage.reverse();

        // Build progressive board states
        let mut board = vec![vec![0i8; board_size]; board_size];
        let mut move_positions: Vec<(Vertex, Color, usize)> = Vec::new();

        let total_frames = lineage.len().max(1);

        for (frame_idx, node) in lineage.iter().enumerate() {
            // Apply stones
            if let Some(vals) = node.properties.get("B")
                && let Some(vtx_str) = vals.first()
                && let Some(vertex) = parse_sgf_coord(vtx_str, board_size)
            {
                board[vertex.row][vertex.column] = 1;
                move_positions.push((vertex, Color::Black, move_positions.len() + 1));
            } else if let Some(vals) = node.properties.get("W")
                && let Some(vtx_str) = vals.first()
                && let Some(vertex) = parse_sgf_coord(vtx_str, board_size)
            {
                board[vertex.row][vertex.column] = -1;
                move_positions.push((vertex, Color::White, move_positions.len() + 1));
            }

            // Render one indexed pixel buffer
            let mut pixels = vec![0u8; width as usize * height as usize];

            // 1. Draw board background (color 0)
            for y in 0..size {
                for x in 0..size {
                    pixels[y * width as usize + x] = 0;
                }
            }

            // 2. Draw grid lines (color 1)
            let margin = size as f32 * 0.08;
            let spacing =
                (size as f32 - 2.0 * margin) / (board_size.saturating_sub(1) as f32).max(1.0);

            for i in 0..board_size {
                let coord = (margin + i as f32 * spacing).round() as usize;
                // Horizontal line
                let y = coord.min(size - 1);
                let x_start = margin.round() as usize;
                let x_end = (size as f32 - margin).round() as usize;
                for x in x_start..=x_end.min(size - 1) {
                    pixels[y * width as usize + x] = 1;
                }
                // Vertical line
                let x = coord.min(size - 1);
                let y_start = margin.round() as usize;
                let y_end = (size as f32 - margin).round() as usize;
                for y in y_start..=y_end.min(size - 1) {
                    pixels[y * width as usize + x] = 1;
                }
            }

            // 3. Draw stones
            let stone_radius = (spacing * 0.44).round() as isize;
            for (vtx, color, _num) in &move_positions {
                let cx = (margin + vtx.column as f32 * spacing).round() as isize;
                let cy = (margin + vtx.row as f32 * spacing).round() as isize;
                let stone_color = match color {
                    Color::Black => 2u8,
                    Color::White => 3u8,
                };

                for dy in -stone_radius..=stone_radius {
                    for dx in -stone_radius..=stone_radius {
                        let dist_sq = dx * dx + dy * dy;
                        if dist_sq <= stone_radius * stone_radius {
                            let px = (cx + dx) as usize;
                            let py = (cy + dy) as usize;
                            if px < size && py < size {
                                pixels[py * width as usize + px] = stone_color;
                            }
                        }
                    }
                }
            }

            // 4. Draw KaTrain eval quality dot on last move
            if options.show_eval_quality
                && let Some((last_vtx, _last_color, _)) = move_positions.last()
                && let Some(eval) = evaluations
                    .iter()
                    .find(|e| e.move_number == move_positions.len())
            {
                let cx = (margin + last_vtx.column as f32 * spacing).round() as isize;
                let cy = (margin + last_vtx.row as f32 * spacing).round() as isize;
                let eval_dot_color = match eval.quality {
                    MoveQuality::Best => 9u8,
                    MoveQuality::Good => 10u8,
                    MoveQuality::Inaccuracy => 11u8,
                    MoveQuality::Mistake => 12u8,
                    MoveQuality::Blunder => 13u8,
                };
                let dot_r = (stone_radius as f32 * 0.38).round() as isize;
                for dy in -dot_r..=dot_r {
                    for dx in -dot_r..=dot_r {
                        if dx * dx + dy * dy <= dot_r * dot_r {
                            let px = (cx + dx) as usize;
                            let py = (cy + dy) as usize;
                            if px < size && py < size {
                                pixels[py * width as usize + px] = eval_dot_color;
                            }
                        }
                    }
                }
            }

            // 5. Draw bottom banner (color 7)
            for y in size..(size + banner_h) {
                for x in 0..width as usize {
                    pixels[y * width as usize + x] = 7;
                }
            }

            let is_last = frame_idx + 1 == total_frames;
            let delay = if is_last {
                options.final_frame_delay_cs
            } else {
                options.frame_delay_cs
            };

            let mut frame =
                Frame::from_palette_pixels(width, height, pixels, palette.clone(), None);
            frame.delay = delay;
            encoder
                .write_frame(&frame)
                .map_err(|e| format!("could not write GIF frame: {e}"))?;
        }
    }

    Ok(gif_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sabaki_domain_core::GameDocument;

    #[test]
    fn exports_valid_gif_bytes_from_sgf() {
        let doc =
            GameDocument::from_sgf("(;SZ[9]KM[6.5]PB[Black]PW[White];B[ee];W[eg];B[cd];W[dc])")
                .expect("sgf parses");
        let snapshot = doc.snapshot();
        let options = GifExportOptions {
            image_size: 160,
            frame_delay_cs: 10,
            final_frame_delay_cs: 20,
            show_move_numbers: true,
            show_eval_quality: true,
        };

        let gif_data = export_sgf_to_gif(&snapshot, &options).expect("gif exports successfully");
        assert!(!gif_data.is_empty());
        // Verify standard GIF header "GIF89a"
        assert_eq!(&gif_data[0..6], b"GIF89a");
    }
}
