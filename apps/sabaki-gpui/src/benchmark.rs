use std::time::{Duration, Instant};

use sabaki_domain_core::{
    CURRENT_TRANSACTION_SCHEMA_VERSION, Color, GameDocument, GameSnapshot, GameTransaction,
    GameTransactionType, NodeId, Vertex,
};

/// Measures the cost of producing and copying full game snapshots, which is the
/// dominant per-mutation cost the GPUI client will consume. This baseline
/// exists to compare against the Electron/Tauri reference on the same machine.
pub struct SnapshotBenchmark {
    pub game: GameDocument,
    pub snapshot: GameSnapshot,
}

impl SnapshotBenchmark {
    pub fn new_with_moves(width: usize, height: usize, move_count: usize) -> Self {
        let mut game = GameDocument::new(width, height).expect("valid board size");
        let vertices = collision_free_vertices(width, height);
        for (index, vertex) in vertices.take(move_count).enumerate() {
            let color = if index % 2 == 0 {
                Color::Black
            } else {
                Color::White
            };
            let _ = game.play_move(color, Some(vertex));
        }
        let snapshot = game.snapshot();
        Self { game, snapshot }
    }

    pub fn run(self, iterations: usize) -> SnapshotBenchmarkResult {
        let start = Instant::now();
        let mut last = self.snapshot.clone();
        for _ in 0..iterations {
            last = self.game.snapshot();
            let _ = last.moves.len();
        }
        let total = start.elapsed();
        let per_snapshot = total / iterations as u32;
        SnapshotBenchmarkResult {
            board_width: self.snapshot.board.width,
            moves: self.snapshot.moves.len(),
            iterations,
            total,
            per_snapshot,
            last_move_count: last.moves.len(),
        }
    }
}

/// Builds a collision-free pseudo-random vertex traversal for a board, so
/// benchmarks can place many stones without tripping occupancy rules.
fn collision_free_vertices(width: usize, height: usize) -> impl Iterator<Item = Vertex> {
    let cell_count = width * height;
    let mut step = cell_count / 2 + 1;
    while gcd(step, cell_count) != 1 {
        step += 1;
    }
    (0..cell_count).map(move |index| {
        let position = (index * step) % cell_count;
        Vertex {
            column: position % width,
            row: position / width,
        }
    })
}

fn gcd(mut a: usize, mut b: usize) -> usize {
    while b != 0 {
        let remainder = a % b;
        a = b;
        b = remainder;
    }
    a
}

fn navigate_transaction(node_id: NodeId) -> GameTransaction {
    GameTransaction {
        schema_version: CURRENT_TRANSACTION_SCHEMA_VERSION,
        transaction_type: GameTransactionType::Navigate,
        color: None,
        vertex: None,
        node_id: Some(node_id),
        property: None,
        values: Vec::new(),
        marker: None,
        nodes: Vec::new(),
        score_override: None,
    }
}

/// Large-game scenarios: a long professional-style game and a branching
/// teaching game. These quantify open/navigation/snapshot cost on documents
/// at the scale real users open.
pub struct LargeGameBenchmark {
    pub game: GameDocument,
}

impl LargeGameBenchmark {
    /// A long main line (default: ~300 moves on 19x19).
    pub fn professional_game(width: usize, height: usize, moves: usize) -> Self {
        let mut game = GameDocument::new(width, height).expect("valid board size");
        let vertices = collision_free_vertices(width, height);
        for (index, vertex) in vertices.take(moves).enumerate() {
            let color = if index % 2 == 0 {
                Color::Black
            } else {
                Color::White
            };
            let _ = game.play_move(color, Some(vertex));
        }
        Self { game }
    }

    /// A main line with a side variation of `branch_length` moves hanging off
    /// every `branch_interval`-th main-line node.
    #[allow(dead_code)]
    pub fn teaching_game(
        width: usize,
        height: usize,
        main_moves: usize,
        branch_interval: usize,
        branch_length: usize,
    ) -> Self {
        let mut game = GameDocument::new(width, height).expect("valid board size");
        let mut vertices = collision_free_vertices(width, height);
        let mut branch_points = Vec::new();
        let mut placed = 0;
        for index in 0..main_moves {
            let Some(vertex) = vertices.next() else {
                break;
            };
            let color = if index % 2 == 0 {
                Color::Black
            } else {
                Color::White
            };
            if game.play_move(color, Some(vertex)).is_ok() {
                placed += 1;
                if placed % branch_interval == 0 && placed < main_moves {
                    branch_points.push(game.snapshot().current_node_id);
                }
            }
        }
        let main_line_end = game.snapshot().current_node_id;
        for point in branch_points {
            let _ = game.apply_transaction(navigate_transaction(point));
            for index in 0..branch_length {
                let Some(vertex) = vertices.next() else {
                    break;
                };
                let color = if index % 2 == 0 {
                    Color::Black
                } else {
                    Color::White
                };
                let _ = game.play_move(color, Some(vertex));
            }
        }
        let _ = game.apply_transaction(navigate_transaction(main_line_end));
        Self { game }
    }

    /// Runs the open/snapshot/navigate phases and returns the measurements.
    pub fn run(
        &self,
        snapshot_iterations: usize,
        navigation_iterations: usize,
    ) -> LargeGameBenchmarkResult {
        let moves = self.game.snapshot().moves.len();

        let open_start = Instant::now();
        let serialized = self.game.to_sgf();
        let reopened = GameDocument::from_sgf(&serialized).expect("benchmark SGF must reparse");
        let open_total = open_start.elapsed();
        let reopened_moves = reopened.snapshot().moves.len();

        let snapshot_start = Instant::now();
        for _ in 0..snapshot_iterations {
            let snapshot = self.game.snapshot();
            let _ = snapshot.moves.len();
        }
        let snapshot_total = snapshot_start.elapsed();
        let per_snapshot = snapshot_total / snapshot_iterations as u32;

        // Navigate from the root along the preferred line to the end, then back.
        let root_id = self.game.snapshot().root_node_id.clone();
        let end_id = self.game.snapshot().current_node_id.clone();
        let mut navigation_game = self.game.clone();
        let navigation_start = Instant::now();
        for _ in 0..navigation_iterations {
            let _ = navigation_game.apply_transaction(navigate_transaction(root_id.clone()));
            let _ = navigation_game.apply_transaction(navigate_transaction(end_id.clone()));
        }
        let navigation_total = navigation_start.elapsed();
        let per_navigation_round = navigation_total / (navigation_iterations * 2) as u32;

        LargeGameBenchmarkResult {
            board_width: self.game.snapshot().board.width,
            moves,
            reopened_moves,
            open_total,
            snapshot_iterations,
            snapshot_total,
            per_snapshot,
            navigation_iterations,
            navigation_total,
            per_navigation_round,
        }
    }
}

pub struct LargeGameBenchmarkResult {
    pub board_width: usize,
    pub moves: usize,
    pub reopened_moves: usize,
    pub open_total: Duration,
    pub snapshot_iterations: usize,
    pub snapshot_total: Duration,
    pub per_snapshot: Duration,
    pub navigation_iterations: usize,
    pub navigation_total: Duration,
    pub per_navigation_round: Duration,
}

impl LargeGameBenchmarkResult {
    pub fn summary(&self) -> String {
        format!(
            "large game: {}x{} {} moves (reopen {}), open {:?}, {} snapshots in {:?} ({:?}/snapshot), {} nav-rounds in {:?} ({:?}/nav-round)",
            self.board_width,
            self.board_width,
            self.moves,
            self.reopened_moves,
            self.open_total,
            self.snapshot_iterations,
            self.snapshot_total,
            self.per_snapshot,
            self.navigation_iterations,
            self.navigation_total,
            self.per_navigation_round
        )
    }
}

pub struct SnapshotBenchmarkResult {
    pub board_width: usize,
    pub moves: usize,
    pub iterations: usize,
    pub total: Duration,
    pub per_snapshot: Duration,
    pub last_move_count: usize,
}

impl SnapshotBenchmarkResult {
    pub fn summary(&self) -> String {
        format!(
            "{}x{} board, {} moves, {} snapshots in {:?} ({:?}/snapshot, last={})",
            self.board_width,
            self.board_width,
            self.moves,
            self.iterations,
            self.total,
            self.per_snapshot,
            self.last_move_count
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{LargeGameBenchmark, SnapshotBenchmark};

    #[test]
    fn benchmark_runs_and_reports_throughput() {
        let benchmark = SnapshotBenchmark::new_with_moves(19, 19, 50);
        let result = benchmark.run(1_000);

        assert_eq!(result.board_width, 19);
        assert_eq!(result.moves, 50);
        assert_eq!(result.iterations, 1_000);
        assert_eq!(result.last_move_count, 50);
        assert!(result.total.as_nanos() > 0);
        assert!(result.summary().contains("50 moves"));
        assert!(result.summary().contains("1000 snapshots"));
        println!("benchmark: {}", result.summary());
    }

    #[test]
    fn professional_game_builds_and_reopens_at_scale() {
        let benchmark = LargeGameBenchmark::professional_game(19, 19, 300);
        let snapshot = benchmark.game.snapshot();
        assert!(
            snapshot.moves.len() >= 250,
            "large games place most of the requested moves, got {}",
            snapshot.moves.len()
        );

        let result = benchmark.run(200, 200);
        assert_eq!(result.moves, snapshot.moves.len());
        assert_eq!(result.reopened_moves, snapshot.moves.len());
        assert!(result.open_total.as_nanos() > 0);
        assert!(result.per_snapshot.as_nanos() > 0);
        assert!(result.per_navigation_round.as_nanos() > 0);
        println!("large-game: {}", result.summary());
    }

    #[test]
    fn teaching_game_builds_branches_and_navigates() {
        let benchmark = LargeGameBenchmark::teaching_game(19, 19, 200, 50, 30);
        let snapshot = benchmark.game.snapshot();
        assert!(snapshot.moves.len() >= 150);
        assert!(
            snapshot.nodes.len() > snapshot.moves.len(),
            "branching adds nodes beyond the main line"
        );

        let result = benchmark.run(200, 200);
        assert_eq!(result.reopened_moves, snapshot.moves.len());
        assert!(result.per_navigation_round.as_nanos() > 0);
        println!("large-game teaching: {}", result.summary());
    }
}
