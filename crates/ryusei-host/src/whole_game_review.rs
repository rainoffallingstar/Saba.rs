//! Whole-game AI review and blunder detection (ported from LizzieYZY).
//!
//! Provides batch whole-game analysis planning, move-by-move winrate drop
//! calculation, blunder classification, and structured inspection records.

use ryusei_domain_core::{Color, GameSnapshot, NodeId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum BlunderGrade {
    /// Severe winrate drop (>= 10.0% loss).
    Blunder,
    /// Inaccuracy / mistake (5.0% - 10.0% loss).
    Mistake,
    /// Minor inaccuracy / normal move (< 5.0% loss).
    Inaccuracy,
}

impl BlunderGrade {
    pub fn label(self) -> &'static str {
        match self {
            BlunderGrade::Blunder => "大恶手 (Blunder)",
            BlunderGrade::Mistake => "疑问手 (Mistake)",
            BlunderGrade::Inaccuracy => "次选点 (Inaccuracy)",
        }
    }

    pub fn badge(self) -> &'static str {
        match self {
            BlunderGrade::Blunder => "🔴",
            BlunderGrade::Mistake => "🟡",
            BlunderGrade::Inaccuracy => "⚪",
        }
    }
}

/// A detected problem move with before/after evaluations and AI recommendations.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BlunderEntry {
    pub move_number: usize,
    pub node_id: String,
    pub player: Color,
    pub played_vertex: Option<String>,
    pub winrate_before: f64,
    pub winrate_after: f64,
    pub winrate_drop: f64,
    pub score_lead_before: Option<f64>,
    pub score_lead_after: Option<f64>,
    pub score_loss: Option<f64>,
    pub recommended_move: Option<String>,
    pub recommended_pv: Vec<String>,
    pub grade: BlunderGrade,
}

/// The ordered engine evaluation fields used by whole-game review.
///
/// Kept as a tuple for the streaming caller, but named here so the public
/// review boundary remains readable and stable.
pub type ReviewedPosition = (
    usize,
    String,
    Color,
    Option<String>,
    f64,
    Option<f64>,
    Option<String>,
    Vec<String>,
);

/// Progress state for an active whole-game batch review run.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct BatchReviewProgress {
    pub current_move: usize,
    pub total_moves: usize,
    pub is_running: bool,
}

impl BatchReviewProgress {
    pub fn percent(self) -> f32 {
        if self.total_moves == 0 {
            0.0
        } else {
            (self.current_move as f32 / self.total_moves as f32) * 100.0
        }
    }
}

/// One move on the selected root-to-current lineage.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LineageMove {
    pub move_number: usize,
    pub node_id: NodeId,
    pub player: Color,
    pub played_vertex: Option<String>,
}

/// Returns the selected root-to-current node path, including the root baseline.
/// Whole-game review needs this pre-move position to evaluate the first move.
pub fn active_lineage_review_nodes(snapshot: &GameSnapshot) -> Vec<NodeId> {
    let mut reverse_path = Vec::new();
    let mut cursor = Some(snapshot.current_node_id.clone());
    let mut visited = std::collections::BTreeSet::new();
    while let Some(node_id) = cursor {
        if !visited.insert(node_id.clone()) {
            break;
        }
        let Some(node) = snapshot.nodes.iter().find(|node| node.id == node_id) else {
            break;
        };
        reverse_path.push(node.id.clone());
        cursor = node.parent_id.clone();
    }
    reverse_path.reverse();
    reverse_path
}

/// Extracts move nodes from the active root-to-current lineage.
///
/// The parent chain, rather than `snapshot.moves.len()` or move parity, is the
/// source of truth. This keeps batch review correct for variations, handicap
/// setup, and documents whose selected node is not the last node in the file.
pub fn active_lineage_moves(snapshot: &GameSnapshot) -> Vec<LineageMove> {
    let mut reverse_path = Vec::new();
    let mut cursor = Some(snapshot.current_node_id.clone());
    let mut visited = std::collections::BTreeSet::new();

    while let Some(node_id) = cursor {
        if !visited.insert(node_id.clone()) {
            break;
        }
        let Some(node) = snapshot.nodes.iter().find(|node| node.id == node_id) else {
            break;
        };
        reverse_path.push(node);
        cursor = node.parent_id.clone();
    }
    reverse_path.reverse();

    reverse_path
        .into_iter()
        .filter_map(|node| {
            let (player, property) = if let Some(value) = node.properties.get("B") {
                (Color::Black, value.first().cloned())
            } else {
                let value = node.properties.get("W")?;
                (Color::White, value.first().cloned())
            };
            Some(LineageMove {
                move_number: 0,
                node_id: node.id.clone(),
                player,
                played_vertex: property,
            })
        })
        .enumerate()
        .map(|(index, mut move_info)| {
            move_info.move_number = index + 1;
            move_info
        })
        .collect()
}

/// Computes the blunder list from a sequence of evaluated game positions.
pub fn find_blunders(
    evaluations: &[ReviewedPosition],
    blunder_threshold_pct: f64,
    mistake_threshold_pct: f64,
) -> Vec<BlunderEntry> {
    let mut blunders = Vec::new();
    if evaluations.len() < 2 {
        return blunders;
    }

    for index in 1..evaluations.len() {
        let (
            _prev_move,
            _prev_id,
            _prev_color,
            _prev_vtx,
            prev_winrate,
            prev_score,
            _prev_rec,
            _prev_pv,
        ) = &evaluations[index - 1];
        let (curr_move, curr_id, curr_color, curr_vtx, curr_winrate, curr_score, curr_rec, curr_pv) =
            &evaluations[index];

        // For Black move: winrate drop is prev_winrate - curr_winrate (from Black's perspective).
        // For White move: winrate drop is (1.0 - prev_winrate) - (1.0 - curr_winrate) = curr_winrate - prev_winrate.
        let drop = match curr_color {
            Color::Black => (prev_winrate - curr_winrate).max(0.0),
            Color::White => (curr_winrate - prev_winrate).max(0.0),
        };

        let drop_pct = drop * 100.0;
        let grade = if drop_pct >= blunder_threshold_pct {
            Some(BlunderGrade::Blunder)
        } else if drop_pct >= mistake_threshold_pct {
            Some(BlunderGrade::Mistake)
        } else {
            None
        };

        if let Some(grade) = grade {
            let score_loss = match (prev_score, curr_score) {
                (Some(p), Some(c)) => match curr_color {
                    Color::Black => Some((p - c).max(0.0)),
                    Color::White => Some((c - p).max(0.0)),
                },
                _ => None,
            };

            blunders.push(BlunderEntry {
                move_number: *curr_move,
                node_id: curr_id.clone(),
                player: *curr_color,
                played_vertex: curr_vtx.clone(),
                winrate_before: *prev_winrate,
                winrate_after: *curr_winrate,
                winrate_drop: drop,
                score_lead_before: *prev_score,
                score_lead_after: *curr_score,
                score_loss,
                recommended_move: curr_rec.clone(),
                recommended_pv: curr_pv.clone(),
                grade,
            });
        }
    }

    blunders
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_nodes_include_the_root_baseline() {
        use ryusei_domain_core::{
            BoardSnapshot, FileStateSnapshot, GameMode, GameSnapshot, HistorySnapshot, NodeSnapshot,
        };
        use std::collections::BTreeMap;
        let snapshot = GameSnapshot {
            schema_version: 1,
            revision: 0,
            root_properties: BTreeMap::new(),
            nodes: vec![
                NodeSnapshot {
                    id: "root".to_owned(),
                    parent_id: None,
                    child_ids: vec!["b1".to_owned()],
                    properties: BTreeMap::new(),
                },
                NodeSnapshot {
                    id: "b1".to_owned(),
                    parent_id: Some("root".to_owned()),
                    child_ids: Vec::new(),
                    properties: BTreeMap::new(),
                },
            ],
            root_node_id: "root".to_owned(),
            current_node_id: "b1".to_owned(),
            preferred_child_by_node: BTreeMap::new(),
            moves: Vec::new(),
            board: BoardSnapshot {
                width: 19,
                height: 19,
                sign_map: vec![vec![0; 19]; 19],
                current_vertex: None,
                next_player: Color::Black,
                move_number: 0,
                markers: vec![vec![None; 19]; 19],
                lines: Vec::new(),
                children_info: Vec::new(),
                siblings_info: Vec::new(),
            },
            history: HistorySnapshot {
                can_undo: false,
                can_redo: false,
                undo_depth: 0,
                redo_depth: 0,
            },
            file_state: FileStateSnapshot {
                path: None,
                format: None,
                is_dirty: false,
            },
            mode: GameMode::Play,
            can_undo: false,
            can_redo: false,
            source_path: None,
            score_overrides: BTreeMap::new(),
            black_captures: 0,
            white_captures: 0,
        };
        assert_eq!(active_lineage_review_nodes(&snapshot), vec!["root", "b1"]);
    }

    #[test]
    fn active_lineage_moves_follow_parent_chain_and_ignore_siblings() {
        use ryusei_domain_core::{
            BoardSnapshot, FileStateSnapshot, GameMode, GameSnapshot, HistorySnapshot,
            NodeSnapshot, Properties,
        };
        use std::collections::BTreeMap;

        let node = |id: &str, parent_id: Option<&str>, properties: &[(&str, &str)]| NodeSnapshot {
            id: id.to_owned(),
            parent_id: parent_id.map(str::to_owned),
            child_ids: Vec::new(),
            properties: properties
                .iter()
                .map(|(key, value)| ((*key).to_owned(), vec![(*value).to_owned()]))
                .collect::<Properties>(),
        };
        let snapshot = GameSnapshot {
            schema_version: 1,
            revision: 0,
            root_properties: BTreeMap::new(),
            nodes: vec![
                node("root", None, &[]),
                node("b1", Some("root"), &[("B", "pd")]),
                node("w1", Some("b1"), &[("W", "dp")]),
                node("sibling", Some("b1"), &[("W", "qq")]),
            ],
            root_node_id: "root".to_owned(),
            current_node_id: "w1".to_owned(),
            preferred_child_by_node: BTreeMap::new(),
            moves: Vec::new(),
            board: BoardSnapshot {
                width: 19,
                height: 19,
                sign_map: vec![vec![0; 19]; 19],
                current_vertex: None,
                next_player: Color::Black,
                move_number: 0,
                markers: vec![vec![None; 19]; 19],
                lines: Vec::new(),
                children_info: Vec::new(),
                siblings_info: Vec::new(),
            },
            history: HistorySnapshot {
                can_undo: false,
                can_redo: false,
                undo_depth: 0,
                redo_depth: 0,
            },
            file_state: FileStateSnapshot {
                path: None,
                format: None,
                is_dirty: false,
            },
            mode: GameMode::Play,
            can_undo: false,
            can_redo: false,
            source_path: None,
            score_overrides: BTreeMap::new(),
            black_captures: 0,
            white_captures: 0,
        };
        let lineage = active_lineage_moves(&snapshot);
        assert_eq!(
            lineage
                .iter()
                .map(|entry| entry.node_id.as_str())
                .collect::<Vec<_>>(),
            vec!["b1", "w1"]
        );
        assert_eq!(lineage[0].move_number, 1);
        assert_eq!(lineage[0].player, Color::Black);
        assert_eq!(lineage[1].player, Color::White);
    }

    #[test]
    fn detects_blunders_and_mistakes_from_evaluations() {
        let evals = vec![
            (
                0,
                "root".to_owned(),
                Color::Black,
                None,
                0.50,
                Some(0.0),
                None,
                vec![],
            ),
            (
                1,
                "n1".to_owned(),
                Color::Black,
                Some("D4".to_owned()),
                0.52,
                Some(0.5),
                Some("D4".to_owned()),
                vec![],
            ),
            // White plays a blunder (winrate for Black jumps from 0.52 to 0.68 -> White loses 16%):
            (
                2,
                "n2".to_owned(),
                Color::White,
                Some("Q16".to_owned()),
                0.68,
                Some(4.0),
                Some("C16".to_owned()),
                vec!["C16".to_owned()],
            ),
            // Black plays a mistake (winrate for Black drops from 0.68 to 0.61 -> Black loses 7%):
            (
                3,
                "n3".to_owned(),
                Color::Black,
                Some("K10".to_owned()),
                0.61,
                Some(2.2),
                Some("R4".to_owned()),
                vec!["R4".to_owned()],
            ),
        ];

        let blunders = find_blunders(&evals, 10.0, 5.0);
        assert_eq!(blunders.len(), 2);

        assert_eq!(blunders[0].move_number, 2);
        assert_eq!(blunders[0].player, Color::White);
        assert_eq!(blunders[0].grade, BlunderGrade::Blunder);
        assert!((blunders[0].winrate_drop - 0.16).abs() < 1e-4);

        assert_eq!(blunders[1].move_number, 3);
        assert_eq!(blunders[1].player, Color::Black);
        assert_eq!(blunders[1].grade, BlunderGrade::Mistake);
        assert!((blunders[1].winrate_drop - 0.07).abs() < 1e-4);
    }
}
