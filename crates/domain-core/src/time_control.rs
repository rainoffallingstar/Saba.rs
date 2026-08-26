//! Deterministic game clock and overtime rules.
//!
//! The clock is deliberately independent from GPUI, wall-clock time, network
//! transports, and engine processes. Callers provide monotonic elapsed time;
//! remote adapters may replace the display snapshot with an authoritative
//! server snapshot.

use std::{collections::BTreeMap, time::Duration};

use serde::{Deserialize, Serialize};

use crate::Color;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TimeControl {
    #[default]
    None,
    Absolute {
        main_time_secs: u64,
    },
    ByoYomi {
        main_time_secs: u64,
        period_time_secs: u64,
        periods: u32,
    },
}

impl TimeControl {
    pub fn main_time_secs(self) -> u64 {
        match self {
            Self::None => 0,
            Self::Absolute { main_time_secs } | Self::ByoYomi { main_time_secs, .. } => {
                main_time_secs
            }
        }
    }

    pub fn to_sgf(self) -> Option<(String, String)> {
        match self {
            Self::None => None,
            Self::Absolute { main_time_secs } => {
                Some((main_time_secs.to_string(), "absolute".to_owned()))
            }
            Self::ByoYomi {
                main_time_secs,
                period_time_secs,
                periods,
            } => Some((
                main_time_secs.to_string(),
                format!("{periods}x{period_time_secs}s byo-yomi"),
            )),
        }
    }

    /// Reads the portable subset of SGF `TM` / `OT` metadata written by this
    /// application, plus the common `NxMs` byo-yomi variants produced by other
    /// tools. Unknown overtime descriptions remain unparsed instead of being
    /// guessed as a different clock system: an unrecognized `OT` never silently
    /// degrades to `Absolute`.
    pub fn from_sgf(properties: &BTreeMap<String, Vec<String>>) -> Self {
        let main_time_secs = properties
            .get("TM")
            .and_then(|values| values.first())
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0);
        let overtime = properties
            .get("OT")
            .and_then(|values| values.first())
            .map(String::as_str)
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        if overtime == "absolute" {
            return Self::Absolute { main_time_secs };
        }
        // Absent `OT` means a plain main-time clock.
        if overtime.is_empty() {
            return if main_time_secs > 0 {
                Self::Absolute { main_time_secs }
            } else {
                Self::None
            };
        }
        match Self::parse_byo_yomi_overtime(&overtime) {
            // A recognized byo-yomi with zero periods or a zero-length period
            // has no meaningful overtime; do not guess a clock system for it.
            Some((periods, period_time_secs)) if periods > 0 && period_time_secs > 0 => {
                Self::ByoYomi {
                    main_time_secs,
                    period_time_secs,
                    periods,
                }
            }
            _ => Self::None,
        }
    }

    /// Parses the numeric `NxMs` byo-yomi overtime description. The `s` unit
    /// and the trailing ` byo-yomi` label are both optional so common variants
    /// (`5x30s byo-yomi`, `5x30 byo-yomi`, `5x30s`, `5x30`) are all accepted.
    /// Returns `None` for anything that is not a recognizable byo-yomi pattern.
    fn parse_byo_yomi_overtime(overtime: &str) -> Option<(u32, u64)> {
        let numeric = match overtime.strip_suffix(" byo-yomi") {
            Some(rest) => rest,
            None => overtime,
        };
        let numeric = numeric.trim_end();
        // Optional trailing seconds unit.
        let numeric = numeric.strip_suffix('s').unwrap_or(numeric);
        let (periods, period_time) = numeric.split_once('x')?;
        Some((
            periods.trim().parse::<u32>().ok()?,
            period_time.trim().parse::<u64>().ok()?,
        ))
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ClockPhase {
    MainTime,
    ByoYomi,
    Expired,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerClock {
    pub main_time_remaining: Duration,
    pub byo_yomi_time_remaining: Duration,
    pub periods_remaining: u32,
    pub phase: ClockPhase,
}

impl PlayerClock {
    fn fresh(control: TimeControl) -> Self {
        let period_time = match control {
            TimeControl::ByoYomi {
                period_time_secs, ..
            } => period_time_secs,
            _ => 0,
        };
        let periods = match control {
            TimeControl::ByoYomi { periods, .. } => periods,
            _ => 0,
        };
        Self {
            main_time_remaining: Duration::from_secs(control.main_time_secs()),
            byo_yomi_time_remaining: Duration::from_secs(period_time),
            periods_remaining: periods,
            phase: ClockPhase::MainTime,
        }
    }

    pub fn display_remaining(self) -> Duration {
        match self.phase {
            ClockPhase::MainTime => self.main_time_remaining,
            ClockPhase::ByoYomi => self.byo_yomi_time_remaining,
            ClockPhase::Expired => Duration::ZERO,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClockState {
    pub control: TimeControl,
    pub black: PlayerClock,
    pub white: PlayerClock,
    pub active_color: Option<Color>,
    pub running: bool,
    pub paused: bool,
    pub expired: Option<Color>,
}

impl ClockState {
    pub fn new(control: TimeControl) -> Self {
        Self {
            control,
            black: PlayerClock::fresh(control),
            white: PlayerClock::fresh(control),
            active_color: None,
            running: false,
            paused: false,
            expired: None,
        }
    }

    pub fn player(self, color: Color) -> PlayerClock {
        match color {
            Color::Black => self.black,
            Color::White => self.white,
        }
    }

    /// Serializes the current clock information to standard SGF timing
    /// properties. `BL/WL` are seconds remaining; `OB/OW` are emitted for
    /// Japanese byo-yomi positions.
    pub fn to_sgf_properties(self) -> BTreeMap<String, Vec<String>> {
        let mut properties = BTreeMap::new();
        if let Some((main_time, overtime)) = self.control.to_sgf() {
            properties.insert("TM".to_owned(), vec![main_time]);
            properties.insert("OT".to_owned(), vec![overtime]);
        }
        properties.insert(
            "BL".to_owned(),
            vec![format!(
                "{:.3}",
                self.black.display_remaining().as_secs_f64()
            )],
        );
        properties.insert(
            "WL".to_owned(),
            vec![format!(
                "{:.3}",
                self.white.display_remaining().as_secs_f64()
            )],
        );
        if matches!(self.control, TimeControl::ByoYomi { .. }) {
            properties.insert(
                "OB".to_owned(),
                vec![self.black.periods_remaining.to_string()],
            );
            properties.insert(
                "OW".to_owned(),
                vec![self.white.periods_remaining.to_string()],
            );
        }
        properties
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClockEvent {
    Started(Color),
    Paused,
    Resumed,
    MoveCommitted { color: Color, elapsed: Duration },
    Expired(Color),
}

/// Owns clock transitions. The caller supplies monotonic durations, making the
/// module deterministic and safe to exercise without sleeping in tests.
#[derive(Clone, Copy, Debug)]
pub struct ClockController {
    state: ClockState,
}

impl ClockController {
    pub fn new(control: TimeControl) -> Self {
        Self {
            state: ClockState::new(control),
        }
    }

    pub fn state(&self) -> ClockState {
        self.state
    }

    pub fn start(&mut self, color: Color) -> ClockEvent {
        self.state.active_color = Some(color);
        self.state.running = !matches!(self.state.control, TimeControl::None);
        self.state.paused = false;
        ClockEvent::Started(color)
    }

    pub fn pause(&mut self) -> ClockEvent {
        self.state.paused = true;
        ClockEvent::Paused
    }

    pub fn resume(&mut self) -> ClockEvent {
        if self.state.active_color.is_some() && self.state.expired.is_none() {
            self.state.paused = false;
            self.state.running = !matches!(self.state.control, TimeControl::None);
        }
        ClockEvent::Resumed
    }

    pub fn tick(&mut self, elapsed: Duration) -> Option<ClockEvent> {
        let color = self.state.active_color?;
        if !self.state.running || self.state.paused || self.state.expired.is_some() {
            return None;
        }
        if matches!(self.state.control, TimeControl::None) {
            return None;
        }
        if self.consume(color, elapsed) {
            self.state.expired = Some(color);
            self.state.running = false;
            self.state.active_color = None;
            Some(ClockEvent::Expired(color))
        } else {
            None
        }
    }

    pub fn on_move_committed(&mut self, color: Color, elapsed: Duration) -> ClockEvent {
        if matches!(self.state.control, TimeControl::None) {
            return ClockEvent::MoveCommitted { color, elapsed };
        }
        if self.state.expired.is_none() {
            if self.consume(color, elapsed) {
                self.state.expired = Some(color);
                self.state.running = false;
                self.state.active_color = None;
                return ClockEvent::Expired(color);
            }
            self.reset_period_if_needed(color);
            self.state.active_color = Some(color.opponent());
            self.state.running = !matches!(self.state.control, TimeControl::None);
            self.state.paused = false;
        }
        ClockEvent::MoveCommitted { color, elapsed }
    }

    /// Replaces local prediction with authoritative remote values.
    pub fn apply_remote_clock(&mut self, state: ClockState) {
        self.state = state;
    }

    fn player_mut(&mut self, color: Color) -> &mut PlayerClock {
        match color {
            Color::Black => &mut self.state.black,
            Color::White => &mut self.state.white,
        }
    }

    fn consume(&mut self, color: Color, elapsed: Duration) -> bool {
        let (phase, main_remaining, periods_remaining) = {
            let player = self.player_mut(color);
            (
                player.phase,
                player.main_time_remaining,
                player.periods_remaining,
            )
        };
        let period_length = match self.state.control {
            TimeControl::ByoYomi {
                period_time_secs, ..
            } => Duration::from_secs(period_time_secs),
            _ => Duration::ZERO,
        };
        let player = self.player_mut(color);
        match phase {
            ClockPhase::Expired => true,
            ClockPhase::MainTime => {
                if elapsed < main_remaining {
                    player.main_time_remaining = main_remaining - elapsed;
                    false
                } else {
                    let overtime = elapsed - main_remaining;
                    player.main_time_remaining = Duration::ZERO;
                    if periods_remaining == 0 || period_length.is_zero() {
                        player.phase = ClockPhase::Expired;
                        true
                    } else {
                        player.phase = ClockPhase::ByoYomi;
                        player.byo_yomi_time_remaining = period_length;
                        Self::consume_byo_yomi(player, overtime, period_length)
                    }
                }
            }
            ClockPhase::ByoYomi => Self::consume_byo_yomi(player, elapsed, period_length),
        }
    }

    fn consume_byo_yomi(
        player: &mut PlayerClock,
        elapsed: Duration,
        period_length: Duration,
    ) -> bool {
        if elapsed < player.byo_yomi_time_remaining {
            player.byo_yomi_time_remaining -= elapsed;
            return false;
        }
        let mut remaining = elapsed;
        // Guard against a zero-length remaining period: without the is_zero
        // check the loop would keep subtracting nothing and spin until the
        // period count drained, which is at best wasteful and at worst an
        // infinite loop.
        while !player.byo_yomi_time_remaining.is_zero()
            && remaining >= player.byo_yomi_time_remaining
        {
            remaining -= player.byo_yomi_time_remaining;
            player.periods_remaining = player.periods_remaining.saturating_sub(1);
            if player.periods_remaining == 0 {
                player.phase = ClockPhase::Expired;
                player.byo_yomi_time_remaining = Duration::ZERO;
                return true;
            }
            player.byo_yomi_time_remaining = period_length;
        }
        player.byo_yomi_time_remaining -= remaining;
        player.phase = ClockPhase::ByoYomi;
        false
    }

    fn reset_period_if_needed(&mut self, color: Color) {
        let period_length = match self.state.control {
            TimeControl::ByoYomi {
                period_time_secs, ..
            } => Duration::from_secs(period_time_secs),
            _ => Duration::ZERO,
        };
        let player = self.player_mut(color);
        // A legal move made before the period expires resets the current
        // period; Japanese byo-yomi periods are consumed by timeout, not by
        // every move.
        if player.phase == ClockPhase::ByoYomi && !period_length.is_zero() {
            player.byo_yomi_time_remaining = period_length;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_clock_never_expires_after_a_move() {
        let mut clock = ClockController::new(TimeControl::None);
        assert_eq!(
            clock.on_move_committed(Color::Black, Duration::ZERO),
            ClockEvent::MoveCommitted {
                color: Color::Black,
                elapsed: Duration::ZERO,
            }
        );
        assert_eq!(clock.state().expired, None);
        assert_eq!(clock.state().active_color, None);
    }

    #[test]
    fn absolute_clock_switches_color_after_a_move() {
        let mut clock = ClockController::new(TimeControl::Absolute { main_time_secs: 60 });
        clock.start(Color::Black);
        assert_eq!(clock.tick(Duration::from_secs(12)), None);
        assert_eq!(
            clock.state().black.main_time_remaining,
            Duration::from_secs(48)
        );
        clock.on_move_committed(Color::Black, Duration::from_secs(3));
        assert_eq!(clock.state().active_color, Some(Color::White));
        assert_eq!(
            clock.state().black.main_time_remaining,
            Duration::from_secs(45)
        );
    }

    #[test]
    fn pause_does_not_consume_time() {
        let mut clock = ClockController::new(TimeControl::Absolute { main_time_secs: 60 });
        clock.start(Color::Black);
        clock.pause();
        assert_eq!(clock.tick(Duration::from_secs(30)), None);
        assert_eq!(
            clock.state().black.main_time_remaining,
            Duration::from_secs(60)
        );
        clock.resume();
        clock.tick(Duration::from_secs(30));
        assert_eq!(
            clock.state().black.main_time_remaining,
            Duration::from_secs(30)
        );
    }

    #[test]
    fn byo_yomi_enters_period_and_resets_after_move() {
        let mut clock = ClockController::new(TimeControl::ByoYomi {
            main_time_secs: 10,
            period_time_secs: 5,
            periods: 2,
        });
        clock.start(Color::Black);
        assert_eq!(clock.tick(Duration::from_secs(12)), None);
        assert_eq!(clock.state().black.phase, ClockPhase::ByoYomi);
        assert_eq!(
            clock.state().black.byo_yomi_time_remaining,
            Duration::from_secs(3)
        );
        clock.on_move_committed(Color::Black, Duration::ZERO);
        assert_eq!(clock.state().black.periods_remaining, 2);
        assert_eq!(
            clock.state().black.byo_yomi_time_remaining,
            Duration::from_secs(5)
        );
        clock.tick(Duration::from_secs(6));
        assert_eq!(
            clock.state().white.main_time_remaining,
            Duration::from_secs(4)
        );
    }

    #[test]
    fn sgf_time_properties_round_trip() {
        let control = TimeControl::ByoYomi {
            main_time_secs: 600,
            period_time_secs: 30,
            periods: 5,
        };
        let state = ClockState::new(control);
        let properties = state.to_sgf_properties();
        assert_eq!(properties.get("TM"), Some(&vec!["600".to_owned()]));
        assert_eq!(
            properties.get("OT"),
            Some(&vec!["5x30s byo-yomi".to_owned()])
        );
        assert_eq!(TimeControl::from_sgf(&properties), control);
    }

    #[test]
    fn byo_yomi_timeout_ends_game() {
        let mut clock = ClockController::new(TimeControl::ByoYomi {
            main_time_secs: 0,
            period_time_secs: 5,
            periods: 1,
        });
        clock.start(Color::White);
        assert_eq!(
            clock.tick(Duration::from_secs(6)),
            Some(ClockEvent::Expired(Color::White))
        );
        assert_eq!(clock.state().expired, Some(Color::White));
    }

    #[test]
    fn from_sgf_parses_common_byo_yomi_variants() {
        let cases = [
            // Canonical form written by this app.
            (
                "5x30s byo-yomi",
                TimeControl::ByoYomi {
                    main_time_secs: 600,
                    period_time_secs: 30,
                    periods: 5,
                },
            ),
            // `s` unit and/or label omitted.
            (
                "5x30 byo-yomi",
                TimeControl::ByoYomi {
                    main_time_secs: 600,
                    period_time_secs: 30,
                    periods: 5,
                },
            ),
            (
                "5x30s",
                TimeControl::ByoYomi {
                    main_time_secs: 600,
                    period_time_secs: 30,
                    periods: 5,
                },
            ),
            (
                "5x30",
                TimeControl::ByoYomi {
                    main_time_secs: 600,
                    period_time_secs: 30,
                    periods: 5,
                },
            ),
            // Mixed case and surrounding whitespace.
            (
                "  5X30S  BYO-YOMI  ",
                TimeControl::ByoYomi {
                    main_time_secs: 600,
                    period_time_secs: 30,
                    periods: 5,
                },
            ),
        ];
        for (ot, expected) in cases {
            let mut properties = BTreeMap::new();
            properties.insert("TM".to_owned(), vec!["600".to_owned()]);
            properties.insert("OT".to_owned(), vec![ot.to_owned()]);
            assert_eq!(TimeControl::from_sgf(&properties), expected, "OT={ot:?}");
        }
    }

    #[test]
    fn from_sgf_does_not_degrade_unknown_formats_to_absolute() {
        for ot in [
            "25x5 min",            // unparseable period length
            "simple ko",           // not a byo-yomi pattern at all
            "n-period canadian",   // unknown overtime label
            "5 periods of 30 sec", // word-based, not NxM
        ] {
            let mut properties = BTreeMap::new();
            properties.insert("TM".to_owned(), vec!["600".to_owned()]);
            properties.insert("OT".to_owned(), vec![ot.to_owned()]);
            assert_eq!(
                TimeControl::from_sgf(&properties),
                TimeControl::None,
                "OT={ot:?} must not silently become Absolute"
            );
        }
    }

    #[test]
    fn from_sgf_handles_zero_periods_and_zero_period_length() {
        // Zero periods: no meaningful overtime, must not guess Absolute.
        let mut properties = BTreeMap::new();
        properties.insert("TM".to_owned(), vec!["600".to_owned()]);
        properties.insert("OT".to_owned(), vec!["0x30s byo-yomi".to_owned()]);
        assert_eq!(TimeControl::from_sgf(&properties), TimeControl::None);

        // Zero period length: likewise not a real byo-yomi.
        let mut properties = BTreeMap::new();
        properties.insert("TM".to_owned(), vec!["600".to_owned()]);
        properties.insert("OT".to_owned(), vec!["5x0s byo-yomi".to_owned()]);
        assert_eq!(TimeControl::from_sgf(&properties), TimeControl::None);
    }

    #[test]
    fn consume_byo_yomi_never_spins_on_zero_length_period() {
        // Force the byo-yomi clock into a state where the current period length
        // is zero; consume must terminate immediately instead of looping.
        let mut clock = ClockController::new(TimeControl::ByoYomi {
            main_time_secs: 0,
            period_time_secs: 0,
            periods: 1000,
        });
        clock.start(Color::Black);
        assert_eq!(
            clock.tick(Duration::from_secs(1)),
            Some(ClockEvent::Expired(Color::Black))
        );
        assert_eq!(clock.state().expired, Some(Color::Black));
    }

    #[test]
    fn consume_byo_yomi_consumes_multiple_periods_in_one_tick() {
        let mut clock = ClockController::new(TimeControl::ByoYomi {
            main_time_secs: 0,
            period_time_secs: 5,
            periods: 3,
        });
        clock.start(Color::Black);
        // 12s of elapsed time spans two full periods (10s) plus 2s of the third.
        assert_eq!(clock.tick(Duration::from_secs(12)), None);
        let state = clock.state();
        assert_eq!(state.black.periods_remaining, 1);
        assert_eq!(state.black.byo_yomi_time_remaining, Duration::from_secs(3));
    }

    #[test]
    fn consume_byo_yomi_exactly_consumes_final_period() {
        let mut clock = ClockController::new(TimeControl::ByoYomi {
            main_time_secs: 0,
            period_time_secs: 5,
            periods: 2,
        });
        clock.start(Color::Black);
        // Exactly 10s uses both periods and expires.
        assert_eq!(
            clock.tick(Duration::from_secs(10)),
            Some(ClockEvent::Expired(Color::Black))
        );
        assert_eq!(clock.state().expired, Some(Color::Black));
    }
}
