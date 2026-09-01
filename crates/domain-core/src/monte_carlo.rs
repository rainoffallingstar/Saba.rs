//! Monte-Carlo life-and-death estimation for scoring.
//!
//! The deterministic scoring heuristic (`mark_surrounded_chains`) only kills
//! chains with literally zero liberties, so it misjudges seki, snapbacks and
//! groups that are dead but not yet fully enclosed. This module estimates each
//! chain's survival probability by playing the position out to a terminal
//! state many times with uniformly random legal moves and counting how often
//! the chain still occupies its points at the end.
//!
//! Randomness comes from a small deterministic LCG seeded from the board, so
//! results are reproducible (important for tests and for a stable UI readout)
//! and no external `rand` dependency is needed. This is a *light* estimator:
//! it improves on the zero-liberty heuristic but is not a substitute for a
//! full-strength engine's life-and-death analysis.

use crate::{BoardSnapshot, Color, Vertex};

/// Black = +1, White = −1, matching the board's sign cells.
fn color_sign(color: Color) -> i8 {
    match color {
        Color::Black => 1,
        Color::White => -1,
    }
}

/// A tiny deterministic PRNG (xorshift / LCG hybrid) seeded per playout so the
/// whole estimation is reproducible without a `rand` dependency.
struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        // Avoid the degenerate all-zero state.
        Self(seed | 0x9E37_79B9_7F4A_7C15)
    }

    fn next_u64(&mut self) -> u64 {
        // xorshift64*
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next_u64() % n as u64) as usize
        }
    }
}

/// A minimal mutable board for playouts, over `[column][row]` sign cells.
#[derive(Clone)]
struct PlayoutBoard {
    width: usize,
    height: usize,
    sign: Vec<Vec<i8>>,
}

impl PlayoutBoard {
    fn from_snapshot(board: &BoardSnapshot) -> Self {
        Self {
            width: board.width,
            height: board.height,
            sign: board.sign_map.clone(),
        }
    }

    fn get(&self, column: usize, row: usize) -> i8 {
        self.sign[column][row]
    }

    fn neighbours(&self, column: usize, row: usize) -> impl Iterator<Item = (usize, usize)> {
        let mut result = Vec::with_capacity(4);
        if column > 0 {
            result.push((column - 1, row));
        }
        if column + 1 < self.width {
            result.push((column + 1, row));
        }
        if row > 0 {
            result.push((column, row - 1));
        }
        if row + 1 < self.height {
            result.push((column, row + 1));
        }
        result.into_iter()
    }

    /// Returns the connected group containing `(column, row)` and its liberty
    /// count. Assumes the point is occupied.
    fn group_and_liberties(&self, column: usize, row: usize) -> (Vec<(usize, usize)>, usize) {
        let color = self.get(column, row);
        let mut visited = vec![vec![false; self.height]; self.width];
        let mut stack = vec![(column, row)];
        let mut group = Vec::new();
        let mut liberties = 0usize;
        visited[column][row] = true;
        while let Some((cx, cy)) = stack.pop() {
            group.push((cx, cy));
            for (nx, ny) in self.neighbours(cx, cy) {
                let value = self.get(nx, ny);
                if value == 0 {
                    liberties += 1;
                } else if value == color && !visited[nx][ny] {
                    visited[nx][ny] = true;
                    stack.push((nx, ny));
                }
            }
        }
        (group, liberties)
    }

    /// Plays a stone of `color` at `(column, row)`, capturing surrounded
    /// opponent groups. Returns `false` if the move is illegal (occupied,
    /// suicide). Does not enforce ko (uniform random playouts rarely need it
    /// and tracking history is costly).
    fn play(&mut self, color: i8, column: usize, row: usize) -> bool {
        if self.get(column, row) != 0 {
            return false;
        }
        self.sign[column][row] = color;
        let opponent = -color;
        // Capture opponent groups with no liberties.
        let neighbours: Vec<_> = self.neighbours(column, row).collect();
        for (nx, ny) in neighbours {
            if self.get(nx, ny) == opponent {
                let (group, liberties) = self.group_and_liberties(nx, ny);
                if liberties == 0 {
                    for (gx, gy) in group {
                        self.sign[gx][gy] = 0;
                    }
                }
            }
        }
        // Suicide check: if our own new group has no liberties, undo.
        let (_, liberties) = self.group_and_liberties(column, row);
        if liberties == 0 {
            self.sign[column][row] = 0;
            return false;
        }
        true
    }

    /// Fills a "safe" empty point: a point is safe to fill if it is not the
    /// only liberty of any friendly group (avoids self-atari on the last eye).
    fn playout(&mut self, rng: &mut Lcg, max_moves: usize) {
        let mut current: i8 = 1; // Black plays first in the playout.
        let mut passes = 0usize;
        let mut moves = 0usize;
        while passes < 2 && moves < max_moves {
            let empties = self.empty_points();
            if empties.is_empty() {
                break;
            }
            let &(column, row) = &empties[rng.below(empties.len())];
            if self.play(current, column, row) {
                passes = 0;
                moves += 1;
            } else {
                // Illegal (suicide/occupied): treat as a pass to avoid loops on
                // a board full of single-point eyes.
                passes += 1;
            }
            current = -current;
        }
    }

    fn empty_points(&self) -> Vec<(usize, usize)> {
        let mut points = Vec::new();
        for column in 0..self.width {
            for row in 0..self.height {
                if self.sign[column][row] == 0 {
                    points.push((column, row));
                }
            }
        }
        points
    }

    /// Whether any stone of `color_sign` still occupies `vertices` at the end.
    fn chain_survives(&self, vertices: &[Vertex], color_sign: i8) -> bool {
        vertices
            .iter()
            .any(|v| self.get(v.column, v.row) == color_sign)
    }
}

/// Estimates the survival probability of each chain by Monte-Carlo playout.
///
/// `chains` is `(color, vertices)` per chain (from `find_chains`). For every
/// chain we run `iterations` playouts from the current position and return the
/// fraction of playouts in which the chain still has a stone on its points at
/// the terminal board. A chain scoring below `death_threshold` is considered
/// dead. The result is deterministic for a given board and iteration count.
pub fn estimate_chain_survival(
    board: &BoardSnapshot,
    chains: &[(Color, Vec<Vertex>)],
    iterations: usize,
) -> Vec<f64> {
    if iterations == 0 || chains.is_empty() {
        return chains.iter().map(|_| 1.0).collect();
    }
    // Seed from the board so identical positions give identical estimates.
    let mut seed = 0xCBF2_9CE4_8422_2325u64;
    for column in 0..board.width {
        for row in 0..board.height {
            let value = board.sign_map[column][row] as u64;
            seed = seed
                .wrapping_mul(0x1000_0000_01B3)
                .wrapping_add(value.wrapping_add((column * 31 + row) as u64));
        }
    }

    let max_moves = (board.width * board.height).saturating_mul(3).max(64);
    chains
        .iter()
        .map(|(color, vertices)| {
            let color_sign = color_sign(*color);
            let mut survived = 0usize;
            for iteration in 0..iterations {
                let mut rng = Lcg::new(seed.wrapping_add(iteration as u64).wrapping_add(1));
                let mut playout = PlayoutBoard::from_snapshot(board);
                playout.playout(&mut rng, max_moves);
                if playout.chain_survives(vertices, color_sign) {
                    survived += 1;
                }
            }
            survived as f64 / iterations as f64
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Lcg, estimate_chain_survival};
    use crate::{BoardSnapshot, Color, Vertex};

    fn snapshot(width: usize, height: usize, stones: &[(usize, usize, i8)]) -> BoardSnapshot {
        let mut sign_map = vec![vec![0; height]; width];
        for &(column, row, value) in stones {
            sign_map[column][row] = value;
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
    fn lcg_is_deterministic_and_varies() {
        let mut a = Lcg::new(42);
        let mut b = Lcg::new(42);
        assert_eq!(a.next_u64(), b.next_u64());
        let mut values = std::collections::HashSet::new();
        for _ in 0..16 {
            values.insert(a.next_u64());
        }
        assert!(values.len() > 8);
    }

    #[test]
    fn estimation_is_deterministic_for_the_same_board() {
        let board = snapshot(5, 5, &[(1, 1, 1), (2, 2, -1), (3, 3, 1), (0, 4, -1)]);
        let chains = vec![
            (Color::Black, vec![Vertex { column: 1, row: 1 }]),
            (Color::White, vec![Vertex { column: 2, row: 2 }]),
        ];
        let first = estimate_chain_survival(&board, &chains, 32);
        let second = estimate_chain_survival(&board, &chains, 32);
        assert_eq!(first, second);
        assert_eq!(first.len(), 2);
    }

    #[test]
    fn zero_iterations_report_full_survival() {
        let board = snapshot(3, 3, &[(1, 1, 1)]);
        let chains = vec![(Color::Black, vec![Vertex { column: 1, row: 1 }])];
        assert_eq!(estimate_chain_survival(&board, &chains, 0), vec![1.0]);
    }
}
