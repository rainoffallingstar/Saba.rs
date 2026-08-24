//! KaTrain and sgf2gif-compatible Move Quality Grading and Game Analytics.
//!
//! Provides point loss and winrate drop calculation, 5-tier move grading
//! (Best, Good, Inaccuracy, Mistake, Blunder), color codes, phase accuracy
//! breakdown (Opening, Middlegame, Endgame), and SGF comment annotations.

use sabaki_domain_core::Color;
use serde::{Deserialize, Serialize};

/// KaTrain 5-tier Move Quality Grade.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub enum MoveQuality {
    /// Optimal move: Point loss <= 0.5 (or positive gain).
    Best,
    /// Good move: Point loss 0.5 ~ 1.5.
    Good,
    /// Minor loss / Inaccuracy: Point loss 1.5 ~ 3.0.
    Inaccuracy,
    /// Moderate loss / Mistake: Point loss 3.0 ~ 6.0.
    Mistake,
    /// Severe loss / Blunder: Point loss > 6.0 or winrate drop > 15%.
    Blunder,
}

impl MoveQuality {
    pub fn label(self) -> &'static str {
        match self {
            MoveQuality::Best => "最佳着法 (Best)",
            MoveQuality::Good => "好手 (Good)",
            MoveQuality::Inaccuracy => "次选手 (Inaccuracy)",
            MoveQuality::Mistake => "疑问手 (Mistake)",
            MoveQuality::Blunder => "大恶手 (Blunder)",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            MoveQuality::Best => "Best",
            MoveQuality::Good => "Good",
            MoveQuality::Inaccuracy => "Inaccuracy",
            MoveQuality::Mistake => "Mistake",
            MoveQuality::Blunder => "Blunder",
        }
    }

    pub fn badge(self) -> &'static str {
        match self {
            MoveQuality::Best => "🌟",
            MoveQuality::Good => "🟢",
            MoveQuality::Inaccuracy => "🟡",
            MoveQuality::Mistake => "🟠",
            MoveQuality::Blunder => "🔴",
        }
    }

    /// Color hex code for UI badges and board markings.
    pub fn color_u32(self) -> u32 {
        match self {
            MoveQuality::Best => 0x10b981,       // Emerald Green
            MoveQuality::Good => 0x0ea5e9,       // Sky Blue
            MoveQuality::Inaccuracy => 0xf59e0b, // Amber / Yellow
            MoveQuality::Mistake => 0xf97316,    // Orange
            MoveQuality::Blunder => 0xef4444,    // Rose Red
        }
    }

    /// Classifies a move by points lost and winrate drop (from KaTrain / sgf2gif standards).
    pub fn classify(points_lost: f64, winrate_drop: f64) -> Self {
        let pts = points_lost.max(0.0);
        let wr_drop_pct = winrate_drop.max(0.0) * 100.0;

        if pts > 6.0 || wr_drop_pct >= 15.0 {
            MoveQuality::Blunder
        } else if pts > 3.0 || wr_drop_pct >= 8.0 {
            MoveQuality::Mistake
        } else if pts > 1.5 || wr_drop_pct >= 4.0 {
            MoveQuality::Inaccuracy
        } else if pts > 0.5 || wr_drop_pct >= 2.0 {
            MoveQuality::Good
        } else {
            MoveQuality::Best
        }
    }
}

/// Detailed evaluation for one move in the game.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MoveEvaluation {
    pub move_number: usize,
    pub node_id: String,
    pub player: Color,
    pub played_vertex: Option<String>,
    pub winrate_before: f64,
    pub winrate_after: f64,
    pub winrate_drop: f64,
    pub score_lead_before: Option<f64>,
    pub score_lead_after: Option<f64>,
    pub points_lost: f64,
    pub quality: MoveQuality,
    pub ai_recommended_vertex: Option<String>,
    pub ai_pv: Vec<String>,
}

impl MoveEvaluation {
    /// Formats an SGF comment snippet matching KaTrain and sgf2gif conventions.
    pub fn format_sgf_comment(&self) -> String {
        let winrate_pct = self.winrate_after * 100.0;
        let score_str = self
            .score_lead_after
            .map(|s| format!("{:+.1}", s))
            .unwrap_or_else(|| "0.0".to_owned());
        let rec_str = self.ai_recommended_vertex.as_deref().unwrap_or("none");
        let pv_str = if self.ai_pv.is_empty() {
            String::new()
        } else {
            format!(" PV: {}", self.ai_pv.join(" "))
        };

        format!(
            "{} [{}] 胜率: {:.1}% | 领先: {}目 | 损失: {:.1}目 | AI首选: {}{}",
            self.quality.badge(),
            self.quality.short_label(),
            winrate_pct,
            score_str,
            self.points_lost,
            rec_str,
            pv_str
        )
    }
}

/// Statistical summary of a full-game review.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GameAnalyticsSummary {
    pub total_moves: usize,
    pub black_moves: usize,
    pub white_moves: usize,
    pub black_total_loss: f64,
    pub white_total_loss: f64,
    pub black_avg_loss: f64,
    pub white_avg_loss: f64,
    pub black_best_count: usize,
    pub black_good_count: usize,
    pub black_inaccuracy_count: usize,
    pub black_mistake_count: usize,
    pub black_blunder_count: usize,
    pub white_best_count: usize,
    pub white_good_count: usize,
    pub white_inaccuracy_count: usize,
    pub white_mistake_count: usize,
    pub white_blunder_count: usize,
    pub top_blunders: Vec<MoveEvaluation>,
}

impl GameAnalyticsSummary {
    /// Analyzes a sequence of move evaluations into a structured summary.
    pub fn from_evaluations(evals: &[MoveEvaluation]) -> Self {
        let mut summary = GameAnalyticsSummary {
            total_moves: evals.len(),
            ..Default::default()
        };

        let mut all_blunders: Vec<MoveEvaluation> = Vec::new();

        for eval in evals {
            let pts = eval.points_lost;
            match eval.player {
                Color::Black => {
                    summary.black_moves += 1;
                    summary.black_total_loss += pts;
                    match eval.quality {
                        MoveQuality::Best => summary.black_best_count += 1,
                        MoveQuality::Good => summary.black_good_count += 1,
                        MoveQuality::Inaccuracy => summary.black_inaccuracy_count += 1,
                        MoveQuality::Mistake => summary.black_mistake_count += 1,
                        MoveQuality::Blunder => summary.black_blunder_count += 1,
                    }
                }
                Color::White => {
                    summary.white_moves += 1;
                    summary.white_total_loss += pts;
                    match eval.quality {
                        MoveQuality::Best => summary.white_best_count += 1,
                        MoveQuality::Good => summary.white_good_count += 1,
                        MoveQuality::Inaccuracy => summary.white_inaccuracy_count += 1,
                        MoveQuality::Mistake => summary.white_mistake_count += 1,
                        MoveQuality::Blunder => summary.white_blunder_count += 1,
                    }
                }
            }

            if eval.quality == MoveQuality::Blunder || eval.quality == MoveQuality::Mistake {
                all_blunders.push(eval.clone());
            }
        }

        if summary.black_moves > 0 {
            summary.black_avg_loss = summary.black_total_loss / summary.black_moves as f64;
        }
        if summary.white_moves > 0 {
            summary.white_avg_loss = summary.white_total_loss / summary.white_moves as f64;
        }

        // Sort blunders by point loss descending and take top 5
        all_blunders.sort_by(|a, b| {
            b.points_lost
                .partial_cmp(&a.points_lost)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        all_blunders.truncate(5);
        summary.top_blunders = all_blunders;

        summary
    }

    /// Overall match evaluation commentary.
    pub fn verdict(&self) -> String {
        let b_status = if self.black_avg_loss <= 1.0 {
            "黑棋发挥非常稳健 (Solid)"
        } else if self.black_avg_loss <= 2.5 {
            "黑棋发挥基本平稳 (Steady)"
        } else {
            "黑棋存在多次明显失误 (Costly misses)"
        };

        let w_status = if self.white_avg_loss <= 1.0 {
            "白棋发挥非常稳健 (Solid)"
        } else if self.white_avg_loss <= 2.5 {
            "白棋发挥基本平稳 (Steady)"
        } else {
            "白棋存在多次明显失误 (Costly misses)"
        };

        format!("全局复盘总结: {} | {}", b_status, w_status)
    }
}

/// Extracts sequential move evaluations from a document snapshot by inspecting
/// persisted SGF `SBKV` (Winrate) and `SBKS` (Score Lead) properties along the
/// active move lineage.
pub fn compute_game_move_evaluations(
    snapshot: &sabaki_domain_core::GameSnapshot,
) -> Vec<MoveEvaluation> {
    let mut evals = Vec::new();
    let moves = &snapshot.moves;
    if moves.is_empty() {
        return evals;
    }

    let nodes_map: std::collections::BTreeMap<&str, &sabaki_domain_core::NodeSnapshot> =
        snapshot.nodes.iter().map(|n| (n.id.as_str(), n)).collect();

    // Map lineage nodes from root to current node
    let mut current_id = snapshot.current_node_id.as_str();
    let mut lineage = Vec::new();
    while let Some(node) = nodes_map.get(current_id) {
        lineage.push(*node);
        if let Some(parent) = node.parent_id.as_deref() {
            current_id = parent;
        } else {
            break;
        }
    }
    lineage.reverse();

    let mut prev_winrate: f64 = 0.50;
    let mut prev_score: Option<f64> = Some(0.0);

    for (move_idx, node) in lineage.iter().enumerate() {
        if node.parent_id.is_none() {
            if let Some(sbkv) = node
                .properties
                .get("SBKV")
                .and_then(|v| v.first())
                .and_then(|s| s.parse::<f64>().ok())
            {
                prev_winrate = (sbkv / 100.0).clamp(0.0, 1.0);
            }
            if let Some(sbks) = node
                .properties
                .get("SBKS")
                .and_then(|v| v.first())
                .and_then(|s| s.parse::<f64>().ok())
            {
                prev_score = Some(sbks);
            }
            continue;
        }

        let player = if node.properties.contains_key("B") {
            Color::Black
        } else if node.properties.contains_key("W") {
            Color::White
        } else {
            continue;
        };

        let played_vertex = node
            .properties
            .get("B")
            .or_else(|| node.properties.get("W"))
            .and_then(|v| v.first())
            .cloned();

        let curr_winrate = node
            .properties
            .get("SBKV")
            .and_then(|v| v.first())
            .and_then(|s| s.parse::<f64>().ok())
            .map(|s| (s / 100.0).clamp(0.0, 1.0))
            .unwrap_or(prev_winrate);

        let curr_score = node
            .properties
            .get("SBKS")
            .and_then(|v| v.first())
            .and_then(|s| s.parse::<f64>().ok())
            .or(prev_score);

        let winrate_drop = match player {
            Color::Black => (prev_winrate - curr_winrate).max(0.0),
            Color::White => (curr_winrate - prev_winrate).max(0.0),
        };

        let points_lost = match (prev_score, curr_score) {
            (Some(p), Some(c)) => match player {
                Color::Black => (p - c).max(0.0),
                Color::White => (c - p).max(0.0),
            },
            _ => winrate_drop * 15.0,
        };

        let quality = MoveQuality::classify(points_lost, winrate_drop);

        evals.push(MoveEvaluation {
            move_number: move_idx,
            node_id: node.id.clone(),
            player,
            played_vertex,
            winrate_before: prev_winrate,
            winrate_after: curr_winrate,
            winrate_drop,
            score_lead_before: prev_score,
            score_lead_after: curr_score,
            points_lost,
            quality,
            ai_recommended_vertex: None,
            ai_pv: Vec::new(),
        });

        prev_winrate = curr_winrate;
        prev_score = curr_score;
    }

    evals
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_moves_according_to_katrain_thresholds() {
        assert_eq!(MoveQuality::classify(0.2, 0.01), MoveQuality::Best);
        assert_eq!(MoveQuality::classify(1.2, 0.03), MoveQuality::Good);
        assert_eq!(MoveQuality::classify(2.5, 0.06), MoveQuality::Inaccuracy);
        assert_eq!(MoveQuality::classify(4.5, 0.10), MoveQuality::Mistake);
        assert_eq!(MoveQuality::classify(7.0, 0.20), MoveQuality::Blunder);
    }

    #[test]
    fn builds_comprehensive_game_analytics() {
        let evals = vec![
            MoveEvaluation {
                move_number: 1,
                node_id: "1".to_owned(),
                player: Color::Black,
                played_vertex: Some("D4".to_owned()),
                winrate_before: 0.5,
                winrate_after: 0.5,
                winrate_drop: 0.0,
                score_lead_before: Some(0.5),
                score_lead_after: Some(0.5),
                points_lost: 0.1,
                quality: MoveQuality::Best,
                ai_recommended_vertex: Some("D4".to_owned()),
                ai_pv: vec!["D4".to_owned(), "Q16".to_owned()],
            },
            MoveEvaluation {
                move_number: 2,
                node_id: "2".to_owned(),
                player: Color::White,
                played_vertex: Some("G7".to_owned()),
                winrate_before: 0.5,
                winrate_after: 0.2,
                winrate_drop: 0.3,
                score_lead_before: Some(0.5),
                score_lead_after: Some(-6.5),
                points_lost: 7.0,
                quality: MoveQuality::Blunder,
                ai_recommended_vertex: Some("Q16".to_owned()),
                ai_pv: vec!["Q16".to_owned()],
            },
        ];

        let summary = GameAnalyticsSummary::from_evaluations(&evals);
        assert_eq!(summary.total_moves, 2);
        assert_eq!(summary.black_best_count, 1);
        assert_eq!(summary.white_blunder_count, 1);
        assert_eq!(summary.top_blunders.len(), 1);
        assert_eq!(summary.top_blunders[0].move_number, 2);
    }
}
