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
use serde_json::Value;
use url::Url;

pub const OGS_GAME_API_ROOT: &str = "https://online-go.com/api/v1/games";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OgsPublicGameState {
    pub game_id: u64,
    pub name: String,
    pub phase: String,
    pub move_number: u32,
    pub black_name: String,
    pub white_name: String,
    pub next_player: Option<Color>,
    pub outcome: Option<String>,
}

pub fn ogs_game_id_from_public_url(value: &str) -> Option<u64> {
    let url = Url::parse(value.trim()).ok()?;
    ogs_game_id_from_url(&url)
}

pub fn ogs_game_id_from_url(url: &Url) -> Option<u64> {
    if !matches!(url.host_str(), Some("online-go.com" | "www.online-go.com")) {
        return None;
    }
    let segments = url.path_segments()?.collect::<Vec<_>>();
    let game_id = match segments.as_slice() {
        ["game", game_id]
        | ["api", "v1", "games", game_id]
        | ["api", "v1", "games", game_id, "sgf"] => *game_id,
        _ => return None,
    };
    game_id.parse::<u64>().ok().filter(|game_id| *game_id > 0)
}

pub fn ogs_public_game_api_url(game_id: u64) -> String {
    format!("{OGS_GAME_API_ROOT}/{game_id}")
}

pub fn parse_ogs_public_game(body: &str) -> Result<OgsPublicGameState, String> {
    let value: Value =
        serde_json::from_str(body).map_err(|error| format!("invalid OGS game JSON: {error}"))?;
    let gamedata = value.get("gamedata").unwrap_or(&value);
    let game_id = value
        .get("id")
        .or_else(|| gamedata.get("game_id"))
        .and_then(Value::as_u64)
        .ok_or_else(|| "OGS game response is missing id".to_owned())?;
    let players = value.get("players").or_else(|| gamedata.get("players"));
    let player_name = |color: &str| {
        players
            .and_then(|players| players.get(color))
            .and_then(|player| player.get("username").or_else(|| player.get("name")))
            .and_then(Value::as_str)
            .unwrap_or(color)
            .to_owned()
    };
    let black_id = gamedata
        .get("black_player_id")
        .or_else(|| value.get("black"))
        .and_then(Value::as_u64);
    let white_id = gamedata
        .get("white_player_id")
        .or_else(|| value.get("white"))
        .and_then(Value::as_u64);
    let current_player = gamedata
        .get("clock")
        .and_then(|clock| clock.get("current_player"))
        .and_then(Value::as_u64);
    let next_player = match current_player {
        Some(id) if Some(id) == black_id => Some(Color::Black),
        Some(id) if Some(id) == white_id => Some(Color::White),
        _ => None,
    };
    let moves = gamedata
        .get("moves")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    Ok(OgsPublicGameState {
        game_id,
        name: value
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("OGS Game")
            .to_owned(),
        phase: gamedata
            .get("phase")
            .or_else(|| value.get("phase"))
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_owned(),
        move_number: u32::try_from(moves).unwrap_or(u32::MAX),
        black_name: player_name("black"),
        white_name: player_name("white"),
        next_player,
        outcome: gamedata
            .get("outcome")
            .or_else(|| value.get("outcome"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
    })
}

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

pub trait OgsPublicGameFetch {
    fn fetch_public_game(&mut self, game_id: u64) -> Result<OgsPublicGameState, String>;
}

pub struct CurlOgsPublicGameFetch;

impl OgsPublicGameFetch for CurlOgsPublicGameFetch {
    fn fetch_public_game(&mut self, game_id: u64) -> Result<OgsPublicGameState, String> {
        let url = ogs_public_game_api_url(game_id);
        let output = std::process::Command::new("curl")
            .args([
                "--fail",
                "--silent",
                "--show-error",
                "--max-time",
                "15",
                "--max-redirs",
                "0",
                "--proto",
                "=https",
                &url,
            ])
            .output()
            .map_err(|error| format!("curl command failed: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "OGS game request failed with exit code {:?}",
                output.status.code()
            ));
        }
        let body = String::from_utf8(output.stdout)
            .map_err(|error| format!("OGS response was not UTF-8: {error}"))?;
        let state = parse_ogs_public_game(&body)?;
        if state.game_id != game_id {
            return Err(format!(
                "OGS response game id {} does not match requested {game_id}",
                state.game_id
            ));
        }
        Ok(state)
    }
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
    fn extracts_public_game_state_without_authentication() {
        let body = r#"{
            "id": 42,
            "name": "Public game",
            "phase": "play",
            "black": 7,
            "white": 8,
            "players": {
                "black": {"username": "Black"},
                "white": {"username": "White"}
            },
            "gamedata": {
                "phase": "play",
                "black_player_id": 7,
                "white_player_id": 8,
                "moves": [[15, 15, 1], [3, 3, 2]],
                "clock": {"current_player": 7}
            }
        }"#;
        let state = parse_ogs_public_game(body).expect("public game parses");
        assert_eq!(state.game_id, 42);
        assert_eq!(state.move_number, 2);
        assert_eq!(state.black_name, "Black");
        assert_eq!(state.white_name, "White");
        assert_eq!(state.next_player, Some(Color::Black));
        assert_eq!(state.phase, "play");
    }

    #[test]
    fn maps_only_canonical_public_ogs_game_urls() {
        let game = Url::parse("https://online-go.com/game/42").unwrap();
        let sgf = Url::parse("https://www.online-go.com/api/v1/games/42/sgf").unwrap();
        let other = Url::parse("https://example.org/game/42").unwrap();
        assert_eq!(ogs_game_id_from_url(&game), Some(42));
        assert_eq!(ogs_game_id_from_url(&sgf), Some(42));
        assert_eq!(ogs_game_id_from_url(&other), None);
        assert_eq!(
            ogs_public_game_api_url(42),
            "https://online-go.com/api/v1/games/42"
        );
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
