//! Read-only winrate history derived from SGF analysis properties.
//!
//! The UI consumes these points without owning an engine session. `SBKV` stores
//! Black's winrate in the original Sabaki SGF convention; live analysis supplies
//! only the current node when no persisted value exists.

use ryusei_domain_core::{Color, GameSnapshot, NodeId};

#[derive(Clone, Debug, PartialEq)]
pub struct WinratePoint {
    pub node_id: NodeId,
    pub move_number: usize,
    pub black_winrate: Option<f64>,
    pub black_score_lead: Option<f64>,
    pub is_current: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WinrateGraphMetric {
    Winrate,
    ScoreLead,
}

impl WinrateGraphMetric {
    pub fn from_setting(value: Option<&str>) -> Self {
        match value {
            Some("scorelead") | Some("score-lead") => Self::ScoreLead,
            _ => Self::Winrate,
        }
    }

    #[allow(dead_code)]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Winrate => "winrate · black",
            Self::ScoreLead => "score lead · black",
        }
    }
}

/// Pure rendering values for one graph point. `y` is normalized to `[0, 1]`
/// with zero at top. The caller converts it into actual plot coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphPlotPoint {
    pub node_id: NodeId,
    pub move_number: usize,
    pub y: Option<f64>,
    pub is_current: bool,
    pub is_blunder: bool,
}

/// Maps a local plot X coordinate to the closest history index, matching the
/// original graph's rounded and clamped scrub semantics.
#[allow(dead_code)]
pub fn graph_index_from_x(x: f32, width: f32, point_count: usize) -> Option<usize> {
    (point_count > 0).then(|| {
        if point_count == 1 || width <= 0.0 {
            return 0;
        }
        (x / width * (point_count - 1) as f32)
            .round()
            .clamp(0.0, (point_count - 1) as f32) as usize
    })
}

/// Converts a completed Analysis-role result into original Sabaki SGF fields.
/// `SBKV` is stored as Black's percent; `SBKS` is Black's signed score lead.
/// Sabaki-managed KataGo reports both values from Black's perspective, so no
/// player-to-move inversion is appropriate here.
pub fn analysis_sgf_properties(
    entry: &ryusei_host::AnalysisEntry,
    _player: Color,
) -> Vec<(&'static str, String)> {
    let black_winrate = entry.winrate.clamp(0.0, 1.0);
    let mut properties = vec![("SBKV", format!("{:.2}", black_winrate * 100.0))];
    if let Some(lead) = entry.score_lead.filter(|lead| lead.is_finite()) {
        properties.push(("SBKS", format!("{lead:.2}")));
    }
    properties
}

/// The SGF node property carrying the persisted candidate list for a reviewed
/// move. Its value is SGF-safe (no `]` or `\`): `vertex,winrate,visits,lead`
/// records joined by `;`, with an empty `lead` for a missing score lead.
pub const CANDIDATES_PROPERTY: &str = "RYK";

/// Serializes completed candidates into the SGF-safe `RYK` value format.
pub fn serialize_analysis_candidates(entries: &[ryusei_host::AnalysisEntry]) -> String {
    entries
        .iter()
        .filter(|entry| !entry.is_during_search)
        .filter_map(|entry| {
            let vertex = entry.vertex.as_deref()?;
            Some(format!(
                "{vertex},{},{},{}",
                entry.winrate,
                entry.visits,
                entry
                    .score_lead
                    .map(|lead| lead.to_string())
                    .unwrap_or_default()
            ))
        })
        .collect::<Vec<_>>()
        .join(";")
}

/// Rebuilds candidates from a persisted `RYK` value. Missing/malformed records
/// are skipped so a hand-edited SGF can never break navigation.
pub fn deserialize_analysis_candidates(value: &str) -> Vec<ryusei_host::AnalysisEntry> {
    value
        .split(';')
        .filter_map(|record| {
            let mut fields = record.split(',');
            let vertex = fields.next()?.trim().to_owned();
            if vertex.is_empty() {
                return None;
            }
            let winrate = fields.next()?.trim().parse::<f64>().ok()?;
            let visits = fields.next()?.trim().parse::<u64>().ok()?;
            let score_lead = fields
                .next()
                .map(str::trim)
                .filter(|lead| !lead.is_empty())
                .and_then(|lead| lead.parse::<f64>().ok());
            Some(ryusei_host::AnalysisEntry {
                id: None,
                vertex: Some(vertex),
                visits,
                winrate,
                score_lead,
                pv: Vec::new(),
                is_during_search: false,
                ownership: None,
                prior: None,
            })
        })
        .collect()
}

pub fn graph_plot_points(
    points: &[WinratePoint],
    metric: WinrateGraphMetric,
    inverted: bool,
    winrate_threshold_percent: f64,
    score_lead_threshold: f64,
) -> Vec<GraphPlotPoint> {
    let score_scale = points
        .iter()
        .filter_map(|point| point.black_score_lead)
        .map(f64::abs)
        .fold(1.0_f64, f64::max);
    let threshold = match metric {
        WinrateGraphMetric::Winrate => (winrate_threshold_percent / 100.0).max(0.0),
        WinrateGraphMetric::ScoreLead => score_lead_threshold.max(0.0),
    };
    let mut previous: Option<f64> = None;
    points
        .iter()
        .map(|point| {
            let value = match metric {
                WinrateGraphMetric::Winrate => point.black_winrate,
                WinrateGraphMetric::ScoreLead => point.black_score_lead,
            };
            let is_blunder = value
                .zip(previous)
                .is_some_and(|(value, previous)| (value - previous).abs() >= threshold);
            if value.is_some() {
                previous = value;
            }
            let y = value.map(|value| match metric {
                WinrateGraphMetric::Winrate => 1.0 - value.clamp(0.0, 1.0),
                WinrateGraphMetric::ScoreLead => {
                    (0.5 - value / (2.0 * score_scale)).clamp(0.0, 1.0)
                }
            });
            GraphPlotPoint {
                node_id: point.node_id.clone(),
                move_number: point.move_number,
                y: y.map(|y| if inverted { 1.0 - y } else { y }),
                is_current: point.is_current,
                is_blunder,
            }
        })
        .collect()
}

fn finite_property(snapshot: &GameSnapshot, node_id: &str, key: &str) -> Option<f64> {
    snapshot
        .nodes
        .iter()
        .find(|node| node.id == node_id)
        .and_then(|node| node.properties.get(key))
        .and_then(|values| values.first())
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite())
}

fn black_winrate_from_node(snapshot: &GameSnapshot, node_id: &str) -> Option<f64> {
    finite_property(snapshot, node_id, "SBKV")
        .map(|value| {
            if value.abs() > 1.0 {
                value / 100.0
            } else {
                value
            }
        })
        .map(|value| value.clamp(0.0, 1.0))
}

/// Builds root-to-current history. A live candidate augments the current point
/// only when the SGF does not already carry a persisted value. `live_winrate`
/// and `live_score_lead` are already from Black's perspective because the
/// managed KataGo config requests `reportAnalysisWinratesAs = BLACK`.
pub fn winrate_history(
    snapshot: &GameSnapshot,
    live_winrate: Option<f64>,
    live_score_lead: Option<f64>,
    _live_player: Color,
) -> Vec<WinratePoint> {
    let mut path = Vec::new();
    let mut cursor = Some(snapshot.current_node_id.clone());
    while let Some(node_id) = cursor {
        let Some(node) = snapshot.nodes.iter().find(|node| node.id == node_id) else {
            break;
        };
        cursor = node.parent_id.clone();
        path.push(node.id.clone());
    }
    path.reverse();

    path.into_iter()
        .enumerate()
        .map(|(move_number, node_id)| {
            let is_current = node_id == snapshot.current_node_id;
            let black_winrate = black_winrate_from_node(snapshot, &node_id)
                .or_else(|| is_current.then_some(live_winrate).flatten());
            let black_score_lead = finite_property(snapshot, &node_id, "SBKS")
                .or_else(|| is_current.then_some(live_score_lead).flatten());
            WinratePoint {
                node_id,
                move_number,
                black_winrate,
                black_score_lead,
                is_current,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        WinrateGraphMetric, analysis_sgf_properties, deserialize_analysis_candidates,
        graph_index_from_x, graph_plot_points, serialize_analysis_candidates, winrate_history,
    };
    use ryusei_domain_core::{Color, GameDocument};

    #[test]
    fn reads_sgf_history_with_black_perspective_live_analysis() {
        let snapshot = GameDocument::from_sgf("(;SZ[5];B[aa]SBKV[0.6];W[bb])")
            .unwrap()
            .snapshot();
        let points = winrate_history(&snapshot, Some(0.3), Some(1.5), Color::White);
        assert_eq!(points.len(), 3);
        assert_eq!(points[1].black_winrate, Some(0.6));
        assert_eq!(points[2].black_winrate, Some(0.3));
        assert_eq!(points[2].black_score_lead, Some(1.5));
        assert!(points[2].is_current);
    }

    #[test]
    fn panel_composition_does_not_double_convert_white_live_winrate() {
        use crate::engine_console::live_analysis_winrate;

        // White to move after one black stone. KataGo's managed config reports
        // Black's winrate even though the requested move color is White.
        let entries = vec![ryusei_host::AnalysisEntry {
            id: None,
            vertex: Some("Q16".to_owned()),
            visits: 100,
            winrate: 0.58,
            score_lead: Some(0.5),
            pv: Vec::new(),
            is_during_search: false,
            ownership: None,
            prior: None,
        }];
        let snapshot = GameDocument::from_sgf("(;SZ[19];B[dd])")
            .unwrap()
            .snapshot();
        let live_player_winrate = live_analysis_winrate(&entries);
        let points = winrate_history(&snapshot, live_player_winrate, None, Color::White);
        let current = points.iter().find(|point| point.is_current).unwrap();
        assert!((current.black_winrate.unwrap() - 0.58).abs() < 1e-9);
    }

    #[test]
    fn normalizes_upstream_percent_values_and_keeps_missing_history_empty() {
        let snapshot = GameDocument::from_sgf("(;SZ[5];B[aa]SBKV[60];W[bb])")
            .unwrap()
            .snapshot();
        let points = winrate_history(&snapshot, None, None, Color::Black);
        assert_eq!(points[0].black_winrate, None);
        assert_eq!(points[1].black_winrate, Some(0.6));
        assert_eq!(points[2].black_winrate, None);
    }

    #[test]
    fn x_scrubbing_rounds_and_clamps_to_a_history_index() {
        assert_eq!(graph_index_from_x(0.0, 200.0, 5), Some(0));
        assert_eq!(graph_index_from_x(74.0, 200.0, 5), Some(1));
        assert_eq!(graph_index_from_x(126.0, 200.0, 5), Some(3));
        assert_eq!(graph_index_from_x(-5.0, 200.0, 5), Some(0));
        assert_eq!(graph_index_from_x(999.0, 200.0, 5), Some(4));
        assert_eq!(graph_index_from_x(20.0, 0.0, 1), Some(0));
        assert_eq!(graph_index_from_x(20.0, 200.0, 0), None);
    }

    #[test]
    fn analysis_properties_use_black_perspective_and_skip_non_finite_score_lead() {
        let entry = ryusei_host::AnalysisEntry {
            id: None,
            vertex: Some("D4".to_owned()),
            visits: 100,
            winrate: 0.25,
            score_lead: Some(3.5),
            pv: Vec::new(),
            is_during_search: false,
            ownership: None,
            prior: None,
        };
        assert_eq!(
            analysis_sgf_properties(&entry, Color::White),
            vec![("SBKV", "25.00".to_owned()), ("SBKS", "3.50".to_owned())]
        );
        let invalid_score = ryusei_host::AnalysisEntry {
            score_lead: Some(f64::NAN),
            ..entry
        };
        assert_eq!(
            analysis_sgf_properties(&invalid_score, Color::Black),
            vec![("SBKV", "25.00".to_owned())]
        );
    }

    #[test]
    fn plot_values_support_inversion_score_lead_and_blunder_thresholds() {
        let snapshot =
            GameDocument::from_sgf("(;SZ[5];B[aa]SBKV[60]SBKS[2];W[bb]SBKV[40]SBKS[-3])")
                .unwrap()
                .snapshot();
        let points = winrate_history(&snapshot, None, None, Color::Black);
        let winrate = graph_plot_points(&points, WinrateGraphMetric::Winrate, false, 15.0, 2.0);
        assert_eq!(winrate[1].y, Some(0.4));
        assert!(winrate[2].is_blunder);
        let inverted = graph_plot_points(&points, WinrateGraphMetric::Winrate, true, 15.0, 2.0);
        assert_eq!(inverted[1].y, Some(0.6));
        let score = graph_plot_points(&points, WinrateGraphMetric::ScoreLead, false, 15.0, 2.0);
        assert!((score[1].y.unwrap() - 1.0 / 6.0).abs() < 1e-9);
        assert_eq!(score[2].y, Some(1.0));
        assert!(score[2].is_blunder);
    }

    #[test]
    fn rejects_non_finite_values_and_clamps_out_of_range_percentages() {
        let snapshot = GameDocument::from_sgf("(;SZ[5];B[aa]SBKV[NaN];W[bb]SBKV[400])")
            .unwrap()
            .snapshot();
        let points = winrate_history(&snapshot, None, None, Color::Black);
        assert_eq!(points[1].black_winrate, None);
        assert_eq!(points[2].black_winrate, Some(1.0));
    }

    #[test]
    fn candidates_round_trip_and_skip_malformed_records() {
        let entries = vec![
            ryusei_host::AnalysisEntry {
                id: None,
                vertex: Some("D4".to_owned()),
                visits: 320,
                winrate: 0.55,
                score_lead: Some(2.5),
                pv: Vec::new(),
                is_during_search: false,
                ownership: None,
                prior: None,
            },
            ryusei_host::AnalysisEntry {
                id: None,
                vertex: Some("Q16".to_owned()),
                visits: 100,
                winrate: 0.45,
                score_lead: None,
                pv: Vec::new(),
                is_during_search: false,
                ownership: None,
                prior: None,
            },
            // During-search entries must be dropped from persistence.
            ryusei_host::AnalysisEntry {
                id: None,
                vertex: Some("C3".to_owned()),
                visits: 10,
                winrate: 0.30,
                score_lead: None,
                pv: Vec::new(),
                is_during_search: true,
                ownership: None,
                prior: None,
            },
        ];
        let encoded = serialize_analysis_candidates(&entries);
        assert!(!encoded.contains(']'));
        assert!(!encoded.contains('\\'));
        let decoded = deserialize_analysis_candidates(&encoded);
        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded[0].vertex.as_deref(), Some("D4"));
        assert_eq!(decoded[0].winrate, 0.55);
        assert_eq!(decoded[0].visits, 320);
        assert_eq!(decoded[0].score_lead, Some(2.5));
        assert_eq!(decoded[1].vertex.as_deref(), Some("Q16"));
        assert_eq!(decoded[1].score_lead, None);

        // Hand-edited garbage must never break navigation.
        assert!(deserialize_analysis_candidates("junk,no,good;D4,0.55,320,2.5").len() == 1);
        assert!(deserialize_analysis_candidates("").is_empty());
    }
}
