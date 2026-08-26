//! KataGo territory ownership and score estimation (ported from LizzieYZY).
//!
//! Converts raw ownership probability arrays into concrete territory counts,
//! prisoner adjustments, komi offsets, and final point lead conclusions.

use ryusei_domain_core::Color;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TerritoryEstimate {
    /// Raw ownership confidence for each board intersection, preserved for
    /// heatmap rendering and export. Positive values favor Black.
    pub ownership: Vec<f64>,
    pub black_territory: f64,
    pub white_territory: f64,
    pub black_prisoners: usize,
    pub white_prisoners: usize,
    pub komi: f64,
    pub black_total: f64,
    pub white_total: f64,
    /// Positive means Black leads; negative means White leads.
    pub lead: f64,
    pub leading_player: Color,
    pub lead_points: f64,
}

impl TerritoryEstimate {
    pub fn summary_text(&self) -> String {
        let leader = match self.leading_player {
            Color::Black => "Black",
            Color::White => "White",
        };
        format!(
            "{leader} +{:.1} pts (B {:.1} vs W {:.1})",
            self.lead_points, self.black_total, self.white_total
        )
    }
}

/// Estimates territory score from KataGo ownership probabilities.
///
/// `threshold` is the minimum confidence (e.g. 0.50) to treat an intersection
/// as definitive territory.
pub fn estimate_territory(
    ownership: &[f64],
    black_prisoners: usize,
    white_prisoners: usize,
    komi: f64,
    threshold: f64,
) -> Option<TerritoryEstimate> {
    if ownership.is_empty() {
        return None;
    }

    let mut black_territory = 0.0_f64;
    let mut white_territory = 0.0_f64;

    for &val in ownership {
        if !val.is_finite() {
            continue;
        }
        if val >= threshold {
            black_territory += 1.0;
        } else if val <= -threshold {
            white_territory += 1.0;
        } else if val > 0.0 {
            black_territory += val;
        } else if val < 0.0 {
            white_territory += val.abs();
        }
    }

    let black_total = black_territory + black_prisoners as f64;
    let white_total = white_territory + white_prisoners as f64 + komi;
    let lead = black_total - white_total;
    let leading_player = if lead >= 0.0 {
        Color::Black
    } else {
        Color::White
    };
    let lead_points = lead.abs();

    Some(TerritoryEstimate {
        ownership: ownership.to_vec(),
        black_territory,
        white_territory,
        black_prisoners,
        white_prisoners,
        komi,
        black_total,
        white_total,
        lead,
        leading_player,
        lead_points,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimates_territory_with_komi_and_prisoners() {
        let mut ownership = vec![0.0; 361];
        // 50 black points, 30 white points
        for value in ownership.iter_mut().take(50) {
            *value = 0.9;
        }
        for value in ownership.iter_mut().take(80).skip(50) {
            *value = -0.9;
        }

        let estimate =
            estimate_territory(&ownership, 3, 1, 7.5, 0.5).expect("estimate must succeed");

        assert_eq!(estimate.ownership.len(), 361);
        assert_eq!(estimate.black_territory, 50.0);
        assert_eq!(estimate.white_territory, 30.0);
        assert_eq!(estimate.black_total, 53.0); // 50 + 3
        assert_eq!(estimate.white_total, 38.5); // 30 + 1 + 7.5
        assert_eq!(estimate.leading_player, Color::Black);
        assert!((estimate.lead_points - 14.5).abs() < 1e-4);
        assert!(estimate.summary_text().contains("Black +14.5 pts"));
    }
}
