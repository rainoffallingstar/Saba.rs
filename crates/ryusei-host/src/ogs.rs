//! Online Go Server match protocol boundary.
//!
//! Authentication and WebSocket transport live behind `OgsTransport`; this
//! module never persists a token. The server owns clock and move authority for
//! every remote competition game, and fair-play policy stays locked throughout
//! an active human match.

use std::time::Duration;

use ryusei_domain_core::{
    AnalysisPolicy, ClockPhase, ClockState, Color, PlayerClock, SessionMode, SessionPolicy,
    SessionSource, TimeControl,
};
use serde::{Deserialize, Serialize};

pub const OGS_GAME_API_ROOT: &str = "https://online-go.com/api/v1/games";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OgsMoveSubmission {
    pub game_id: u64,
    pub move_number: u32,
    /// OGS protocol coordinate; `pass` is represented by `None`.
    pub vertex: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OgsServerClock {
    pub black_main_remaining: Duration,
    pub white_main_remaining: Duration,
    pub black_byo_yomi_remaining: Duration,
    pub white_byo_yomi_remaining: Duration,
    pub black_periods: u32,
    pub white_periods: u32,
    pub active_color: Option<Color>,
    pub paused: bool,
}

impl OgsServerClock {
    pub fn to_clock_state(self, control: TimeControl) -> ClockState {
        let phase_for = |main_remaining: Duration, periods: u32| match control {
            TimeControl::ByoYomi { .. } if main_remaining.is_zero() && periods > 0 => {
                ClockPhase::ByoYomi
            }
            _ => ClockPhase::MainTime,
        };
        ClockState {
            control,
            black: PlayerClock {
                main_time_remaining: self.black_main_remaining,
                byo_yomi_time_remaining: self.black_byo_yomi_remaining,
                periods_remaining: self.black_periods,
                phase: phase_for(self.black_main_remaining, self.black_periods),
            },
            white: PlayerClock {
                main_time_remaining: self.white_main_remaining,
                byo_yomi_time_remaining: self.white_byo_yomi_remaining,
                periods_remaining: self.white_periods,
                phase: phase_for(self.white_main_remaining, self.white_periods),
            },
            active_color: self.active_color,
            running: self.active_color.is_some() && !self.paused,
            paused: self.paused,
            expired: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OgsGameUpdate {
    pub game_id: u64,
    pub move_number: u32,
    pub next_player: Color,
    pub clock: OgsServerClock,
}

pub trait OgsTransport {
    /// The implementation owns transient authentication (if needed); callers
    /// never provide nor persist an OGS password or bearer token here.
    fn submit_move(&mut self, submission: &OgsMoveSubmission) -> Result<OgsGameUpdate, String>;
    fn fetch_game(&mut self, game_id: u64) -> Result<OgsGameUpdate, String>;
}

#[derive(Clone, Debug)]
pub struct OgsCompetitionSession {
    pub game_id: u64,
    pub policy: SessionPolicy,
    pub move_number: u32,
    pub next_player: Color,
    pub server_clock: OgsServerClock,
}

impl OgsCompetitionSession {
    pub fn new(game_id: u64, update: OgsGameUpdate) -> Result<Self, OgsError> {
        if update.game_id != game_id {
            return Err(OgsError::GameIdMismatch {
                expected: game_id,
                actual: update.game_id,
            });
        }
        Ok(Self {
            game_id,
            policy: SessionPolicy::new(SessionMode::Match, SessionSource::RemoteCompetition)
                .lock_fair_play(true),
            move_number: update.move_number,
            next_player: update.next_player,
            server_clock: update.clock,
        })
    }

    pub fn apply_server_update(&mut self, update: OgsGameUpdate) -> Result<(), OgsError> {
        if update.game_id != self.game_id {
            return Err(OgsError::GameIdMismatch {
                expected: self.game_id,
                actual: update.game_id,
            });
        }
        if update.move_number < self.move_number {
            return Err(OgsError::StaleServerUpdate {
                current: self.move_number,
                received: update.move_number,
            });
        }
        self.move_number = update.move_number;
        self.next_player = update.next_player;
        self.server_clock = update.clock;
        Ok(())
    }

    pub fn analysis_allowed(&self) -> bool {
        self.policy.analysis != AnalysisPolicy::FairPlayLockedOff
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum OgsError {
    #[error("OGS update is for game {actual}, expected game {expected}")]
    GameIdMismatch { expected: u64, actual: u64 },
    #[error("OGS update is stale: current move {current}, received {received}")]
    StaleServerUpdate { current: u32, received: u32 },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn update(game_id: u64, move_number: u32) -> OgsGameUpdate {
        OgsGameUpdate {
            game_id,
            move_number,
            next_player: Color::White,
            clock: OgsServerClock {
                black_main_remaining: Duration::from_secs(532),
                white_main_remaining: Duration::from_secs(581),
                black_byo_yomi_remaining: Duration::from_secs(30),
                white_byo_yomi_remaining: Duration::from_secs(30),
                black_periods: 5,
                white_periods: 5,
                active_color: Some(Color::White),
                paused: false,
            },
        }
    }

    #[test]
    fn human_ogs_competition_locks_analysis_and_uses_server_clock() {
        let session = OgsCompetitionSession::new(17, update(17, 12)).expect("game matches");
        assert!(!session.analysis_allowed());
        let state = session.server_clock.to_clock_state(TimeControl::ByoYomi {
            main_time_secs: 600,
            period_time_secs: 30,
            periods: 5,
        });
        assert_eq!(state.active_color, Some(Color::White));
        assert_eq!(state.black.main_time_remaining, Duration::from_secs(532));
        assert_eq!(state.black.byo_yomi_time_remaining, Duration::from_secs(30));
    }

    #[test]
    fn rejects_out_of_order_ogs_updates() {
        let mut session = OgsCompetitionSession::new(17, update(17, 12)).expect("game matches");
        assert!(matches!(
            session.apply_server_update(update(17, 11)),
            Err(OgsError::StaleServerUpdate { .. })
        ));
    }
}
