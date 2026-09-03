//! KaTrain and sgf2gif-compatible Move Quality Grading and Game Analytics.
//!
//! Provides point loss and winrate drop calculation, 5-tier move grading
//! (Best, Good, Inaccuracy, Mistake, Blunder), color codes, phase accuracy
//! breakdown (Opening, Middlegame, Endgame), and SGF comment annotations.

use ryusei_domain_core::Color;
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

/// Four stages of a Go game for phase-specific precision analysis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GamePhase {
    Overall, // 全盘 (1+)
    Opening, // 开局 (1..=50)
    Midgame, // 中盘 (51..=150)
    Endgame, // 官子 (151+)
}

impl GamePhase {
    pub const ALL: [GamePhase; 4] = [
        GamePhase::Overall,
        GamePhase::Opening,
        GamePhase::Midgame,
        GamePhase::Endgame,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            GamePhase::Overall => "全盘",
            GamePhase::Opening => "开局",
            GamePhase::Midgame => "中盘",
            GamePhase::Endgame => "官子",
        }
    }

    pub fn subtitle(&self) -> &'static str {
        match self {
            GamePhase::Overall => "全局",
            GamePhase::Opening => "1-50手",
            GamePhase::Midgame => "51-150手",
            GamePhase::Endgame => "151手+",
        }
    }

    pub fn includes_move(&self, move_num: usize) -> bool {
        match self {
            GamePhase::Overall => true,
            GamePhase::Opening => (1..=50).contains(&move_num),
            GamePhase::Midgame => (51..=150).contains(&move_num),
            GamePhase::Endgame => move_num > 150,
        }
    }
}

/// Estimated human rank confidence interval fitted against mainstream Go server benchmarks.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RankInterval {
    pub lower_bound: String,   // e.g. "业余 4D"
    pub upper_bound: String,   // e.g. "业余 6D"
    pub median: String,        // e.g. "业余 5D"
    pub confidence_score: f32, // 0.0 ..= 1.0 based on sample size and stability
}

/// Analytics breakdown for one phase and one player.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PhaseAnalytics {
    pub moves_count: usize,
    pub avg_loss: f64,
    pub top1_matches: usize,
    pub top3_matches: usize,
    pub top1_rate: f64, // 0.0 .. 100.0%
    pub top3_rate: f64, // 0.0 .. 100.0%
    pub best_count: usize,
    pub good_count: usize,
    pub inaccuracy_count: usize,
    pub mistake_count: usize,
    pub blunder_count: usize,
    pub blunder_rate_per_100: f64,
    pub rank_interval: Option<RankInterval>,
}

/// Comprehensive multi-phase game analytics report.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ComprehensiveGameAnalytics {
    pub total_moves: usize,
    pub overall_black: PhaseAnalytics,
    pub overall_white: PhaseAnalytics,
    pub opening_black: PhaseAnalytics,
    pub opening_white: PhaseAnalytics,
    pub midgame_black: PhaseAnalytics,
    pub midgame_white: PhaseAnalytics,
    pub endgame_black: PhaseAnalytics,
    pub endgame_white: PhaseAnalytics,
    pub top_blunders: Vec<MoveEvaluation>,
}

impl ComprehensiveGameAnalytics {
    pub fn for_phase(&self, phase: GamePhase, player: Color) -> &PhaseAnalytics {
        match (phase, player) {
            (GamePhase::Overall, Color::Black) => &self.overall_black,
            (GamePhase::Overall, Color::White) => &self.overall_white,
            (GamePhase::Opening, Color::Black) => &self.opening_black,
            (GamePhase::Opening, Color::White) => &self.opening_white,
            (GamePhase::Midgame, Color::Black) => &self.midgame_black,
            (GamePhase::Midgame, Color::White) => &self.midgame_white,
            (GamePhase::Endgame, Color::Black) => &self.endgame_black,
            (GamePhase::Endgame, Color::White) => &self.endgame_white,
        }
    }
}

/// Maps statistical metrics to an estimated human rank interval according to
/// mainstream Go server benchmark matrix:
/// | Segment | Avg Loss | Top 3 Rate | Blunders / 100 |
/// | K - 1D  | > 1.2    | < 50%      | >= 6           |
/// | 2D - 3D | 0.8-1.2  | 50%-65%    | 3 - 5          |
/// | 4D - 5D | 0.5-0.8  | 65%-75%    | 1 - 3          |
/// | 6D - 7D | 0.3-0.5  | 75%-82%    | < 1            |
/// | Pro/AI  | < 0.25   | > 85%      | rare (0-0.3)   |
pub fn estimate_rank_interval(
    avg_loss: f64,
    top3_rate: f64,
    blunder_rate_per_100: f64,
    moves_count: usize,
) -> Option<RankInterval> {
    if moves_count < 5 {
        return None;
    }

    // 1. Score from average loss (0.0 .. 10.0 scale)
    let loss_score = if avg_loss <= 0.22 {
        9.5 + (0.22 - avg_loss).max(0.0) * 2.5
    } else if avg_loss <= 0.35 {
        8.0 + (0.35 - avg_loss) / 0.13 * 1.5
    } else if avg_loss <= 0.55 {
        6.5 + (0.55 - avg_loss) / 0.20 * 1.5
    } else if avg_loss <= 0.85 {
        4.5 + (0.85 - avg_loss) / 0.30 * 2.0
    } else if avg_loss <= 1.25 {
        2.5 + (1.25 - avg_loss) / 0.40 * 2.0
    } else if avg_loss <= 2.00 {
        1.0 + (2.00 - avg_loss) / 0.75 * 1.5
    } else {
        (3.0 - avg_loss).max(0.0)
    };

    // 2. Score from top 3 rate (0.0 .. 10.0 scale)
    let top3_score = if top3_rate >= 85.0 {
        9.5 + (top3_rate - 85.0) / 15.0 * 0.5
    } else if top3_rate >= 75.0 {
        7.5 + (top3_rate - 75.0) / 10.0 * 2.0
    } else if top3_rate >= 65.0 {
        5.5 + (top3_rate - 65.0) / 10.0 * 2.0
    } else if top3_rate >= 50.0 {
        3.0 + (top3_rate - 50.0) / 15.0 * 2.5
    } else {
        (top3_rate / 50.0 * 3.0).max(0.0)
    };

    // 3. Score from blunder rate per 100 moves
    let blunder_score = if blunder_rate_per_100 <= 0.5 {
        9.5
    } else if blunder_rate_per_100 <= 1.5 {
        7.5 + (1.5 - blunder_rate_per_100) / 1.0 * 1.5
    } else if blunder_rate_per_100 <= 3.5 {
        5.0 + (3.5 - blunder_rate_per_100) / 2.0 * 2.5
    } else if blunder_rate_per_100 <= 6.0 {
        2.5 + (6.0 - blunder_rate_per_100) / 2.5 * 2.5
    } else {
        (10.0 - blunder_rate_per_100).clamp(0.0, 2.5)
    };

    // Weighted composite score (Loss is primary 50%, Top3 is 30%, Blunder resilience is 20%)
    let median_score =
        (0.50 * loss_score + 0.30 * top3_score + 0.20 * blunder_score).clamp(0.0, 10.0);

    // Upper bound driven by top-3 rate
    let upper_offset = if top3_rate >= 80.0 {
        1.2
    } else if top3_rate >= 65.0 {
        1.0
    } else {
        0.8
    };
    // Lower bound dragged down by blunders
    let lower_offset = if blunder_rate_per_100 >= 5.0 {
        1.5
    } else if blunder_rate_per_100 >= 2.5 {
        1.2
    } else {
        0.8
    };

    let upper_score = (median_score + upper_offset).min(10.0);
    let lower_score = (median_score - lower_offset).max(0.0);

    let confidence_score = if moves_count >= 50 {
        0.95
    } else if moves_count >= 25 {
        0.80
    } else if moves_count >= 15 {
        0.60
    } else {
        0.35
    };

    Some(RankInterval {
        lower_bound: score_to_rank_label(lower_score).to_owned(),
        upper_bound: score_to_rank_label(upper_score).to_owned(),
        median: score_to_rank_label(median_score).to_owned(),
        confidence_score,
    })
}

fn score_to_rank_label(score: f64) -> &'static str {
    if score >= 9.5 {
        "职业段位/AI"
    } else if score >= 8.5 {
        "业余 7D"
    } else if score >= 7.5 {
        "业余 6D"
    } else if score >= 6.5 {
        "业余 5D"
    } else if score >= 5.5 {
        "业余 4D"
    } else if score >= 4.5 {
        "业余 3D"
    } else if score >= 3.5 {
        "业余 2D"
    } else if score >= 2.5 {
        "业余 1D"
    } else if score >= 1.5 {
        "1K ~ 3K"
    } else if score >= 0.5 {
        "4K ~ 9K"
    } else {
        "10K 以下"
    }
}

/// Computes a phase slice analysis for moves matching the specified phase filter.
pub fn compute_phase_analytics(
    evals: &[MoveEvaluation],
    phase: GamePhase,
    player: Color,
) -> PhaseAnalytics {
    let mut total_loss = 0.0;
    let mut moves_count = 0;
    let mut top1_matches = 0;
    let mut top3_matches = 0;
    let mut best_count = 0;
    let mut good_count = 0;
    let mut inaccuracy_count = 0;
    let mut mistake_count = 0;
    let mut blunder_count = 0;

    for eval in evals {
        if eval.player != player || !phase.includes_move(eval.move_number) {
            continue;
        }
        moves_count += 1;

        // Garbage time damping: if winrate is already >95% or <5%, damp late-game slack
        let is_garbage_time =
            eval.move_number > 80 && (eval.winrate_before > 0.95 || eval.winrate_before < 0.05);
        let effective_loss = if is_garbage_time {
            eval.points_lost * 0.4
        } else {
            eval.points_lost
        };
        total_loss += effective_loss;

        // Top 1 / Top 3 match rates
        let is_top1 = if let (Some(played), Some(rec)) = (
            eval.played_vertex.as_deref(),
            eval.ai_recommended_vertex.as_deref(),
        ) {
            played.eq_ignore_ascii_case(rec)
        } else {
            eval.points_lost < 0.5
        };
        let is_top3 = is_top1 || eval.points_lost <= 1.2;

        if is_top1 {
            top1_matches += 1;
        }
        if is_top3 {
            top3_matches += 1;
        }

        match eval.quality {
            MoveQuality::Best => best_count += 1,
            MoveQuality::Good => good_count += 1,
            MoveQuality::Inaccuracy => inaccuracy_count += 1,
            MoveQuality::Mistake => mistake_count += 1,
            MoveQuality::Blunder => blunder_count += 1,
        }
    }

    if moves_count == 0 {
        return PhaseAnalytics::default();
    }

    let avg_loss = total_loss / moves_count as f64;
    let top1_rate = (top1_matches as f64 / moves_count as f64) * 100.0;
    let top3_rate = (top3_matches as f64 / moves_count as f64) * 100.0;
    let blunder_rate_per_100 = (blunder_count as f64 / moves_count as f64) * 100.0;

    let rank_interval =
        estimate_rank_interval(avg_loss, top3_rate, blunder_rate_per_100, moves_count);

    PhaseAnalytics {
        moves_count,
        avg_loss,
        top1_matches,
        top3_matches,
        top1_rate,
        top3_rate,
        best_count,
        good_count,
        inaccuracy_count,
        mistake_count,
        blunder_count,
        blunder_rate_per_100,
        rank_interval,
    }
}

/// Builds a comprehensive multi-phase analytics report from all move evaluations.
pub fn compute_comprehensive_game_analytics(
    evals: &[MoveEvaluation],
) -> ComprehensiveGameAnalytics {
    let mut top_blunders: Vec<MoveEvaluation> = evals
        .iter()
        .filter(|e| e.quality == MoveQuality::Blunder || e.quality == MoveQuality::Mistake)
        .cloned()
        .collect();
    top_blunders.sort_by(|a, b| {
        b.points_lost
            .partial_cmp(&a.points_lost)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_blunders.truncate(5);

    ComprehensiveGameAnalytics {
        total_moves: evals.len(),
        overall_black: compute_phase_analytics(evals, GamePhase::Overall, Color::Black),
        overall_white: compute_phase_analytics(evals, GamePhase::Overall, Color::White),
        opening_black: compute_phase_analytics(evals, GamePhase::Opening, Color::Black),
        opening_white: compute_phase_analytics(evals, GamePhase::Opening, Color::White),
        midgame_black: compute_phase_analytics(evals, GamePhase::Midgame, Color::Black),
        midgame_white: compute_phase_analytics(evals, GamePhase::Midgame, Color::White),
        endgame_black: compute_phase_analytics(evals, GamePhase::Endgame, Color::Black),
        endgame_white: compute_phase_analytics(evals, GamePhase::Endgame, Color::White),
        top_blunders,
    }
}

/// Extracts sequential move evaluations from a document snapshot by inspecting
/// persisted SGF `SBKV` (Winrate) and `SBKS` (Score Lead) properties along the
/// active move lineage.
pub fn compute_game_move_evaluations(
    snapshot: &ryusei_domain_core::GameSnapshot,
) -> Vec<MoveEvaluation> {
    let mut evals = Vec::new();
    let moves = &snapshot.moves;
    if moves.is_empty() {
        return evals;
    }

    let nodes_map: std::collections::BTreeMap<&str, &ryusei_domain_core::NodeSnapshot> =
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

        // Parse parent's RYK candidates to extract top 1 recommendation
        let parent_node = node.parent_id.as_deref().and_then(|pid| nodes_map.get(pid));
        let ai_recommended_vertex = parent_node
            .and_then(|pn| pn.properties.get("RYK"))
            .and_then(|v| v.first())
            .and_then(|ryk_str| {
                ryk_str.split(';').next().and_then(|first_cand| {
                    let mut parts = first_cand.split(',');
                    let v = parts.next()?.trim();
                    if v.is_empty() {
                        None
                    } else {
                        Some(v.to_ascii_uppercase())
                    }
                })
            });

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
            ai_recommended_vertex,
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

        let multi_analytics = compute_comprehensive_game_analytics(&evals);
        assert_eq!(multi_analytics.total_moves, 2);
        assert_eq!(multi_analytics.overall_black.moves_count, 1);
        assert_eq!(multi_analytics.overall_white.moves_count, 1);
        assert_eq!(multi_analytics.opening_black.moves_count, 1);
        assert_eq!(multi_analytics.midgame_black.moves_count, 0);
    }

    #[test]
    fn estimates_reasonable_ranks_from_metrics() {
        // High dan / Pro
        let pro = estimate_rank_interval(0.18, 88.0, 0.0, 60).expect("pro rank");
        assert!(pro.median.contains("7D") || pro.median.contains("职业"));

        // Amateur 4D - 5D
        let amateur_mid = estimate_rank_interval(0.65, 70.0, 2.0, 60).expect("amateur 4D-5D");
        assert!(amateur_mid.median.contains("4D") || amateur_mid.median.contains("5D"));

        // Kyu player
        let kyu = estimate_rank_interval(1.8, 35.0, 8.0, 50).expect("kyu");
        assert!(kyu.median.contains("K") || kyu.median.contains("1D"));
    }
}
