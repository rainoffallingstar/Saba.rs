use std::collections::VecDeque;
use std::{collections::BTreeMap, rc::Rc};

use std::time::Duration;

use gpui::{
    Animation, AnimationExt as _, App, BoxShadow, Div, FontWeight, InteractiveElement,
    IntoElement as _, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement,
    Stateful, Styled, Window, div, hsla, point, pulsating_between, px, rgb,
};
#[cfg(test)]
use gpui::{Pixels, Point};
#[cfg(test)]
use ryusei_domain_core::MoveDto;
use ryusei_domain_core::{BoardSnapshot, Color, GameSnapshot, NodeSnapshot, Vertex};

use crate::engine_console::parse_gtp_vertex;
use crate::markup::markup_symbol;
use crate::theme::{ThemeColor, ThemeTokens, ui_palette};

/// Projects a GTP principal variation into numbered ghost stones without
/// mutating the document or applying moves to the host. Invalid/pass tokens are
/// ignored, matching the tolerant streaming preview behavior.
pub fn pv_preview_points(
    board_width: usize,
    next_player: Color,
    pv: &[String],
    limit: usize,
) -> Vec<(Vertex, Color, usize)> {
    let mut color = next_player;
    let mut points = Vec::new();
    for move_str in pv.iter().take(limit) {
        if let Some((column, row)) = parse_gtp_vertex(board_width, move_str) {
            points.push((Vertex { column, row }, color, points.len() + 1));
            color = color.opponent();
        }
    }
    points
}

const BOARD_MARGIN_PX: f32 = 28.0;

/// Rendering options derived from the shell settings and document state.
#[derive(Clone, Debug, Default)]
pub struct GobanRenderOptions {
    /// Draw A-T column and 1-19 row labels in the margins.
    pub show_coordinates: bool,
    /// Coordinate label style: `A1` (SGF-style) or `1-1`.
    pub coordinates_type: String,
    /// Draw the move number on each stone.
    pub show_move_numbers: bool,
    /// Move number per vertex according to `move_numbers_type`.
    pub move_numbers: BTreeMap<Vertex, usize>,
    /// Temporary line or arrow shown while the user is dragging a markup tool.
    pub line_preview: Option<ryusei_domain_core::BoardLineSnapshot>,
    /// Empty intersection currently under the pointer, rendered as a ghost stone.
    pub hovered_vertex: Option<Vertex>,
    /// Side to move for the hover ghost stone.
    pub hover_stone_color: Option<Color>,
    /// Engine candidates annotated directly on the board.
    pub analysis_candidates: Vec<AnalysisCandidate>,
    /// Move evaluations mapped to vertices for KaTrain-style colored quality dots.
    pub eval_dots: BTreeMap<Vertex, ryusei_host::MoveQuality>,
    /// Prospective PV variation ghost sequence preview.
    pub pv_preview: Vec<(Vertex, Color, usize)>,
    /// Optional territory ownership probabilities from KataGo.
    pub ownership: Option<Vec<f64>>,
    /// Alive-stone overrides from `GameSnapshot.score_overrides`; overridden
    /// vertices get a cross marker.
    pub score_overrides: BTreeMap<Vertex, i8>,
    /// Draw child-move ghost stones.
    pub show_next_moves: bool,
    /// Draw sibling-variation ghost stones.
    pub show_siblings: bool,
    /// Label child-move annotations next to ghost stones.
    pub show_move_colorization: bool,
}

/// Builds the 1-based move number per vertex from a document's move list.
/// `pass` moves (no vertex) are skipped.
#[cfg(test)]
pub fn move_numbers_from_moves(moves: &[MoveDto]) -> BTreeMap<Vertex, usize> {
    moves
        .iter()
        .enumerate()
        .filter_map(|(index, move_dto)| move_dto.vertex.map(|vertex| (vertex, index + 1)))
        .collect()
}

/// A compact engine candidate label shown beside an intersection.
#[derive(Clone, Debug, PartialEq)]
pub struct AnalysisCandidate {
    pub vertex: Vertex,
    pub winrate_percent: f64,
    pub visits: u64,
    pub score_lead: Option<f64>,
    pub is_best: bool,
}

fn node_move_vertex(node: &NodeSnapshot) -> Option<Vertex> {
    ["B", "W"]
        .into_iter()
        .find_map(|property| node.properties.get(property)?.first())
        .and_then(|value| parse_sgf_vertex(value))
}

fn current_node_lineage(snapshot: &GameSnapshot) -> Vec<NodeSnapshot> {
    let nodes_by_id: BTreeMap<_, _> = snapshot
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();
    let mut lineage = Vec::new();
    let mut current_id = snapshot.current_node_id.as_str();
    while let Some(node) = nodes_by_id.get(current_id) {
        lineage.push((*node).clone());
        let Some(parent_id) = node.parent_id.as_deref() else {
            break;
        };
        current_id = parent_id;
    }
    lineage.reverse();
    lineage
}

/// Builds on-board move numbers from the active SGF path. `start` labels the
/// whole game, while `variation` and `hotspot` start immediately after the
/// nearest branch point or `HO` marker, respectively. They deliberately show
/// no labels when the requested anchor is absent, matching Sabaki's behavior.
/// Builds on-board move numbers from the active SGF path. `start` / `all` labels
/// the whole game, `last_N` displays only the most recent N moves, while
/// `variation` and `hotspot` start immediately after the nearest branch point
/// or `HO` marker, respectively.
pub fn move_numbers_for_snapshot(
    snapshot: &GameSnapshot,
    move_numbers_type: &str,
) -> BTreeMap<Vertex, usize> {
    let lineage = current_node_lineage(snapshot);
    let total_lineage = lineage.len();
    let start_index = match move_numbers_type {
        "last_1" => Some(total_lineage.saturating_sub(1)),
        "last_3" => Some(total_lineage.saturating_sub(3)),
        "last_5" => Some(total_lineage.saturating_sub(5)),
        "last_10" => Some(total_lineage.saturating_sub(10)),
        "last_20" => Some(total_lineage.saturating_sub(20)),
        "variation" => {
            let is_main_line = lineage
                .windows(2)
                .all(|nodes| nodes[0].child_ids.first() == Some(&nodes[1].id));
            if is_main_line {
                None
            } else {
                lineage
                    .iter()
                    .rposition(|node| node.child_ids.len() > 1)
                    .map(|index| index + 1)
            }
        }
        "hotspot" => lineage
            .iter()
            .rposition(|node| node.properties.contains_key("HO"))
            .map(|index| index + 1),
        _ => Some(0),
    };
    let Some(start_index) = start_index else {
        return BTreeMap::new();
    };

    let mut move_number = 0usize;
    lineage
        .into_iter()
        .skip(start_index)
        .filter_map(|node| {
            let vertex = node_move_vertex(&node)?;
            move_number += 1;
            Some((vertex, move_number))
        })
        .collect()
}

pub fn board_spacing(board: &BoardSnapshot, board_pixel_size: f32) -> f32 {
    (board_pixel_size - 2.0 * BOARD_MARGIN_PX) / (board.width.max(2) - 1) as f32
}

/// Computes a lightweight territory-ownership map from board geometry alone.
///
/// This is the estimator-mode fallback used when KataGo has not produced an
/// ownership tensor. Each empty intersection is weighted by the ratio of BFS
/// distances to the nearest black and white stones: `+` means black influence,
/// `-` means white influence. Occupied intersections carry their stone sign.
pub fn estimate_ownership_from_board(board: &BoardSnapshot) -> Option<Vec<f64>> {
    let width = board.width;
    let height = board.height;
    if width == 0 || height == 0 {
        return None;
    }

    let black_dist = ownership_distance_field(board, 1);
    let white_dist = ownership_distance_field(board, -1);

    let mut ownership = Vec::with_capacity(width * height);
    for row in 0..height {
        for column in 0..width {
            let idx = row * width + column;
            let stone = board.sign_map[row][column];
            if stone == 1 {
                ownership.push(1.0);
                continue;
            }
            if stone == -1 {
                ownership.push(-1.0);
                continue;
            }
            let black = black_dist[idx];
            let white = white_dist[idx];
            if black == usize::MAX && white == usize::MAX {
                ownership.push(0.0);
                continue;
            }
            let black = black as f64;
            let white = white as f64;
            let value = (white - black) / (black + white).max(1.0);
            ownership.push(value.clamp(-1.0, 1.0));
        }
    }
    Some(ownership)
}

/// Multi-source BFS distance field from every stone of `target_sign`
/// (`1` = black, `-1` = white) to every empty intersection.
fn ownership_distance_field(board: &BoardSnapshot, target_sign: i8) -> Vec<usize> {
    let width = board.width;
    let height = board.height;
    let mut distances = vec![usize::MAX; width * height];
    let mut queue = VecDeque::new();
    for row in 0..height {
        for column in 0..width {
            if board.sign_map[row][column] == target_sign {
                let idx = row * width + column;
                distances[idx] = 0;
                queue.push_back((column, row));
            }
        }
    }
    while let Some((column, row)) = queue.pop_front() {
        let current = distances[row * width + column];
        let neighbors = [
            (column.wrapping_sub(1), row),
            (column + 1, row),
            (column, row.wrapping_sub(1)),
            (column, row + 1),
        ];
        for (next_column, next_row) in neighbors {
            if next_column < width && next_row < height {
                let idx = next_row * width + next_column;
                if distances[idx] == usize::MAX {
                    distances[idx] = current + 1;
                    queue.push_back((next_column, next_row));
                }
            }
        }
    }
    distances
}

/// Converts a point relative to the goban element into a board vertex, or
/// `None` when the point falls outside the board.
#[cfg(test)]
pub fn vertex_at(
    board: &BoardSnapshot,
    board_pixel_size: f32,
    position: Point<Pixels>,
) -> Option<Vertex> {
    if board.width < 2 || board.height < 2 {
        return None;
    }
    let spacing = board_spacing(board, board_pixel_size);
    let column_index = ((f32::from(position.x) - BOARD_MARGIN_PX) / spacing).round() as i64;
    let row_index = ((f32::from(position.y) - BOARD_MARGIN_PX) / spacing).round() as i64;
    if column_index < 0
        || row_index < 0
        || column_index >= board.width as i64
        || row_index >= board.height as i64
    {
        return None;
    }
    Some(Vertex {
        column: column_index as usize,
        row: row_index as usize,
    })
}

/// Returns the pixel position of an intersection relative to the goban
/// element's origin, for overlays (markers, best-move indicators).
pub fn intersection_position(
    board: &BoardSnapshot,
    board_pixel_size: f32,
    column: usize,
    row: usize,
) -> (f32, f32) {
    let spacing = board_spacing(board, board_pixel_size);
    (
        BOARD_MARGIN_PX + column as f32 * spacing,
        BOARD_MARGIN_PX + row as f32 * spacing,
    )
}

fn board_line(from: (f32, f32), to: (f32, f32), thickness_px: f32, color: u32) -> Div {
    let (x1, y1) = from;
    let (x2, y2) = to;
    let is_horizontal = (y1 - y2).abs() < 0.001;
    if is_horizontal {
        div()
            .absolute()
            .left(px(x1))
            .top(px(y1 - thickness_px / 2.0))
            .w(px(x2 - x1))
            .h(px(thickness_px))
            .bg(rgb(color))
    } else {
        div()
            .absolute()
            .left(px(x1 - thickness_px / 2.0))
            .top(px(y1))
            .w(px(thickness_px))
            .h(px(y2 - y1))
            .bg(rgb(color))
    }
}

/// Renders an arbitrary angled annotation stroke as closely-spaced round
/// segments. GPUI's primitive `Div` has no rotation transform, so this keeps
/// diagonal SGF lines and arrowheads geometrically correct without CSS.
fn markup_stroke(from: (f32, f32), to: (f32, f32), thickness_px: f32, color: u32) -> Vec<Div> {
    let delta_x = to.0 - from.0;
    let delta_y = to.1 - from.1;
    let distance = (delta_x * delta_x + delta_y * delta_y).sqrt();
    let steps = (distance / (thickness_px * 0.65)).ceil().max(1.0) as usize;
    (0..=steps)
        .map(|step| {
            let progress = step as f32 / steps as f32;
            let x = from.0 + delta_x * progress;
            let y = from.1 + delta_y * progress;
            div()
                .absolute()
                .left(px(x - thickness_px / 2.0))
                .top(px(y - thickness_px / 2.0))
                .size(px(thickness_px))
                .rounded_full()
                .bg(rgb(color))
        })
        .collect()
}

/// Computes the two wing endpoints of an arrowhead at `end`, pointing back
/// along the line from `start` to `end`. Pure so the geometry is testable.
pub fn arrowhead_vertices(start: (f32, f32), end: (f32, f32), wing_length: f32) -> [(f32, f32); 2] {
    let (dx, dy) = (end.0 - start.0, end.1 - start.1);
    let length = (dx * dx + dy * dy).sqrt().max(1e-6);
    let (ux, uy) = (dx / length, dy / length);
    let (px, py) = (-uy, ux);
    let half_width = wing_length * 0.5;
    [
        (
            end.0 - ux * wing_length + px * half_width,
            end.1 - uy * wing_length + py * half_width,
        ),
        (
            end.0 - ux * wing_length - px * half_width,
            end.1 - uy * wing_length - py * half_width,
        ),
    ]
}

/// The SGF column letter for a zero-based column index, skipping `I`.
fn column_letter(column: usize) -> char {
    let offset = if column >= 8 { column + 1 } else { column };
    char::from_u32((b'A' + offset as u8) as u32).unwrap_or('A')
}

/// Formats a zero-based vertex as an SGF point like `dd` (lowercase).
pub fn format_sgf_vertex(vertex: Vertex) -> String {
    let column = char::from_u32((b'a' + vertex.column as u8) as u32).unwrap_or('a');
    let row = char::from_u32((b'a' + vertex.row as u8) as u32).unwrap_or('a');
    format!("{column}{row}")
}

/// Parses an SGF point like `dd` back into a zero-based vertex.
#[allow(dead_code)]
pub fn parse_sgf_vertex(value: &str) -> Option<Vertex> {
    let mut characters = value.chars();
    let column_char = characters.next()?;
    let row_char = characters.next()?;
    if characters.next().is_some() {
        return None;
    }
    if !column_char.is_ascii_lowercase() || !row_char.is_ascii_lowercase() {
        return None;
    }
    Some(Vertex {
        column: usize::from(column_char as u8 - b'a'),
        row: usize::from(row_char as u8 - b'a'),
    })
}

fn stone(color: Color, x: f32, y: f32, size: f32, stone_black: u32, stone_white: u32) -> Div {
    let stone_color = match color {
        Color::Black => rgb(stone_black),
        Color::White => rgb(stone_white),
    };
    let border_color = match color {
        Color::Black => rgb(0x111114),
        Color::White => rgb(0xd0d0d8),
    };
    let highlight_color = match color {
        Color::Black => hsla(0.0, 0.0, 1.0, 0.22),
        Color::White => hsla(0.0, 0.0, 1.0, 0.85),
    };

    // Approximate the design's radial-gradient stones (`#55555c→#1d1d20→#050507`
    // / `…→#c4c4c8`) with layered translucent overlays: a broad top-left sheen,
    // a soft top-centre glow, and a deepening bottom-right shade. GPUI has no
    // radial gradient, so three stacked discs stand in for the three stops.
    let sheen = match color {
        Color::Black => hsla(0.0, 0.0, 0.42, 0.35), // #55555c-ish lift
        Color::White => hsla(0.0, 0.0, 1.0, 0.55),
    };
    let shade = match color {
        Color::Black => hsla(0.0, 0.0, 0.0, 0.45), // #050507 base
        Color::White => hsla(220.0, 0.10, 0.55, 0.28), // #c4c4c8 base
    };
    div()
        .absolute()
        .left(px(x - size / 2.0))
        .top(px(y - size / 2.0))
        .size(px(size))
        .rounded_full()
        .border_1()
        .border_color(border_color)
        .bg(stone_color)
        // Drop shadow ≈ feDropShadow dx1.5 dy3.5 blur2.5 rgba(0,0,0,0.32)
        .shadow(vec![BoxShadow {
            color: hsla(0.0, 0.0, 0.0, 0.32),
            offset: point(px(1.5), px(3.5)),
            blur_radius: px(2.5),
            spread_radius: px(0.0),
        }])
        // Deepening bottom-right shade first (the gradient's dark/far stop),
        // kept inside the stone so the sheen and highlight read on top of it.
        .child(
            div()
                .absolute()
                .bottom(px(size * 0.0))
                .right(px(size * 0.0))
                .size(px(size * 0.80))
                .rounded_full()
                .bg(shade),
        )
        // Broad top-left sheen (largest, softest lift).
        .child(
            div()
                .absolute()
                .top(px(size * 0.02))
                .left(px(size * 0.04))
                .size(px(size * 0.72))
                .rounded_full()
                .bg(sheen),
        )
        // Primary specular highlight in top-left.
        .child(
            div()
                .absolute()
                .top(px(size * 0.10))
                .left(px(size * 0.14))
                .size(px(size * 0.36))
                .rounded_full()
                .bg(highlight_color),
        )
}

/// Renders a child or sibling ghost stone. Child moves are filled; sibling
/// variations are outlined so the current line stays visually dominant.
fn ghost_stone(
    color: Color,
    x: f32,
    y: f32,
    size: f32,
    stone_black: u32,
    stone_white: u32,
    filled: bool,
) -> Div {
    let ghost_size = if filled { size * 0.85 } else { size * 0.7 };
    let stone_color = match color {
        Color::Black => rgb(stone_black),
        Color::White => rgb(stone_white),
    };
    let mut ghost = div()
        .absolute()
        .left(px(x - ghost_size / 2.0))
        .top(px(y - ghost_size / 2.0))
        .size(px(ghost_size))
        .rounded_full()
        .border_1()
        .border_color(stone_color);
    if filled {
        ghost = ghost.bg(stone_color);
    }
    ghost
}

/// Renders a move number centered on a stone, in the contrast color.
fn move_number_div(x: f32, y: f32, stone_size: f32, number: usize, on_black: bool) -> Div {
    let text_w = stone_size * 0.8;
    let text_h = stone_size * 0.8;
    div()
        .absolute()
        .left(px(x - text_w / 2.0))
        .top(px(y - text_h / 2.0))
        .w(px(text_w))
        .h(px(text_h))
        .flex()
        .items_center()
        .justify_center()
        .text_xs()
        .text_color(rgb(if on_black { 0xffffff } else { 0x111111 }))
        .child(number.to_string())
}

fn star_point_vertices(width: usize, height: usize) -> Vec<(usize, usize)> {
    if width < 9 || height < 9 {
        return Vec::new();
    }
    let columns: Vec<usize> = if width >= 13 {
        vec![3, width / 2, width - 4]
    } else {
        vec![3, width - 4]
    };
    let rows: Vec<usize> = if height >= 13 {
        vec![3, height / 2, height - 4]
    } else {
        vec![3, height - 4]
    };
    let mut result = Vec::new();
    for &column in &columns {
        for &row in &rows {
            result.push((column, row));
        }
    }
    result
}

/// Renders the transparent hit-testing layer for the goban. Each vertex gets
/// its own explicit hitbox, so clicks work without converting window-global
/// coordinates back through whatever layout surrounds the board.
pub fn render_goban_click_layer<F, G, H>(
    board: &BoardSnapshot,
    board_pixel_size: f32,
    on_vertex_mouse_down: Rc<F>,
    on_vertex_mouse_move: Rc<G>,
    on_vertex_mouse_up: Rc<H>,
) -> Div
where
    F: Fn(Vertex, &MouseDownEvent, &mut Window, &mut App) + 'static,
    G: Fn(Vertex, &MouseMoveEvent, &mut Window, &mut App) + 'static,
    H: Fn(Vertex, &MouseUpEvent, &mut Window, &mut App) + 'static,
{
    let spacing = board_spacing(board, board_pixel_size);
    let mut layer = div().absolute().top_0().left_0().size(px(board_pixel_size));
    for row in 0..board.height {
        for column in 0..board.width {
            let vertex = Vertex { column, row };
            let mouse_down_handler = on_vertex_mouse_down.clone();
            let mouse_move_handler = on_vertex_mouse_move.clone();
            let mouse_up_handler = on_vertex_mouse_up.clone();
            let (x, y) = intersection_position(board, board_pixel_size, column, row);
            layer = layer.child(
                div()
                    .absolute()
                    .left(px(x - spacing / 2.0))
                    .top(px(y - spacing / 2.0))
                    .size(px(spacing))
                    .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                        mouse_down_handler(vertex, event, window, cx);
                    })
                    .on_mouse_move(move |event, window, cx| {
                        mouse_move_handler(vertex, event, window, cx);
                    })
                    .on_mouse_up(MouseButton::Left, move |event, window, cx| {
                        mouse_up_handler(vertex, event, window, cx);
                    }),
            );
        }
    }
    layer
}

fn wood_is_dark(color: ThemeColor) -> bool {
    let luminance = 0.2126 * f32::from(color.red)
        + 0.7152 * f32::from(color.green)
        + 0.0722 * f32::from(color.blue);
    luminance < 128.0
}

pub fn render_goban(
    board: &BoardSnapshot,
    board_pixel_size: f32,
    theme: &ThemeTokens,
    options: &GobanRenderOptions,
) -> Stateful<Div> {
    render_goban_with_id("goban", board, board_pixel_size, theme, options)
}

/// Renders a goban with a caller-owned stable element id. Secondary boards
/// (such as the analysis preview) must not share the main board's hit-test and
/// visual-test identity.
pub fn render_goban_with_id(
    element_id: &'static str,
    board: &BoardSnapshot,
    board_pixel_size: f32,
    theme: &ThemeTokens,
    options: &GobanRenderOptions,
) -> Stateful<Div> {
    let width = board.width;
    let height = board.height;
    let board_size = board_pixel_size;
    let spacing = board_spacing(board, board_pixel_size);
    let stone_size = (spacing * 0.95).max(10.0);
    let wood_color = theme.board_wood_color().rgb_u32();
    let wood_theme_color = theme.board_wood_color();
    let coordinate_color = if wood_is_dark(wood_theme_color) {
        0xe8d9c2
    } else {
        0x6b4f2a
    };
    let line_color = theme.board_line_color().rgb_u32();
    let star_point_color = theme.star_point_color().rgb_u32();
    let stone_black = theme.stone_black_color().rgb_u32();
    let stone_white = theme.stone_white_color().rgb_u32();

    // Semantic analysis/markup colors derived from the active theme so the
    // board overlays follow the same design tokens as the surrounding shell.
    let palette = ui_palette(theme);
    let accent_color = palette.accent; // best candidate / last-move ring
    let success_color = palette.success; // good move
    let warn_color = palette.warn; // inaccuracy
    let danger_color = palette.danger_text; // mistake / blunder / markup

    let mut children: Vec<Div> = Vec::new();

    for column in 0..width {
        let (x, _) = intersection_position(board, board_pixel_size, column, 0);
        children.push(board_line(
            (x, BOARD_MARGIN_PX),
            (x, board_size - BOARD_MARGIN_PX),
            1.5,
            line_color,
        ));
    }
    for row in 0..height {
        let (_, y) = intersection_position(board, board_pixel_size, 0, row);
        children.push(board_line(
            (BOARD_MARGIN_PX, y),
            (board_size - BOARD_MARGIN_PX, y),
            1.5,
            line_color,
        ));
    }

    let star_size = (spacing * 0.16).clamp(3.0, 7.0);
    for (column, row) in star_point_vertices(width, height) {
        let (x, y) = intersection_position(board, board_pixel_size, column, row);
        children.push(
            div()
                .absolute()
                .left(px(x - star_size / 2.0))
                .top(px(y - star_size / 2.0))
                .size(px(star_size))
                .rounded_full()
                .bg(rgb(star_point_color)),
        );
    }

    // Board lines, arrows, and the in-progress drag preview.
    let mut annotation_lines = board.lines.clone();
    if let Some(preview) = &options.line_preview {
        annotation_lines.push(preview.clone());
    }
    for line in annotation_lines {
        let start =
            intersection_position(board, board_pixel_size, line.start.column, line.start.row);
        let end = intersection_position(board, board_pixel_size, line.end.column, line.end.row);
        let color = if options.line_preview.as_ref() == Some(&line) {
            0x8e44ad
        } else {
            danger_color
        };
        children.extend(markup_stroke(start, end, 2.0, color));
        if line.line_type == "arrow" {
            for wing in arrowhead_vertices(start, end, 9.0) {
                children.extend(markup_stroke(end, wing, 2.0, color));
            }
        }
    }

    // KataGo territory ownership heatmap (LizzieYZY feature port)
    if let Some(ownership) = &options.ownership {
        for row in 0..height {
            for column in 0..width {
                let idx = row * width + column;
                if let Some(&val) = ownership.get(idx)
                    && val.abs() >= 0.15
                {
                    let (x, y) = intersection_position(board, board_pixel_size, column, row);
                    let tile_size = (spacing * 0.72).clamp(10.0, 26.0);
                    let is_black = val > 0.0;
                    let alpha = (val.abs() as f32 * 0.45).clamp(0.12, 0.45);
                    let tile_bg = if is_black {
                        hsla(215.0, 0.85, 0.45, alpha)
                    } else {
                        hsla(0.0, 0.0, 1.0, alpha)
                    };
                    children.push(
                        div()
                            .absolute()
                            .left(px(x - tile_size / 2.0))
                            .top(px(y - tile_size / 2.0))
                            .size(px(tile_size))
                            .rounded(px(2.5))
                            .bg(tile_bg),
                    );
                }
            }
        }
    }

    for row in 0..height {
        for column in 0..width {
            let sign = board
                .sign_map
                .get(row)
                .and_then(|r| r.get(column))
                .copied()
                .unwrap_or(0);
            let color = match sign {
                1 => Some(Color::Black),
                -1 => Some(Color::White),
                _ => None,
            };
            let (x, y) = intersection_position(board, board_pixel_size, column, row);
            if let Some(color) = color {
                children.push(stone(color, x, y, stone_size, stone_black, stone_white));
                if options.show_move_numbers
                    && let Some(number) = options.move_numbers.get(&Vertex { column, row })
                {
                    children.push(move_number_div(
                        x,
                        y,
                        stone_size,
                        *number,
                        color == Color::Black,
                    ));
                }
            }
        }
    }

    // Ghost stones for sibling variations and child moves.
    if options.show_siblings {
        for variation in &board.siblings_info {
            if variation.vertex.column < width && variation.vertex.row < height {
                let (x, y) = intersection_position(
                    board,
                    board_pixel_size,
                    variation.vertex.column,
                    variation.vertex.row,
                );
                children.push(ghost_stone(
                    variation.color,
                    x,
                    y,
                    stone_size,
                    stone_black,
                    stone_white,
                    false,
                ));
            }
        }
    }
    if options.show_next_moves {
        for variation in &board.children_info {
            if variation.vertex.column < width && variation.vertex.row < height {
                let (x, y) = intersection_position(
                    board,
                    board_pixel_size,
                    variation.vertex.column,
                    variation.vertex.row,
                );
                children.push(ghost_stone(
                    variation.color,
                    x,
                    y,
                    stone_size,
                    stone_black,
                    stone_white,
                    true,
                ));
                if options.show_move_colorization
                    && let Some(annotation) = variation.annotation.as_deref()
                {
                    children.push(
                        div()
                            .absolute()
                            .left(px(x + 8.0))
                            .top(px(y - 7.0))
                            .text_xs()
                            .text_color(rgb(danger_color))
                            .child(annotation.to_owned()),
                    );
                }
            }
        }
    }

    if let (Some(vertex), Some(color)) = (options.hovered_vertex, options.hover_stone_color)
        && vertex.row < height
        && vertex.column < width
        && board
            .sign_map
            .get(vertex.row)
            .and_then(|r| r.get(vertex.column))
            == Some(&0)
    {
        let (x, y) = intersection_position(board, board_pixel_size, vertex.column, vertex.row);
        children.push(ghost_stone(
            color,
            x,
            y,
            stone_size,
            stone_black,
            stone_white,
            false,
        ));
    }

    // KaTrain-style AI analysis recommendation circles on empty intersections
    for candidate in &options.analysis_candidates {
        if candidate.vertex.row < height && candidate.vertex.column < width {
            if board
                .sign_map
                .get(candidate.vertex.row)
                .and_then(|r| r.get(candidate.vertex.column))
                != Some(&0)
            {
                continue;
            }
            let (x, y) = intersection_position(
                board,
                board_pixel_size,
                candidate.vertex.column,
                candidate.vertex.row,
            );
            let size = stone_size;
            let bg_color = if candidate.is_best {
                accent_color // best candidate move
            } else if candidate.winrate_percent >= 50.0 {
                success_color // good move
            } else if candidate.winrate_percent >= 40.0 {
                warn_color // inaccuracy
            } else {
                danger_color // mistake / blunder
            };

            let winrate_str = format!("{:.0}%", candidate.winrate_percent);
            let sub_str = if candidate.visits >= 1000 {
                format!("{:.1}k", candidate.visits as f64 / 1000.0)
            } else if let Some(lead) = candidate.score_lead {
                format!("{:+.1}", lead)
            } else {
                format!("{}v", candidate.visits)
            };

            let candidate_dot = div()
                .absolute()
                .left(px(x - size / 2.0))
                .top(px(y - size / 2.0))
                .size(px(size))
                .rounded_full()
                .border_2()
                // Design: the top candidate is a gold-ringed blue dot (金边蓝点);
                // other candidates keep a pale-blue ring.
                .border_color(rgb(if candidate.is_best {
                    0xf5c518
                } else {
                    0xc0d8f8
                }))
                .bg(rgb(bg_color))
                .shadow_md()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(0xffffff))
                        .line_height(px(size * 0.40))
                        .child(winrate_str),
                )
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(0xe0f2fe))
                        .line_height(px(size * 0.36))
                        .child(sub_str),
                );

            // The recommended (best) candidate breathes gently to draw the eye,
            // mirroring the design's pulsing top pick. Other candidates stay
            // static so the board doesn't shimmer.
            let dot_id = (
                "ai-candidate",
                (candidate.vertex.column * 64 + candidate.vertex.row),
            );
            let candidate_dot = if candidate.is_best {
                candidate_dot
                    .with_animation(
                        dot_id,
                        Animation::new(Duration::from_millis(1600))
                            .repeat()
                            .with_easing(pulsating_between(0.6, 1.0)),
                        |element, delta| element.opacity(delta),
                    )
                    .into_any_element()
            } else {
                candidate_dot.into_any_element()
            };
            children.push(div().child(candidate_dot));
        }
    }

    // KaTrain-style Move Quality Eval Dots on played stones. All five tiers
    // (Best/Good/Inaccuracy/Mistake/Blunder) render as a color dot; a Blunder
    // additionally carries a white "!" and pulses (design: 大恶手红色闪烁微标).
    for (vtx, quality) in &options.eval_dots {
        if vtx.row < height && vtx.column < width {
            let (x, y) = intersection_position(board, board_pixel_size, vtx.column, vtx.row);
            let dot_size = (spacing * 0.32).clamp(6.0, 12.0);
            let is_blunder = *quality == ryusei_host::MoveQuality::Blunder;
            let mut dot = div()
                .absolute()
                .left(px(x + spacing * 0.18))
                .top(px(y - spacing * 0.32))
                .size(px(dot_size))
                .rounded_full()
                .border_1()
                .border_color(rgb(0xffffff))
                .bg(rgb(quality.color_u32()))
                .flex()
                .items_center()
                .justify_center();
            if is_blunder {
                dot = dot.child(
                    div()
                        .text_color(rgb(0xffffff))
                        .font_weight(FontWeight::BOLD)
                        .line_height(px(dot_size))
                        .child("!"),
                );
            }
            let dot = if is_blunder {
                dot.with_animation(
                    ("blunder-pulse", (vtx.column * 64 + vtx.row)),
                    Animation::new(Duration::from_millis(1200))
                        .repeat()
                        .with_easing(pulsating_between(0.55, 1.0)),
                    |element, delta| element.opacity(delta),
                )
                .into_any_element()
            } else {
                dot.into_any_element()
            };
            children.push(div().child(dot));
        }
    }

    // Prospective PV variation sequence preview stones (rendered consistently with solid played stones)
    for (vtx, color, step_num) in &options.pv_preview {
        if vtx.row < height
            && vtx.column < width
            && board
                .sign_map
                .get(vtx.row)
                .and_then(|row| row.get(vtx.column))
                == Some(&0)
        {
            let (x, y) = intersection_position(board, board_pixel_size, vtx.column, vtx.row);
            // Solid stone matching real stones
            children.push(stone(*color, x, y, stone_size, stone_black, stone_white));
            // Move number overlaid clearly in contrasting color
            children.push(
                div()
                    .absolute()
                    .left(px(x - stone_size / 2.0))
                    .top(px(y - stone_size / 2.0))
                    .size(px(stone_size))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_xs()
                    .font_weight(FontWeight::BOLD)
                    .text_color(if *color == Color::Black {
                        rgb(0xffffff)
                    } else {
                        rgb(0x111114)
                    })
                    .child(step_num.to_string()),
            );
        }
    }

    // Last-move indicator on the current vertex.
    if let Some(current) = board.current_vertex
        && current.column < width
        && current.row < height
    {
        let (x, y) = intersection_position(board, board_pixel_size, current.column, current.row);
        let dot_size = (spacing * 0.35).clamp(8.0, 14.0);
        children.push(
            div()
                .absolute()
                .left(px(x - dot_size / 2.0))
                .top(px(y - dot_size / 2.0))
                .size(px(dot_size))
                .rounded_full()
                .border_2()
                .border_color(rgb(0xffffff))
                .bg(rgb(danger_color)),
        );
    }

    let marker_w = (spacing * 0.7).clamp(14.0, 28.0);
    let marker_h = (spacing * 0.8).clamp(16.0, 32.0);
    for row in 0..height {
        for column in 0..width {
            if let Some(marker) = board
                .markers
                .get(row)
                .and_then(|r| r.get(column))
                .and_then(|m| m.as_ref())
            {
                let (x, y) = intersection_position(board, board_pixel_size, column, row);
                children.push(
                    div()
                        .absolute()
                        .left(px(x - marker_w / 2.0))
                        .top(px(y - marker_h / 2.0))
                        .w(px(marker_w))
                        .h(px(marker_h))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_sm()
                        .text_color(rgb(danger_color))
                        .child(markup_symbol(&marker.marker_type, marker.label.as_deref())),
                );
            }
        }
    }

    // Scoring overrides: mark overridden vertices with a cross.
    for (vertex, override_value) in &options.score_overrides {
        if *override_value == 0 || vertex.column >= width || vertex.row >= height {
            continue;
        }
        let (x, y) = intersection_position(board, board_pixel_size, vertex.column, vertex.row);
        children.push(
            div()
                .absolute()
                .left(px(x - marker_w / 2.0))
                .top(px(y - marker_h / 2.0))
                .w(px(marker_w))
                .h(px(marker_h))
                .flex()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(rgb(danger_color))
                .child("×"),
        );
    }

    // Coordinates in all 4 margins (top, bottom, left, right).
    if options.show_coordinates {
        let numeric_coordinates = options.coordinates_type == "1-1";
        for column in 0..width {
            let (x, _) = intersection_position(board, board_pixel_size, column, 0);
            let letter = if numeric_coordinates {
                (column + 1).to_string()
            } else {
                column_letter(column).to_string()
            };
            // Top coordinate
            children.push(
                div()
                    .absolute()
                    .left(px(x - 8.0))
                    .top(px(BOARD_MARGIN_PX * 0.75 - 7.0))
                    .w(px(16.0))
                    .h(px(14.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(coordinate_color))
                    .child(letter.clone()),
            );
            // Bottom coordinate
            children.push(
                div()
                    .absolute()
                    .left(px(x - 8.0))
                    .top(px(board_size - BOARD_MARGIN_PX * 0.75 - 7.0))
                    .w(px(16.0))
                    .h(px(14.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(coordinate_color))
                    .child(letter),
            );
        }
        for row in 0..height {
            let (_, y) = intersection_position(board, board_pixel_size, 0, row);
            let number = if numeric_coordinates {
                (row + 1).to_string()
            } else {
                (height - row).to_string()
            };
            // Left coordinate
            children.push(
                div()
                    .absolute()
                    .left(px(BOARD_MARGIN_PX * 0.75 - 8.0))
                    .top(px(y - 7.0))
                    .w(px(16.0))
                    .h(px(14.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(coordinate_color))
                    .child(number.clone()),
            );
            // Right coordinate
            children.push(
                div()
                    .absolute()
                    .left(px(board_size - BOARD_MARGIN_PX * 0.75 - 8.0))
                    .top(px(y - 7.0))
                    .w(px(16.0))
                    .h(px(14.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(coordinate_color))
                    .child(number),
            );
        }
    }

    // For the kaya skin the surrounding container already paints the warm
    // gradient + border + shadow; painting the inner wood flat on top would
    // hide it, so let the gradient show through. Other skins keep their solid
    // board color.
    let skin = crate::theme::BoardSkin::from_theme(theme);
    let inner_wood = if skin.gradient.is_some() {
        hsla(0.0, 0.0, 0.0, 0.0).into()
    } else {
        rgb(wood_color)
    };

    // Kaya inset glow (design `inset 0 0 40px rgba(160,110,40,0.28)`): GPUI
    // has no inset box-shadow, so approximate it with a soft dark vignette ring
    // hugging the board edge. Painted only for the gradient (kaya) skin.
    let inset_glow: Option<Div> = skin.gradient.is_some().then(|| {
        div()
            .absolute()
            .left(px(BOARD_MARGIN_PX / 2.0))
            .top(px(BOARD_MARGIN_PX / 2.0))
            .size(px(board_size - BOARD_MARGIN_PX))
            .rounded(px(crate::theme::BOARD_RADIUS / 2.0))
            .border_1()
            .border_color(hsla(0.08, 0.55, 0.25, 0.28))
            .shadow(vec![gpui::BoxShadow {
                color: hsla(0.08, 0.55, 0.30, 0.22),
                offset: point(px(0.0), px(0.0)),
                blur_radius: px(18.0),
                spread_radius: px(-6.0),
            }])
    });

    div()
        .id(element_id)
        .debug_selector(move || element_id.to_owned())
        .relative()
        .size(px(board_size))
        .flex_none()
        .child(
            div()
                .debug_selector(move || format!("{element_id}-wood"))
                .absolute()
                .left(px(BOARD_MARGIN_PX / 2.0))
                .top(px(BOARD_MARGIN_PX / 2.0))
                .size(px(board_size - BOARD_MARGIN_PX))
                .bg(inner_wood),
        )
        .children(inset_glow)
        .children(children)
}

#[cfg(test)]
mod tests {
    use super::{
        GobanRenderOptions, arrowhead_vertices, board_spacing, column_letter,
        estimate_ownership_from_board, format_sgf_vertex, move_numbers_from_moves,
        parse_sgf_vertex, render_goban, vertex_at,
    };
    use gpui::px;
    use ryusei_domain_core::{BoardSnapshot, Color, MoveDto, Vertex};

    fn test_board(width: usize, height: usize) -> BoardSnapshot {
        ryusei_domain_core::GameDocument::new(width, height)
            .unwrap()
            .snapshot()
            .board
    }

    #[test]
    fn hit_tests_board_vertices_from_pixel_positions() {
        let board = test_board(19, 19);
        let board_size = 400.0;
        let spacing = board_spacing(&board, board_size);

        let origin = vertex_at(&board, board_size, gpui::Point::new(px(28.0), px(28.0)));
        assert_eq!(origin, Some(Vertex { column: 0, row: 0 }));

        let center = vertex_at(
            &board,
            board_size,
            gpui::Point::new(px(28.0 + 9.0 * spacing), px(28.0 + 9.0 * spacing)),
        );
        assert_eq!(center, Some(Vertex { column: 9, row: 9 }));

        let outside = vertex_at(&board, board_size, gpui::Point::new(px(5.0), px(5.0)));
        assert_eq!(outside, None);
    }

    #[test]
    fn hit_test_round_trips_with_intersection_geometry() {
        let board = test_board(9, 9);
        let board_size = 300.0;
        let spacing = board_spacing(&board, board_size);
        for column in 0..9 {
            for row in 0..9 {
                let position = gpui::Point::new(
                    px(28.0 + column as f32 * spacing),
                    px(28.0 + row as f32 * spacing),
                );
                assert_eq!(
                    vertex_at(&board, board_size, position),
                    Some(Vertex { column, row })
                );
            }
        }
    }

    #[test]
    fn renders_a_board_element_for_any_supported_size() {
        let board = test_board(19, 19);
        let options = GobanRenderOptions {
            show_coordinates: true,
            show_move_numbers: true,
            ..GobanRenderOptions::default()
        };
        let element = render_goban(
            &board,
            400.0,
            &crate::theme::ThemeTokens::default(),
            &options,
        );
        let _ = element;
    }

    #[test]
    fn arrowhead_wings_point_back_along_the_line() {
        // Horizontal line: wings sit behind the end, above and below it.
        let wings = arrowhead_vertices((0.0, 0.0), (10.0, 0.0), 6.0);
        assert!((wings[0].0 - 4.0).abs() < 1e-6);
        assert!((wings[0].1 - 3.0).abs() < 1e-6);
        assert!((wings[1].0 - 4.0).abs() < 1e-6);
        assert!((wings[1].1 + 3.0).abs() < 1e-6);

        // Degenerate zero-length line still yields finite wings.
        let degenerate = arrowhead_vertices((5.0, 5.0), (5.0, 5.0), 6.0);
        assert!(degenerate[0].0.is_finite());
    }

    #[test]
    fn column_letters_skip_i() {
        assert_eq!(column_letter(0), 'A');
        assert_eq!(column_letter(7), 'H');
        assert_eq!(column_letter(8), 'J');
        assert_eq!(column_letter(18), 'T');
    }

    #[test]
    fn pv_preview_points_are_numbered_and_alternate_colors() {
        let pv = vec![
            "D4".to_owned(),
            "Q16".to_owned(),
            "pass".to_owned(),
            "J10".to_owned(),
        ];
        let points = super::pv_preview_points(19, Color::Black, &pv, 8);
        assert_eq!(points.len(), 3);
        assert_eq!(points[0].0, Vertex { column: 3, row: 15 });
        assert_eq!(points[0].1, Color::Black);
        assert_eq!(points[0].2, 1);
        assert_eq!(points[1].1, Color::White);
        assert_eq!(points[1].2, 2);
        assert_eq!(points[2].1, Color::Black);
        assert_eq!(points[2].2, 3);
    }

    #[test]
    fn move_numbers_are_one_based_and_skip_passes() {
        let moves = vec![
            MoveDto {
                color: Color::Black,
                vertex: Some(Vertex { column: 3, row: 3 }),
            },
            MoveDto {
                color: Color::White,
                vertex: None,
            },
            MoveDto {
                color: Color::Black,
                vertex: Some(Vertex { column: 15, row: 3 }),
            },
        ];
        let numbers = move_numbers_from_moves(&moves);
        assert_eq!(numbers.get(&Vertex { column: 3, row: 3 }), Some(&1));
        assert_eq!(numbers.get(&Vertex { column: 15, row: 3 }), Some(&3));
        assert_eq!(numbers.len(), 2);
    }

    #[test]
    fn move_numbers_follow_the_selected_start_or_hotspot_anchor() {
        let snapshot =
            ryusei_domain_core::GameDocument::from_sgf("(;GM[1]FF[4]SZ[9];B[dd];W[ee]HO[];B[ff])")
                .unwrap()
                .snapshot();

        let from_start = super::move_numbers_for_snapshot(&snapshot, "start");
        assert_eq!(from_start.get(&Vertex { column: 3, row: 3 }), Some(&1));
        assert_eq!(from_start.get(&Vertex { column: 4, row: 4 }), Some(&2));
        assert_eq!(from_start.get(&Vertex { column: 5, row: 5 }), Some(&3));

        let from_hotspot = super::move_numbers_for_snapshot(&snapshot, "hotspot");
        assert_eq!(from_hotspot.len(), 1);
        assert_eq!(from_hotspot.get(&Vertex { column: 5, row: 5 }), Some(&1));
    }

    #[test]
    fn sgf_vertices_round_trip() {
        for vertex in [
            Vertex { column: 0, row: 0 },
            Vertex {
                column: 18,
                row: 18,
            },
        ] {
            let encoded = format_sgf_vertex(vertex);
            assert_eq!(parse_sgf_vertex(&encoded), Some(vertex));
        }
        assert_eq!(format_sgf_vertex(Vertex { column: 3, row: 3 }), "dd");
        assert_eq!(parse_sgf_vertex("dd"), Some(Vertex { column: 3, row: 3 }));
        assert_eq!(parse_sgf_vertex("DD"), None);
        assert_eq!(parse_sgf_vertex("d"), None);
        assert_eq!(parse_sgf_vertex("ddd"), None);
    }

    #[test]
    fn estimate_ownership_marks_black_and_white_influence() {
        let mut board = test_board(19, 19);
        // A single black stone influences nearby empty points positively.
        board.sign_map[9][9] = 1;
        let ownership = estimate_ownership_from_board(&board).expect("non-empty board");
        assert_eq!(ownership.len(), 19 * 19);
        let idx = |column: usize, row: usize| row * 19 + column;
        assert_eq!(ownership[idx(9, 9)], 1.0);
        assert!(ownership[idx(9, 8)] > 0.0, "adjacent point leans black");

        // A single white stone flips the same empty point negative.
        board.sign_map[9][9] = -1;
        let ownership = estimate_ownership_from_board(&board).expect("non-empty board");
        assert_eq!(ownership[idx(9, 9)], -1.0);
        assert!(ownership[idx(9, 8)] < 0.0, "adjacent point leans white");

        // An empty board produces a neutral (all zero) ownership map.
        let empty = test_board(19, 19);
        let ownership = estimate_ownership_from_board(&empty).expect("non-empty board");
        assert!(ownership.iter().all(|value| *value == 0.0));
    }
}
