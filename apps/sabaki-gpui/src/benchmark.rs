use std::time::{Duration, Instant};

use sabaki_domain_core::{Color, GameDocument, GameSnapshot, Vertex};

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
        for index in 0..move_count {
            let color = if index % 2 == 0 {
                Color::Black
            } else {
                Color::White
            };
            let column = index % width;
            let row = (index / width) % height;
            game.play_move(color, Some(Vertex { column, row }))
                .expect("benchmark moves fill distinct vertices without self-capture");
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
    use super::SnapshotBenchmark;

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
}
