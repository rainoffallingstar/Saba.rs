use std::{collections::BTreeMap, rc::Rc};

use gpui::{
    App, Div, InteractiveElement, MouseButton, MouseDownEvent, ParentElement, Pixels, Point,
    Stateful, Styled, Window, div, px, rgb,
};
use sabaki_domain_core::{BoardSnapshot, Color, MoveDto, Vertex};

use crate::markup::markup_symbol;
use crate::theme::ThemeTokens;

const BOARD_MARGIN_PX: f32 = 28.0;
const STONE_SIZE_PX: f32 = 22.0;

/// Rendering options derived from the shell settings and document state.
#[derive(Clone, Debug, Default)]
pub struct GobanRenderOptions {
    /// Draw A-T column and 1-19 row labels in the margins.
    pub show_coordinates: bool,
    /// Draw the move number on each stone.
    pub show_move_numbers: bool,
    /// Move number per vertex (1-based), built from the document's moves.
    pub move_numbers: BTreeMap<Vertex, usize>,
    /// Alive-stone overrides from `GameSnapshot.score_overrides`; overridden
    /// vertices get a cross marker.
    pub score_overrides: BTreeMap<Vertex, i8>,
}

/// Builds the 1-based move number per vertex from a document's move list.
/// `pass` moves (no vertex) are skipped.
pub fn move_numbers_from_moves(moves: &[MoveDto]) -> BTreeMap<Vertex, usize> {
    moves
        .iter()
        .enumerate()
        .filter_map(|(index, move_dto)| move_dto.vertex.map(|vertex| (vertex, index + 1)))
        .collect()
}

pub fn board_spacing(board: &BoardSnapshot, board_pixel_size: f32) -> f32 {
    (board_pixel_size - 2.0 * BOARD_MARGIN_PX) / (board.width.max(2) - 1) as f32
}

/// Converts a point relative to the goban element into a board vertex, or
/// `None` when the point falls outside the board.
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

fn stone(color: Color, x: f32, y: f32, stone_black: u32, stone_white: u32) -> Div {
    let stone_color = match color {
        Color::Black => rgb(stone_black),
        Color::White => rgb(stone_white),
    };
    div()
        .absolute()
        .left(px(x - STONE_SIZE_PX / 2.0))
        .top(px(y - STONE_SIZE_PX / 2.0))
        .size(px(STONE_SIZE_PX))
        .rounded_full()
        .border_1()
        .border_color(rgb(0x000000))
        .bg(stone_color)
}

/// Renders a move number centered on a stone, in the contrast color.
fn move_number_div(x: f32, y: f32, number: usize, on_black: bool) -> Div {
    div()
        .absolute()
        .left(px(x - 6.0))
        .top(px(y - 7.0))
        .w(px(12.0))
        .h(px(14.0))
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
pub fn render_goban_click_layer<F>(
    board: &BoardSnapshot,
    board_pixel_size: f32,
    on_vertex_clicked: Rc<F>,
) -> Div
where
    F: Fn(Vertex, &MouseDownEvent, &mut Window, &mut App) + 'static,
{
    let spacing = board_spacing(board, board_pixel_size);
    let mut layer = div().absolute().top_0().left_0().size(px(board_pixel_size));
    for row in 0..board.height {
        for column in 0..board.width {
            let vertex = Vertex { column, row };
            let handler = on_vertex_clicked.clone();
            let (x, y) = intersection_position(board, board_pixel_size, column, row);
            layer = layer.child(
                div()
                    .absolute()
                    .left(px(x - spacing / 2.0))
                    .top(px(y - spacing / 2.0))
                    .size(px(spacing))
                    .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                        handler(vertex, event, window, cx);
                    }),
            );
        }
    }
    layer
}

pub fn render_goban(
    board: &BoardSnapshot,
    board_pixel_size: f32,
    theme: &ThemeTokens,
    options: &GobanRenderOptions,
) -> Stateful<Div> {
    let width = board.width;
    let height = board.height;
    let board_size = board_pixel_size;
    let wood_color = theme.board_wood_color().rgb_u32();
    let line_color = theme.board_line_color().rgb_u32();
    let star_point_color = theme.star_point_color().rgb_u32();
    let stone_black = theme.stone_black_color().rgb_u32();
    let stone_white = theme.stone_white_color().rgb_u32();

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

    for (column, row) in star_point_vertices(width, height) {
        let (x, y) = intersection_position(board, board_pixel_size, column, row);
        children.push(
            div()
                .absolute()
                .left(px(x - 3.0))
                .top(px(y - 3.0))
                .size(px(6.0))
                .rounded_full()
                .bg(rgb(star_point_color)),
        );
    }

    // Board lines and arrows from the snapshot markup.
    for line in &board.lines {
        let start =
            intersection_position(board, board_pixel_size, line.start.column, line.start.row);
        let end = intersection_position(board, board_pixel_size, line.end.column, line.end.row);
        children.push(board_line(start, end, 2.0, 0xc0392b));
        if line.line_type == "arrow" {
            for wing in arrowhead_vertices(start, end, 9.0) {
                children.push(board_line(end, wing, 2.0, 0xc0392b));
            }
        }
    }

    for row in 0..height {
        for column in 0..width {
            let sign = board.sign_map[row][column];
            let color = match sign {
                1 => Some(Color::Black),
                -1 => Some(Color::White),
                _ => None,
            };
            let (x, y) = intersection_position(board, board_pixel_size, column, row);
            if let Some(color) = color {
                children.push(stone(color, x, y, stone_black, stone_white));
                if options.show_move_numbers {
                    if let Some(number) = options.move_numbers.get(&Vertex { column, row }) {
                        children.push(move_number_div(x, y, *number, color == Color::Black));
                    }
                }
            }
        }
    }

    // Last-move indicator on the current vertex.
    if let Some(current) = board.current_vertex {
        let (x, y) = intersection_position(board, board_pixel_size, current.column, current.row);
        children.push(
            div()
                .absolute()
                .left(px(x - 4.0))
                .top(px(y - 4.0))
                .size(px(8.0))
                .rounded_full()
                .bg(rgb(0xc0392b)),
        );
    }

    for row in 0..height {
        for column in 0..width {
            if let Some(marker) = &board.markers[row][column] {
                let (x, y) = intersection_position(board, board_pixel_size, column, row);
                children.push(
                    div()
                        .absolute()
                        .left(px(x - 9.0))
                        .top(px(y - 11.0))
                        .w(px(18.0))
                        .h(px(22.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_sm()
                        .text_color(rgb(0xc0392b))
                        .child(markup_symbol(&marker.marker_type, marker.label.as_deref())),
                );
            }
        }
    }

    // Scoring overrides: mark overridden vertices with a cross.
    for (vertex, override_value) in &options.score_overrides {
        if *override_value == 0 {
            continue;
        }
        let (x, y) = intersection_position(board, board_pixel_size, vertex.column, vertex.row);
        children.push(
            div()
                .absolute()
                .left(px(x - 8.0))
                .top(px(y - 10.0))
                .w(px(16.0))
                .h(px(20.0))
                .flex()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(rgb(0xc0392b))
                .child("×"),
        );
    }

    // Coordinates in the margins.
    if options.show_coordinates {
        for column in 0..width {
            let (x, _) = intersection_position(board, board_pixel_size, column, 0);
            children.push(
                div()
                    .absolute()
                    .left(px(x - 6.0))
                    .top(px(board_size - BOARD_MARGIN_PX + 6.0))
                    .w(px(12.0))
                    .h(px(14.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_xs()
                    .text_color(rgb(0x6b4f2a))
                    .child(column_letter(column).to_string()),
            );
        }
        for row in 0..height {
            let (_, y) = intersection_position(board, board_pixel_size, 0, row);
            children.push(
                div()
                    .absolute()
                    .left(px(BOARD_MARGIN_PX / 2.0 - 6.0))
                    .top(px(y - 7.0))
                    .w(px(12.0))
                    .h(px(14.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_xs()
                    .text_color(rgb(0x6b4f2a))
                    .child((row + 1).to_string()),
            );
        }
    }

    div()
        .id("goban")
        .debug_selector(|| "goban".to_owned())
        .relative()
        .size(px(board_size))
        .child(
            div()
                .debug_selector(|| "goban-wood".to_owned())
                .absolute()
                .left(px(BOARD_MARGIN_PX / 2.0))
                .top(px(BOARD_MARGIN_PX / 2.0))
                .size(px(board_size - BOARD_MARGIN_PX))
                .bg(rgb(wood_color)),
        )
        .children(children)
}

#[cfg(test)]
mod tests {
    use super::{
        GobanRenderOptions, arrowhead_vertices, board_spacing, column_letter, format_sgf_vertex,
        move_numbers_from_moves, parse_sgf_vertex, render_goban, vertex_at,
    };
    use gpui::px;
    use sabaki_domain_core::{BoardSnapshot, Color, MoveDto, Vertex};

    fn test_board(width: usize, height: usize) -> BoardSnapshot {
        sabaki_domain_core::GameDocument::new(width, height)
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
}
