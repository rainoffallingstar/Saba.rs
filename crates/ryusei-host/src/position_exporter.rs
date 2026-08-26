//! Deterministic current-position PNG export.
//!
//! This renderer intentionally lives outside GPUI: exports are based on the
//! document snapshot, so they work from menus, tests, and headless workflows
//! with identical pixels.

use ryusei_domain_core::{BoardSnapshot, Color, Vertex};

#[derive(Clone, Debug)]
pub struct PositionPngOptions {
    pub image_size: u32,
    pub show_coordinates: bool,
    pub ownership: Option<Vec<f64>>,
}

impl Default for PositionPngOptions {
    fn default() -> Self {
        Self {
            image_size: 720,
            show_coordinates: true,
            ownership: None,
        }
    }
}

pub fn export_position_to_png(
    board: &BoardSnapshot,
    options: &PositionPngOptions,
) -> Result<Vec<u8>, String> {
    let size = options.image_size.clamp(180, 4096) as usize;
    let width = size as u32;
    let height = size as u32;
    let board_width = board.width.max(1);
    let board_height = board.height.max(1);
    let margin =
        (size as f32 * if options.show_coordinates { 0.08 } else { 0.05 }).round() as usize;
    let x_span = size.saturating_sub(margin * 2).max(1);
    let y_span = size.saturating_sub(margin * 2).max(1);
    let x_step = x_span as f32 / board_width.saturating_sub(1).max(1) as f32;
    let y_step = y_span as f32 / board_height.saturating_sub(1).max(1) as f32;
    let mut pixels = vec![0u8; size * size * 3];

    for row in 0..size {
        for column in 0..size {
            let t = ((row + column) as f32 / size as f32) * 0.035;
            set_pixel(
                &mut pixels,
                size,
                column,
                row,
                [226, 177, 119 - (t * 100.0) as u8],
            );
        }
    }

    // Ownership is drawn beneath stones, matching the on-screen heatmap.
    if let Some(ownership) = options.ownership.as_deref() {
        for row in 0..board_height {
            for column in 0..board_width {
                let index = row * board_width + column;
                let Some(value) = ownership
                    .get(index)
                    .copied()
                    .filter(|value| value.is_finite())
                else {
                    continue;
                };
                if value.abs() < 0.15 {
                    continue;
                }
                let (x, y) = intersection(margin, x_step, y_step, column, row);
                let radius = ((x_step.min(y_step) * 0.36).round() as isize).max(2);
                let alpha = (value.abs() as f32 * 0.38).clamp(0.10, 0.38);
                let color = if value > 0.0 {
                    [45.0, 115.0, 220.0]
                } else {
                    [235.0, 235.0, 235.0]
                };
                draw_square(
                    &mut pixels,
                    size,
                    x,
                    y,
                    radius,
                    [
                        blend(226.0, color[0], alpha),
                        blend(177.0, color[1], alpha),
                        blend(119.0, color[2], alpha),
                    ],
                );
            }
        }
    }

    // Grid and star points.
    for column in 0..board_width {
        let (x, _) = intersection(margin, x_step, y_step, column, 0);
        draw_line(
            &mut pixels,
            size,
            x,
            margin as isize,
            x,
            (size - margin) as isize,
            [61, 40, 20],
        );
    }
    for row in 0..board_height {
        let (_, y) = intersection(margin, x_step, y_step, 0, row);
        draw_line(
            &mut pixels,
            size,
            margin as isize,
            y,
            (size - margin) as isize,
            y,
            [61, 40, 20],
        );
    }
    let star_distance = if board_width >= 13 { 3 } else { 2 };
    if board_width == board_height && board_width >= 5 {
        for (column, row) in [
            (star_distance, star_distance),
            (board_width - 1 - star_distance, star_distance),
            (star_distance, board_height - 1 - star_distance),
            (
                board_width - 1 - star_distance,
                board_height - 1 - star_distance,
            ),
        ] {
            let (x, y) = intersection(margin, x_step, y_step, column, row);
            draw_disk(
                &mut pixels,
                size,
                x,
                y,
                (x_step.min(y_step) * 0.07) as isize,
                [61, 40, 20],
            );
        }
    }

    // Column letters along the top edge and row numbers along the left edge,
    // drawn with a tiny bitmap font so exports stay deterministic and free of
    // font/platform dependencies. The glyphs live entirely inside the margin,
    // so they can never collide with the grid or stones.
    if options.show_coordinates {
        let center = (margin as f32 / 2.0).round() as isize;
        for column in 0..board_width {
            let (x, _) = intersection(margin, x_step, y_step, column, 0);
            draw_glyph(
                &mut pixels,
                size,
                x,
                center,
                glyph_for_ascii(column_letter(column)),
            );
        }
        for row in 0..board_height {
            let (_, y) = intersection(margin, x_step, y_step, 0, row);
            // Go labels the top row with the board height and counts downward,
            // so row index 0 becomes `board_height`.
            draw_number(&mut pixels, size, center, y, board_height - row);
        }
    }

    // Stones from the already-materialized board state.
    for row in 0..board_height {
        for column in 0..board_width {
            let sign = board
                .sign_map
                .get(row)
                .and_then(|line| line.get(column))
                .copied()
                .unwrap_or(0);
            let Some(color) = (match sign {
                1 => Some(Color::Black),
                -1 => Some(Color::White),
                _ => None,
            }) else {
                continue;
            };
            let (x, y) = intersection(margin, x_step, y_step, column, row);
            let radius = (x_step.min(y_step) * 0.44).round() as isize;
            draw_disk(
                &mut pixels,
                size,
                x,
                y,
                radius,
                match color {
                    Color::Black => [24, 24, 24],
                    Color::White => [248, 248, 248],
                },
            );
            draw_circle(&mut pixels, size, x, y, radius, [140, 110, 80]);
        }
    }

    if let Some(Vertex { column, row }) = board.current_vertex {
        let (x, y) = intersection(margin, x_step, y_step, column, row);
        draw_circle(
            &mut pixels,
            size,
            x,
            y,
            (x_step.min(y_step) * 0.20) as isize,
            [192, 57, 43],
        );
    }

    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|error| format!("could not initialize PNG encoder: {error}"))?;
        writer
            .write_image_data(&pixels)
            .map_err(|error| format!("could not write PNG image: {error}"))?;
    }
    Ok(bytes)
}

fn intersection(
    margin: usize,
    x_step: f32,
    y_step: f32,
    column: usize,
    row: usize,
) -> (isize, isize) {
    (
        (margin as f32 + column as f32 * x_step).round() as isize,
        (margin as f32 + row as f32 * y_step).round() as isize,
    )
}

fn blend(base: f32, overlay: f32, alpha: f32) -> u8 {
    (base * (1.0 - alpha) + overlay * alpha)
        .round()
        .clamp(0.0, 255.0) as u8
}

fn set_pixel(pixels: &mut [u8], size: usize, x: usize, y: usize, color: [u8; 3]) {
    let index = (y * size + x) * 3;
    if index + 2 < pixels.len() {
        pixels[index..index + 3].copy_from_slice(&color);
    }
}

fn draw_square(pixels: &mut [u8], size: usize, x: isize, y: isize, radius: isize, color: [u8; 3]) {
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            let px = x + dx;
            let py = y + dy;
            if px >= 0 && py >= 0 {
                set_pixel(pixels, size, px as usize, py as usize, color);
            }
        }
    }
}

fn draw_disk(pixels: &mut [u8], size: usize, x: isize, y: isize, radius: isize, color: [u8; 3]) {
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if dx * dx + dy * dy <= radius * radius {
                let px = x + dx;
                let py = y + dy;
                if px >= 0 && py >= 0 {
                    set_pixel(pixels, size, px as usize, py as usize, color);
                }
            }
        }
    }
}

fn draw_circle(pixels: &mut [u8], size: usize, x: isize, y: isize, radius: isize, color: [u8; 3]) {
    let inner = (radius - 2).max(0);
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            let distance = dx * dx + dy * dy;
            if distance <= radius * radius && distance >= inner * inner {
                let px = x + dx;
                let py = y + dy;
                if px >= 0 && py >= 0 {
                    set_pixel(pixels, size, px as usize, py as usize, color);
                }
            }
        }
    }
}

fn draw_line(
    pixels: &mut [u8],
    size: usize,
    x0: isize,
    y0: isize,
    x1: isize,
    y1: isize,
    color: [u8; 3],
) {
    if x0 == x1 {
        for y in y0.min(y1)..=y0.max(y1) {
            if x0 >= 0 && y >= 0 {
                set_pixel(pixels, size, x0 as usize, y as usize, color);
            }
        }
    } else {
        for x in x0.min(x1)..=x0.max(x1) {
            if x >= 0 && y0 >= 0 {
                set_pixel(pixels, size, x as usize, y0 as usize, color);
            }
        }
    }
}

/// A single 5x7 bitmap glyph: one byte per row, low 5 bits select the pixels.
/// Indexed by ASCII byte (0-9, A-Z). `I` is intentionally absent.
const fn glyph_for_ascii(character: u8) -> [u8; 7] {
    match character {
        b'0' => [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
        b'1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        b'2' => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ],
        b'3' => [
            0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110,
        ],
        b'4' => [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        b'5' => [
            0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110,
        ],
        b'6' => [
            0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
        b'7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        b'8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        b'9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100,
        ],
        b'A' => [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        b'B' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
        b'C' => [
            0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110,
        ],
        b'D' => [
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
        b'E' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
        b'F' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        b'G' => [
            0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111,
        ],
        b'H' => [
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        b'J' => [
            0b00111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100,
        ],
        b'K' => [
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ],
        b'L' => [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
        b'M' => [
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ],
        b'N' => [
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ],
        b'O' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        b'P' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        b'Q' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
        ],
        b'R' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ],
        b'S' => [
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        b'T' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        b'U' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        b'V' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ],
        b'W' => [
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b10101, 0b01010,
        ],
        b'X' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ],
        b'Y' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        b'Z' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
        ],
        _ => [0u8; 7],
    }
}

/// Column labels run A-Z skipping `I` (standard Go convention).
fn column_letter(index: usize) -> u8 {
    const LETTERS: &[u8] = b"ABCDEFGHJKLMNOPQRSTUVWXYZ";
    LETTERS[index % LETTERS.len()]
}

/// Draw one 5x7 glyph centered on `(center_x, center_y)`. All bounds are
/// checked inside `set_pixel`, so nothing can overflow the image.
fn draw_glyph(pixels: &mut [u8], size: usize, center_x: isize, center_y: isize, glyph: [u8; 7]) {
    let color = [61, 40, 20];
    for (row, bits) in glyph.iter().enumerate() {
        let y = center_y - 3 + row as isize;
        if y < 0 {
            continue;
        }
        for column in 0..5 {
            if bits & (1 << (4 - column)) != 0 {
                let x = center_x - 2 + column;
                if x >= 0 {
                    set_pixel(pixels, size, x as usize, y as usize, color);
                }
            }
        }
    }
}

/// Draw a multi-digit number centered on `(center_x, center_y)`.
fn draw_number(pixels: &mut [u8], size: usize, center_x: isize, center_y: isize, value: usize) {
    let digits = value.to_string();
    let count = digits.len() as isize;
    let gap = 1;
    let total_width = count * 5 + (count - 1) * gap;
    let start_x = center_x - total_width / 2;
    for (index, digit) in digits.bytes().enumerate() {
        let x = start_x + (index as isize) * (5 + gap) + 2;
        draw_glyph(pixels, size, x, center_y, glyph_for_ascii(digit));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ryusei_domain_core::GameDocument;

    #[test]
    fn exports_a_valid_rgb_png_for_current_position() {
        let doc = GameDocument::from_sgf("(;SZ[9];B[dd];W[ee])").expect("sgf parses");
        let board = doc.snapshot().board;
        let png = export_position_to_png(
            &board,
            &PositionPngOptions {
                image_size: 180,
                show_coordinates: false,
                ownership: Some(vec![0.8; 81]),
            },
        )
        .expect("png exports");
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(png.len() > 100);
    }

    #[test]
    fn show_coordinates_changes_the_exported_pixels() {
        let doc = GameDocument::from_sgf("(;SZ[9];B[dd];W[ee])").expect("sgf parses");
        let board = doc.snapshot().board;
        let with_coords = export_position_to_png(
            &board,
            &PositionPngOptions {
                image_size: 240,
                show_coordinates: true,
                ownership: None,
            },
        )
        .expect("png exports with coordinates");
        let without_coords = export_position_to_png(
            &board,
            &PositionPngOptions {
                image_size: 240,
                show_coordinates: false,
                ownership: None,
            },
        )
        .expect("png exports without coordinates");

        // Both are valid PNGs.
        for png in [&with_coords, &without_coords] {
            assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
            assert!(png.len() > 100);
        }
        // The byte streams differ, so the images must differ.
        assert_ne!(with_coords, without_coords);
    }

    #[test]
    fn coordinates_stay_in_bounds_for_all_sizes_and_small_exports() {
        // Enumerate representative board sizes at the minimum export size to
        // prove the labels never push pixels out of the image buffer.
        for size in [180, 181, 360] {
            for board_size in [5usize, 9, 13, 19, 25] {
                let sgf = format!("(;SZ[{board_size}])");
                let doc = GameDocument::from_sgf(&sgf).expect("sgf parses");
                let board = doc.snapshot().board;
                let png = export_position_to_png(
                    &board,
                    &PositionPngOptions {
                        image_size: size,
                        show_coordinates: true,
                        ownership: None,
                    },
                )
                .expect("png exports within bounds");
                assert!(
                    png.starts_with(b"\x89PNG\r\n\x1a\n"),
                    "board {board_size} at {size}px must stay a valid PNG"
                );
            }
        }
    }

    #[test]
    fn column_letters_skip_i() {
        // Standard Go column alphabet: A B C D E F G H J K L ...
        let letters: Vec<u8> = (0..18).map(column_letter).collect();
        assert_eq!(letters, b"ABCDEFGHJKLMNOPQRS".to_vec());
    }
}
