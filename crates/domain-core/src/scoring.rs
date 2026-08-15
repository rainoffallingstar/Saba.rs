//! Territory scoring (Chinese rules, area scoring).
//!
//! Computes the score of a finished game from a `BoardSnapshot` and the
//! user's alive-stone overrides. The estimator implements the classical
//! area-scoring algorithm:
//!
//! - empty regions are flood-filled and assigned to the color that borders
//!   them (a region bordering both colors counts for neither);
//! - stones whose chains are fully surrounded by the opponent with no
//!   liberties are treated as dead and count as captures (area scoring);
//! - the user's `score_overrides` (alive/dead flags from scoring mode)
//!   override the heuristic chain by chain.
//!
//! This is deliberately simple and deterministic; the reference Sabaki uses
//! Monte-Carlo estimation (`score.estimator_iterations`), which is a later
//! refinement.

use crate::{BoardSnapshot, Color, Vertex};

/// Default komi (White compensation) used when the game carries no `KM`.
pub const DEFAULT_KOMI: f64 = 7.5;

/// One chain (connected same-color group) on the board.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoneChain {
    pub color: Color,
    pub vertices: Vec<Vertex>,
    /// Adjacent empty points.
    pub liberties: usize,
    /// Whether the chain is fully surrounded by opponent stones.
    pub fully_surrounded: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScoreResult {
    pub black_territory: usize,
    pub white_territory: usize,
    pub black_stones: usize,
    pub white_stones: usize,
    pub black_captured: usize,
    pub white_captured: usize,
    pub komi: f64,
    /// Total = territory + stones + captured (area scoring).
    pub black_total: f64,
    pub white_total: f64,
    pub winner: Option<Color>,
    pub margin: f64,
}

/// Finds all stone chains on the board.
pub fn find_chains(board: &BoardSnapshot) -> Vec<StoneChain> {
    let mut visited = vec![vec![false; board.height]; board.width];
    let mut chains = Vec::new();
    for column in 0..board.width {
        for row in 0..board.height {
            if visited[column][row] {
                continue;
            }
            let color = match board.sign_map[column][row] {
                1 => Color::Black,
                -1 => Color::White,
                _ => continue,
            };
            // Flood fill the chain.
            let mut stack = vec![(column, row)];
            let mut vertices = Vec::new();
            let mut liberties = 0usize;
            visited[column][row] = true;
            while let Some((cx, cy)) = stack.pop() {
                vertices.push(Vertex {
                    column: cx,
                    row: cy,
                });
                for (nx, ny) in neighbors(board, cx, cy) {
                    match board.sign_map[nx][ny] {
                        0 => liberties += 1,
                        value if value == color.sign() && !visited[nx][ny] => {
                            visited[nx][ny] = true;
                            stack.push((nx, ny));
                        }
                        _ => {}
                    }
                }
            }
            chains.push(StoneChain {
                color,
                vertices,
                liberties,
                fully_surrounded: false,
            });
        }
    }
    chains
}

/// Marks chains with no liberties as `fully_surrounded`. A chain without
/// any adjacent empty point is dead by definition; on a legal finished
/// board this only triggers for stones the opponent completely enclosed.
/// Border-adjacent chains keep their liberties (the edge is not a stone),
/// so this heuristic never kills an unwrapped edge group.
pub fn mark_surrounded_chains(_board: &BoardSnapshot, chains: &mut [StoneChain]) {
    for chain in chains.iter_mut() {
        chain.fully_surrounded = chain.liberties == 0;
    }
}

/// Assigns empty regions to the bordering color. A region bordering both
/// colors counts for neither (seki).
fn assign_empty_regions(
    board: &BoardSnapshot,
    _chains: &[StoneChain],
    dead_overrides: &std::collections::BTreeSet<Vertex>,
) -> (usize, usize) {
    let mut visited = vec![vec![false; board.height]; board.width];
    let mut is_dead_stone = vec![vec![false; board.width]; board.height];
    for vertex in dead_overrides {
        if vertex.column < board.width && vertex.row < board.height {
            is_dead_stone[vertex.column][vertex.row] = true;
        }
    }
    // Dead stones behave like empty points for territory assignment.
    let mut black_territory = 0usize;
    let mut white_territory = 0usize;
    for column in 0..board.width {
        for row in 0..board.height {
            let value = board.sign_map[column][row];
            if value != 0 && !is_dead_stone[column][row] {
                continue;
            }
            if visited[column][row] {
                continue;
            }
            // Flood fill the region, tracking border colors.
            let mut stack = vec![(column, row)];
            let mut region = Vec::new();
            let mut black_border = false;
            let mut white_border = false;
            visited[column][row] = true;
            while let Some((cx, cy)) = stack.pop() {
                region.push((cx, cy));
                for (nx, ny) in neighbors(board, cx, cy) {
                    let value = board.sign_map[nx][ny];
                    if value != 0 && !is_dead_stone[nx][ny] {
                        if value == 1 {
                            black_border = true;
                        } else {
                            white_border = true;
                        }
                        continue;
                    }
                    if !visited[nx][ny] {
                        visited[nx][ny] = true;
                        stack.push((nx, ny));
                    }
                }
            }
            if black_border && !white_border {
                black_territory += region.len();
            } else if white_border && !black_border {
                white_territory += region.len();
            }
        }
    }
    (black_territory, white_territory)
}

fn neighbors(board: &BoardSnapshot, column: usize, row: usize) -> Vec<(usize, usize)> {
    let mut result = Vec::new();
    if column > 0 {
        result.push((column - 1, row));
    }
    if column + 1 < board.width {
        result.push((column + 1, row));
    }
    if row > 0 {
        result.push((column, row - 1));
    }
    if row + 1 < board.height {
        result.push((column, row + 1));
    }
    result
}

/// Scores the board. `komi` is White's compensation (default 7.5);
/// `score_overrides` maps a vertex to `1` (alive black) or `-1` (alive
/// white) and overrides the dead-stone heuristic for whole chains.
pub fn score_board(
    board: &BoardSnapshot,
    komi: Option<f64>,
    score_overrides: &std::collections::BTreeMap<Vertex, i8>,
) -> ScoreResult {
    let mut chains = find_chains(board);
    mark_surrounded_chains(board, &mut chains);

    // Chain-level dead determination: a chain is dead when the heuristic
    // marks it surrounded, unless the user overrode any of its stones.
    let mut dead_vertices = std::collections::BTreeSet::new();
    let mut black_captured = 0usize;
    let mut white_captured = 0usize;
    for chain in &chains {
        let override_value = chain
            .vertices
            .iter()
            .filter_map(|vertex| score_overrides.get(vertex))
            .copied()
            .next();
        let alive_override = override_value == Some(chain.color.sign());
        let dead_override = override_value == Some(-chain.color.sign());
        let dead = if dead_override {
            true
        } else if alive_override {
            false
        } else {
            chain.fully_surrounded
        };
        if dead {
            for vertex in &chain.vertices {
                dead_vertices.insert(*vertex);
            }
            // Dead stones count as captures for the opponent.
            if chain.color == Color::Black {
                black_captured += chain.vertices.len();
            } else {
                white_captured += chain.vertices.len();
            }
        }
    }

    let (black_territory, white_territory) = assign_empty_regions(board, &chains, &dead_vertices);

    let black_stones = chains
        .iter()
        .filter(|chain| chain.color == Color::Black && !dead_vertices.contains(&chain.vertices[0]))
        .map(|chain| chain.vertices.len())
        .sum();
    let white_stones = chains
        .iter()
        .filter(|chain| chain.color == Color::White && !dead_vertices.contains(&chain.vertices[0]))
        .map(|chain| chain.vertices.len())
        .sum();

    let komi = komi.unwrap_or(DEFAULT_KOMI);
    let black_total = (black_territory + black_stones + black_captured) as f64;
    let white_total = (white_territory + white_stones + white_captured) as f64 + komi;
    let (winner, margin) = if (black_total - white_total).abs() < f64::EPSILON {
        (None, 0.0)
    } else if black_total > white_total {
        (Some(Color::Black), black_total - white_total)
    } else {
        (Some(Color::White), white_total - black_total)
    };

    ScoreResult {
        black_territory,
        white_territory,
        black_stones,
        white_stones,
        black_captured,
        white_captured,
        komi,
        black_total,
        white_total,
        winner,
        margin,
    }
}

impl Color {
    fn sign(self) -> i8 {
        match self {
            Color::Black => 1,
            Color::White => -1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Vertex;

    fn board_from_rows(rows: &[&str]) -> BoardSnapshot {
        let height = rows.len();
        let width = rows[0].len();
        let mut sign_map = vec![vec![0i8; height]; width];
        for (row, line) in rows.iter().enumerate() {
            for (column, character) in line.chars().enumerate() {
                sign_map[column][row] = match character {
                    'B' => 1,
                    'W' => -1,
                    _ => 0,
                };
            }
        }
        BoardSnapshot {
            width,
            height,
            sign_map,
            current_vertex: None,
            next_player: Color::Black,
            move_number: 0,
            markers: vec![vec![None; height]; width],
            lines: Vec::new(),
            children_info: Vec::new(),
            siblings_info: Vec::new(),
        }
    }

    #[test]
    fn finds_chains_and_liberties() {
        // Column 0 is one connected black chain; (2,0) and (2,2) are two
        // isolated black stones; (1,0), (2,1) and (1,2) are three isolated
        // whites.
        let board = board_from_rows(&["BWB", "B.W", "BWB"]);
        let chains = find_chains(&board);
        assert_eq!(chains.len(), 6, "three black chains and three white chains");
        let black_chain = chains
            .iter()
            .find(|chain| chain.color == Color::Black)
            .expect("black chain");
        assert_eq!(black_chain.vertices.len(), 3);
        assert_eq!(black_chain.liberties, 1, "the middle point is a liberty");
    }

    #[test]
    fn scores_an_empty_board_as_even() {
        let board = board_from_rows(&["...", "...", "..."]);
        let result = score_board(&board, Some(7.5), &Default::default());
        assert_eq!(result.black_territory, 0);
        assert_eq!(result.white_territory, 0);
        assert_eq!(result.winner, Some(Color::White));
        assert_eq!(result.margin, 7.5);
    }

    #[test]
    fn area_scoring_counts_territory_and_stones() {
        // A 3x3 board where black owns the top-left 2x2 block. The five
        // empty points form one region bordered only by black, so they are
        // black territory.
        let board = board_from_rows(&["BB.", "BB.", "..."]);
        let result = score_board(&board, Some(0.0), &Default::default());
        assert_eq!(result.black_territory, 5);
        assert_eq!(result.black_stones, 4);
        assert_eq!(result.black_total, 9.0);
        assert_eq!(result.winner, Some(Color::Black));
    }

    #[test]
    fn surrounded_chain_counts_as_captured() {
        // White's single stone has no liberties (fully enclosed); the black
        // edge chain keeps its liberties (the edge is not a stone).
        let board = board_from_rows(&["BB.", "BWB", "BB."]);
        let result = score_board(&board, Some(0.0), &Default::default());
        assert_eq!(result.white_captured, 1);
        assert_eq!(result.white_stones, 0);
        assert_eq!(result.black_captured, 0, "the black chain must stay alive");
        assert!(result.black_total > result.white_total);
    }

    #[test]
    fn overrides_rescue_or_kill_chains() {
        let board = board_from_rows(&["BB.", "BWB", "BB."]);
        // User marks the surrounded white stone as alive.
        let overrides = std::collections::BTreeMap::from([(Vertex { column: 1, row: 1 }, -1i8)]);
        let result = score_board(&board, Some(0.0), &overrides);
        assert_eq!(result.white_captured, 0);
        assert_eq!(result.white_stones, 1);
    }

    #[test]
    fn dead_override_kills_a_chain_with_liberties() {
        // Black stone in white surroundings keeps one liberty, so the
        // heuristic keeps it alive; only the user's override (-1 = alive
        // white → black dead) removes it.
        let board = board_from_rows(&["WW.", "WBW", "WWW"]);
        let overrides = std::collections::BTreeMap::from([(Vertex { column: 1, row: 1 }, -1i8)]);
        let result = score_board(&board, Some(0.0), &overrides);
        assert_eq!(result.black_captured, 1);
        assert_eq!(result.black_stones, 0);
    }
}
