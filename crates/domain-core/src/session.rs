//! Top-level session vocabulary, separate from board interaction tools.

use serde::{Deserialize, Serialize};

use crate::Color;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SessionMode {
    #[default]
    Match,
    Record,
    Live,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AnalysisPolicy {
    Off,
    #[default]
    Manual,
    Continuous,
    FairPlayLockedOff,
}

impl SessionMode {
    pub fn default_analysis_policy(self) -> AnalysisPolicy {
        match self {
            Self::Match => AnalysisPolicy::Off,
            Self::Record => AnalysisPolicy::Manual,
            Self::Live => AnalysisPolicy::Continuous,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SessionSource {
    #[default]
    Local,
    Library,
    RemoteCompetition,
    LiveBroadcast,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionPolicy {
    pub mode: SessionMode,
    pub source: SessionSource,
    pub analysis: AnalysisPolicy,
    pub participants: MatchParticipants,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PlayerKind {
    #[default]
    Human,
    Ai,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchParticipants {
    pub black: PlayerKind,
    pub white: PlayerKind,
}

impl MatchParticipants {
    pub const fn human_vs_human() -> Self {
        Self {
            black: PlayerKind::Human,
            white: PlayerKind::Human,
        }
    }

    pub const fn human_vs_ai() -> Self {
        Self {
            black: PlayerKind::Human,
            white: PlayerKind::Ai,
        }
    }

    pub const fn ai_vs_ai() -> Self {
        Self {
            black: PlayerKind::Ai,
            white: PlayerKind::Ai,
        }
    }

    pub const fn player(self, color: Color) -> PlayerKind {
        match color {
            Color::Black => self.black,
            Color::White => self.white,
        }
    }

    pub const fn label(self) -> &'static str {
        match (self.black, self.white) {
            (PlayerKind::Human, PlayerKind::Human) => "人人对弈",
            (PlayerKind::Human, PlayerKind::Ai) => "人机对弈（黑方人类）",
            (PlayerKind::Ai, PlayerKind::Human) => "人机对弈（白方人类）",
            (PlayerKind::Ai, PlayerKind::Ai) => "AI 对弈",
        }
    }
}

impl SessionPolicy {
    pub fn new(mode: SessionMode, source: SessionSource) -> Self {
        Self {
            mode,
            source,
            analysis: mode.default_analysis_policy(),
            participants: MatchParticipants::human_vs_human(),
        }
    }

    /// Human remote competition is always analysis-free until the game ends.
    pub fn lock_fair_play(mut self, locked: bool) -> Self {
        if locked && self.source == SessionSource::RemoteCompetition {
            self.analysis = AnalysisPolicy::FairPlayLockedOff;
        } else if self.analysis == AnalysisPolicy::FairPlayLockedOff {
            self.analysis = self.mode.default_analysis_policy();
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_modes_have_distinct_default_analysis_policies() {
        assert_eq!(
            SessionMode::Match.default_analysis_policy(),
            AnalysisPolicy::Off
        );
        assert_eq!(
            SessionMode::Record.default_analysis_policy(),
            AnalysisPolicy::Manual
        );
        assert_eq!(
            SessionMode::Live.default_analysis_policy(),
            AnalysisPolicy::Continuous
        );
    }

    #[test]
    fn match_participants_distinguish_human_and_ai_turns() {
        let human_vs_ai = MatchParticipants::human_vs_ai();
        assert_eq!(human_vs_ai.player(Color::Black), PlayerKind::Human);
        assert_eq!(human_vs_ai.player(Color::White), PlayerKind::Ai);
        assert_eq!(human_vs_ai.label(), "人机对弈（黑方人类）");
        assert_eq!(
            MatchParticipants::ai_vs_ai().player(Color::Black),
            PlayerKind::Ai
        );
    }

    #[test]
    fn remote_competition_locks_analysis() {
        let policy = SessionPolicy::new(SessionMode::Match, SessionSource::RemoteCompetition)
            .lock_fair_play(true);
        assert_eq!(policy.analysis, AnalysisPolicy::FairPlayLockedOff);
        assert_eq!(policy.lock_fair_play(false).analysis, AnalysisPolicy::Off);
    }
}
