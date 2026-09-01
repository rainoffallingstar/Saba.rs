//! Pure formatting helpers for shell UI labels.
//!
//! Extracted from `panels.rs` so the label-building rules (clock text, candidate
//! tier badges) are unit-testable without a GPUI window and are not duplicated
//! across the titlebar and player bar.

use ryusei_domain_core::{ClockPhase, PlayerClock};

/// Formats a player's clock for the VS pill / player bar.
///
/// - Main time / Fischer: `MM:SS`.
/// - Japanese byo-yomi: `MM:SS (periods_left)`.
/// - Expired: `TIME`.
pub fn format_clock(clock: PlayerClock) -> String {
    let seconds = clock.display_remaining().as_secs();
    let time = format!("{:02}:{:02}", seconds / 60, seconds % 60);
    match clock.phase {
        ClockPhase::ByoYomi => format!("{time} ({})", clock.periods_remaining),
        ClockPhase::Expired => "TIME".to_owned(),
        ClockPhase::MainTime => time,
    }
}

/// The candidate-ranking badge shown on an AI Top-5 card.
///
/// This is the *candidate ordering* tier (Top1 = Best, then winrate bands), not
/// the KaTrain five-tier *played-move* quality (`MoveQuality`) used on the board.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CandidateTier {
    Best,
    Good,
    Inaccuracy,
    Mistake,
}

impl CandidateTier {
    pub fn label(self) -> &'static str {
        match self {
            Self::Best => "Best",
            Self::Good => "Good",
            Self::Inaccuracy => "Inacc",
            Self::Mistake => "Mistake",
        }
    }
}

/// Classifies a candidate card into its ranking tier from its rank (1-based)
/// and engine winrate in `[0, 1]`.
pub fn candidate_tier(rank: usize, winrate: f64) -> CandidateTier {
    if rank == 1 {
        CandidateTier::Best
    } else if winrate >= 0.50 {
        CandidateTier::Good
    } else if winrate >= 0.40 {
        CandidateTier::Inaccuracy
    } else {
        CandidateTier::Mistake
    }
}

/// Normalises one player's average loss against the larger of the two players'
/// losses, for the loss-comparison bars in the review summary. Returns a ratio
/// in `[0, 1]`; a zero/negative pair yields `0` (empty bars).
pub fn loss_ratio(loss: f64, other_loss: f64) -> f32 {
    let max_loss = loss.max(other_loss).max(0.01);
    (loss / max_loss).clamp(0.0, 1.0) as f32
}

#[cfg(test)]
mod tests {
    use super::{CandidateTier, candidate_tier, format_clock, loss_ratio};
    use ryusei_domain_core::{ClockPhase, PlayerClock};
    use std::time::Duration;

    fn clock(main: u64, byo: u64, periods: u32, phase: ClockPhase) -> PlayerClock {
        PlayerClock {
            main_time_remaining: Duration::from_secs(main),
            byo_yomi_time_remaining: Duration::from_secs(byo),
            periods_remaining: periods,
            phase,
        }
    }

    #[test]
    fn formats_main_time_as_mm_ss() {
        assert_eq!(
            format_clock(clock(600, 0, 0, ClockPhase::MainTime)),
            "10:00"
        );
        assert_eq!(format_clock(clock(75, 0, 0, ClockPhase::MainTime)), "01:15");
    }

    #[test]
    fn formats_byo_yomi_with_periods_remaining() {
        assert_eq!(
            format_clock(clock(0, 30, 4, ClockPhase::ByoYomi)),
            "00:30 (4)"
        );
    }

    #[test]
    fn formats_expired_as_time() {
        assert_eq!(format_clock(clock(0, 0, 0, ClockPhase::Expired)), "TIME");
    }

    #[test]
    fn candidate_tier_ranks_top_and_bands_winrate() {
        assert_eq!(candidate_tier(1, 0.20), CandidateTier::Best);
        assert_eq!(candidate_tier(2, 0.55), CandidateTier::Good);
        assert_eq!(candidate_tier(2, 0.45), CandidateTier::Inaccuracy);
        assert_eq!(candidate_tier(3, 0.30), CandidateTier::Mistake);
        // Boundary: 0.50 is Good, 0.40 is Inaccuracy.
        assert_eq!(candidate_tier(2, 0.50), CandidateTier::Good);
        assert_eq!(candidate_tier(2, 0.40), CandidateTier::Inaccuracy);
    }

    #[test]
    fn loss_ratio_normalises_against_the_larger_loss() {
        assert_eq!(loss_ratio(0.94, 0.32), 1.0);
        assert!((loss_ratio(0.32, 0.94) - (0.32 / 0.94) as f32).abs() < 1e-6);
        // Both zero → empty bars, never a divide-by-zero spike.
        assert_eq!(loss_ratio(0.0, 0.0), 0.0);
        // The larger side always fills the bar.
        assert_eq!(loss_ratio(2.0, 1.0), 1.0);
    }
}
