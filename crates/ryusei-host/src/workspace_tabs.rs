//! Persistable workspace tabs for multiple independent SGF sessions.
//!
//! The GPUI shell owns the active `HostApplication`; inactive tabs are held as
//! serialized SGF snapshots so a background tab cannot accidentally share a
//! mutable game tree, clock, or transient editor state with the active one.

use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize};

use ryusei_domain_core::{ClockState, GameMode, SessionPolicy, TimeControl, Vertex};

use crate::{AnalysisEntry, SourceEncoding};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceTab {
    pub id: String,
    pub title: String,
    pub sgf: String,
    pub source_path: Option<String>,
    pub source_encoding: SourceEncoding,
    /// Whether the snapshot contains edits not written to `source_path`.
    #[serde(default)]
    pub is_dirty: bool,
    /// Fingerprint of the last accepted on-disk source contents.
    #[serde(default)]
    pub source_fingerprint: Option<String>,
    /// Node selected when this tab was captured; SGF alone does not encode it.
    #[serde(default)]
    pub current_node_id: Option<String>,
    pub clock: ClockState,
    pub policy: SessionPolicy,
    #[serde(default = "default_game_mode")]
    pub mode: GameMode,
    #[serde(default)]
    pub last_vertex: Option<Vertex>,
    #[serde(default)]
    pub analysis_enabled: bool,
    #[serde(default)]
    pub analysis: Vec<AnalysisEntry>,
    #[serde(default)]
    pub analysis_best_move: Option<Vertex>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceTabs {
    tabs: Vec<WorkspaceTab>,
    active_tab_id: String,
    next_tab_number: u64,
}

impl WorkspaceTabs {
    /// Validates that this workspace satisfies every structural invariant that
    /// the rest of the host relies on. Used both at deserialization time (so a
    /// corrupt or hand-edited file is rejected up front) and as an explicit
    /// post-load check by callers that read from external sources.
    ///
    /// Rejects: empty `tabs`, empty or duplicated tab ids, and an
    /// `active_tab_id` that is empty or does not resolve to a live tab.
    pub fn validate(&self) -> Result<(), WorkspaceTabError> {
        if self.tabs.is_empty() {
            return Err(WorkspaceTabError::EmptyTabs);
        }
        let mut seen = std::collections::HashSet::new();
        for tab in &self.tabs {
            if tab.id.is_empty() {
                return Err(WorkspaceTabError::EmptyTabId);
            }
            if !seen.insert(&tab.id) {
                return Err(WorkspaceTabError::DuplicateTabId(tab.id.clone()));
            }
        }
        if self.active_tab_id.is_empty() {
            return Err(WorkspaceTabError::EmptyActiveTabId);
        }
        if !seen.contains(&self.active_tab_id) {
            return Err(WorkspaceTabError::UnknownActiveTab(
                self.active_tab_id.clone(),
            ));
        }
        Ok(())
    }

    /// Non-panicking accessor for the active tab. Prefer this over
    /// [`Self::active_tab`] on any path that consumes externally supplied
    /// data, where an out-of-range `active_tab_id` must not become a panic.
    pub fn try_active_tab(&self) -> Result<&WorkspaceTab, WorkspaceTabError> {
        if self.active_tab_id.is_empty() {
            return Err(WorkspaceTabError::EmptyActiveTabId);
        }
        self.tabs
            .iter()
            .find(|tab| tab.id == self.active_tab_id)
            .ok_or_else(|| WorkspaceTabError::UnknownActiveTab(self.active_tab_id.clone()))
    }

    /// Validated deserialization: parses the raw JSON payload without the
    /// strict `Deserialize` guard, then runs [`Self::validate`], surfacing the
    /// specific invariant violation rather than a generic parse error. This is
    /// the entry point for callers that need to distinguish *why* a payload
    /// was rejected.
    pub fn deserialize_validated(json: &str) -> Result<Self, WorkspaceTabError> {
        let raw: RawWorkspaceTabs = serde_json::from_str(json)
            .map_err(|error| WorkspaceTabError::InvalidJson(error.to_string()))?;
        let tabs = WorkspaceTabs::from(raw);
        tabs.validate()?;
        Ok(tabs)
    }

    pub fn new(initial_sgf: String, title: impl Into<String>) -> Self {
        let id = "session-1".to_owned();
        Self {
            tabs: vec![WorkspaceTab {
                id: id.clone(),
                title: sanitize_title(title.into()),
                sgf: initial_sgf,
                source_path: None,
                source_encoding: SourceEncoding::Utf8,
                is_dirty: false,
                source_fingerprint: None,
                current_node_id: None,
                clock: ClockState::new(TimeControl::None),
                policy: SessionPolicy::default(),
                mode: GameMode::Play,
                last_vertex: None,
                analysis_enabled: false,
                analysis: Vec::new(),
                analysis_best_move: None,
            }],
            active_tab_id: id,
            next_tab_number: 2,
        }
    }

    pub fn tabs(&self) -> &[WorkspaceTab] {
        &self.tabs
    }

    pub fn active_tab_id(&self) -> &str {
        &self.active_tab_id
    }

    pub fn active_tab(&self) -> &WorkspaceTab {
        self.tabs
            .iter()
            .find(|tab| tab.id == self.active_tab_id)
            .expect("workspace tabs always retain an active tab")
    }

    #[allow(clippy::too_many_arguments)]
    pub fn capture_active(
        &mut self,
        sgf: String,
        title: impl Into<String>,
        source_path: Option<String>,
        source_encoding: SourceEncoding,
        is_dirty: bool,
        source_fingerprint: Option<String>,
        current_node_id: Option<String>,
        clock: ClockState,
        policy: SessionPolicy,
        mode: GameMode,
        last_vertex: Option<Vertex>,
        analysis_enabled: bool,
        analysis: Vec<AnalysisEntry>,
        analysis_best_move: Option<Vertex>,
    ) {
        let active_id = self.active_tab_id.clone();
        let tab = self
            .tabs
            .iter_mut()
            .find(|tab| tab.id == active_id)
            .expect("workspace tabs always retain an active tab");
        tab.sgf = sgf;
        tab.title = sanitize_title(title.into());
        tab.source_path = source_path;
        tab.source_encoding = source_encoding;
        tab.is_dirty = is_dirty;
        tab.source_fingerprint = source_fingerprint;
        tab.current_node_id = current_node_id;
        tab.clock = clock;
        tab.policy = policy;
        tab.mode = mode;
        tab.last_vertex = last_vertex;
        tab.analysis_enabled = analysis_enabled;
        tab.analysis = analysis;
        tab.analysis_best_move = analysis_best_move;
    }

    pub fn create_tab(
        &mut self,
        sgf: String,
        title: impl Into<String>,
        source_encoding: SourceEncoding,
    ) -> &WorkspaceTab {
        let id = format!("session-{}", self.next_tab_number);
        self.next_tab_number += 1;
        self.tabs.push(WorkspaceTab {
            id: id.clone(),
            title: sanitize_title(title.into()),
            sgf,
            source_path: None,
            source_encoding,
            is_dirty: false,
            source_fingerprint: None,
            current_node_id: None,
            clock: ClockState::new(TimeControl::None),
            policy: SessionPolicy::default(),
            mode: GameMode::Play,
            last_vertex: None,
            analysis_enabled: false,
            analysis: Vec::new(),
            analysis_best_move: None,
        });
        self.active_tab_id = id;
        self.active_tab()
    }

    /// Returns an immutable snapshot without changing the active selection.
    /// Callers can restore and validate it before committing activation.
    pub fn tab_snapshot(&self, id: &str) -> Result<WorkspaceTab, WorkspaceTabError> {
        self.tabs
            .iter()
            .find(|tab| tab.id == id)
            .cloned()
            .ok_or_else(|| WorkspaceTabError::UnknownTab(id.to_owned()))
    }

    /// Activates an existing background session and returns its complete SGF
    /// snapshot for the shell to restore.
    pub fn activate(&mut self, id: &str) -> Result<WorkspaceTab, WorkspaceTabError> {
        let tab = self.tab_snapshot(id)?;
        self.active_tab_id = tab.id.clone();
        Ok(tab)
    }

    /// Closes a non-last tab and returns the next tab to load when the active
    /// session was closed. The final workspace tab is intentionally retained.
    pub fn close(&mut self, id: &str) -> Result<Option<WorkspaceTab>, WorkspaceTabError> {
        if self.tabs.len() == 1 {
            return Err(WorkspaceTabError::CannotCloseLastTab);
        }
        let index = self
            .tabs
            .iter()
            .position(|tab| tab.id == id)
            .ok_or_else(|| WorkspaceTabError::UnknownTab(id.to_owned()))?;
        let was_active = self.active_tab_id == id;
        self.tabs.remove(index);
        if !was_active {
            return Ok(None);
        }
        let next_index = index.min(self.tabs.len() - 1);
        let next = self.tabs[next_index].clone();
        self.active_tab_id = next.id.clone();
        Ok(Some(next))
    }
}

/// Structural (non-validating) view of a persisted workspace used to parse
/// arbitrary JSON before the invariant checks in [`WorkspaceTabs::validate`]
/// decide whether it is acceptable.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawWorkspaceTabs {
    tabs: Vec<WorkspaceTab>,
    active_tab_id: String,
    next_tab_number: u64,
}

impl From<RawWorkspaceTabs> for WorkspaceTabs {
    fn from(raw: RawWorkspaceTabs) -> Self {
        Self {
            tabs: raw.tabs,
            active_tab_id: raw.active_tab_id,
            next_tab_number: raw.next_tab_number,
        }
    }
}

/// Custom deserializer that rejects structurally invalid workspaces at parse
/// time. The JSON shape is unchanged from the derived implementation, so
/// existing persisted files continue to load; the extra step is purely the
/// invariant check from [`WorkspaceTabs::validate`].
impl<'de> Deserialize<'de> for WorkspaceTabs {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawWorkspaceTabs::deserialize(deserializer)?;
        let tabs = WorkspaceTabs::from(raw);
        tabs.validate().map_err(de::Error::custom)?;
        Ok(tabs)
    }
}

fn default_game_mode() -> GameMode {
    GameMode::Play
}

fn sanitize_title(title: String) -> String {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        "Untitled Game".to_owned()
    } else {
        trimmed.chars().take(48).collect()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum WorkspaceTabError {
    #[error("unknown workspace tab `{0}`")]
    UnknownTab(String),
    #[error("the last workspace session cannot be closed")]
    CannotCloseLastTab,
    #[error("workspace contains no tabs")]
    EmptyTabs,
    #[error("workspace tab has an empty id")]
    EmptyTabId,
    #[error("duplicate workspace tab id `{0}`")]
    DuplicateTabId(String),
    #[error("workspace has an empty active tab id")]
    EmptyActiveTabId,
    #[error("active tab id `{0}` does not reference a live tab")]
    UnknownActiveTab(String),
    #[error("invalid workspace tabs payload: {0}")]
    InvalidJson(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabs_keep_background_sgf_independent_from_the_active_session() {
        let mut tabs = WorkspaceTabs::new("(;SZ[19])".to_owned(), "Opening");
        tabs.capture_active(
            "(;SZ[19];B[pd])".to_owned(),
            "Opening game",
            Some("/games/opening.sgf".to_owned()),
            SourceEncoding::ShiftJis,
            true,
            Some("fingerprint".to_owned()),
            Some("node-2".to_owned()),
            ClockState::new(TimeControl::Absolute {
                main_time_secs: 600,
            }),
            SessionPolicy::default(),
            GameMode::Edit,
            Some(Vertex { column: 3, row: 3 }),
            true,
            vec![],
            Some(Vertex { column: 4, row: 4 }),
        );
        let created = tabs
            .create_tab("(;SZ[9])".to_owned(), "Study", SourceEncoding::Utf8)
            .clone();
        let restored = tabs.activate("session-1").expect("first tab exists");

        assert_eq!(created.id, "session-2");
        assert_eq!(restored.sgf, "(;SZ[19];B[pd])");
        assert_eq!(restored.source_path.as_deref(), Some("/games/opening.sgf"));
        assert_eq!(restored.source_encoding, SourceEncoding::ShiftJis);
        assert!(restored.is_dirty);
        assert_eq!(restored.source_fingerprint.as_deref(), Some("fingerprint"));
        assert_eq!(restored.current_node_id.as_deref(), Some("node-2"));
        assert_eq!(
            restored.clock.control,
            TimeControl::Absolute {
                main_time_secs: 600
            }
        );
        assert_eq!(tabs.active_tab_id(), "session-1");
        assert_eq!(restored.mode, GameMode::Edit);
        assert!(restored.analysis_enabled);
        assert_eq!(
            restored.analysis_best_move,
            Some(Vertex { column: 4, row: 4 })
        );
    }

    #[test]
    fn tab_snapshot_does_not_commit_activation() {
        let mut tabs = WorkspaceTabs::new("(;SZ[19])".to_owned(), "One");
        tabs.create_tab("(;SZ[13])".to_owned(), "Two", SourceEncoding::Utf8);
        let snapshot = tabs.tab_snapshot("session-1").expect("tab exists");
        assert_eq!(snapshot.id, "session-1");
        assert_eq!(tabs.active_tab_id(), "session-2");
    }

    #[test]
    fn closing_the_active_tab_selects_an_adjacent_background_tab() {
        let mut tabs = WorkspaceTabs::new("(;SZ[19])".to_owned(), "One");
        tabs.create_tab("(;SZ[13])".to_owned(), "Two", SourceEncoding::Utf8);
        let replacement = tabs
            .close("session-2")
            .expect("tab closes")
            .expect("replacement");
        assert_eq!(replacement.id, "session-1");
        assert_eq!(tabs.active_tab_id(), "session-1");
        assert!(matches!(
            tabs.close("session-1"),
            Err(WorkspaceTabError::CannotCloseLastTab)
        ));
    }

    #[test]
    fn serialized_tabs_restore_the_active_selection() {
        let mut tabs = WorkspaceTabs::new("(;SZ[19])".to_owned(), "One");
        tabs.create_tab("(;SZ[13])".to_owned(), "Two", SourceEncoding::Utf8);
        let json = serde_json::to_string(&tabs).expect("tabs serialize");
        let restored: WorkspaceTabs = serde_json::from_str(&json).expect("tabs deserialize");
        assert_eq!(restored.active_tab().title, "Two");
    }

    #[test]
    fn valid_workspace_passes_validation() {
        let mut tabs = WorkspaceTabs::new("(;SZ[19])".to_owned(), "One");
        tabs.create_tab("(;SZ[13])".to_owned(), "Two", SourceEncoding::Utf8);
        assert!(tabs.validate().is_ok());
        assert_eq!(tabs.try_active_tab().expect("active tab").id, "session-2");
        assert_eq!(tabs.active_tab().id, "session-2");
    }

    /// Serializes a valid workspace, then mutates the resulting JSON to break a
    /// single invariant, so every payload carries all required `WorkspaceTab`
    /// fields and the test targets only the invariant under test.
    fn mutate_workspace_json(mutate: impl FnOnce(&mut serde_json::Value)) -> String {
        let mut tabs = WorkspaceTabs::new("(;SZ[19])".to_owned(), "One");
        tabs.create_tab("(;SZ[13])".to_owned(), "Two", SourceEncoding::Utf8);
        let mut value: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&tabs).expect("tabs serialize"))
                .expect("serialized output parses");
        mutate(&mut value);
        value.to_string()
    }

    #[test]
    fn empty_tabs_are_rejected() {
        let json = mutate_workspace_json(|value| {
            value["tabs"] = serde_json::json!([]);
            value["activeTabId"] = serde_json::json!("session-2");
        });
        assert!(serde_json::from_str::<WorkspaceTabs>(&json).is_err());
        assert!(matches!(
            WorkspaceTabs::deserialize_validated(&json),
            Err(WorkspaceTabError::EmptyTabs)
        ));
    }

    #[test]
    fn empty_active_tab_id_is_rejected() {
        let json = mutate_workspace_json(|value| value["activeTabId"] = serde_json::json!(""));
        assert!(serde_json::from_str::<WorkspaceTabs>(&json).is_err());
        assert!(matches!(
            WorkspaceTabs::deserialize_validated(&json),
            Err(WorkspaceTabError::EmptyActiveTabId)
        ));
    }

    #[test]
    fn active_tab_id_missing_from_tabs_is_rejected() {
        let json = mutate_workspace_json(|value| {
            value["activeTabId"] = serde_json::json!("session-99");
        });
        let error = serde_json::from_str::<WorkspaceTabs>(&json).expect_err("must be rejected");
        assert!(error.to_string().contains("session-99"));
        assert!(matches!(
            WorkspaceTabs::deserialize_validated(&json),
            Err(WorkspaceTabError::UnknownActiveTab(id)) if id == "session-99"
        ));
    }

    #[test]
    fn duplicate_tab_ids_are_rejected() {
        let json = mutate_workspace_json(|value| {
            value["tabs"][1]["id"] = serde_json::json!("session-1");
        });
        assert!(serde_json::from_str::<WorkspaceTabs>(&json).is_err());
        assert!(matches!(
            WorkspaceTabs::deserialize_validated(&json),
            Err(WorkspaceTabError::DuplicateTabId(id)) if id == "session-1"
        ));
    }

    #[test]
    fn empty_tab_id_is_rejected() {
        let json = mutate_workspace_json(|value| value["tabs"][0]["id"] = serde_json::json!(""));
        assert!(serde_json::from_str::<WorkspaceTabs>(&json).is_err());
        assert!(matches!(
            WorkspaceTabs::deserialize_validated(&json),
            Err(WorkspaceTabError::EmptyTabId)
        ));
    }

    #[test]
    fn try_active_tab_is_non_panicking_on_invalid_state() {
        // Even a hand-constructed broken value must not panic through the
        // fallible accessor.
        let tabs = WorkspaceTabs {
            tabs: vec![WorkspaceTab {
                id: "session-1".to_owned(),
                title: "One".to_owned(),
                sgf: "(;SZ[19])".to_owned(),
                source_path: None,
                source_encoding: SourceEncoding::Utf8,
                is_dirty: false,
                source_fingerprint: None,
                current_node_id: None,
                clock: ClockState::new(TimeControl::None),
                policy: SessionPolicy::default(),
                mode: GameMode::Play,
                last_vertex: None,
                analysis_enabled: false,
                analysis: Vec::new(),
                analysis_best_move: None,
            }],
            active_tab_id: "session-99".to_owned(),
            next_tab_number: 2,
        };
        assert!(matches!(
            tabs.try_active_tab(),
            Err(WorkspaceTabError::UnknownActiveTab(id)) if id == "session-99"
        ));
        assert!(matches!(
            tabs.validate(),
            Err(WorkspaceTabError::UnknownActiveTab(_))
        ));
    }
}
