use gpui::{Div, ParentElement, Pixels, Point, Styled, div, px, rgb};
use sabaki_domain_core::{BoardSnapshot, Color, Vertex};

use crate::markup::markup_symbol;
use crate::theme::ThemeTokens;

const BOARD_MARGIN_PX: f32 = 28.0;
const STONE_SIZE_PX: f32 = 22.0;

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

pub fn render_goban(board: &BoardSnapshot, board_pixel_size: f32, theme: &ThemeTokens) -> Div {
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
            }
        }
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

    div()
        .absolute()
        .left(px(BOARD_MARGIN_PX / 2.0))
        .top(px(BOARD_MARGIN_PX / 2.0))
        .size(px(board_size - BOARD_MARGIN_PX))
        .bg(rgb(wood_color))
        .child(div().relative().size(px(board_size)).children(children))
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

#[cfg(test)]
mod tests {
    use super::{board_spacing, render_goban, vertex_at};
    use gpui::px;
    use sabaki_domain_core::{BoardSnapshot, Vertex};

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
        let element = render_goban(&board, 400.0, &crate::theme::ThemeTokens::default());
        let _ = element;
    }
}
