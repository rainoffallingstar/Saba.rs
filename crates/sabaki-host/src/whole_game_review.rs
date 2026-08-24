//! Whole-game AI review and blunder detection (ported from LizzieYZY).
//!
//! Provides batch whole-game analysis planning, move-by-move winrate drop
//! calculation, blunder classification, and structured inspection records.

use sabaki_domain_core::Color;
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
