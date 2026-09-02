#[allow(dead_code)]
mod benchmark;
mod dialog_service;
mod engine_console;
mod external_file;
mod file_workflow;
mod goban_view;
mod icons;
mod layout;
mod markdown;
mod markup;
mod native_text_input;
mod navigation;
mod navigation_rail;
mod node_inspector;
mod panels;
mod plugin_contribution;
mod plugin_panel;
mod settings;
mod settings_form;
mod sound_feedback;
mod text_inputs;
mod theme;
mod ui_format;
mod variation_tree;
mod winrate_graph;

use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

use gpui::{
    Animation, AnimationExt as _, App, Application, Bounds, Context, Div, Entity, FontWeight,
    InteractiveElement, KeyBinding, Menu, MenuItem, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, SharedString, Task, TitlebarOptions, Window, WindowBounds, WindowOptions,
    actions, div, ease_out_quint, point, prelude::*, px, rgb, size,
};
use ryusei_domain_core::gtp::AnalysisStream;
use ryusei_domain_core::legacy::handicap_placement;
use ryusei_domain_core::{
    AnalysisPolicy, ClockController, ClockEvent, Color, GameMode, MatchParticipants, MoveDto,
    OpeningConvention, PlayerKind, SessionMode, SessionPolicy, SessionSource, TimeControl, Vertex,
};
use ryusei_host::{HostPersistence, OgsPublicGameFetch, replay_position_stream_commands};

use crate::dialog_service::{DialogService, NativeGameFileAccess, RfdDialogService};
use crate::engine_console::{
    EngineLogEntry, EngineRole, EngineRoleAssignments, analysis_command_from_settings,
    best_analysis_entry, best_analysis_move, entry_for_response, format_console_command,
    merge_analysis_entries, parse_gtp_vertex, parse_stream_entries,
};
use crate::external_file::{ExternalCheckOutcome, check_external_file, track_after_file_operation};
use crate::file_workflow::{
    NativeHostPersistence, NativePluginPersistence, NativeSettingsPersistence, capture_autosave,
    clear_autosave, record_opened_file,
};
use crate::goban_view::format_sgf_vertex;
use crate::layout::{
    SplitPane, clamp_pane_size, pane_size_for_drag, pane_size_from_settings, right_pane_visible,
};
use crate::markup::{
    MarkupTool, create_clear_markup_transactions, create_line_transaction,
    create_markup_transaction, create_scoring_transaction, create_setup_transactions,
    next_scoring_override,
};
use crate::native_text_input::{InputKeyResult, NativeTextInput};
use crate::navigation::{
    NavigationDirection, navigation_availability, navigation_target, position_label,
};
use crate::node_inspector::{
    create_comment_transaction, create_hotspot_transaction, current_node_metadata,
};
use crate::plugin_panel::{PluginPanelEntry, apply_process_info, entry_from_record};
use crate::settings::{
    ThemeChoice, theme_from_setting, window_bounds_from_settings, window_maximized_from_settings,
};
use crate::settings_form::editable_setting_value;
use crate::settings_form::{
    SettingEdit, SettingRow, apply_setting_edit, number_edit, panel_setting_rows,
    string_array_edit, toggle_boolean_edit,
};
use crate::sound_feedback::{SoundCue, SoundSink, platform_sound_sink, play_if_enabled};
use crate::text_inputs::TextInputs;
use crate::theme::{ThemeTokens, UiPalette, ui_palette};
use crate::variation_tree::build_variation_tree_layout;
use crate::winrate_graph::{
    CANDIDATES_PROPERTY, WinrateGraphMetric, analysis_sgf_properties,
    deserialize_analysis_candidates, graph_plot_points, serialize_analysis_candidates,
    winrate_history,
};

#[allow(dead_code)]
const BOARD_PIXEL_SIZE: f32 = 420.0;
const NAVIGATION_RAIL_WIDTH: f32 = 64.0;

actions!(
    ryusei_gpui,
    [
        NewGame,
        OpenGame,
        ToggleEnginesSidebar,
        SaveGame,
        SaveGameAs,
        UndoMove,
        RedoMove,
        GoToFirstNode,
        GoToPreviousNode,
        GoToNextNode,
        GoToLastNode,
        OpenPreferences,
        OpenGameInfo,
        OpenScore,
        OpenAbout,
        ToggleGameGraph,
        ToggleComments,
        ToggleCoordinates,
        ToggleMoveNumbers,
        SetPlayMode,
        SetEditMode,
        SetScoringMode,
        SetEstimatorMode,
        SetSessionMatch,
        SetSessionRecord,
        SetSessionLive,
        SetPlayersHumanVsHuman,
        SetPlayersHumanVsAi,
        SetPlayersAiVsHuman,
        SetPlayersAiVsAi,
        SetOpeningFree,
        SetOpeningAncientSeatStones,
        SetTimeNone,
        SetTimeAbsolute600,
        SetTimeByoYomi,
        StartAnalysis,
        StopAnalysis,
        GenerateEngineMove,
        ToggleGtpTerminal,
        ToggleBottomDeck,
        StartWholeGameReview,
        ExportGif,
        ExportPositionPng,
        SetThemeClassic,
        SetThemeDark,
        SetThemeMist,
        SetBoardSize19,
        SetBoardSize13,
        SetBoardSize9,
        SetCoordsA1,
        SetCoords1_1,
        SetVisits100,
        SetVisits500,
        SetVisits1000,
        SetVisitsUnlimited,
        StartReviewQuick,
        StartReviewPreliminary,
        StartReviewIntermediate,
        StartReviewAdvanced,
        PluginKataGoSetup,
        PluginFoxSync,
        PluginPositionToSgf,
        PluginInstallZip,
        TogglePluginMenu,
        FocusNext,
        FocusPrev,
        Quit,
    ]
);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BottomDeckTab {
    WinrateGraph,
    VariationTree,
    GtpTerminal,
    KataGo,
    FoxSync,
    PositionSgf,
    PluginManager,
    Engines,
    Generic(String),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LeftSidebarTab {
    #[default]
    AiEvaluation,
    Library,
}

/// Engine-configuration surfaces hosted in the left engine sidebar after the
/// bottom deck was slimmed down to the three analysis tabs (design §4.2).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EngineConfigPanel {
    KataGo,
    Engines,
    FoxSync,
    PositionSgf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
enum ActiveDrawer {
    Preferences,
    Library,
    Profile,
    Goals,
    LiveCapture,
    GameInfo,
    Score,
    About,
    OgsAccount,
    Review,
    Export,
    MatchSetup,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ActiveTextInput {
    Comment,
    #[allow(dead_code)]
    NodeTitle,
    FoxQuery,
    LiveUrl,
    LibraryId,
    LibraryName,
    LibraryGithubUrl,
    LibraryReference,
    LibraryLicenseName,
    LibraryLicenseUrl,
    GtpInput,
    EngineSpec,
    OgsUsername,
    OgsPassword,
    OgsGameId,
    OgsChat,
}

struct ShellApp {
    host: ryusei_host::HostApplication,
    /// The active document is materialized in `host`; non-active documents
    /// remain independent serialized workspace-tab snapshots.
    workspace_tabs: ryusei_host::WorkspaceTabs,
    file_access: NativeGameFileAccess,
    dialog_service: Box<dyn DialogService>,
    external_file: ryusei_host::ExternalFileStore,
    persistence: NativeHostPersistence,
    recent_files: ryusei_host::RecentFilesStore,
    autosave: ryusei_host::AutosaveStore,
    settings: ryusei_host::SettingsStore,
    settings_persistence: NativeSettingsPersistence,
    sound_sink: Box<dyn SoundSink>,
    engine_store: ryusei_host::EngineStore,
    engine_controller: ryusei_host::EngineController<EngineRole, ryusei_host::ProcessGtpTransport>,
    active_console_role: Option<EngineRole>,
    engine_roles: EngineRoleAssignments,
    analysis: Vec<ryusei_host::AnalysisEntry>,
    analysis_best_move: Option<Vertex>,
    analysis_run: ryusei_host::AnalysisRunController,
    analysis_task: Option<Task<()>>,
    /// Timed playback through the active main line. Kept separate from analysis
    /// so review/analysis streams never collide with the demo/autoplay loop.
    autoplay_task: Option<Task<()>>,
    /// Timed principal-variation playback: `(candidate vertex, visible steps)`.
    /// The board truncates the hovered candidate's PV to the visible step count
    /// while a 400ms timer advances it, giving the prototype's animated PV demo.
    pv_animation: Option<(String, usize)>,
    pv_animation_task: Option<Task<()>>,
    /// Background handshake/replay task. Blocking GTP I/O must never run on
    /// GPUI's foreground event loop.
    engine_connect_tasks: BTreeMap<EngineRole, Task<()>>,
    /// Invalidates late connection/command callbacks per role. A Black-role
    /// reconnect must not stale a concurrently running Analysis/White task.
    engine_generations: BTreeMap<EngineRole, u64>,
    /// A bounded raw GTP console request temporarily owns a ready session while
    /// waiting for its response, keeping the UI event loop responsive.
    engine_command_tasks: BTreeMap<EngineRole, Task<()>>,
    /// An immutable analysis intent waiting for a matching role connection.
    /// Binding it to a role generation and node prevents an old handshake from
    /// starting analysis after a disconnect, role change, or navigation.
    pending_analysis_request: Option<PendingAnalysisRequest>,
    /// An immutable AI move intent waiting for a matching role connection.
    pending_engine_move: Option<PendingEngineMove>,
    batch_review_progress: Option<ryusei_host::BatchReviewProgress>,
    batch_review_state: Option<BatchReviewState>,
    /// A batch review owns a fixed search budget for its entire run. This is
    /// intentionally independent from the interactive analysis preference.
    batch_review_profile: Option<ryusei_domain_core::ReviewProfile>,
    /// Set while the optional per-move background review is analysing a move
    /// during a live match. Skips the fair-play lock and uses the 80v budget.
    background_review: bool,
    /// Candidate currently hovered in either the board overlay or candidate
    /// list. The vertex is stored instead of a copied PV so live engine updates
    /// immediately refresh the preview line.
    hovered_candidate_vertex: Option<String>,
    /// Index into the winrate graph points currently under the pointer, used to
    /// render the floating readout tooltip (move/winrate/quality).
    winrate_hover_index: Option<usize>,
    /// Ephemeral move used for response analysis. It is replayed into the
    /// engine only and never appended to the SGF tree.
    trial_move: Option<MoveDto>,
    last_analysis_trial_move: Option<MoveDto>,
    active_analysis_trial_move: Option<MoveDto>,
    /// Set when a maxVisits quick-switch lands while analysis is streaming;
    /// after the run finishes cleanly, analysis restarts with the new limit.
    restart_analysis_after_stop: bool,
    /// Keep analysis continuous across real moves/navigation when the user has
    /// started analysis explicitly.
    analysis_enabled: bool,
    /// Top-level play/study/broadcast behavior, deliberately separate from
    /// the board's Play/Edit/Scoring interaction mode.
    session_policy: SessionPolicy,
    /// Local prediction for Match clocks. Remote providers will replace it with
    /// their server-authoritative state through the same controller.
    clock: ClockController,
    clock_last_updated: Instant,
    #[allow(dead_code)]
    clock_tick_task: Option<Task<()>>,
    /// Last byo-yomi whole-second the countdown cue fired for, so the tick
    /// sounds at most once per second while the period runs low.
    last_byoyomi_tick_secs: Option<u64>,
    restart_analysis_after_position_change: bool,
    /// Node the attached engine was last replayed to. When a new analysis run
    /// targets the same node, the engine position (and thus KataGo's search
    /// tree) is reused so a deeper `maxVisits` pass builds on the shallow one.
    last_analysis_node: Option<ryusei_domain_core::NodeId>,
    engine_log: Vec<EngineLogEntry>,
    /// Aggregated editable text buffers and their focus handles.
    text_inputs: TextInputs,
    engine_spec_editing_name: Option<String>,
    /// Which engine-configuration panel is expanded in the left engine sidebar
    /// (KataGo setup / engine manager). `None` collapses the section.
    engine_config_panel: Option<EngineConfigPanel>,
    gtp_terminal_open: bool,
    live_source_url: Option<String>,
    live_ogs_state: Option<ryusei_host::OgsPublicGameState>,
    /// Polls public OGS broadcasts while this document remains the active live source.
    live_ogs_poll_task: Option<Task<()>>,
    live_ogs_poll_generation: u64,
    ogs_auth_state: ryusei_host::OgsAuthState,
    ogs_client: Arc<ryusei_host::LiveOgsClient>,
    #[allow(dead_code)]
    ogs_state_task: Option<Task<()>>,
    ogs_login_in_progress: bool,
    ogs_projected_moves: u32,
    ogs_projected_game_id: Option<u64>,
    ogs_was_searching: bool,
    ogs_marking_dead: bool,
    ogs_removed_stones: BTreeSet<String>,
    ogs_last_pass_notified_move: u32,
    library_rights_confirmed: bool,
    library_sources: Vec<ryusei_host::SgfLibrarySource>,
    library_selected_source: Option<String>,
    library_entries: Vec<ryusei_host::SgfLibraryEntry>,
    library_status: SharedString,
    library_task: Option<Task<()>>,
    library_syncing_source: Option<String>,
    theme_choice: ThemeChoice,
    theme: ThemeTokens,
    palette: UiPalette,
    left_sidebar_tab: LeftSidebarTab,
    left_sidebar_width: f32,
    right_sidebar_width: f32,
    peer_list_height: f32,
    winrate_graph_height: f32,
    properties_height: f32,
    split_drag: Option<SplitDrag>,
    active_drawer: Option<ActiveDrawer>,
    active_plugin_popover: Option<String>,
    game_graph_context_node: Option<ryusei_domain_core::NodeId>,
    #[allow(dead_code)]
    installed_themes: Vec<ryusei_host::InstalledTheme>,
    #[allow(dead_code)]
    legacy_asar_themes: Vec<std::path::PathBuf>,
    #[allow(dead_code)]
    board_size: usize,
    #[allow(dead_code)]
    settings_editing_key: Option<String>,
    plugin_controller: ryusei_host::PluginController<NativePluginPersistence>,
    installed_plugins: Vec<PluginPanelEntry>,
    last_vertex: Option<Vertex>,
    active_tool: MarkupTool,
    mode: GameMode,
    line_start: Option<Vertex>,
    hovered_vertex: Option<Vertex>,
    /// Whether the node-comment box shows a rendered Markdown preview instead
    /// of the editable plain-text source (PRD §4.3 live preview).
    comment_preview: bool,
    active_text_input: Option<ActiveTextInput>,
    status: SharedString,
    /// Prominent transient notification shown as a centered toast overlay.
    toast: Option<SharedString>,
    /// Live KataGo setup panel state. Network refreshes run off the UI thread.
    katago_local: Option<ryusei_host::KataGoLocalInfo>,
    katago_release: Option<ryusei_host::KataGoReleaseInfo>,
    katago_weights: Vec<ryusei_host::KataGoWeightInfo>,
    katago_panel_status: SharedString,
    katago_panel_task: Option<Task<()>>,
}

/// Active splitter drag state. Window-global mouse move/up listeners are
/// registered while this is `Some` so the drag continues outside the handle.
#[derive(Clone, Copy)]
struct SplitDrag {
    pane: SplitPane,
    start_position: f32,
    start_size: f32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PendingAnalysisRequest {
    role: EngineRole,
    role_generation: u64,
    node_id: ryusei_domain_core::NodeId,
}

impl PendingAnalysisRequest {
    fn matches(&self, role: EngineRole, role_generation: u64, node_id: &str) -> bool {
        self.role == role && self.role_generation == role_generation && self.node_id == node_id
    }
}

/// A queued AI move intent waiting for its engine role to finish connecting.
/// Mirrors [`PendingAnalysisRequest`] so a human-vs-AI match never stalls
/// waiting for an engine handshake that has not completed yet.
#[derive(Clone, Debug, Eq, PartialEq)]
struct PendingEngineMove {
    role: EngineRole,
    role_generation: u64,
    color: Color,
}

impl PendingEngineMove {
    fn matches(&self, role: EngineRole, role_generation: u64, color: Color) -> bool {
        self.role == role && self.role_generation == role_generation && self.color == color
    }
}

struct BatchReviewState {
    original_node_id: ryusei_domain_core::NodeId,
    node_ids: Vec<ryusei_domain_core::NodeId>,
    next_index: usize,
}

impl BatchReviewState {
    fn new(
        original_node_id: ryusei_domain_core::NodeId,
        node_ids: Vec<ryusei_domain_core::NodeId>,
    ) -> Option<Self> {
        (!node_ids.is_empty()).then_some(Self {
            original_node_id,
            node_ids,
            next_index: 0,
        })
    }

    fn current_node_id(&self) -> Option<&ryusei_domain_core::NodeId> {
        self.node_ids.get(self.next_index)
    }

    fn advance(&mut self) -> Option<&ryusei_domain_core::NodeId> {
        if self.next_index + 1 >= self.node_ids.len() {
            return None;
        }
        self.next_index += 1;
        self.current_node_id()
    }
}

#[cfg(test)]
mod batch_review_state_tests {
    use super::BatchReviewState;

    #[test]
    fn advances_in_order_and_stops_at_last_node() {
        let mut state = BatchReviewState::new(
            "root".to_owned(),
            vec!["b1".to_owned(), "w1".to_owned(), "b2".to_owned()],
        )
        .expect("non-empty review plan");
        assert_eq!(state.original_node_id, "root");
        assert_eq!(state.current_node_id(), Some(&"b1".to_owned()));
        assert_eq!(state.advance(), Some(&"w1".to_owned()));
        assert_eq!(state.advance(), Some(&"b2".to_owned()));
        assert_eq!(state.advance(), None);
        assert_eq!(state.current_node_id(), Some(&"b2".to_owned()));
    }

    #[test]
    fn empty_review_plan_is_rejected() {
        assert!(BatchReviewState::new("root".to_owned(), Vec::new()).is_none());
    }

    #[test]
    fn pending_request_requires_matching_role_generation_and_node() {
        let request = super::PendingAnalysisRequest {
            role: super::EngineRole::Analysis,
            role_generation: 7,
            node_id: "node-a".to_owned(),
        };
        assert!(request.matches(super::EngineRole::Analysis, 7, "node-a"));
        assert!(!request.matches(super::EngineRole::White, 7, "node-a"));
        assert!(!request.matches(super::EngineRole::Analysis, 8, "node-a"));
        assert!(!request.matches(super::EngineRole::Analysis, 7, "node-b"));
    }
}

/// Resolves the new-game defaults stored in the settings store and returns
/// the board size plus root SGF properties (`KM`, `HA`, and standard `AB`
/// handicap stones). Missing values fall back to the upstream Sabaki defaults.
fn default_board_size(settings: &ryusei_host::SettingsStore) -> usize {
    settings
        .get("game.default_board_size")
        .and_then(serde_json::Value::as_f64)
        .map(|value| value.round() as i64)
        .filter(|value| (2..=25).contains(value))
        .unwrap_or(19) as usize
}

fn default_new_game_properties_for_size(
    settings: &ryusei_host::SettingsStore,
    size: usize,
) -> BTreeMap<String, Vec<String>> {
    let komi = settings
        .get("game.default_komi")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(6.5);
    let handicap = settings
        .get("game.default_handicap")
        .and_then(serde_json::Value::as_f64)
        .map(|value| value.round() as usize)
        .unwrap_or(0);

    let mut properties = BTreeMap::new();
    if let Some(value) = settings.get_str("game.default_ruleset")
        && !value.trim().is_empty()
    {
        let ruleset = ryusei_host::GoRuleset::from_setting(Some(value));
        properties.insert("RU".to_owned(), vec![ruleset.sgf_name().to_owned()]);
    }
    properties.insert("KM".to_owned(), vec![komi.to_string()]);
    let stones = handicap_placement(size, handicap);
    if !stones.is_empty() {
        properties.insert("HA".to_owned(), vec![handicap.to_string()]);
        properties.insert(
            "AB".to_owned(),
            stones
                .into_iter()
                .map(|(column, row)| format_sgf_vertex(Vertex { column, row }))
                .collect(),
        );
    }
    let opening = OpeningConvention::from_setting(settings.get_str("game.opening_convention"));
    opening.apply_to_root_properties(size, &mut properties, format_sgf_vertex);
    properties
}

/// Resolves the new-game defaults stored in the settings store and returns
/// the board size plus root SGF properties (`KM`, `HA`, and standard `AB`
/// handicap stones). Missing values fall back to the upstream Sabaki defaults.
fn default_new_game_properties(
    settings: &ryusei_host::SettingsStore,
) -> (usize, BTreeMap<String, Vec<String>>) {
    let size = default_board_size(settings);
    let properties = default_new_game_properties_for_size(settings, size);
    (size, properties)
}

fn workspace_tab_title(snapshot: &ryusei_domain_core::GameSnapshot) -> String {
    snapshot
        .file_state
        .path
        .as_deref()
        .and_then(|path| std::path::Path::new(path).file_name())
        .and_then(std::ffi::OsStr::to_str)
        .map(ToOwned::to_owned)
        .or_else(|| {
            snapshot
                .root_properties
                .get("GN")
                .and_then(|values| values.first())
                .filter(|title| !title.trim().is_empty())
                .cloned()
        })
        .unwrap_or_else(|| "Untitled Game".to_owned())
}

impl ShellApp {
    #[expect(
        clippy::too_many_arguments,
        reason = "P2 will replace direct shell construction dependencies with dedicated controllers"
    )]
    fn new(
        mut settings: ryusei_host::SettingsStore,
        settings_persistence: NativeSettingsPersistence,
        persistence: NativeHostPersistence,
        plugin_persistence: NativePluginPersistence,
        initial_status: String,
        startup_file: Option<PathBuf>,
        dialog_service: Box<dyn DialogService>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut host = ryusei_host::HostApplication::default();
        let file_access = NativeGameFileAccess;
        let mut events = RecordingSink;
        let (default_size, default_properties) = default_new_game_properties(&settings);
        let left_sidebar_width = pane_size_from_settings(
            &settings,
            "view.leftsidebar_width",
            "view.leftsidebar_minwidth",
            360.0,
        );
        let right_sidebar_width = pane_size_from_settings(
            &settings,
            "view.sidebar_width",
            "view.sidebar_minwidth",
            360.0,
        );
        let peer_list_height = pane_size_from_settings(
            &settings,
            "view.peerlist_height",
            "view.peerlist_minheight",
            130.0,
        );
        let winrate_graph_height = pane_size_from_settings(
            &settings,
            "view.winrategraph_height",
            "view.winrategraph_minheight",
            90.0,
        );
        let properties_height = pane_size_from_settings(
            &settings,
            "view.properties_height",
            "view.properties_minheight",
            180.0,
        );

        let mut status = initial_status;
        let has_startup_file = startup_file.is_some();
        if let Some(path) = startup_file {
            match host.open(path.clone(), &file_access, &mut events) {
                Ok(_) => status = format!("opened {}", path.display()),
                Err(error) => status = format!("could not open {}: {error}", path.display()),
            }
        } else {
            match host.create_new_with_properties(
                default_size,
                default_size,
                &default_properties,
                &mut events,
            ) {
                Ok(_) => {}
                Err(error) => status = format!("could not create new game: {error}"),
            }
        }
        let initial_snapshot = host.snapshot();
        let fallback_tabs =
            ryusei_host::WorkspaceTabs::new(host.to_sgf(), workspace_tab_title(&initial_snapshot));
        let mut workspace_tabs = fallback_tabs;
        if !has_startup_file {
            let persisted_tabs = match persistence.load_workspace_tabs() {
                Ok(Some(tabs)) => Some(tabs),
                Ok(None) => settings.get_str("workspace.tabs").and_then(|json| {
                    match ryusei_host::WorkspaceTabs::deserialize_validated(json) {
                        Ok(tabs) => Some(tabs),
                        Err(error) => {
                            status =
                                format!("legacy workspace sessions could not be restored: {error}");
                            None
                        }
                    }
                }),
                Err(error) => {
                    status = format!("workspace sessions could not be restored: {error}");
                    None
                }
            };
            if let Some(tabs) = persisted_tabs {
                match tabs.try_active_tab().cloned() {
                    Ok(active) => match host.restore_workspace_tab_with_state(
                        &active.sgf,
                        active.source_path,
                        active.source_encoding,
                        active.is_dirty,
                        active.current_node_id.as_deref(),
                        &mut events,
                    ) {
                        Ok(_) => workspace_tabs = tabs,
                        Err(error) => {
                            status =
                                format!("active workspace session could not be restored: {error}")
                        }
                    },
                    Err(error) => {
                        status = format!("workspace sessions are invalid: {error}");
                    }
                }
            }
        }
        let initial_snapshot = host.snapshot();
        let initial_clock_control = TimeControl::from_sgf(&initial_snapshot.root_properties);
        let board_size = initial_snapshot.board.width;

        let recent_files = persistence.load_recent_files().unwrap_or_default();
        let autosave = persistence.load_autosave();
        let theme_choice = theme_from_setting(settings.get_str("theme.current"));
        let theme = theme_choice.tokens();
        let palette = ui_palette(&theme);
        let (installed_themes, legacy_asar_themes) = match file_workflow::theme_root() {
            Ok(theme_root) => match ryusei_host::scan_theme_root(&theme_root) {
                Ok(scan) => (scan.themes, scan.legacy_asar),
                Err(error) => {
                    status = format!("theme scan failed: {error}");
                    (Vec::new(), Vec::new())
                }
            },
            Err(error) => {
                status = format!("theme directory unavailable: {error}");
                (Vec::new(), Vec::new())
            }
        };
        let plugin_install_root = match file_workflow::plugin_install_root() {
            Ok(root) => root,
            Err(error) => {
                status = format!("plugin directory unavailable: {error}");
                std::env::temp_dir().join("ryusei-gpui-plugins")
            }
        };
        let plugin_controller = match ryusei_host::PluginController::restore(
            plugin_persistence,
            &plugin_install_root,
        ) {
            Ok(controller) => controller,
            Err(error) => {
                status = format!("plugin scan failed: {error}");
                let fallback_persistence = NativePluginPersistence::for_current_user()
                    .unwrap_or_else(|_| NativePluginPersistence::new(std::env::temp_dir()));
                ryusei_host::PluginController::from_store(
                    ryusei_host::PluginStore::default(),
                    fallback_persistence,
                )
            }
        };
        let installed_plugins = plugin_controller
            .records()
            .iter()
            .map(entry_from_record)
            .collect();
        let engine_store = ryusei_host::EngineStore::from_settings(&settings).unwrap_or_default();
        let engine_roles = EngineRoleAssignments::from_settings(&settings);
        // Analysis is the principal board feedback loop. The legacy setting is
        // optional, so a fresh Ryusei profile must expose analysis markers
        // instead of silently hiding a connected KataGo session.
        if settings.get_bool("board.show_analysis").is_none() {
            let _ = settings.set("board.show_analysis", serde_json::json!(true));
        }
        // HumanSL is the default style for AI-vs-human play. It only becomes
        // active once a HumanSL network is installed, so existing installations
        // without that optional asset remain fully usable.
        if settings.get_bool("katago.human_sl_enabled").is_none() {
            let _ = settings.set("katago.human_sl_enabled", serde_json::json!(true));
        }
        // Board coordinates default on: reviewers expect the A–T / 1–19 frame
        // labels without hunting for the player-bar toggle.
        if settings.get_bool("view.show_coordinates").is_none() {
            let _ = settings.set("view.show_coordinates", serde_json::json!(true));
        }
        if settings.get_bool("view.show_leftsidebar").is_none() {
            let _ = settings.set("view.show_leftsidebar", serde_json::json!(true));
        }
        if settings.get_bool("view.show_analysis_preview").is_none() {
            let _ = settings.set("view.show_analysis_preview", serde_json::json!(true));
        }
        if settings.get("view.leftsidebar_minwidth").is_none() {
            let _ = settings.set("view.leftsidebar_minwidth", serde_json::json!(240));
        }
        if settings.get("view.sidebar_minwidth").is_none() {
            let _ = settings.set("view.sidebar_minwidth", serde_json::json!(240));
        }
        let active_console_role = EngineRole::ALL
            .into_iter()
            .find(|role| engine_roles.get(*role).is_some());
        let session_policy = workspace_tabs.active_tab().policy;
        let active_tab_mode = workspace_tabs.active_tab().mode;
        let active_tab_last_vertex = workspace_tabs.active_tab().last_vertex;
        let active_tab_analysis = workspace_tabs.active_tab().analysis.clone();
        let active_tab_analysis_best_move = workspace_tabs.active_tab().analysis_best_move;
        // A new local Match is analysis-off by default. Live sessions opt into
        // continuous analysis when selected through the Session menu.
        let analysis_enabled = workspace_tabs.active_tab().analysis_enabled
            || session_policy.analysis == AnalysisPolicy::Continuous;
        let katago_local = crate::file_workflow::current_user_config_directory()
            .ok()
            .and_then(|base| ryusei_host::inspect_katago_local(&base).ok());
        let katago_weights = katago_local
            .as_ref()
            .map(|local| local.weights.clone())
            .unwrap_or_default();
        let active_clock = workspace_tabs.active_tab().clock;
        let mut clock = ClockController::new(initial_clock_control);
        if active_clock.control == initial_clock_control {
            clock.apply_remote_clock(active_clock);
        } else if !matches!(initial_clock_control, TimeControl::None) {
            clock.start(host.snapshot().board.next_player);
        }

        let library_sources: Vec<ryusei_host::SgfLibrarySource> = settings
            .get_str("library.sources")
            .and_then(|value| serde_json::from_str(value).ok())
            .unwrap_or_default();

        // OGS callbacks run on a socket reader thread. A bounded async channel
        // coalesces notifications without ever blocking GPUI's foreground executor.
        let (ogs_state_tx, ogs_state_rx) = async_channel::bounded::<()>(1);
        let ogs_client = Arc::new(ryusei_host::LiveOgsClient::new());
        ogs_client.set_on_state_change(Some(Box::new(move || {
            let _ = ogs_state_tx.try_send(());
        })));
        let ogs_state_task = Some(cx.spawn(
            move |weak: gpui::WeakEntity<ShellApp>, cx: &mut gpui::AsyncApp| {
                let mut cx = cx.clone();
                async move {
                    while ogs_state_rx.recv().await.is_ok() {
                        if weak
                            .update(&mut cx, |shell, cx| {
                                shell.refresh_ogs_account_state(cx);
                                cx.notify();
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            },
        ));

        let clock_tick_task = Some(cx.spawn(
            move |weak: gpui::WeakEntity<ShellApp>, cx: &mut gpui::AsyncApp| {
                let mut cx = cx.clone();
                async move {
                    loop {
                        cx.background_executor()
                            .timer(Duration::from_millis(200))
                            .await;
                        let keep_going = weak
                            .update(&mut cx, |shell, cx| {
                                if shell.clock.state().running && !shell.clock.state().paused {
                                    shell.advance_clock(Instant::now(), cx);
                                    cx.notify();
                                }
                            })
                            .is_ok();
                        if !keep_going {
                            break;
                        }
                    }
                }
            },
        ));

        let mut shell = Self {
            host,
            workspace_tabs,
            file_access,
            dialog_service,
            persistence,
            recent_files,
            autosave,
            settings,
            settings_persistence,
            sound_sink: platform_sound_sink(),
            external_file: ryusei_host::ExternalFileStore::default(),
            engine_store,
            engine_controller: ryusei_host::EngineController::default(),
            active_console_role,
            engine_roles,
            analysis: active_tab_analysis,
            analysis_best_move: active_tab_analysis_best_move,
            analysis_run: ryusei_host::AnalysisRunController::default(),
            analysis_task: None,
            autoplay_task: None,
            pv_animation: None,
            pv_animation_task: None,
            engine_connect_tasks: BTreeMap::new(),
            engine_generations: EngineRole::ALL.into_iter().map(|role| (role, 0)).collect(),
            engine_command_tasks: BTreeMap::new(),
            pending_analysis_request: None,
            pending_engine_move: None,
            batch_review_progress: None,
            batch_review_state: None,
            batch_review_profile: None,
            background_review: false,
            hovered_candidate_vertex: None,
            winrate_hover_index: None,
            trial_move: None,
            last_analysis_trial_move: None,
            active_analysis_trial_move: None,
            restart_analysis_after_stop: false,
            // Keep a configured Analysis role continuously evaluating the
            // current position; explicit Stop remains the opt-out.
            analysis_enabled,
            session_policy,
            clock,
            clock_last_updated: Instant::now(),
            clock_tick_task,
            last_byoyomi_tick_secs: None,
            restart_analysis_after_position_change: false,
            last_analysis_node: None,
            engine_log: Vec::new(),
            text_inputs: TextInputs::new(cx),
            engine_spec_editing_name: None,
            engine_config_panel: None,
            gtp_terminal_open: false,
            live_source_url: None,
            live_ogs_state: None,
            live_ogs_poll_task: None,
            live_ogs_poll_generation: 0,
            ogs_auth_state: ryusei_host::OgsAuthState::SignedOut,
            ogs_client,
            ogs_state_task,
            ogs_login_in_progress: false,
            ogs_projected_moves: 0,
            ogs_projected_game_id: None,
            ogs_was_searching: false,
            ogs_marking_dead: false,
            ogs_removed_stones: BTreeSet::new(),
            ogs_last_pass_notified_move: 0,
            library_rights_confirmed: false,
            library_selected_source: library_sources.first().map(|source| source.id.clone()),
            library_sources,
            library_entries: Vec::new(),
            library_status: "尚未同步".into(),
            library_task: None,
            library_syncing_source: None,
            theme_choice,
            theme,
            palette,
            left_sidebar_tab: LeftSidebarTab::AiEvaluation,
            left_sidebar_width,
            right_sidebar_width,
            peer_list_height,
            winrate_graph_height,
            properties_height,
            split_drag: None,
            active_drawer: None,
            active_plugin_popover: None,
            game_graph_context_node: None,
            installed_themes,
            legacy_asar_themes,
            board_size,
            settings_editing_key: None,
            plugin_controller,
            installed_plugins,
            last_vertex: active_tab_last_vertex,
            active_tool: MarkupTool::Play,
            mode: active_tab_mode,
            line_start: None,
            hovered_vertex: None,
            comment_preview: false,
            active_text_input: None,
            status: status.into(),
            toast: None,
            katago_local,
            katago_release: None,
            katago_weights,
            katago_panel_status: "尚未刷新官网 KataGo 信息".into(),
            katago_panel_task: None,
        };
        if let Some(path) = shell.host.snapshot().file_state.path {
            match shell.workspace_tabs.active_tab().source_fingerprint.clone() {
                Some(fingerprint) => shell
                    .external_file
                    .track_file_with_fingerprint(std::path::PathBuf::from(path), fingerprint),
                None => shell
                    .external_file
                    .track_file(std::path::PathBuf::from(path), &shell.host.to_sgf()),
            }
        }
        // 启动时尝试恢复持久化的 OGS 登录会话（30 天内有效）。
        {
            let client = Arc::clone(&shell.ogs_client);
            let weak = cx.entity().downgrade();
            cx.spawn(
                move |_: gpui::WeakEntity<ShellApp>, cx: &mut gpui::AsyncApp| {
                    let mut cx = cx.clone();
                    async move {
                        let result = cx
                            .background_executor()
                            .spawn(async move { client.restore_session() })
                            .await;
                        weak.update(&mut cx, |shell, cx| {
                            shell.refresh_ogs_account_state(cx);
                            if result.is_ok() {
                                shell.status = "已恢复 OGS 登录会话".into();
                            }
                            cx.notify();
                        })
                        .ok();
                    }
                },
            )
            .detach();
        }
        shell
    }

    /// Parses a GTP command line into `(name, arguments)`.
    fn parse_engine_command_line(draft: &str) -> (String, Vec<String>) {
        let mut tokens = draft.split_whitespace();
        let name = tokens.next().unwrap_or_default().to_owned();
        let arguments = tokens.map(ToOwned::to_owned).collect();
        (name, arguments)
    }

    #[allow(dead_code)]
    fn on_engine_selected(
        &mut self,
        name: &str,
        _: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.active_console_role = EngineRole::ALL
            .into_iter()
            .find(|role| self.engine_roles.get(*role) == Some(name));
        window.focus(&self.text_inputs.engine_input_focus_handle);
        cx.notify();
    }

    fn persist_engine_roles(&mut self) -> Result<(), String> {
        let previous = self.settings.clone();
        for role in EngineRole::ALL {
            self.settings
                .set(
                    role.setting_key(),
                    serde_json::json!(self.engine_roles.get(role)),
                )
                .map_err(|error| error.to_string())?;
        }
        if let Err(error) =
            ryusei_host::persist_settings_store(&self.settings, &mut self.settings_persistence)
        {
            self.settings = previous;
            return Err(error);
        }
        Ok(())
    }

    #[allow(dead_code)]
    fn on_engine_role_toggled(&mut self, role: EngineRole, name: &str, cx: &mut Context<Self>) {
        // A selected role button focuses that role for attach/detach and the
        // GTP console; clicking the already focused button clears its role.
        if self.engine_roles.get(role) == Some(name) && self.active_console_role != Some(role) {
            self.active_console_role = Some(role);
            self.status = format!("{} engine {name} selected", role.label()).into();
            cx.notify();
            return;
        }

        let previous = self.engine_roles.get(role).map(ToOwned::to_owned);
        let selected = self.engine_roles.toggle(role, name);
        if previous.as_deref() != self.engine_roles.get(role) {
            self.disconnect_engine_role(role);
        }
        self.active_console_role = selected.then_some(role);
        match self.persist_engine_roles() {
            Ok(()) => {
                self.status = format!(
                    "{} engine {} {}",
                    role.label(),
                    name,
                    if selected { "selected" } else { "cleared" }
                )
                .into();
            }
            Err(error) => {
                self.engine_roles = EngineRoleAssignments::from_settings(&self.settings);
                self.status = format!("engine roles not persisted: {error}").into();
            }
        }
        cx.notify();
    }

    fn on_fox_query_focus(
        &mut self,
        _: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.active_text_input = Some(ActiveTextInput::FoxQuery);
        window.focus(&self.text_inputs.fox_query_focus_handle);
        cx.notify();
    }

    fn on_fox_query_key_down(
        &mut self,
        event: &gpui::KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match Self::handle_text_input_key(&mut self.text_inputs.fox_query_input, event) {
            InputKeyResult::Submit => {
                self.fetch_fox_query(cx);
            }
            InputKeyResult::Cancel => {
                self.text_inputs.fox_query_input.set_text("");
                cx.notify();
            }
            InputKeyResult::Changed | InputKeyResult::Ignored => {
                cx.notify();
            }
        }
    }

    fn on_live_url_focus(
        &mut self,
        _: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.active_text_input = Some(ActiveTextInput::LiveUrl);
        window.focus(&self.text_inputs.live_url_focus_handle);
        cx.notify();
    }

    fn on_live_url_key_down(
        &mut self,
        event: &gpui::KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match Self::handle_text_input_key(&mut self.text_inputs.live_url_input, event) {
            InputKeyResult::Submit => self.capture_public_live_game(cx),
            InputKeyResult::Cancel => {
                self.text_inputs.live_url_input.set_text("");
                cx.notify();
            }
            InputKeyResult::Changed | InputKeyResult::Ignored => cx.notify(),
        }
    }

    fn on_ogs_username_focus(
        &mut self,
        _: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.active_text_input = Some(ActiveTextInput::OgsUsername);
        window.focus(&self.text_inputs.ogs_username_focus_handle);
        cx.notify();
    }

    fn on_ogs_username_key_down(
        &mut self,
        event: &gpui::KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let InputKeyResult::Submit =
            Self::handle_text_input_key(&mut self.text_inputs.ogs_username_input, event)
        {
            _window.focus(&self.text_inputs.ogs_password_focus_handle);
        }
        cx.notify();
    }

    fn on_ogs_password_focus(
        &mut self,
        _: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.active_text_input = Some(ActiveTextInput::OgsPassword);
        window.focus(&self.text_inputs.ogs_password_focus_handle);
        cx.notify();
    }

    fn on_ogs_password_key_down(
        &mut self,
        event: &gpui::KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let InputKeyResult::Submit =
            Self::handle_text_input_key(&mut self.text_inputs.ogs_password_input, event)
        {
            self.ogs_login(cx);
        }
        cx.notify();
    }

    fn on_ogs_game_id_focus(
        &mut self,
        _: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.active_text_input = Some(ActiveTextInput::OgsGameId);
        window.focus(&self.text_inputs.ogs_game_id_focus_handle);
        cx.notify();
    }

    fn on_ogs_game_id_key_down(
        &mut self,
        event: &gpui::KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let InputKeyResult::Submit =
            Self::handle_text_input_key(&mut self.text_inputs.ogs_game_id_input, event)
        {
            self.connect_ogs_game(cx);
        }
        cx.notify();
    }

    fn on_ogs_chat_focus(
        &mut self,
        _: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.active_text_input = Some(ActiveTextInput::OgsChat);
        window.focus(&self.text_inputs.ogs_chat_focus_handle);
        cx.notify();
    }

    fn on_ogs_chat_key_down(
        &mut self,
        event: &gpui::KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let InputKeyResult::Submit =
            Self::handle_text_input_key(&mut self.text_inputs.ogs_chat_input, event)
        {
            self.ogs_send_chat(cx);
        }
        cx.notify();
    }

    fn capture_public_live_game(&mut self, cx: &mut Context<Self>) {
        let url = self.text_inputs.live_url_input.text().trim().to_owned();
        if let Err(error) = ryusei_host::validate_public_https_url(&url) {
            self.show_toast(format!("公共直播地址无效: {error}"), cx);
            return;
        }
        self.show_toast("正在读取公共直播棋谱…".to_owned(), cx);
        let weak = cx.entity().downgrade();
        cx.spawn(
            move |_: gpui::WeakEntity<ShellApp>, cx: &mut gpui::AsyncApp| {
                let mut cx = cx.clone();
                async move {
                    let result = cx
                        .background_executor()
                        .spawn(async move {
                            let mut fetch = ryusei_host::CurlPublicPageFetch;
                            ryusei_host::capture_public_live_sgf(&url, &mut fetch)
                                .map_err(|error| error.to_string())
                        })
                        .await;
                    weak.update(&mut cx, |shell, cx| {
                        shell.apply_public_live_capture(result, cx)
                    })
                    .ok();
                }
            },
        )
        .detach();
    }

    fn apply_public_live_capture(
        &mut self,
        result: Result<ryusei_host::StarRiverCapture, String>,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(capture) => {
                let mut events = RecordingSink;
                match self.host.restore_from_sgf(&capture.sgf, &mut events) {
                    Ok(_) => {
                        self.external_file.detach_file();
                        self.disconnect_all_engine_sessions();
                        self.stop_ogs_public_poll();
                        self.last_vertex = None;
                        self.live_source_url = Some(capture.page_url.clone());
                        self.mode = GameMode::Play;
                        self.session_policy =
                            SessionPolicy::new(SessionMode::Live, SessionSource::LiveBroadcast);
                        self.analysis_enabled = true;
                        self.active_drawer = None;
                        self.synchronize_recovery();
                        self.status = "公共直播棋谱已载入，只读观察模式".into();
                        self.show_toast("公共直播棋谱已载入".to_owned(), cx);
                        self.start_analysis(cx);
                        self.refresh_ogs_public_state(cx);
                        self.start_ogs_public_poll(cx);
                    }
                    Err(error) => self.show_toast(format!("直播棋谱解析失败: {error}"), cx),
                }
            }
            Err(error) => self.show_toast(format!("公共直播读取失败: {error}"), cx),
        }
        cx.notify();
    }

    fn refresh_ogs_public_state(&mut self, cx: &mut Context<Self>) {
        let Some(url) = self.live_source_url.clone() else {
            return;
        };
        let Some(game_id) = ryusei_host::ogs_game_id_from_public_url(&url) else {
            self.live_ogs_state = None;
            return;
        };
        let weak = cx.entity().downgrade();
        cx.spawn(
            move |_: gpui::WeakEntity<ShellApp>, cx: &mut gpui::AsyncApp| {
                let mut cx = cx.clone();
                async move {
                    let result = cx
                        .background_executor()
                        .spawn(async move {
                            let mut fetch = ryusei_host::CurlOgsPublicGameFetch;
                            fetch.fetch_public_game(game_id)
                        })
                        .await;
                    let _ = weak.update(&mut cx, |shell, cx| {
                        match result {
                            Ok(state) => {
                                shell.status = format!(
                                    "OGS #{} · {} vs {} · 第 {} 手 · {}",
                                    state.game_id,
                                    state.black_name,
                                    state.white_name,
                                    state.move_number,
                                    state.phase,
                                )
                                .into();
                                shell.live_ogs_state = Some(state);
                            }
                            Err(error) => {
                                shell.status = format!("OGS 公共状态读取失败：{error}").into();
                            }
                        }
                        cx.notify();
                    });
                }
            },
        )
        .detach();
    }

    fn stop_ogs_public_poll(&mut self) {
        self.live_ogs_poll_generation = self.live_ogs_poll_generation.wrapping_add(1);
        self.live_ogs_poll_task = None;
    }

    /// Polls only public OGS metadata. A changed move number triggers one SGF
    /// capture; unchanged states never reload the board or restart KataGo.
    fn start_ogs_public_poll(&mut self, cx: &mut Context<Self>) {
        if self.live_ogs_poll_task.is_some() {
            return;
        }
        let Some(url) = self.live_source_url.clone() else {
            return;
        };
        if ryusei_host::ogs_game_id_from_public_url(&url).is_none() {
            return;
        }
        self.live_ogs_poll_generation = self.live_ogs_poll_generation.wrapping_add(1);
        let generation = self.live_ogs_poll_generation;
        let weak = cx.entity().downgrade();
        self.live_ogs_poll_task = Some(cx.spawn(
            move |_: gpui::WeakEntity<ShellApp>, cx: &mut gpui::AsyncApp| {
                let mut cx = cx.clone();
                let url = url.clone();
                async move {
                    loop {
                        cx.background_executor()
                            .timer(Duration::from_secs(20))
                            .await;
                        let Some(game_id) = ryusei_host::ogs_game_id_from_public_url(&url) else {
                            break;
                        };
                        let state = cx
                            .background_executor()
                            .spawn(async move {
                                let mut fetch = ryusei_host::CurlOgsPublicGameFetch;
                                fetch.fetch_public_game(game_id)
                            })
                            .await;
                        let reload = weak
                            .update(&mut cx, |shell, cx| {
                                if shell.live_ogs_poll_generation != generation
                                    || shell.live_source_url.as_deref() != Some(url.as_str())
                                {
                                    return false;
                                }
                                match state {
                                    Ok(state) => {
                                        let changed =
                                            shell.live_ogs_state.as_ref().is_some_and(|previous| {
                                                previous.move_number != state.move_number
                                            });
                                        shell.live_ogs_state = Some(state);
                                        cx.notify();
                                        changed
                                    }
                                    Err(error) => {
                                        shell.status = format!("OGS 自动刷新失败：{error}").into();
                                        cx.notify();
                                        false
                                    }
                                }
                            })
                            .unwrap_or(false);
                        if !reload {
                            continue;
                        }
                        let refresh_url = url.clone();
                        let capture = cx
                            .background_executor()
                            .spawn(async move {
                                let mut fetch = ryusei_host::CurlPublicPageFetch;
                                ryusei_host::capture_public_live_sgf(&refresh_url, &mut fetch)
                                    .map_err(|error| error.to_string())
                            })
                            .await;
                        let still_current = weak
                            .update(&mut cx, |shell, cx| {
                                if shell.live_ogs_poll_generation != generation
                                    || shell.live_source_url.as_deref() != Some(url.as_str())
                                {
                                    return false;
                                }
                                shell.apply_public_live_capture(capture, cx);
                                false
                            })
                            .unwrap_or(false);
                        if !still_current {
                            break;
                        }
                    }
                }
            },
        ));
    }

    fn on_library_input_focus(
        &mut self,
        field: ActiveTextInput,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.active_text_input = Some(field);
        let focus = match field {
            ActiveTextInput::LibraryId => &self.text_inputs.library_id_focus_handle,
            ActiveTextInput::LibraryName => &self.text_inputs.library_name_focus_handle,
            ActiveTextInput::LibraryGithubUrl => &self.text_inputs.library_github_url_focus_handle,
            ActiveTextInput::LibraryReference => &self.text_inputs.library_reference_focus_handle,
            ActiveTextInput::LibraryLicenseName => {
                &self.text_inputs.library_license_name_focus_handle
            }
            ActiveTextInput::LibraryLicenseUrl => {
                &self.text_inputs.library_license_url_focus_handle
            }
            _ => return,
        };
        window.focus(focus);
        cx.notify();
    }

    fn on_library_input_key_down(
        &mut self,
        event: &gpui::KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let result = match self.active_text_input {
            Some(ActiveTextInput::LibraryId) => {
                Self::handle_text_input_key(&mut self.text_inputs.library_id_input, event)
            }
            Some(ActiveTextInput::LibraryName) => {
                Self::handle_text_input_key(&mut self.text_inputs.library_name_input, event)
            }
            Some(ActiveTextInput::LibraryGithubUrl) => {
                Self::handle_text_input_key(&mut self.text_inputs.library_github_url_input, event)
            }
            Some(ActiveTextInput::LibraryReference) => {
                Self::handle_text_input_key(&mut self.text_inputs.library_reference_input, event)
            }
            Some(ActiveTextInput::LibraryLicenseName) => {
                Self::handle_text_input_key(&mut self.text_inputs.library_license_name_input, event)
            }
            Some(ActiveTextInput::LibraryLicenseUrl) => {
                Self::handle_text_input_key(&mut self.text_inputs.library_license_url_input, event)
            }
            _ => InputKeyResult::Ignored,
        };
        if result == InputKeyResult::Submit {
            self.sync_library(cx);
        } else {
            cx.notify();
        }
    }

    fn fetch_fox_query(&mut self, cx: &mut Context<Self>) {
        let query = self.text_inputs.fox_query_input.text().trim().to_owned();
        if query.is_empty() {
            self.status = "输入野狐用户名或 ID 后按 Enter 查询".into();
            self.show_toast("请输入野狐用户名或 ID 后按 Enter 查询".to_owned(), cx);
            cx.notify();
            return;
        }
        self.show_toast(format!("🦊 正在查询野狐用户 {query} 的最新棋谱..."), cx);
        let weak = cx.entity().downgrade();
        cx.spawn(
            move |_: gpui::WeakEntity<ShellApp>, cx: &mut gpui::AsyncApp| {
                let mut cx = cx.clone();
                async move {
                    let result: Result<(ryusei_host::FoxGameSummary, String), String> = cx
                        .background_executor()
                        .spawn(async move {
                            let games = ryusei_host::fetch_user_recent_games(&query)?;
                            let game = games
                                .first()
                                .ok_or_else(|| "未查询到近期对局记录".to_owned())?;
                            let sgf = ryusei_host::fetch_game_sgf(&game.chess_id)?;
                            Ok((game.clone(), sgf))
                        })
                        .await;
                    weak.update(&mut cx, |shell, cx| shell.apply_fox_game_result(result, cx))
                        .ok();
                }
            },
        )
        .detach();
    }

    fn apply_fox_game_result(
        &mut self,
        result: Result<(ryusei_host::FoxGameSummary, String), String>,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok((game, sgf)) => {
                let mut events = RecordingSink;
                match self.host.restore_from_sgf(&sgf, &mut events) {
                    Ok(_) => {
                        self.last_vertex = None;
                        self.external_file.detach_file();
                        self.disconnect_all_engine_sessions();
                        let toast = format!(
                            "🦊 成功导入野狐棋谱: {} ({}) vs {} ({}) [{}]",
                            game.black_name,
                            game.black_rank,
                            game.white_name,
                            game.white_rank,
                            game.result
                        );
                        self.status = toast.clone().into();
                        self.show_toast(toast, cx);
                    }
                    Err(err) => self.show_toast(format!("棋谱解析失败: {err}"), cx),
                }
            }
            Err(err) => self.show_toast(format!("野狐对局同步失败: {err}"), cx),
        }
        cx.notify();
    }

    fn on_engine_input_focus(
        &mut self,
        _: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.active_text_input = Some(ActiveTextInput::GtpInput);
        window.focus(&self.text_inputs.engine_input_focus_handle);
        cx.notify();
    }

    fn on_engine_key_down(
        &mut self,
        event: &gpui::KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match Self::handle_text_input_key(&mut self.text_inputs.gtp_input, event) {
            InputKeyResult::Submit => {
                let draft = self.text_inputs.gtp_input.text().trim().to_owned();
                self.send_engine_command(&draft, cx);
                self.text_inputs.gtp_input.set_text("");
                cx.notify();
            }
            InputKeyResult::Cancel => {
                self.text_inputs.gtp_input.set_text("");
                cx.notify();
            }
            InputKeyResult::Changed | InputKeyResult::Ignored => {
                cx.notify();
            }
        }
    }

    fn on_engine_spec_focus(
        &mut self,
        _: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.active_text_input = Some(ActiveTextInput::EngineSpec);
        window.focus(&self.text_inputs.engine_spec_focus_handle);
        cx.notify();
    }

    fn on_engine_spec_key_down(
        &mut self,
        event: &gpui::KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match Self::handle_text_input_key(&mut self.text_inputs.engine_spec_input, event) {
            InputKeyResult::Submit => self.save_engine_spec(cx),
            InputKeyResult::Cancel => {
                self.text_inputs.engine_spec_input.set_text("");
                self.engine_spec_editing_name = None;
                cx.notify();
            }
            InputKeyResult::Changed | InputKeyResult::Ignored => cx.notify(),
        }
    }

    fn persist_engine_configuration(&mut self) -> Result<(), String> {
        self.engine_store
            .save(&mut self.settings)
            .map_err(|error| error.to_string())?;
        for role in EngineRole::ALL {
            self.settings
                .set(
                    role.setting_key(),
                    serde_json::json!(self.engine_roles.get(role)),
                )
                .map_err(|error| error.to_string())?;
        }
        ryusei_host::persist_settings_store(&self.settings, &mut self.settings_persistence)
    }

    pub fn save_engine_spec(&mut self, cx: &mut Context<Self>) {
        let spec = self.text_inputs.engine_spec_input.text().trim().to_owned();
        let record = match crate::engine_console::parse_engine_spec(&spec) {
            Ok(record) => record,
            Err(error) => {
                self.show_toast(format!("引擎配置无效: {error}"), cx);
                return;
            }
        };
        let previous_store = self.engine_store.clone();
        let previous_roles = self.engine_roles.clone();
        let prior_name = self.engine_spec_editing_name.take();
        if prior_name
            .as_deref()
            .is_some_and(|old_name| old_name != record.name)
            && self
                .engine_store
                .list()
                .iter()
                .any(|existing| existing.name == record.name)
        {
            self.engine_spec_editing_name = prior_name;
            self.show_toast(format!("保存引擎失败: 重复名称 '{}'", record.name), cx);
            return;
        }
        if let Some(old_name) = prior_name.as_deref()
            && old_name != record.name
        {
            self.engine_store.remove(old_name);
            self.engine_roles.clear_engine(old_name);
            for role in EngineRole::ALL {
                if self.engine_roles.get(role).is_none() {
                    self.disconnect_engine_role(role);
                }
            }
        }
        let result = if prior_name.is_some() {
            self.engine_store.upsert(record.clone());
            Ok(())
        } else {
            self.engine_store
                .add(record.clone())
                .map_err(|error| error.to_string())
        };
        if let Err(error) = result.and_then(|_| self.persist_engine_configuration()) {
            self.engine_store = previous_store;
            self.engine_roles = previous_roles;
            self.show_toast(format!("保存引擎失败: {error}"), cx);
            return;
        }
        self.text_inputs.engine_spec_input.set_text("");
        self.status = format!("已保存 GTP 引擎: {}", record.name).into();
        self.show_toast(self.status.clone(), cx);
        cx.notify();
    }

    pub fn choose_engine_executable(&mut self, cx: &mut Context<Self>) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("选择 GTP 引擎可执行文件")
            .pick_file()
        else {
            return;
        };
        let path = path.display().to_string();
        let current = self.text_inputs.engine_spec_input.text().trim();
        let next = if current.split('|').count() >= 2 {
            let mut fields = current.splitn(4, '|').map(str::trim);
            let name = fields.next().unwrap_or_default();
            let _old_path = fields.next();
            let args = fields.next().unwrap_or_default();
            let commands = fields.next().unwrap_or_default();
            format!("{name} | {path} | {args} | {commands}")
        } else {
            let name = std::path::Path::new(&path)
                .file_stem()
                .and_then(|name| name.to_str())
                .unwrap_or("GTP Engine");
            format!("{name} | {path} |  | ")
        };
        self.text_inputs.engine_spec_input.set_text(&next);
        self.status = "已选择 GTP 引擎可执行文件；请确认规格后保存".into();
        cx.notify();
    }

    pub fn edit_engine_spec(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(record) = self
            .engine_store
            .list()
            .iter()
            .find(|record| record.name == name)
        else {
            return;
        };
        let spec = format!(
            "{} | {} | {} | {}",
            record.name,
            record.path,
            record.args,
            record.commands.as_deref().unwrap_or_default()
        );
        self.text_inputs.engine_spec_input.set_text(&spec);
        self.engine_spec_editing_name = Some(name.to_owned());
        self.status = format!("正在编辑引擎: {name}").into();
        cx.notify();
    }

    pub fn remove_engine_spec(&mut self, name: &str, cx: &mut Context<Self>) {
        let previous_store = self.engine_store.clone();
        let previous_roles = self.engine_roles.clone();
        if !self.engine_store.remove(name) {
            return;
        }
        self.engine_roles.clear_engine(name);
        for role in EngineRole::ALL {
            if previous_roles.get(role) == Some(name) {
                self.disconnect_engine_role(role);
            }
        }
        if let Err(error) = self.persist_engine_configuration() {
            self.engine_store = previous_store;
            self.engine_roles = previous_roles;
            self.show_toast(format!("删除引擎失败: {error}"), cx);
            return;
        }
        if self.engine_spec_editing_name.as_deref() == Some(name) {
            self.text_inputs.engine_spec_input.set_text("");
            self.engine_spec_editing_name = None;
        }
        self.status = format!("已删除 GTP 引擎: {name}").into();
        self.show_toast(self.status.clone(), cx);
        cx.notify();
    }

    pub fn test_engine_spec(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(record) = self
            .engine_store
            .list()
            .iter()
            .find(|record| record.name == name)
            .cloned()
        else {
            self.show_toast(format!("找不到引擎配置: {name}"), cx);
            return;
        };
        let arguments = crate::engine_console::parse_engine_arguments(&record.args);
        let runtime_dir = self.engine_runtime_directory();
        let display_name = record.name.clone();
        self.status = format!("正在测试 {display_name} 的 GTP 握手…").into();
        let weak = cx.entity().downgrade();
        cx.spawn(
            move |_: gpui::WeakEntity<ShellApp>, cx: &mut gpui::AsyncApp| {
                let mut cx = cx.clone();
                async move {
                    let result = cx
                        .background_executor()
                        .spawn(async move {
                            let transport = ryusei_host::ProcessGtpTransport::start_in(
                                &record.path,
                                &arguments,
                                Some(&runtime_dir),
                            )
                            .map_err(|error| format!("进程启动失败: {error}"))?;
                            let mut session =
                                ryusei_host::EngineSession::start(transport, &record, 19)
                                    .map_err(|error| format!("握手失败: {error}"))?;
                            let label = match session.state() {
                                ryusei_host::EngineSessionState::Ready { name, version } => {
                                    format!("{name} {version}")
                                }
                                ryusei_host::EngineSessionState::Stopped => "stopped".to_owned(),
                            };
                            session
                                .stop()
                                .map_err(|error| format!("握手完成但停止失败: {error}"))?;
                            Ok::<_, String>(label)
                        })
                        .await;
                    weak.update(&mut cx, |shell, cx| match result {
                        Ok(label) => {
                            shell.status = format!("GTP 握手成功: {label}").into();
                            shell.show_toast(shell.status.clone(), cx);
                        }
                        Err(error) => {
                            shell.status = format!("GTP 握手诊断失败: {error}").into();
                            shell.show_toast(shell.status.clone(), cx);
                        }
                    })
                    .ok();
                }
            },
        )
        .detach();
        cx.notify();
    }

    pub fn assign_engine_role(&mut self, role: EngineRole, name: &str, cx: &mut Context<Self>) {
        if self.engine_roles.get(role) == Some(name) {
            self.engine_roles.toggle(role, name);
            self.disconnect_engine_role(role);
        } else {
            self.disconnect_engine_role(role);
            self.engine_roles.assign(role, name);
        }
        if let Err(error) = self.persist_engine_configuration() {
            self.show_toast(format!("保存引擎角色失败: {error}"), cx);
            return;
        }
        self.status = self
            .engine_roles
            .get(role)
            .map(|assigned| format!("{} 角色使用 {assigned}", role.label()))
            .unwrap_or_else(|| format!("已清空 {} 引擎角色", role.label()))
            .into();
        cx.notify();
    }

    fn toggle_gtp_terminal(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_bottom_deck_open() && self.active_bottom_tab() == BottomDeckTab::GtpTerminal {
            self.close_bottom_deck(cx);
        } else {
            self.switch_bottom_tab(BottomDeckTab::GtpTerminal, cx);
        }
    }

    /// Records a console transcript entry only while the persisted
    /// `gtp.console_log_enabled` setting permits it.
    fn record_engine_log(&mut self, entry: EngineLogEntry) {
        if self
            .settings
            .get_bool("gtp.console_log_enabled")
            .unwrap_or(true)
        {
            self.engine_log.push(entry);
        }
    }

    fn send_engine_command(&mut self, draft: &str, cx: &mut Context<Self>) {
        let draft = draft.trim();
        if draft.is_empty() {
            return;
        }
        let role = self
            .active_console_role
            .unwrap_or(crate::engine_console::EngineRole::Analysis);

        // A streaming role remains connected but cannot safely accept an
        // arbitrary console command until its stream is stopped. Do not try to
        // auto-connect it again: that was the old detached/reconnect loop.
        if self.engine_controller.is_streaming(role) {
            let message = format!(
                "{} engine is analyzing — stop analysis before sending raw GTP commands",
                role.label()
            );
            self.status = message.clone().into();
            self.record_engine_log(EngineLogEntry {
                command: draft.to_owned(),
                success: false,
                response: message,
            });
            cx.notify();
            return;
        }
        // Auto-connect an unconnected configured role. A successful connection
        // now remains Ready instead of immediately starting a stream.
        if !self.engine_controller.is_attached(role) && self.engine_roles.get(role).is_some() {
            self.on_engine_connect(role, cx);
            let message = format!(
                "{} engine is connecting; retry the command when it is ready",
                role.label()
            );
            self.status = message.clone().into();
            self.record_engine_log(EngineLogEntry {
                command: draft.to_owned(),
                success: false,
                response: message,
            });
            cx.notify();
            return;
        }

        if self.engine_command_tasks.contains_key(&role)
            || self.engine_controller.is_command_pending(role)
        {
            self.status = format!("{} engine is completing another command", role.label()).into();
            cx.notify();
            return;
        }
        let (name, arguments) = Self::parse_engine_command_line(draft);
        let formatted = format_console_command(&name, &arguments);
        let session = match self.engine_controller.lease_for_command(role) {
            Ok(session) => session,
            Err(_) => {
                self.status = format!("{} engine is not ready", role.label()).into();
                self.record_engine_log(EngineLogEntry {
                    command: formatted,
                    success: false,
                    response: "engine is not ready for a bounded command".to_owned(),
                });
                cx.notify();
                return;
            }
        };
        self.status = format!("{} engine: running {formatted}…", role.label()).into();
        self.text_inputs.gtp_input.set_text("");
        self.text_inputs.engine_draft = "".into();
        let command_generation = self
            .engine_generations
            .get(&role)
            .copied()
            .unwrap_or_default();
        let weak = cx.entity().downgrade();
        self.engine_command_tasks.insert(
            role,
            cx.spawn(
                move |_: gpui::WeakEntity<ShellApp>, cx: &mut gpui::AsyncApp| {
                    let mut cx = cx.clone();
                    async move {
                        let (session, result) = cx
                            .background_executor()
                            .spawn(async move {
                                let mut session = session;
                                let result = session.send_command(&name, arguments);
                                if result.is_err() {
                                    let _ = session.stop();
                                }
                                (session, result)
                            })
                            .await;
                        let _ = weak.update(&mut cx, |shell, cx| {
                            shell.engine_command_tasks.remove(&role);
                            if shell
                                .engine_generations
                                .get(&role)
                                .copied()
                                .unwrap_or_default()
                                != command_generation
                            {
                                shell.engine_controller.discard_command_lease(role);
                                cx.background_executor()
                                    .spawn(async move {
                                        let mut session = session;
                                        let _ = session.stop();
                                    })
                                    .detach();
                                return;
                            }
                            match result {
                                Ok(response) => {
                                    shell.engine_controller.return_command_lease(role, session);
                                    shell.record_engine_log(entry_for_response(
                                        formatted.clone(),
                                        &response,
                                    ));
                                    shell.status =
                                        format!("{} engine: {formatted}", role.label()).into();
                                }
                                Err(error) => {
                                    shell.engine_controller.discard_command_lease(role);
                                    shell.record_engine_log(EngineLogEntry {
                                        command: formatted.clone(),
                                        success: false,
                                        response: format!("protocol error: {error}"),
                                    });
                                    shell.status =
                                        format!("{} engine failed: {error}", role.label()).into();
                                }
                            }
                            cx.notify();
                        });
                    }
                },
            ),
        );
        cx.notify();
    }

    /// Ensures that a role has an assigned, configured engine record. If not
    /// yet configured, it auto-discovers KataGo or existing engine records in the
    /// store, assigns the role, and persists it.
    fn ensure_engine_role_configured(
        &mut self,
        role: EngineRole,
    ) -> Option<ryusei_host::EngineRecord> {
        // 1. If role is explicitly assigned to a record in the store, return it
        // after repairing a stale KataGo model path (a re-downloaded/renamed
        // model otherwise makes the engine exit during the handshake).
        if let Some(name) = self.engine_roles.get(role)
            && let Some(record) = self
                .engine_store
                .list()
                .iter()
                .find(|r| r.name == name)
                .cloned()
        {
            let repaired = ryusei_host::repair_katago_engine_record(&record);
            if repaired != record {
                self.engine_store.upsert(repaired.clone());
                let _ = self.engine_store.save(&mut self.settings);
                let _ = ryusei_host::persist_settings_store(
                    &self.settings,
                    &mut self.settings_persistence,
                );
            }
            return Some(repaired);
        }

        // 2. Look for any existing KataGo / engine record in the store.
        let candidate = self
            .engine_store
            .list()
            .iter()
            .find(|r| {
                let n = r.name.to_lowercase();
                let p = r.path.to_lowercase();
                n.contains("katago") || n.contains("kata") || p.contains("katago")
            })
            .or_else(|| self.engine_store.list().first())
            .cloned();

        if let Some(record) = candidate {
            let repaired = ryusei_host::repair_katago_engine_record(&record);
            let engine_name = repaired.name.clone();
            self.engine_roles.assign(role, &engine_name);
            if repaired != record {
                self.engine_store.upsert(repaired.clone());
                let _ = self.engine_store.save(&mut self.settings);
                let _ = ryusei_host::persist_settings_store(
                    &self.settings,
                    &mut self.settings_persistence,
                );
            }
            let _ = self.persist_engine_roles();
            return Some(repaired);
        }

        // 3. Auto-discover KataGo executable from system / local storage
        let base_dir = crate::file_workflow::current_user_config_directory()
            .unwrap_or_else(|_| std::env::temp_dir());
        if let Ok(env) = ryusei_host::ensure_katago_environment(
            &base_dir,
            ryusei_host::KataGoModelTier::Balanced,
            None,
        ) {
            let engine_name = env.engine_record.name.clone();
            self.engine_store.upsert(env.engine_record.clone());
            self.engine_roles.assign(role, &engine_name);
            let _ = self.engine_store.save(&mut self.settings);
            let _ = self.persist_engine_roles();
            let _ =
                ryusei_host::persist_settings_store(&self.settings, &mut self.settings_persistence);
            return Some(env.engine_record);
        }

        None
    }

    /// Begins a role-specific engine connection without blocking GPUI's event
    /// loop. Process spawn, cold-model handshake and full board replay run on
    /// the background executor; the foreground callback only installs the
    /// already-prepared session.
    fn on_engine_connect(&mut self, role: EngineRole, cx: &mut Context<Self>) {
        if self.engine_connect_tasks.contains_key(&role) {
            self.status = "an engine connection is already in progress".into();
            cx.notify();
            return;
        }
        if role == EngineRole::Analysis && self.analysis_task.is_some() {
            self.status = "stop the active analysis before reattaching its engine".into();
            cx.notify();
            return;
        }
        if self.engine_controller.is_attached(role) {
            self.status = format!("{} engine is already attached", role.label()).into();
            cx.notify();
            return;
        }
        let Some(record) = self.ensure_engine_role_configured(role) else {
            let message = format!(
                "未找到可用的 {} 引擎，请先在设置中配置 KataGo",
                role.label()
            );
            self.status = message.clone().into();
            self.show_toast(message, cx);
            cx.notify();
            return;
        };
        if let Err(error) = ryusei_host::validate_katago_engine_record(&record) {
            self.status = error.clone().into();
            self.show_toast(format!("引擎启动检查失败: {error}"), cx);
            cx.notify();
            return;
        }

        let arguments = crate::engine_console::parse_engine_arguments(&record.args);
        let snapshot = self.host.snapshot();
        let board_size = snapshot.board.width;
        let rule_config = ryusei_host::GameRuleConfig::from_root_properties(
            &snapshot.root_properties,
            board_size,
        );
        let moves = snapshot.moves.clone();
        let runtime_dir = self.engine_runtime_directory();
        let display_name = record.name.clone();
        let connection_generation = {
            let generation = self.engine_generations.entry(role).or_default();
            *generation = generation.wrapping_add(1);
            *generation
        };
        let diagnostic = format!("引擎: {}，参数: {}", record.path, record.args);
        self.status = format!("正在连接 {} 引擎（加载模型并同步棋局）…", role.label()).into();
        self.show_toast(format!("🔌 正在连接 {} 引擎…", role.label()), cx);

        let weak = cx.entity().downgrade();
        self.engine_connect_tasks.insert(role, cx.spawn(
            move |_: gpui::WeakEntity<ShellApp>, cx: &mut gpui::AsyncApp| {
                let mut cx = cx.clone();
                async move {
                    let result: Result<
                        ryusei_host::EngineSession<ryusei_host::ProcessGtpTransport>,
                        String,
                    > = cx
                        .background_executor()
                        .spawn(async move {
                            let transport = ryusei_host::ProcessGtpTransport::start_in(
                                &record.path,
                                &arguments,
                                Some(&runtime_dir),
                            )
                            .map_err(|error| format!("引擎进程启动失败: {error}"))?;
                            ryusei_host::EngineController::<
                                EngineRole,
                                ryusei_host::ProcessGtpTransport,
                            >::prepare_session_with_rules(
                                transport,
                                &record,
                                board_size,
                                &moves,
                                Some(&rule_config),
                            )
                            .map_err(|error| format!("引擎连接握手失败: {error}"))
                        })
                        .await;
                    let _ = weak.update(&mut cx, |shell, cx| {
                        let is_stale = shell
                            .engine_generations
                            .get(&role)
                            .copied()
                            .unwrap_or_default()
                            != connection_generation;
                        match result {
                            Ok(session) if is_stale => {
                                cx.background_executor()
                                    .spawn(async move {
                                        let mut session = session;
                                        let _ = session.stop();
                                    })
                                    .detach();
                            }
                            Err(_) if is_stale => {}
                            result => {
                                shell.engine_connect_tasks.remove(&role);
                                match result {
                                    Ok(session) => {
                                        match shell.engine_controller.attach_prepared(role, session) {
                                            Ok(()) => {
                                                shell.active_console_role = Some(role);
                                                shell.status = format!(
                                                    "{} engine {display_name} ready",
                                                    role.label()
                                                )
                                                .into();
                                                let auto_analysis =
                                                    role == EngineRole::Analysis && shell.analysis_enabled;
                                                shell.show_toast(
                                                    if auto_analysis {
                                                        format!(
                                                            "🔌 已连接 {} 引擎: {display_name}（就绪，正在启动自动分析）",
                                                            role.label()
                                                        )
                                                    } else {
                                                        format!(
                                                            "🔌 已连接 {} 引擎: {display_name}（就绪）",
                                                            role.label()
                                                        )
                                                    },
                                                    cx,
                                                );
                                                let current_node_id = shell
                                                    .host
                                                    .snapshot()
                                                    .current_node_id;
                                                let request_matches = shell
                                                    .pending_analysis_request
                                                    .as_ref()
                                                    .is_some_and(|request| {
                                                        request.matches(
                                                            role,
                                                            connection_generation,
                                                            &current_node_id,
                                                        )
                                                    });
                                                if request_matches {
                                                    shell.pending_analysis_request = None;
                                                    if let Some(target) = shell
                                                        .batch_review_state
                                                        .as_ref()
                                                        .and_then(BatchReviewState::current_node_id)
                                                        .cloned()
                                                        && shell.host.snapshot().current_node_id
                                                            != target
                                                    {
                                                        shell.navigate_to_node_with_batch_policy(
                                                            target, false, cx,
                                                        );
                                                    }
                                                    shell.start_analysis(cx);
                                                }

                                                // Fulfil a queued AI move once the
                                                // requested role finishes connecting.
                                                let pending_move_matches = shell
                                                    .pending_engine_move
                                                    .as_ref()
                                                    .is_some_and(|pending| {
                                                        pending.matches(
                                                            role,
                                                            connection_generation,
                                                            shell
                                                                .host
                                                                .snapshot()
                                                                .board
                                                                .next_player,
                                                        )
                                                    });
                                                if pending_move_matches {
                                                    let pending = shell
                                                        .pending_engine_move
                                                        .take()
                                                        .expect("just matched a pending engine move");
                                                    shell.trigger_engine_genmove(
                                                        pending.role,
                                                        pending.color,
                                                        cx,
                                                    );
                                                }
                                            }
                                            Err(error) => {
                                                shell.status =
                                                    format!("engine attach failed: {error}").into();
                                                shell.show_toast(
                                                    format!("引擎连接失败: {error}"),
                                                    cx,
                                                );
                                            }
                                        }
                                    }
                                    Err(error) => {
                                        if shell
                                            .pending_analysis_request
                                            .as_ref()
                                            .is_some_and(|request| {
                                                request.role == role
                                                    && request.role_generation
                                                        == connection_generation
                                            })
                                        {
                                            shell.pending_analysis_request = None;
                                        }
                                        shell.status = error.clone().into();
                                        shell.show_toast(
                                            format!("{error}\n（{diagnostic}）"),
                                            cx,
                                        );
                                    }
                                }
                                cx.notify();
                            }
                        }
                    });
                }
            },
        ));
    }

    fn disconnect_engine_role(&mut self, role: EngineRole) {
        let generation = self.engine_generations.entry(role).or_default();
        *generation = generation.wrapping_add(1);
        self.engine_connect_tasks.remove(&role);
        if self
            .pending_analysis_request
            .as_ref()
            .is_some_and(|request| request.role == role)
        {
            self.pending_analysis_request = None;
        }
        if self
            .pending_engine_move
            .as_ref()
            .is_some_and(|pending| pending.role == role)
        {
            self.pending_engine_move = None;
        }
        if role == EngineRole::Analysis && self.analysis_task.is_some() {
            self.analysis_run.cancel_and_dispose();
        }
        self.engine_controller.detach(role);
        if self.active_console_role == Some(role) {
            self.active_console_role = None;
        }
    }

    fn on_engine_disconnect(
        &mut self,
        role: EngineRole,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let attached = self.engine_controller.is_attached(role)
            || (role == EngineRole::Analysis && self.analysis_task.is_some());
        self.disconnect_engine_role(role);
        if role == EngineRole::Analysis {
            self.analysis_enabled = false;
            self.restart_analysis_after_position_change = false;
            self.analysis.clear();
            self.analysis_best_move = None;
            self.last_analysis_node = None;
        }
        self.status = if attached {
            format!("{} engine detached", role.label())
        } else {
            format!("{} engine is already detached", role.label())
        }
        .into();
        cx.notify();
    }

    #[allow(dead_code)]
    fn on_analyze(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.analysis_enabled = true;
        self.start_analysis(cx);
    }

    /// Requests analysis from the role-specific Analysis engine and marks the
    /// best candidate on the board.
    fn start_analysis(&mut self, cx: &mut Context<Self>) {
        if self.session_policy.analysis == AnalysisPolicy::FairPlayLockedOff
            && !self.background_review
        {
            self.status = "AI analysis is locked for this remote competition".into();
            cx.notify();
            return;
        }
        if self.analysis_task.is_some() {
            self.status = "analysis is already running; stop it before starting another run".into();
            cx.notify();
            return;
        }
        let analysis_snapshot = self.host.snapshot();
        let trial_move = self.trial_move.clone();
        let (command, command_arguments) = analysis_command_from_settings(&self.settings);
        // KataGo limits search depth via `maxVisits`; 0 means unlimited.
        // Applied through `kata-set-param` right before the stream starts.
        let max_visits = self
            .batch_review_profile
            .map(ryusei_domain_core::ReviewProfile::visits)
            .or_else(|| {
                self.settings
                    .get("engines.analysis_max_visits")
                    .and_then(serde_json::Value::as_u64)
            })
            .unwrap_or(500);
        if !self.engine_controller.is_attached(EngineRole::Analysis) {
            let name = self
                .engine_roles
                .get(EngineRole::Analysis)
                .unwrap_or_default();
            if !name.is_empty() {
                let role_generation = self
                    .engine_generations
                    .get(&EngineRole::Analysis)
                    .copied()
                    .unwrap_or_default()
                    .wrapping_add(1);
                self.pending_analysis_request = Some(PendingAnalysisRequest {
                    role: EngineRole::Analysis,
                    role_generation,
                    node_id: analysis_snapshot.current_node_id.clone(),
                });
                self.on_engine_connect(EngineRole::Analysis, cx);
                return;
            } else {
                self.status = "select an analysis engine first (e.g. KataGo in Settings)".into();
                cx.notify();
                return;
            }
        }
        if self.engine_controller.is_streaming(EngineRole::Analysis) {
            self.status =
                "analysis engine is already streaming; stop it before starting another run".into();
            cx.notify();
            return;
        }

        let analysis_player = trial_move
            .as_ref()
            .map_or(analysis_snapshot.board.next_player, |move_dto| {
                move_dto.color.opponent()
            });
        let run = self
            .analysis_run
            .begin(analysis_snapshot.current_node_id.clone(), analysis_player);
        self.active_analysis_trial_move = trial_move.clone();
        // Clear the previous node's candidates so a fresh analysis run never
        // leaves stale markers on the board while the engine searches.
        self.analysis.clear();
        self.analysis_best_move = None;
        let analysis_board_size = analysis_snapshot.board.width;
        let mut analysis_moves = analysis_snapshot.moves.clone();
        if let Some(trial_move) = trial_move.clone() {
            analysis_moves.push(trial_move);
        }
        let bounded_arguments = command_arguments.clone();

        // Bounded `analyze` responses also own a session worker. The GTP
        // request can wait for the engine's search deadline, so it must not
        // run inside this foreground event handler.
        if command == "analyze" && self.engine_controller.is_attached(EngineRole::Analysis) {
            let session = match self
                .engine_controller
                .lease_for_command(EngineRole::Analysis)
            {
                Ok(session) => session,
                Err(error) => {
                    self.status = format!("analysis session unavailable: {error}").into();
                    cx.notify();
                    return;
                }
            };
            let task_run = run.clone();
            let task_command = command.clone();
            let task_arguments = bounded_arguments;
            let task_board_size = analysis_board_size;
            let task_moves = analysis_moves;
            let task_trial = trial_move.clone();
            self.status = "analysis: waiting for bounded engine response…".into();
            let weak = cx.entity().downgrade();
            self.analysis_task = Some(cx.spawn(
                move |_: gpui::WeakEntity<ShellApp>, cx: &mut gpui::AsyncApp| {
                    let mut cx = cx.clone();
                    async move {
                        let (session, result) = cx
                            .background_executor()
                            .spawn(async move {
                                let mut session = session;
                                let result = if task_trial.is_some() {
                                    ryusei_host::EngineController::<
                                        EngineRole,
                                        ryusei_host::ProcessGtpTransport,
                                    >::replay_leased(
                                        &mut session, task_board_size, &task_moves
                                    )
                                    .map_err(|error| error.to_string())
                                    .and_then(|()| {
                                        session
                                            .analyze(&task_command, task_arguments)
                                            .map_err(|error| error.to_string())
                                    })
                                } else {
                                    session
                                        .analyze(&task_command, task_arguments)
                                        .map_err(|error| error.to_string())
                                };
                                if result.is_err() {
                                    let _ = session.stop();
                                }
                                (session, result)
                            })
                            .await;
                        if task_run.should_dispose() {
                            let _ = cx
                                .background_executor()
                                .spawn(async move {
                                    let mut session = session;
                                    let _ = session.stop();
                                })
                                .await;
                            let _ = weak.update(&mut cx, |shell, cx| {
                                shell.analysis_task = None;
                                shell
                                    .engine_controller
                                    .discard_command_lease(EngineRole::Analysis);
                                cx.notify();
                            });
                            return;
                        }
                        let _ = weak.update(&mut cx, |shell, cx| {
                            shell.analysis_task = None;
                            match result {
                                Ok(entries) if task_run.is_current() => {
                                    shell
                                        .engine_controller
                                        .return_command_lease(EngineRole::Analysis, session);
                                    shell.set_analysis(entries, cx);
                                    shell.analysis_finished(
                                        &task_run,
                                        ryusei_host::AnalysisRunOutcome::Completed,
                                        cx,
                                    );
                                }
                                Ok(_) => {
                                    shell
                                        .engine_controller
                                        .discard_command_lease(EngineRole::Analysis);
                                }
                                Err(error) => {
                                    shell
                                        .engine_controller
                                        .discard_command_lease(EngineRole::Analysis);
                                    shell.analysis_finished(
                                        &task_run,
                                        ryusei_host::AnalysisRunOutcome::Failed(error.to_string()),
                                        cx,
                                    );
                                }
                            }
                            cx.notify();
                        });
                    }
                },
            ));
            cx.notify();
            return;
        }

        // Streaming commands (kata-analyze / lz-analyze): reuse the already
        // connected engine session when it supports streaming (no second
        // process), otherwise fall back to a fresh analysis process with the
        // current position replayed into it.
        let board_size = self.host.snapshot().board.width;
        let mut moves = self.host.snapshot().moves.clone();
        if let Some(trial_move) = trial_move.clone() {
            moves.push(trial_move);
        }

        // Session mode reuses a connected ready session, but all replay and
        // stream-start GTP I/O is performed by the worker below.
        let session_mode = self.engine_controller.is_ready(EngineRole::Analysis);
        if self.engine_controller.is_attached(EngineRole::Analysis) && !session_mode {
            self.status = "analysis engine is busy; wait for it to become ready".into();
            self.analysis_run.finish(&run);
            cx.notify();
            return;
        }
        let task_run = run.clone();
        let task_command = command.clone();
        let task_arguments = command_arguments.clone();
        let preparation_command = task_command.clone();
        let task_board_size = board_size;
        let task_moves = moves.clone();
        let task_max_visits = max_visits;
        let task_position_changed = self.last_analysis_node.as_ref()
            != Some(&analysis_snapshot.current_node_id)
            || self.last_analysis_trial_move != trial_move;
        self.last_analysis_node = Some(analysis_snapshot.current_node_id.clone());
        self.last_analysis_trial_move = trial_move.clone();

        if session_mode {
            let session = match self
                .engine_controller
                .lease_for_analysis(EngineRole::Analysis)
            {
                Ok(session) => session,
                Err(error) => {
                    self.status = format!("analysis session unavailable: {error}").into();
                    self.analysis_run.finish(&run);
                    cx.notify();
                    return;
                }
            };
            self.status =
                format!("analysis: streaming {command} on attached Analysis engine").into();
            let session_run = task_run.clone();
            self.analysis_task = Some(cx.spawn(
                move |shell_weak: gpui::WeakEntity<ShellApp>, cx: &mut gpui::AsyncApp| {
                    let mut cx = cx.clone();
                    async move {
                        let (mut session, preparation) = cx
                            .background_executor()
                            .spawn(async move {
                                let mut session = session;
                                let result: Result<(), String> = if task_position_changed {
                                    ryusei_host::EngineController::<
                                        EngineRole,
                                        ryusei_host::ProcessGtpTransport,
                                    >::replay_leased(
                                        &mut session, task_board_size, &task_moves
                                    )
                                    .map_err(|error| error.to_string())
                                } else {
                                    Ok(())
                                };
                                let result = result.and_then(|()| {
                                    if preparation_command == "kata-analyze" {
                                        session
                                            .send_command(
                                                "kata-set-param",
                                                vec![
                                                    "maxVisits".to_owned(),
                                                    task_max_visits.to_string(),
                                                ],
                                            )
                                            .map(|response| {
                                                if response.success {
                                                    Ok(())
                                                } else {
                                                    Err(format!(
                                                        "engine rejected kata-set-param: {}",
                                                        response.content
                                                    ))
                                                }
                                            })
                                            .map_err(|error| error.to_string())??;
                                    }
                                    Ok(())
                                });
                                let result = result.and_then(|()| {
                                    session
                                        .stream_analyze(&preparation_command, task_arguments)
                                        .map_err(|error| error.to_string())
                                });
                                (session, result)
                            })
                            .await;
                        if let Err(error) = preparation {
                            let _ = cx
                                .background_executor()
                                .spawn(async move {
                                    let _ = session.stop();
                                })
                                .await;
                            let _ = shell_weak.update(&mut cx, |shell, cx| {
                                shell.analysis_task = None;
                                shell
                                    .engine_controller
                                    .discard_analysis_lease(EngineRole::Analysis);
                                shell.analysis_finished(
                                    &session_run,
                                    ryusei_host::AnalysisRunOutcome::Failed(format!(
                                        "analysis preparation failed: {error}"
                                    )),
                                    cx,
                                );
                            });
                            return;
                        }
                        let mut pending: Vec<ryusei_host::AnalysisEntry> = Vec::new();
                        let mut last_flush = Instant::now();
                        let mut target_reached = false;
                        let mut outcome = ryusei_host::AnalysisRunOutcome::Failed(
                            "analysis stream ended without a completed result".to_owned(),
                        );
                        loop {
                            if target_reached || session_run.should_stop() {
                                if session_run.is_current() {
                                    let (next_session, _) = cx
                                        .background_executor()
                                        .spawn(async move {
                                            let mut session = session;
                                            let result = ryusei_host::EngineController::<
                                                EngineRole,
                                                ryusei_host::ProcessGtpTransport,
                                            >::stop_leased_analysis(
                                                &mut session
                                            );
                                            (session, result)
                                        })
                                        .await;
                                    session = next_session;
                                }
                                // Drain the stream tail so the engine's final
                                // search results land in the analysis set
                                // before the session returns to the controller.
                                let drain_deadline = Instant::now() + Duration::from_secs(3);
                                let mut saw_header = false;
                                while Instant::now() < drain_deadline {
                                    let (next_session, next_line) = cx
                                        .background_executor()
                                        .spawn(async move {
                                            let mut session = session;
                                            let line = ryusei_host::EngineController::<
                                                EngineRole,
                                                ryusei_host::ProcessGtpTransport,
                                            >::recv_analysis_line(
                                                &mut session,
                                                Duration::from_millis(50),
                                            );
                                            (session, line)
                                        })
                                        .await;
                                    session = next_session;
                                    match next_line {
                                        Some(line) => {
                                            let trimmed = line.trim();
                                            if trimmed.starts_with('=') || trimmed.starts_with('?')
                                            {
                                                saw_header = true;
                                                continue;
                                            }
                                            if trimmed.is_empty() {
                                                if saw_header {
                                                    break;
                                                }
                                                continue;
                                            }
                                            pending.extend(parse_stream_entries(
                                                &task_command,
                                                trimmed,
                                            ));
                                        }
                                        None => {
                                            if ryusei_host::EngineController::<
                                                EngineRole,
                                                ryusei_host::ProcessGtpTransport,
                                            >::session_stream_closed(
                                                &session
                                            ) {
                                                break;
                                            }
                                        }
                                    }
                                }
                                outcome = if session_run.should_dispose() {
                                    ryusei_host::AnalysisRunOutcome::Cancelled
                                } else {
                                    ryusei_host::AnalysisRunOutcome::Completed
                                };
                                break;
                            }
                            let (next_session, next_line) = cx
                                .background_executor()
                                .spawn(async move {
                                    let mut session = session;
                                    let line = ryusei_host::EngineController::<
                                        EngineRole,
                                        ryusei_host::ProcessGtpTransport,
                                    >::recv_analysis_line(
                                        &mut session, Duration::from_millis(50)
                                    );
                                    (session, line)
                                })
                                .await;
                            session = next_session;
                            if let Some(line) = next_line {
                                let trimmed = line.trim();
                                // GTP responses begin with `=` / `?` and are
                                // followed by an empty line before the streamed
                                // `info` records. Skip both instead of treating
                                // the empty line as the end of the stream.
                                if trimmed.is_empty()
                                    || trimmed.starts_with('=')
                                    || trimmed.starts_with('?')
                                {
                                    continue;
                                }
                                let entries = parse_stream_entries(&task_command, trimmed);
                                let proxy_completion = task_command == "kata-analyze"
                                    && trimmed.starts_with('{')
                                    && entries.iter().any(|entry| !entry.is_during_search);
                                target_reached = proxy_completion
                                    || (task_max_visits > 0
                                        && entries
                                            .iter()
                                            .map(|entry| entry.visits)
                                            .max()
                                            .unwrap_or_default()
                                            >= task_max_visits);
                                pending.extend(entries);
                                // Official KataGo GTP `kata-analyze` emits
                                // continuous `info move` records without a
                                // completion sentinel. JSON proxy adapters
                                // retain their explicit completion record.
                                if proxy_completion {
                                    outcome = ryusei_host::AnalysisRunOutcome::Completed;
                                    break;
                                }
                            } else if ryusei_host::EngineController::<
                                EngineRole,
                                ryusei_host::ProcessGtpTransport,
                            >::session_stream_closed(&session)
                            {
                                let stderr = ryusei_host::EngineController::<
                                    EngineRole,
                                    ryusei_host::ProcessGtpTransport,
                                >::session_stderr_tail(
                                    &session
                                );
                                let _ = shell_weak.update(&mut cx, |shell, cx| {
                                    shell.show_toast(
                                        if stderr.is_empty() {
                                            "⚠️ KataGo 引擎进程已退出，请重新连接".to_owned()
                                        } else {
                                            format!(
                                                "⚠️ KataGo 引擎进程已退出: {}",
                                                stderr.lines().last().unwrap_or_default()
                                            )
                                        },
                                        cx,
                                    );
                                });
                                break;
                            }
                            if last_flush.elapsed() >= Duration::from_millis(120)
                                && !pending.is_empty()
                            {
                                let batch = std::mem::take(&mut pending);
                                let _ = shell_weak.update(&mut cx, |shell, cx| {
                                    shell.push_analysis_batch(&session_run, batch, cx)
                                });
                                last_flush = Instant::now();
                            }
                        }
                        if !pending.is_empty() {
                            let batch = std::mem::take(&mut pending);
                            let _ = shell_weak.update(&mut cx, |shell, cx| {
                                shell.push_analysis_batch(&session_run, batch, cx)
                            });
                        }
                        // A stale/cancelled run owns cleanup in this worker. A
                        // live run may return its session to the foreground
                        // controller after all blocking I/O is finished.
                        if session_run.should_dispose()
                            || matches!(outcome, ryusei_host::AnalysisRunOutcome::Failed(_))
                        {
                            let final_outcome = if session_run.should_dispose() {
                                ryusei_host::AnalysisRunOutcome::Cancelled
                            } else {
                                outcome
                            };
                            let _ = cx
                                .background_executor()
                                .spawn(async move {
                                    let mut session = session;
                                    let _ = session.stop();
                                })
                                .await;
                            let _ = shell_weak.update(&mut cx, |shell, cx| {
                                shell
                                    .engine_controller
                                    .discard_analysis_lease(EngineRole::Analysis);
                                shell.analysis_finished(&session_run, final_outcome, cx);
                            });
                        } else {
                            let _ = shell_weak.update(&mut cx, |shell, cx| {
                                shell.finish_streaming_analysis(&session_run, session, outcome, cx);
                            });
                        }
                    }
                },
            ));
            cx.notify();
            return;
        }

        let analysis_engine = self.engine_roles.get(EngineRole::Analysis);
        let Some(record) = self
            .engine_store
            .list()
            .iter()
            .find(|record| analysis_engine.is_none_or(|name| record.name == name))
            .cloned()
        else {
            self.status = match analysis_engine {
                Some(name) => format!("selected analysis engine {name} is not configured"),
                None => "no engine configured for analysis".to_owned(),
            }
            .into();
            cx.notify();
            return;
        };
        let arguments = crate::engine_console::parse_engine_arguments(&record.args);
        let executable = record.path.clone();
        let runtime_dir = self.engine_runtime_directory();
        let full_command = if command_arguments.is_empty() {
            command.clone()
        } else {
            format!("{} {}", command, command_arguments.join(" "))
        };
        self.status = format!("analysis: preparing {command}").into();
        let stream_run = task_run.clone();
        let preparation_command = task_command.clone();
        self.analysis_task = Some(cx.spawn(
            move |shell_weak: gpui::WeakEntity<ShellApp>, cx: &mut gpui::AsyncApp| {
                let mut cx = cx.clone();
                async move {
                    let preparation = cx
                        .background_executor()
                        .spawn(async move {
                            AnalysisStream::start_in(&executable, &arguments, Some(&runtime_dir))
                                .map_err(|error| error.to_string())
                                .and_then(|mut stream| {
                                    let setup_timeout =
                                        ryusei_domain_core::gtp::DEFAULT_COMMAND_TIMEOUT;
                                    let replay = |stream: &mut AnalysisStream| -> Result<(), String> {
                                        let commands =
                                            replay_position_stream_commands(board_size, &moves);
                                        for command in commands {
                                            let response = stream
                                                .send_request(&command, setup_timeout)
                                                .map_err(|error| error.to_string())?;
                                            if !response.success {
                                                return Err(format!(
                                                    "engine rejected setup command `{command}`: {}",
                                                    response.content
                                                ));
                                            }
                                        }
                                        Ok(())
                                    };
                                    replay(&mut stream)?;
                                    if preparation_command == "kata-analyze" {
                                        let command =
                                            format!("kata-set-param maxVisits {max_visits}");
                                        let response = stream
                                            .send_request(&command, setup_timeout)
                                            .map_err(|error| error.to_string())?;
                                        if !response.success {
                                            return Err(format!(
                                                "engine rejected setup command `{command}`: {}",
                                                response.content
                                            ));
                                        }
                                    }
                                    stream
                                        .send_command(&full_command)
                                        .map_err(|error| error.to_string())?;
                                    Ok(stream)
                                })
                        })
                        .await;
                    let Ok(mut stream) = preparation else {
                        let error = preparation
                            .err()
                            .unwrap_or_else(|| "unknown analysis preparation failure".to_owned());
                        let _ = shell_weak.update(&mut cx, |shell, cx| {
                            shell.analysis_finished(
                                &stream_run,
                                ryusei_host::AnalysisRunOutcome::Failed(format!(
                                    "analysis preparation failed: {error}"
                                )),
                                cx,
                            );
                        });
                        return;
                    };
                    let mut pending: Vec<ryusei_host::AnalysisEntry> = Vec::new();
                    let mut last_flush = Instant::now();
                    let mut target_reached = false;
                    let mut outcome = ryusei_host::AnalysisRunOutcome::Failed(
                        "analysis process ended without a completed result".to_owned(),
                    );
                    loop {
                        if target_reached || stream_run.should_stop() {
                            if stream_run.is_current() {
                                let (next_stream, _) = cx
                                    .background_executor()
                                    .spawn(async move {
                                        let mut stream = stream;
                                        let result = stream.send_command("stop");
                                        (stream, result)
                                    })
                                    .await;
                                stream = next_stream;
                            }
                            // Drain the stream tail so the final search
                            // results land in the analysis set.
                            let drain_deadline = Instant::now() + Duration::from_secs(3);
                            let mut saw_header = false;
                            while Instant::now() < drain_deadline {
                                let (next_stream, next_line) = cx
                                    .background_executor()
                                    .spawn(async move {
                                        let mut stream = stream;
                                        let line =
                                            stream.recv_line_timeout(Duration::from_millis(50));
                                        (stream, line)
                                    })
                                    .await;
                                stream = next_stream;
                                match next_line {
                                    Some(line) => {
                                        let trimmed = line.trim();
                                        if trimmed.starts_with('=') || trimmed.starts_with('?') {
                                            saw_header = true;
                                            continue;
                                        }
                                        if trimmed.is_empty() {
                                            if saw_header {
                                                break;
                                            }
                                            continue;
                                        }
                                        pending
                                            .extend(parse_stream_entries(&task_command, trimmed));
                                    }
                                    None => {
                                        if stream.is_stream_closed() {
                                            break;
                                        }
                                    }
                                }
                            }
                            outcome = if stream_run.should_dispose() {
                                ryusei_host::AnalysisRunOutcome::Cancelled
                            } else {
                                ryusei_host::AnalysisRunOutcome::Completed
                            };
                            break;
                        }
                        let (next_stream, next_line) = cx
                            .background_executor()
                            .spawn(async move {
                                let mut stream = stream;
                                let line = stream.recv_line_timeout(Duration::from_millis(50));
                                (stream, line)
                            })
                            .await;
                        stream = next_stream;
                        if let Some(line) = next_line {
                            let trimmed = line.trim();
                            // Skip GTP response headers and blank terminators;
                            // only `info` records feed the analysis set.
                            if trimmed.is_empty()
                                || trimmed.starts_with('=')
                                || trimmed.starts_with('?')
                            {
                                continue;
                            }
                            let entries = parse_stream_entries(&task_command, trimmed);
                            let proxy_completion = task_command == "kata-analyze"
                                && trimmed.starts_with('{')
                                && entries.iter().any(|entry| !entry.is_during_search);
                            target_reached = proxy_completion
                                || (max_visits > 0
                                    && entries
                                        .iter()
                                        .map(|entry| entry.visits)
                                        .max()
                                        .unwrap_or_default()
                                        >= max_visits);
                            pending.extend(entries);
                            // Official KataGo GTP `kata-analyze` emits
                            // continuous `info move` records without a
                            // completion sentinel. JSON proxy adapters
                            // retain their explicit completion record.
                            if proxy_completion {
                                outcome = ryusei_host::AnalysisRunOutcome::Completed;
                                break;
                            }
                        } else if stream.is_stream_closed() {
                            let _ = shell_weak.update(&mut cx, |shell, cx| {
                                shell.show_toast(
                                    "⚠️ KataGo 分析进程已退出，请重新连接".to_owned(),
                                    cx,
                                );
                            });
                            break;
                        }
                        if last_flush.elapsed() >= Duration::from_millis(120) && !pending.is_empty()
                        {
                            let batch = std::mem::take(&mut pending);
                            let _ = shell_weak.update(&mut cx, |shell, cx| {
                                shell.push_analysis_batch(&stream_run, batch, cx)
                            });
                            last_flush = Instant::now();
                        }
                    }
                    if !pending.is_empty() {
                        let batch = std::mem::take(&mut pending);
                        let _ = shell_weak.update(&mut cx, |shell, cx| {
                            shell.push_analysis_batch(&stream_run, batch, cx)
                        });
                    }
                    let _ = cx
                        .background_executor()
                        .spawn(async move {
                            drop(stream);
                        })
                        .await;
                    let _ = shell_weak.update(&mut cx, |shell, cx| {
                        shell.analysis_finished(&stream_run, outcome, cx)
                    });
                }
            },
        ));
        cx.notify();
    }

    /// Replaces the analysis set with a merged batch from the streaming task
    /// and refreshes the best-move marker.
    fn push_analysis_batch(
        &mut self,
        run: &ryusei_host::AnalysisRunTicket,
        entries: Vec<ryusei_host::AnalysisEntry>,
        cx: &mut Context<Self>,
    ) {
        if !run.is_current() {
            return;
        }
        // A leftover interactive stream must never paint the board once a
        // remote competition is fair-play locked; only the opted-in per-move
        // background review may populate analysis during the game.
        if self.session_policy.analysis == AnalysisPolicy::FairPlayLockedOff
            && !self.background_review
        {
            return;
        }
        self.analysis = merge_analysis_entries(&self.analysis, entries);
        self.set_analysis(self.analysis.clone(), cx);
    }

    /// Stores the strongest completed Analysis-role candidate as upstream
    /// compatible `SBKV` (Black percent) and finite `SBKS` (Black score lead).
    /// The tracked node/player gate prevents a late streaming batch from
    /// annotating a node reached after the analysis request started.
    fn persist_analysis_snapshot(&mut self) -> bool {
        let snapshot = self.host.snapshot();
        let Some(player) = self.analysis_run.player_for_node(&snapshot.current_node_id) else {
            return false;
        };
        let completed_entries = self
            .analysis
            .iter()
            .filter(|entry| !entry.is_during_search)
            .cloned()
            .collect::<Vec<_>>();
        let Some(entry) = best_analysis_entry(&completed_entries) else {
            return false;
        };
        let mut events = RecordingSink;
        for (property, value) in analysis_sgf_properties(entry, player) {
            if self
                .host
                .apply_transaction(
                    crate::node_inspector::create_property_transaction(
                        &snapshot.current_node_id,
                        property,
                        vec![value],
                    ),
                    &mut events,
                )
                .is_err()
            {
                return false;
            }
        }
        // Persist the full candidate list so reviewing any move later can
        // restore the on-board candidate markers and winrates without re-running
        // the engine.
        let candidates = serialize_analysis_candidates(&completed_entries);
        if !candidates.is_empty()
            && self
                .host
                .apply_transaction(
                    crate::node_inspector::create_property_transaction(
                        &snapshot.current_node_id,
                        CANDIDATES_PROPERTY,
                        vec![candidates],
                    ),
                    &mut events,
                )
                .is_err()
        {
            return false;
        }
        self.synchronize_recovery();
        true
    }

    /// Stores an analysis set and refreshes the best-move marker and status.
    fn set_analysis(&mut self, entries: Vec<ryusei_host::AnalysisEntry>, cx: &mut Context<Self>) {
        self.analysis = entries;
        let board_size = self.host.snapshot().board.width;
        self.analysis_best_move = best_analysis_move(&self.analysis, board_size)
            .map(|(column, row)| Vertex { column, row });
        self.status = format!(
            "analysis: {} candidates{}",
            self.analysis.len(),
            self.analysis_best_move
                .map(|_| " — best move marked".to_owned())
                .unwrap_or_default()
        )
        .into();
        cx.notify();
    }

    /// Returns a leased Analysis session once its streaming worker exits.
    fn finish_streaming_analysis(
        &mut self,
        run: &ryusei_host::AnalysisRunTicket,
        session: ryusei_host::EngineSession<ryusei_host::ProcessGtpTransport>,
        outcome: ryusei_host::AnalysisRunOutcome,
        cx: &mut Context<Self>,
    ) {
        if self.analysis_run.should_dispose(run) {
            self.engine_controller
                .discard_analysis_lease(EngineRole::Analysis);
            self.analysis_finished(run, ryusei_host::AnalysisRunOutcome::Cancelled, cx);
            return;
        }
        if self.analysis_run.replay_required(run) {
            // A stale worker must never replay on the foreground thread. The
            // next analysis request captures the current position and performs
            // replay in its background preparation phase.
            self.engine_controller
                .return_analysis_lease(EngineRole::Analysis, session);
            self.analysis_run.clear_replay(run);
            self.analysis_finished(run, ryusei_host::AnalysisRunOutcome::Cancelled, cx);
            let restart = self.restart_analysis_after_position_change;
            self.restart_analysis_after_position_change = false;
            if restart {
                self.status = "analysis stopped; replaying the new position…".into();
                self.start_analysis(cx);
            } else {
                self.status = "analysis stopped; next run will replay the new position".into();
            }
            return;
        }
        self.analysis_run.clear_replay(run);
        self.engine_controller
            .return_analysis_lease(EngineRole::Analysis, session);
        self.analysis_finished(run, outcome, cx);
    }

    /// Clears the running-analysis state once the matching streaming task ends.
    fn analysis_finished(
        &mut self,
        run: &ryusei_host::AnalysisRunTicket,
        mut outcome: ryusei_host::AnalysisRunOutcome,
        cx: &mut Context<Self>,
    ) {
        if !self.analysis_run.finish(run) {
            // No newer analysis can start until this worker has completed, so
            // clearing the task here releases the reconnect guard as well.
            self.analysis_task = None;
            return;
        }
        self.analysis_task = None;
        let analysis_was_trial = self.active_analysis_trial_move.take().is_some();
        if matches!(outcome, ryusei_host::AnalysisRunOutcome::Completed)
            && !analysis_was_trial
            && !self.persist_analysis_snapshot()
        {
            outcome = ryusei_host::AnalysisRunOutcome::Failed(
                "analysis completed without a persistable candidate".to_owned(),
            );
        }

        if self.batch_review_state.is_some()
            && matches!(outcome, ryusei_host::AnalysisRunOutcome::Completed)
        {
            let next_target = self
                .batch_review_state
                .as_mut()
                .and_then(|state| state.advance().cloned());
            if let Some(next_target) = next_target {
                if let Some(progress) = self.batch_review_progress.as_mut() {
                    progress.current_move = self
                        .batch_review_state
                        .as_ref()
                        .map_or(progress.current_move, |state| state.next_index + 1);
                }
                self.navigate_to_node_with_batch_policy(next_target, false, cx);
                self.status = format!(
                    "全盘复盘：正在分析第 {}/{} 手",
                    self.batch_review_progress
                        .map_or(0, |progress| progress.current_move),
                    self.batch_review_progress
                        .map_or(0, |progress| progress.total_moves),
                )
                .into();
                cx.notify();
                self.start_analysis(cx);
                return;
            }

            let original_node = self
                .batch_review_state
                .as_ref()
                .map(|state| state.original_node_id.clone());
            self.batch_review_state = None;
            self.batch_review_progress = None;
            self.batch_review_profile = None;
            if let Some(original_node) = original_node
                && self.host.snapshot().current_node_id != original_node
            {
                self.navigate_to_node_with_batch_policy(original_node, false, cx);
            }
            self.status = "全盘复盘完成".into();
            self.show_toast("✅ 全盘复盘完成".to_owned(), cx);
            cx.notify();
            return;
        }

        if self.batch_review_state.is_some() {
            let original_node = self
                .batch_review_state
                .as_ref()
                .map(|state| state.original_node_id.clone());
            self.batch_review_state = None;
            self.batch_review_progress = None;
            self.batch_review_profile = None;
            if let Some(original_node) = original_node
                && self.host.snapshot().current_node_id != original_node
            {
                self.navigate_to_node_with_batch_policy(original_node, false, cx);
            }
            let message = match &outcome {
                ryusei_host::AnalysisRunOutcome::Cancelled => "全盘复盘已取消".to_owned(),
                ryusei_host::AnalysisRunOutcome::Failed(error) => {
                    format!("全盘复盘失败: {error}")
                }
                ryusei_host::AnalysisRunOutcome::Completed => unreachable!(),
            };
            self.status = message.clone().into();
            self.show_toast(message, cx);
            cx.notify();
            return;
        }

        if analysis_was_trial && matches!(outcome, ryusei_host::AnalysisRunOutcome::Completed) {
            self.last_analysis_trial_move = self.trial_move.clone();
        }
        self.status = match &outcome {
            ryusei_host::AnalysisRunOutcome::Completed if analysis_was_trial => {
                "试下分析完成：已更新 AI 应对".into()
            }
            ryusei_host::AnalysisRunOutcome::Completed => "analysis finished".into(),
            ryusei_host::AnalysisRunOutcome::Cancelled => "analysis cancelled".into(),
            ryusei_host::AnalysisRunOutcome::Failed(error) => {
                format!("analysis failed: {error}").into()
            }
        };
        // A per-move background review finished: clear its budget override so
        // the next interactive analysis uses the user's normal max-visits.
        if self.background_review {
            self.background_review = false;
            self.batch_review_profile = None;
        }
        cx.notify();

        // A maxVisits quick-switch landed while analysis was streaming; the
        // search has now stopped and the session returned, so restart with
        // the new limit on the same node (reusing KataGo's search tree).
        if self.restart_analysis_after_stop {
            self.restart_analysis_after_stop = false;
            self.start_analysis(cx);
        } else if self.restart_analysis_after_position_change {
            self.restart_analysis_after_position_change = false;
            self.start_analysis(cx);
        }
    }

    #[allow(dead_code)]
    fn on_analysis_stop(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.stop_analysis(cx);
    }

    /// Requests the streaming analysis task to stop and emit its final
    /// candidates.
    fn stop_analysis(&mut self, cx: &mut Context<Self>) {
        self.analysis_enabled = false;
        self.restart_analysis_after_position_change = false;
        if self.analysis_task.is_some() {
            self.analysis_run.request_stop();
            self.status = "stopping analysis".into();
        } else {
            self.status = "no analysis running".into();
        }
        cx.notify();
    }

    /// Persists a new analysis `maxVisits` value and, when analysis is
    /// currently streaming, schedules a restart so the new depth applies
    /// immediately without losing the search tree.
    fn apply_analysis_visits(&mut self, visits: u64, cx: &mut Context<Self>) {
        let _ = self
            .settings
            .set("engines.analysis_max_visits", serde_json::json!(visits));
        let _ = ryusei_host::persist_settings_store(&self.settings, &mut self.settings_persistence);

        if self.analysis_task.is_some() {
            self.restart_analysis_after_stop = true;
            self.stop_analysis(cx);
        } else {
            self.show_toast(
                format!(
                    "🔍 分析深度已设为 {}",
                    if visits == 0 {
                        "无限".to_owned()
                    } else {
                        visits.to_string()
                    }
                ),
                cx,
            );
        }
        cx.notify();
    }

    fn start_review_profile_action(
        &mut self,
        profile: ryusei_domain_core::ReviewProfile,
        cx: &mut Context<Self>,
    ) {
        self.start_whole_game_review_with_profile(profile, cx);
    }

    fn start_whole_game_review(&mut self, cx: &mut Context<Self>) {
        self.start_whole_game_review_with_profile(ryusei_domain_core::ReviewProfile::default(), cx);
    }

    fn start_whole_game_review_with_profile(
        &mut self,
        profile: ryusei_domain_core::ReviewProfile,
        cx: &mut Context<Self>,
    ) {
        if self.session_policy.analysis == AnalysisPolicy::FairPlayLockedOff {
            self.show_toast("OGS 远程对局期间禁止 AI 全盘复盘".to_owned(), cx);
            return;
        }
        if self
            .batch_review_progress
            .is_some_and(|progress| progress.is_running)
        {
            self.stop_whole_game_review(cx);
            return;
        }
        if self.analysis_task.is_some()
            || self
                .engine_connect_tasks
                .contains_key(&EngineRole::Analysis)
            || self
                .engine_command_tasks
                .contains_key(&EngineRole::Analysis)
            || self.engine_controller.is_streaming(EngineRole::Analysis)
        {
            self.show_toast("请先停止当前引擎操作，再开始全盘复盘".to_owned(), cx);
            return;
        }

        let snapshot = self.host.snapshot();
        let lineage = ryusei_host::active_lineage_moves(&snapshot);
        if lineage.is_empty() {
            self.status = "当前棋谱没有可复盘的落子".into();
            self.show_toast("当前棋谱没有可复盘的落子".to_owned(), cx);
            cx.notify();
            return;
        }
        let node_ids = ryusei_host::active_lineage_review_nodes(&snapshot);
        self.batch_review_profile = Some(profile);
        self.batch_review_state = BatchReviewState::new(snapshot.current_node_id.clone(), node_ids);
        self.batch_review_progress = Some(ryusei_host::BatchReviewProgress {
            current_move: 1,
            total_moves: lineage.len() + 1,
            is_running: true,
        });
        self.show_toast(
            format!(
                "⏩ {} {} visits 全盘 AI 复盘分析 (共 {} 手)...",
                profile.label(),
                profile.visits(),
                lineage.len()
            ),
            cx,
        );

        if !self.engine_controller.is_attached(EngineRole::Analysis) {
            let role_generation = self
                .engine_generations
                .get(&EngineRole::Analysis)
                .copied()
                .unwrap_or_default()
                .wrapping_add(1);
            self.pending_analysis_request = Some(PendingAnalysisRequest {
                role: EngineRole::Analysis,
                role_generation,
                node_id: snapshot.current_node_id.clone(),
            });
            self.on_engine_connect(EngineRole::Analysis, cx);
            self.show_toast("正在连接 KataGo；连接完成后将开始全盘复盘".to_owned(), cx);
            return;
        }

        let first_node = self
            .batch_review_state
            .as_ref()
            .and_then(|state| state.node_ids.first())
            .cloned();
        if let Some(first_node) = first_node
            && snapshot.current_node_id != first_node
        {
            self.navigate_to_node_with_batch_policy(first_node, false, cx);
        }
        self.start_analysis(cx);
        cx.notify();
    }

    fn stop_whole_game_review(&mut self, cx: &mut Context<Self>) {
        self.cancel_whole_game_review(true, cx);
    }

    fn cancel_whole_game_review(&mut self, restore_original: bool, cx: &mut Context<Self>) {
        let original_node = restore_original.then(|| {
            self.batch_review_state
                .as_ref()
                .map(|state| state.original_node_id.clone())
        });
        if self.analysis_task.is_some() {
            self.analysis_run.cancel_and_dispose();
        }
        self.pending_analysis_request = None;
        self.batch_review_state = None;
        self.batch_review_progress = None;
        self.batch_review_profile = None;
        if let Some(Some(original_node)) = original_node
            && self.host.snapshot().current_node_id != original_node
        {
            self.navigate_to_node_with_batch_policy(original_node, false, cx);
        }
        self.show_toast("全盘复盘已停止".to_owned(), cx);
        cx.notify();
    }

    /// Generates a move from the engine configured for the current turn (Play vs AI).
    fn generate_engine_move(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.host.snapshot();
        let color = snapshot.board.next_player;
        let role = match color {
            Color::Black => EngineRole::Black,
            Color::White => EngineRole::White,
        };
        let target_role = if self.engine_controller.is_attached(role) {
            role
        } else if self.engine_controller.is_attached(EngineRole::Analysis) {
            EngineRole::Analysis
        } else {
            role
        };
        self.trigger_engine_genmove(target_role, color, cx);
    }

    /// Asks the engine attached to the specified role for a move without
    /// blocking GPUI while KataGo searches.
    fn trigger_engine_genmove(&mut self, role: EngineRole, color: Color, cx: &mut Context<Self>) {
        self.advance_clock(Instant::now(), cx);
        if self.clock.state().expired.is_some() {
            return;
        }
        if self.engine_command_tasks.contains_key(&role)
            || self.engine_controller.is_streaming(role)
        {
            self.status = format!("{} engine is busy", role.label()).into();
            cx.notify();
            return;
        }
        let session = match self.engine_controller.lease_for_command(role) {
            Ok(session) => session,
            Err(_) => {
                let name = self.engine_roles.get(role).unwrap_or_default();
                self.status = format!(
                    "attach selected {} engine {name} before generating a move",
                    role.label()
                )
                .into();
                cx.notify();
                return;
            }
        };
        let color_str = match color {
            Color::Black => "B",
            Color::White => "W",
        }
        .to_owned();
        self.status = format!("{} engine is thinking…", role.label()).into();
        let clock_state = self.clock.state();
        let command_generation = self
            .engine_generations
            .get(&role)
            .copied()
            .unwrap_or_default();
        let weak = cx.entity().downgrade();
        let engine_color = color_str.clone();
        self.engine_command_tasks.insert(
            role,
            cx.spawn(
                move |_: gpui::WeakEntity<ShellApp>, cx: &mut gpui::AsyncApp| {
                    let mut cx = cx.clone();
                    async move {
                        let (session, result) = cx
                            .background_executor()
                            .spawn(async move {
                                let mut session = session;
                                let result = session
                                    .sync_clock_state(clock_state)
                                    .map_err(|error| error.to_string())
                                    .and_then(|()| {
                                        session
                                            .generate_move(&engine_color)
                                            .map_err(|error| error.to_string())
                                    });
                                (session, result)
                            })
                            .await;
                        let _ = weak.update(&mut cx, |shell, cx| {
                            shell.engine_command_tasks.remove(&role);
                            if shell
                                .engine_generations
                                .get(&role)
                                .copied()
                                .unwrap_or_default()
                                != command_generation
                            {
                                shell.engine_controller.discard_command_lease(role);
                                cx.background_executor()
                                    .spawn(async move {
                                        let mut session = session;
                                        let _ = session.stop();
                                    })
                                    .detach();
                                return;
                            }
                            match result {
                                Ok(response) if response.success => {
                                    let response_text = response.content.trim().to_owned();
                                    shell.engine_controller.return_command_lease(role, session);
                                    shell.record_engine_log(entry_for_response(
                                        format!("{}: genmove {}", role.label(), color_str),
                                        &response,
                                    ));
                                    let board_size = shell.host.snapshot().board.width;
                                    let vertex = parse_gtp_vertex(board_size, &response_text)
                                        .map(|(column, row)| Vertex { column, row });
                                    shell.advance_clock(Instant::now(), cx);
                                    if shell.clock.state().expired.is_some() {
                                        return;
                                    }
                                    let mut events = RecordingSink;
                                    match shell.host.play_move(color, vertex, &mut events) {
                                        Ok(_) => {
                                            if !shell.commit_clock_move(color, cx) {
                                                return;
                                            }
                                            shell.last_vertex = vertex;
                                            shell.status = format!(
                                                "{} AI played {response_text}",
                                                role.label()
                                            )
                                            .into();
                                            shell.synchronize_recovery();
                                            shell.play_sound_if_enabled(if vertex.is_some() {
                                                SoundCue::StonePlaced
                                            } else {
                                                SoundCue::Pass
                                            });
                                            shell.sync_engine_position(
                                                Some(role),
                                                color,
                                                vertex,
                                                cx,
                                            );
                                            shell.maybe_background_review_current_position(cx);
                                            shell.request_configured_engine_turn(cx);
                                        }
                                        Err(error) => {
                                            shell.status =
                                                format!("engine move rejected: {error}").into()
                                        }
                                    }
                                }
                                Ok(response) => {
                                    shell.engine_controller.return_command_lease(role, session);
                                    shell.record_engine_log(entry_for_response(
                                        format!("{}: genmove {}", role.label(), color_str),
                                        &response,
                                    ));
                                    shell.status = format!(
                                        "{} engine genmove failed: {}",
                                        role.label(),
                                        response.content
                                    )
                                    .into();
                                }
                                Err(error) => {
                                    shell.engine_controller.discard_command_lease(role);
                                    shell.status =
                                        format!("{} engine genmove failed: {error}", role.label())
                                            .into();
                                }
                            }
                            cx.notify();
                        });
                    }
                },
            ),
        );
        cx.notify();
    }

    #[allow(dead_code)]
    fn on_engine_move(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.generate_engine_move(cx);
    }

    /// Applies an installed theme package: swaps the active tokens and
    /// records the choice as `theme:<id>` under `theme.current`.
    #[allow(dead_code)]
    fn on_installed_theme_selected(&mut self, theme_id: &str, cx: &mut Context<Self>) {
        let Some(theme) = self
            .installed_themes
            .iter()
            .find(|theme| theme.manifest.id == theme_id)
        else {
            self.status = format!("theme {theme_id} is not installed").into();
            cx.notify();
            return;
        };
        self.theme = theme.tokens.clone();
        self.palette = ui_palette(&self.theme);
        match self.settings.set(
            "theme.current",
            serde_json::json!(format!("theme:{theme_id}")),
        ) {
            Ok(_) => match ryusei_host::persist_settings_store(
                &self.settings,
                &mut self.settings_persistence,
            ) {
                Ok(()) => self.status = format!("applied theme {theme_id}").into(),
                Err(error) => self.status = format!("theme not persisted: {error}").into(),
            },
            Err(error) => {
                self.status = format!("theme not accepted: {error}").into();
            }
        }
        cx.notify();
    }

    /// Re-scans the themes root and refreshes the installed-theme list.
    #[allow(dead_code)]
    fn refresh_installed_themes(&mut self) {
        match file_workflow::theme_root() {
            Ok(theme_root) => match ryusei_host::scan_theme_root(&theme_root) {
                Ok(scan) => {
                    self.installed_themes = scan.themes;
                    self.legacy_asar_themes = scan.legacy_asar;
                }
                Err(error) => self.status = format!("theme scan failed: {error}").into(),
            },
            Err(error) => self.status = format!("theme directory unavailable: {error}").into(),
        }
    }

    /// Picks a theme package directory with a native dialog, validates and
    /// installs it into the themes root, then refreshes the panel.
    #[allow(dead_code)]
    fn on_theme_install(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.dialog_service.pick_open_path() else {
            self.status = "theme install cancelled".into();
            cx.notify();
            return;
        };
        if !path.is_dir() {
            self.status = format!("{} is not a theme package directory", path.display()).into();
            cx.notify();
            return;
        }
        let theme_root = match file_workflow::theme_root() {
            Ok(root) => root,
            Err(error) => {
                self.status = format!("theme directory unavailable: {error}").into();
                cx.notify();
                return;
            }
        };
        match ryusei_host::install_theme(&path, &theme_root) {
            Ok(theme) => {
                self.refresh_installed_themes();
                self.status = format!(
                    "installed theme {} v{}",
                    theme.manifest.name, theme.manifest.version
                )
                .into();
            }
            Err(error) => self.status = format!("theme install failed: {error}").into(),
        }
        cx.notify();
    }

    /// Removes an installed theme by id.
    #[allow(dead_code)]
    fn on_theme_uninstall(&mut self, theme_id: &str, cx: &mut Context<Self>) {
        let theme_root = match file_workflow::theme_root() {
            Ok(root) => root,
            Err(error) => {
                self.status = format!("theme directory unavailable: {error}").into();
                cx.notify();
                return;
            }
        };
        match ryusei_host::uninstall_theme(&theme_root, theme_id) {
            Ok(()) => {
                self.refresh_installed_themes();
                self.status = format!("uninstalled theme {theme_id}").into();
            }
            Err(error) => self.status = format!("theme uninstall failed: {error}").into(),
        }
        cx.notify();
    }

    pub(crate) fn append_comment_tag(&mut self, tag: &str, cx: &mut Context<Self>) {
        let current = self.text_inputs.comment_input.text().to_owned();
        let updated = if current.is_empty() {
            tag.to_owned()
        } else {
            format!("{current} {tag}")
        };
        self.text_inputs.comment_input.set_text(&updated);
        self.save_comment(&updated, cx);
    }

    fn toggle_comment_preview(&mut self, cx: &mut Context<Self>) {
        self.comment_preview = !self.comment_preview;
        cx.notify();
    }

    /// Selects a theme, swaps the active tokens and persists the choice under
    /// the `theme.current` setting key through the host settings workflow.
    /// Aligns the gpui-component theme (Button/Badge/Checkbox colors) with the
    /// active shell palette's luminance, so component controls stay legible on
    /// both light and dark board themes instead of being forced dark.
    fn sync_component_theme(&self, cx: &mut Context<Self>) {
        let mode = if self.palette.is_dark() {
            gpui_component::ThemeMode::Dark
        } else {
            gpui_component::ThemeMode::Light
        };
        gpui_component::Theme::change(mode, None, cx);
    }

    pub(crate) fn on_theme_selected(&mut self, choice: ThemeChoice, cx: &mut Context<Self>) {
        self.theme_choice = choice;
        self.theme = choice.tokens();
        self.palette = ui_palette(&self.theme);
        self.sync_component_theme(cx);
        match self
            .settings
            .set("theme.current", serde_json::json!(choice.setting_value()))
        {
            Ok(_) => match ryusei_host::persist_settings_store(
                &self.settings,
                &mut self.settings_persistence,
            ) {
                Ok(()) => {}
                Err(error) => {
                    self.status = format!("theme not persisted: {error}").into();
                }
            },
            Err(error) => {
                self.status = format!("theme not accepted: {error}").into();
            }
        }
        self.status = format!("theme: {}", choice.label()).into();
        cx.notify();
    }

    /// Records the current window size and maximized state under the
    /// `window.width`/`window.height`/`window.maximized` settings and persists
    /// the store, so the next launch restores them.
    fn remember_window_bounds(&mut self, width: f64, height: f64, maximized: bool) {
        let mut accepted = true;
        if let Err(error) = self.settings.set("window.width", serde_json::json!(width)) {
            accepted = false;
            self.status = format!("window size not saved: {error}").into();
        }
        if let Err(error) = self
            .settings
            .set("window.height", serde_json::json!(height))
        {
            accepted = false;
            self.status = format!("window size not saved: {error}").into();
        }
        if let Err(error) = self
            .settings
            .set("window.maximized", serde_json::json!(maximized))
        {
            accepted = false;
            self.status = format!("window maximized state not saved: {error}").into();
        }
        if !accepted {
            return;
        }
        if let Err(error) =
            ryusei_host::persist_settings_store(&self.settings, &mut self.settings_persistence)
        {
            self.status = format!("window size not persisted: {error}").into();
        }
    }

    /// Installs a plugin from a user-selected `.zip` archive.
    fn install_plugin_zip(&mut self, cx: &mut Context<Self>) {
        let Some(zip_path) = self.dialog_service.pick_open_zip_path() else {
            return;
        };
        let install_root = match file_workflow::plugin_install_root() {
            Ok(root) => root,
            Err(e) => {
                self.status = format!("failed to get plugin root: {e}").into();
                cx.notify();
                return;
            }
        };
        match self.plugin_controller.install_zip(&zip_path, &install_root) {
            Ok(outcome) => {
                self.status = outcome.message.clone().into();
                self.show_toast(outcome.message, cx);
                self.installed_plugins = self
                    .plugin_controller
                    .records()
                    .iter()
                    .map(entry_from_record)
                    .collect();
            }
            Err(error) => {
                self.status = format!("plugin installation failed: {error}").into();
                self.show_toast(format!("插件安装失败: {error}"), cx);
            }
        }
        self.synchronize_recovery();
        cx.notify();
    }

    #[allow(dead_code)]
    fn on_install_plugin_zip(
        &mut self,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.install_plugin_zip(cx);
    }

    /// Delegates enablement, persistence and native-process lifecycle to the
    /// host plugin Module, then refreshes the UI projection.
    #[allow(dead_code)]
    fn on_plugin_toggle(&mut self, plugin_id: &str) {
        match self.plugin_controller.toggle(plugin_id) {
            Ok(outcome) => self.status = outcome.message.into(),
            Err(error) => self.status = format!("plugin toggle failed: {error}").into(),
        }
        self.refresh_plugin_processes();
    }

    /// Grants the manifest permissions and enables the plugin through the
    /// controller's single persisted lifecycle operation.
    #[allow(dead_code)]
    fn on_plugin_grant(&mut self, plugin_id: &str, cx: &mut Context<Self>) {
        match self.plugin_controller.grant_and_enable(plugin_id) {
            Ok(outcome) => self.status = outcome.message.into(),
            Err(error) => self.status = format!("permission grant failed: {error}").into(),
        }
        self.refresh_plugin_processes();
        cx.notify();
    }

    /// Refreshes the panel projection from the host-owned process snapshots.
    fn refresh_plugin_processes(&mut self) {
        let process_infos = self.plugin_controller.process_infos();
        self.installed_plugins = self
            .plugin_controller
            .records()
            .iter()
            .map(entry_from_record)
            .collect();
        for entry in &mut self.installed_plugins {
            if let Some(info) = process_infos
                .iter()
                .find(|info| info.plugin_id == entry.plugin_id)
            {
                apply_process_info(entry, info);
            }
        }
    }

    /// Authorizes native execution for a native plugin after an explicit
    /// confirmation, then grants its permissions and enables it.
    #[allow(dead_code)]
    fn on_plugin_authorize(&mut self, plugin_id: &str, cx: &mut Context<Self>) {
        let choice = rfd::MessageDialog::new()
            .set_title("Authorize Native Plugin")
            .set_description(
                "This plugin runs native code with your user permissions. \
                 Only authorize plugins you trust. Authorize execution?",
            )
            .set_buttons(rfd::MessageButtons::OkCancel)
            .show();
        if choice != rfd::MessageDialogResult::Ok {
            self.status = "native authorization cancelled".into();
            cx.notify();
            return;
        }
        match self.plugin_controller.authorize_and_enable(plugin_id) {
            Ok(outcome) => self.status = outcome.message.into(),
            Err(error) => self.status = format!("native authorization failed: {error}").into(),
        }
        self.refresh_plugin_processes();
        cx.notify();
    }

    /// Dispatches a plugin command. WASM plugins are invoked in-process
    /// through the sandboxed runtime; declarative plugins have no execution
    /// body yet and are recorded in the status bar.
    /// Dispatches a native command through the host controller's supervised
    /// process lifecycle and restart policy.
    fn dispatch_native_plugin_command(&mut self, plugin_id: &str, command_id: &str) {
        match self
            .plugin_controller
            .dispatch_native(plugin_id, command_id)
        {
            Ok(outcome) => self.status = outcome.message.into(),
            Err(error) => {
                self.status = format!("plugin {plugin_id} command failed: {error}").into()
            }
        }
        self.refresh_plugin_processes();
    }

    /// Delegates the model transfer to the host task module. The UI owns only
    /// user feedback; temporary files, validation, and replacement semantics
    /// stay behind the KataGo resource seam.
    fn download_katago_model(
        &mut self,
        base_dir: &Path,
        tier: ryusei_host::KataGoModelTier,
        starting_message: &'static str,
        _success_message: &'static str,
        cx: &mut Context<Self>,
    ) {
        self.show_toast(starting_message, cx);
        let base_dir = base_dir.to_path_buf();
        let weak = cx.entity().downgrade();
        cx.spawn(
            move |_: gpui::WeakEntity<ShellApp>, cx: &mut gpui::AsyncApp| {
                let mut cx = cx.clone();
                let base_dir_for_task = base_dir.clone();
                async move {
                    let base_dir_for_thread = base_dir_for_task.clone();
                    let result = cx
                        .background_executor()
                        .spawn(async move {
                            if tier == ryusei_host::KataGoModelTier::Balanced {
                                ryusei_host::install_latest_katago_weight(&base_dir_for_thread)
                            } else {
                                ryusei_host::install_katago_model(&base_dir_for_thread, tier)
                                    .map_err(|error| error.to_string())
                            }
                        })
                        .await;
                    weak.update(&mut cx, |shell, cx| match result {
                        Ok(path) => {
                            let mut executable_exists = false;
                            let activation = ryusei_host::set_active_katago_model(
                                &base_dir_for_task,
                                &path,
                            );
                            if activation.is_ok()
                                && let Ok(env) = ryusei_host::ensure_katago_environment(
                                    &base_dir_for_task,
                                    tier,
                                    None,
                                )
                            {
                                executable_exists = env.executable_exists;
                                let engine_name = env.engine_record.name.clone();
                                shell.engine_store.upsert(env.engine_record.clone());
                                shell
                                    .engine_roles
                                    .assign(EngineRole::Analysis, &engine_name);
                                shell.engine_roles.assign(EngineRole::White, &engine_name);
                                let _ = shell.engine_store.save(&mut shell.settings);
                                let _ = shell.persist_engine_roles();
                                let _ = ryusei_host::persist_settings_store(
                                    &shell.settings,
                                    &mut shell.settings_persistence,
                                );
                                if env.executable_exists {
                                    shell.on_engine_connect(EngineRole::Analysis, cx);
                                }
                            }
                            let ready_message = if let Err(error) = activation {
                                format!("⚠️ 权重已下载但统一模型链接创建失败: {error}")
                            } else if executable_exists {
                                format!("KataGo model installed and ready: {}", path.display())
                            } else {
                                format!(
                                    "KataGo model downloaded: {}. Install/configure the KataGo executable before analysis.",
                                    path.display()
                                )
                            };
                            shell.status = ready_message.clone().into();
                            shell.show_toast(ready_message, cx);
                        }
                        Err(error) => {
                            let message = format!("⚠️ 权重模型下载失败: {error}");
                            shell.status = message.clone().into();
                            shell.show_toast(message, cx);
                        }
                    })
                    .ok();
                }
            },
        )
        .detach();
    }

    /// The writable directory engine subprocesses run in. KataGo's generated
    /// config writes a relative `logDir`; running from a packaged app's
    /// read-only cwd makes it abort during startup (surfaced as a handshake
    /// failure). The engine runtime dir lives under the user config directory,
    /// which is always writable.
    fn engine_runtime_directory(&mut self) -> std::path::PathBuf {
        let base = crate::file_workflow::current_user_config_directory()
            .unwrap_or_else(|_| std::env::temp_dir());
        let runtime = base.join("engines").join("katago").join("runtime");
        std::fs::create_dir_all(&runtime).ok();
        runtime
    }

    /// Shows a prominent transient toast notification, auto-clearing after a
    /// short delay so plugin commands produce visible feedback.
    fn show_toast(&mut self, message: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.toast = Some(message.into());
        cx.notify();
        cx.spawn(
            move |weak: gpui::WeakEntity<ShellApp>, cx: &mut gpui::AsyncApp| {
                let mut cx = cx.clone();
                async move {
                    cx.background_executor()
                        .timer(std::time::Duration::from_secs(3))
                        .await;
                    weak.update(&mut cx, |shell, cx| {
                        shell.toast = None;
                        cx.notify();
                    })
                    .ok();
                }
            },
        )
        .detach();
    }

    fn refresh_katago_panel(&mut self, cx: &mut Context<Self>) {
        if self.katago_panel_task.is_some() {
            return;
        }
        let Some(base) = crate::file_workflow::current_user_config_directory().ok() else {
            self.katago_panel_status = "无法确定 KataGo 配置目录".into();
            cx.notify();
            return;
        };
        self.katago_panel_status = "正在读取本机与官网 KataGo 信息…".into();
        let weak = cx.entity().downgrade();
        self.katago_panel_task = Some(cx.spawn(
            move |_: gpui::WeakEntity<ShellApp>, cx: &mut gpui::AsyncApp| {
                let mut cx = cx.clone();
                async move {
                    let result = cx
                        .background_executor()
                        .spawn(async move {
                            let local = ryusei_host::inspect_katago_local(&base)
                                .map_err(|error| error.to_string())?;
                            let mut release = ryusei_host::fetch_katago_latest_release()?;
                            let official_weights = ryusei_host::fetch_katago_official_weights()?;
                            // KataGo Training publishes newest first. Put this
                            // catalog before GitHub release assets so the first
                            // five model choices really are the newest weights.
                            release.assets.splice(0..0, official_weights);
                            // The main catalog remains useful even when the
                            // optional special-model catalog is temporarily unavailable.
                            if let Ok(human_sl_assets) =
                                ryusei_host::fetch_katago_human_sl_weights()
                            {
                                release.assets.extend(human_sl_assets);
                            }
                            let weights = ryusei_host::merge_katago_weight_catalog_with_limit(
                                &local,
                                &release,
                                ryusei_host::KATAGO_LATEST_WEIGHT_DISPLAY_LIMIT,
                            );
                            Ok::<_, String>((local, release, weights))
                        })
                        .await;
                    let _ = weak.update(&mut cx, |shell, cx| {
                        shell.katago_panel_task = None;
                        match result {
                            Ok((local, release, weights)) => {
                                shell.katago_local = Some(local);
                                shell.katago_release = Some(release);
                                shell.katago_weights = weights;
                                shell.katago_panel_status = "本机与官网信息已更新".into();
                            }
                            Err(error) => {
                                shell.katago_panel_status =
                                    format!("官网信息读取失败: {error}").into();
                            }
                        }
                        cx.notify();
                    });
                }
            },
        ));
        cx.notify();
    }

    pub fn set_human_sl_profile(&mut self, profile: &str, cx: &mut Context<Self>) {
        if !ryusei_host::is_valid_human_sl_profile(profile) {
            self.show_toast(format!("不支持的 HumanSL 档位: {profile}"), cx);
            return;
        }
        if let Err(error) = self
            .settings
            .set("katago.human_sl_profile", serde_json::json!(profile))
            .and_then(|_| {
                ryusei_host::persist_settings_store(&self.settings, &mut self.settings_persistence)
                    .map_err(|message| ryusei_host::SettingValidationError {
                        key: "katago.human_sl_profile".to_owned(),
                        expected: "a persistable HumanSL profile".to_owned(),
                        found: message,
                    })
            })
        {
            self.show_toast(format!("保存 HumanSL 档位失败: {error}"), cx);
            return;
        }
        self.katago_panel_status =
            format!("HumanSL 档位已选择为 {profile}；重新启用 HumanSL 权重后生效").into();
        self.show_toast(self.katago_panel_status.clone(), cx);
        cx.notify();
    }

    /// Installs the available HumanSL model as the White-side engine when the
    /// default is enabled. Existing White assignments are respected.
    fn enable_default_human_sl(
        &mut self,
        base: &Path,
        normal_model: &Path,
    ) -> Result<Option<String>, String> {
        if !self
            .settings
            .get_bool("katago.human_sl_enabled")
            .unwrap_or(true)
        {
            return Ok(None);
        }
        let Some(human_model) = ryusei_host::find_installed_human_sl_model(base) else {
            return Ok(None);
        };
        let profile = self
            .settings
            .get_str("katago.human_sl_profile")
            .unwrap_or("rank_5k")
            .to_owned();
        let record = ryusei_host::prepare_katago_human_sl_engine(
            base,
            normal_model,
            &human_model,
            &profile,
        )?;
        let engine_name = record.name.clone();
        self.engine_store.upsert(record);
        let replace_default_white = self.engine_roles.get(EngineRole::White).is_none_or(|name| {
            let name = name.to_ascii_lowercase();
            name.contains("katago") && !name.contains("humansl")
        });
        if replace_default_white {
            self.engine_roles.assign(EngineRole::White, &engine_name);
        }
        let _ = self.engine_store.save(&mut self.settings);
        let _ = self.persist_engine_roles();
        let _ = ryusei_host::persist_settings_store(&self.settings, &mut self.settings_persistence);
        Ok(Some(engine_name))
    }

    fn activate_katago_weight(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(base) = crate::file_workflow::current_user_config_directory().ok() else {
            return;
        };
        let model = ryusei_host::katago_storage_dir(&base)
            .join("models")
            .join(name);

        if ryusei_host::is_human_sl_weight_name(name) {
            let human_profile = self
                .settings
                .get_str("katago.human_sl_profile")
                .unwrap_or("rank_5k")
                .to_owned();
            let result = ryusei_host::find_installed_normal_katago_model(&base)
                .ok_or_else(|| "请先下载一个普通 KataGo 权重；HumanSL 需要普通 -model 与 HumanSL -human-model 成对启动".to_owned())
                .and_then(|normal_model| {
                    ryusei_host::prepare_katago_human_sl_engine(
                        &base,
                        &normal_model,
                        &model,
                        &human_profile,
                    )
                });
            match result {
                Ok(record) => {
                    let engine_name = record.name.clone();
                    self.engine_store.upsert(record);
                    self.engine_roles.assign(EngineRole::White, &engine_name);
                    let _ = self.engine_store.save(&mut self.settings);
                    let _ = self.persist_engine_roles();
                    let _ = ryusei_host::persist_settings_store(
                        &self.settings,
                        &mut self.settings_persistence,
                    );
                    self.katago_panel_status = format!(
                        "HumanSL 已配置为白方引擎（{human_profile}）：{name}；未替换普通分析权重"
                    )
                    .into();
                }
                Err(error) => {
                    self.katago_panel_status = format!("HumanSL 配置失败: {error}").into();
                    self.show_toast(self.katago_panel_status.clone(), cx);
                }
            }
            cx.notify();
            return;
        }

        match ryusei_host::set_active_katago_model(&base, &model) {
            Ok(_) => {
                if let Ok(environment) = ryusei_host::ensure_katago_environment(
                    &base,
                    ryusei_host::KataGoModelTier::Balanced,
                    None,
                ) {
                    self.engine_store.upsert(environment.engine_record);
                    let _ = self.engine_store.save(&mut self.settings);
                    let _ = ryusei_host::persist_settings_store(
                        &self.settings,
                        &mut self.settings_persistence,
                    );
                }
                self.katago_local = ryusei_host::inspect_katago_local(&base).ok();
                self.katago_weights = self
                    .katago_local
                    .as_ref()
                    .map(|local| local.weights.clone())
                    .unwrap_or_default();
                self.katago_panel_status = format!("已切换当前标准权重: {name}").into();
                cx.notify();
            }
            Err(error) => {
                self.katago_panel_status = format!("切换权重失败: {error}").into();
                self.show_toast(self.katago_panel_status.clone(), cx);
                cx.notify();
            }
        }
    }

    fn download_katago_weight_asset(&mut self, name: &str, cx: &mut Context<Self>) {
        let name = name.to_owned();
        let Some(release) = self.katago_release.clone() else {
            self.katago_panel_status = "请先刷新官网权重列表".into();
            cx.notify();
            return;
        };
        let Some(asset) = release.assets.into_iter().find(|asset| asset.name == name) else {
            self.katago_panel_status = format!("官网未找到权重: {name}").into();
            cx.notify();
            return;
        };
        let Some(base) = crate::file_workflow::current_user_config_directory().ok() else {
            return;
        };
        self.katago_panel_status = format!("正在下载权重: {name}").into();
        let weak = cx.entity().downgrade();
        let worker_base = base.clone();
        cx.spawn(
            move |_: gpui::WeakEntity<ShellApp>, cx: &mut gpui::AsyncApp| {
                let mut cx = cx.clone();
                async move {
                    let result =
                        cx.background_executor()
                            .spawn(async move {
                                ryusei_host::download_katago_weight(&worker_base, &asset)
                            })
                            .await;
                    let _ = weak.update(&mut cx, |shell, cx| {
                        match result {
                            Ok(path) => {
                                let human_sl = ryusei_host::is_human_sl_weight_name(&name);
                                if !human_sl {
                                    // A newly downloaded official network becomes
                                    // the default immediately through the stable link.
                                    let _ = ryusei_host::set_active_katago_model(&base, &path);
                                }
                                shell.katago_local = ryusei_host::inspect_katago_local(&base).ok();
                                if !human_sl {
                                    if let Some(normal_model) =
                                        ryusei_host::find_latest_installed_normal_katago_model(
                                            &base,
                                        )
                                    {
                                        let _ = shell.enable_default_human_sl(&base, &normal_model);
                                    }
                                } else if let Some(normal_model) =
                                    ryusei_host::find_latest_installed_normal_katago_model(&base)
                                {
                                    let _ = shell.enable_default_human_sl(&base, &normal_model);
                                }
                                shell.katago_weights = shell
                                    .katago_local
                                    .as_ref()
                                    .map(|local| {
                                        shell
                                            .katago_release
                                            .as_ref()
                                            .map(|release| {
                                                ryusei_host::merge_katago_weight_catalog_with_limit(
                                                    local,
                                                    release,
                                                    ryusei_host::KATAGO_LATEST_WEIGHT_DISPLAY_LIMIT,
                                                )
                                            })
                                            .unwrap_or_else(|| local.weights.clone())
                                    })
                                    .unwrap_or_default();
                                shell.katago_panel_status = format!("权重下载完成: {name}").into();
                            }
                            Err(error) => {
                                shell.katago_panel_status = format!("权重下载失败: {error}").into()
                            }
                        }
                        cx.notify();
                    });
                }
            },
        )
        .detach();
        cx.notify();
    }

    fn update_katago_binary_from_panel(&mut self, cx: &mut Context<Self>) {
        let Some(release) = self.katago_release.clone() else {
            self.katago_panel_status = "请先刷新官网版本信息".into();
            cx.notify();
            return;
        };
        let Some(base) = crate::file_workflow::current_user_config_directory().ok() else {
            return;
        };
        self.katago_panel_status = "正在下载并安装官网最新 KataGo…".into();
        let weak = cx.entity().downgrade();
        let worker_base = base.clone();
        cx.spawn(
            move |_: gpui::WeakEntity<ShellApp>, cx: &mut gpui::AsyncApp| {
                let mut cx = cx.clone();
                async move {
                    let result =
                        cx.background_executor()
                            .spawn(async move {
                                ryusei_host::update_katago_binary(&worker_base, &release)
                            })
                            .await;
                    let _ = weak.update(&mut cx, |shell, cx| {
                        match result {
                            Ok(path) => {
                                if let Some(record) = shell
                                    .engine_store
                                    .list()
                                    .iter()
                                    .find(|record| {
                                        record.name.to_ascii_lowercase().contains("katago")
                                    })
                                    .cloned()
                                {
                                    let mut updated = record;
                                    updated.path = path.display().to_string();
                                    shell.engine_store.upsert(updated);
                                    let _ = shell.engine_store.save(&mut shell.settings);
                                    let _ = ryusei_host::persist_settings_store(
                                        &shell.settings,
                                        &mut shell.settings_persistence,
                                    );
                                }
                                shell.katago_local = ryusei_host::inspect_katago_local(&base).ok();
                                shell.katago_panel_status =
                                    format!("KataGo 更新完成: {}", path.display()).into();
                            }
                            Err(error) => {
                                shell.katago_panel_status =
                                    format!("KataGo 更新失败: {error}").into()
                            }
                        }
                        cx.notify();
                    });
                }
            },
        )
        .detach();
        cx.notify();
    }

    fn on_plugin_command(&mut self, plugin_id: &str, command_id: &str, cx: &mut Context<Self>) {
        let builtin = ryusei_host::BuiltinPluginCommandRegistry::resolve(plugin_id, command_id);
        if builtin.is_some_and(|command| command.is_katago()) {
            let backend = ryusei_host::HardwareBackend::detect_current_platform();
            let base_dir = match file_workflow::plugin_install_root() {
                Ok(root) => root,
                Err(_) => std::env::temp_dir(),
            };

            match builtin.expect("KataGo command was classified") {
                ryusei_host::BuiltinPluginCommand::KataGoRefresh => {
                    self.refresh_katago_panel(cx);
                }
                ryusei_host::BuiltinPluginCommand::KataGoUpdateBinary => {
                    self.update_katago_binary_from_panel(cx);
                }
                ryusei_host::BuiltinPluginCommand::KataGoSetup => {
                    let latest_model =
                        ryusei_host::find_latest_installed_normal_katago_model(&base_dir);
                    match ryusei_host::ensure_katago_environment(
                        &base_dir,
                        ryusei_host::KataGoModelTier::Balanced,
                        latest_model.as_deref(),
                    ) {
                        Ok(env) => {
                            let engine_name = env.engine_record.name.clone();
                            self.engine_store.upsert(env.engine_record.clone());
                            if self
                                .settings
                                .get_bool("katago.human_sl_enabled")
                                .unwrap_or(true)
                                && let Some(normal_model) =
                                    ryusei_host::find_latest_installed_normal_katago_model(
                                        &base_dir,
                                    )
                            {
                                let _ = self.enable_default_human_sl(&base_dir, &normal_model);
                            }
                            if self.engine_roles.get(EngineRole::Analysis).is_none() {
                                self.engine_roles.assign(EngineRole::Analysis, &engine_name);
                            }
                            if self.engine_roles.get(EngineRole::White).is_none() {
                                self.engine_roles.assign(EngineRole::White, &engine_name);
                            }
                            let _ = self.engine_store.save(&mut self.settings);
                            let _ = self.persist_engine_roles();
                            let _ = ryusei_host::persist_settings_store(
                                &self.settings,
                                &mut self.settings_persistence,
                            );

                            let msg = if env.executable_exists && env.model_exists {
                                self.on_engine_connect(EngineRole::Analysis, cx);
                                format!(
                                    "⚡ KataGo 引擎已成功配置并连接 ({})！\n已开始实时流式分析",
                                    backend.label()
                                )
                            } else if env.executable_exists {
                                format!(
                                    "⚡ KataGo 已配置 ({})！尚未下载模型，请点击【⭐ 下载最新官方权重】即可开始分析",
                                    backend.label()
                                )
                            } else {
                                "⚡ KataGo 配置已写入！\n提示: 未检测到 katago，请确保已安装 (macOS: brew install katago)".to_string()
                            };
                            self.status = msg.clone().into();
                            self.show_toast(msg, cx);
                        }
                        Err(err) => {
                            let msg = format!("KataGo 配置失败: {err}");
                            self.status = msg.clone().into();
                            self.show_toast(msg, cx);
                        }
                    }
                }
                ryusei_host::BuiltinPluginCommand::KataGoDownload(
                    ryusei_host::KataGoModelTier::Balanced,
                ) => self.download_katago_model(
                    &base_dir,
                    ryusei_host::KataGoModelTier::Balanced,
                    "⭐ 开始下载 10B 推荐模型 (94MB)...",
                    "⭐ 10B 推荐权重模型下载成功并就绪！",
                    cx,
                ),
                ryusei_host::BuiltinPluginCommand::KataGoDownload(
                    ryusei_host::KataGoModelTier::Lightweight,
                ) => self.download_katago_model(
                    &base_dir,
                    ryusei_host::KataGoModelTier::Lightweight,
                    "⚡ 开始下载 38MB 轻量分析模型...",
                    "⚡ 38MB 轻量分析模型下载成功！",
                    cx,
                ),
                ryusei_host::BuiltinPluginCommand::KataGoDownload(
                    ryusei_host::KataGoModelTier::Strongest,
                ) => self.download_katago_model(
                    &base_dir,
                    ryusei_host::KataGoModelTier::Strongest,
                    "🏆 开始下载 240MB 最强模型...",
                    "🏆 240MB 专家模型下载成功！",
                    cx,
                ),
                _ => unreachable!("registry only classifies KataGo commands here"),
            }
            return;
        }

        if builtin == Some(ryusei_host::BuiltinPluginCommand::FoxFetchLatest) {
            // A command click is a convenience path only. It uses the visible
            // user query if present; otherwise it tells the user exactly how to
            // select a game instead of downloading an unrelated hard-coded SGF.
            self.fetch_fox_query(cx);
            return;
        }

        if builtin == Some(ryusei_host::BuiltinPluginCommand::PositionCheck) {
            let snap = self.host.snapshot();
            let black_stones = snap
                .board
                .sign_map
                .iter()
                .flat_map(|row| row.iter())
                .filter(|&&s| s == 1)
                .count();
            let white_stones = snap
                .board
                .sign_map
                .iter()
                .flat_map(|row| row.iter())
                .filter(|&&s| s == -1)
                .count();
            let move_desc = if snap.moves.is_empty() {
                "开局".to_owned()
            } else {
                format!("第 {} 手", snap.moves.len())
            };
            let msg = format!(
                "📊 局面检查完成: 黑子 {black_stones} 颗, 白子 {white_stones} 颗, 当前手序: {move_desc}"
            );
            self.status = msg.clone().into();
            self.show_toast(msg, cx);
            return;
        }

        if builtin == Some(ryusei_host::BuiltinPluginCommand::SgfExport) {
            self.save_as(cx);
            self.show_toast("💾 已打开 SGF 导出文件对话框", cx);
            return;
        }

        let Some(record) = self.plugin_controller.record(plugin_id).cloned() else {
            self.status = format!("plugin {plugin_id} is not installed").into();
            cx.notify();
            return;
        };
        if matches!(
            record.manifest.runtime,
            ryusei_plugin_runtime::PluginRuntime::Native
        ) {
            self.dispatch_native_plugin_command(plugin_id, command_id);
        } else if matches!(
            record.manifest.runtime,
            ryusei_plugin_runtime::PluginRuntime::Wasm
        ) {
            let snapshot_json =
                serde_json::to_string(&self.host.snapshot()).unwrap_or_else(|_| "{}".to_owned());
            match ryusei_host::load_wasm_module(&record)
                .and_then(|module| {
                    ryusei_host::invoke_wasm_command(
                        &record,
                        &module,
                        command_id,
                        serde_json::json!({}),
                        Some(&snapshot_json),
                    )
                })
                .map_err(ryusei_host::WasmWorkflowError::into_plugin_error)
            {
                Ok(result) => {
                    // Apply every transaction the plugin proposed through
                    // game.submitTransaction; the host validates each one
                    // (legal move, ko, occupied vertex, ...) before it lands
                    // in the document and undo history.
                    let mut applied = 0usize;
                    let mut rejected = 0usize;
                    for proposal in &result.proposed_transactions {
                        let Ok(transaction) = serde_json::from_value::<
                            ryusei_domain_core::GameTransaction,
                        >(proposal.clone()) else {
                            rejected += 1;
                            continue;
                        };
                        let mut events = RecordingSink;
                        match self.host.apply_transaction(transaction, &mut events) {
                            Ok(_) => applied += 1,
                            Err(_) => rejected += 1,
                        }
                    }
                    let mut status = format!(
                        "plugin {plugin_id} command {command_id} → {}",
                        result.response
                    );
                    if applied > 0 || rejected > 0 {
                        status.push_str(&format!(
                            " (transactions: {applied} applied, {rejected} rejected)"
                        ));
                    }
                    self.status = status.into();
                }
                Err(error) => {
                    self.status = format!("plugin {plugin_id} command failed: {error}").into();
                }
            }
        } else {
            self.status =
                format!("plugin {plugin_id} command {command_id} dispatched (declarative)").into();
        }
        cx.notify();
    }

    /// Resets the board to the given size as a fresh game using the persisted
    /// new-game defaults for komi and handicap.
    fn on_board_size_selected(&mut self, size: usize, cx: &mut Context<Self>) {
        self.new_game_at(size, cx);
    }

    /// Stops every role session and prevents a leased Analysis session from
    /// returning after its worker exits.
    fn disconnect_all_engine_sessions(&mut self) {
        for generation in self.engine_generations.values_mut() {
            *generation = generation.wrapping_add(1);
        }
        self.engine_connect_tasks.clear();
        self.engine_command_tasks.clear();
        self.pending_analysis_request = None;
        if self.analysis_task.is_some() {
            self.analysis_run.cancel_and_dispose();
        }
        self.engine_controller.detach_all();
        self.active_console_role = None;
        self.analysis.clear();
        self.analysis_best_move = None;
    }

    fn capture_active_workspace_tab(&mut self) {
        let snapshot = self.host.snapshot();
        self.workspace_tabs.capture_active(
            self.host.to_sgf(),
            workspace_tab_title(&snapshot),
            snapshot.file_state.path,
            self.host.source_encoding(),
            snapshot.file_state.is_dirty,
            self.external_file.tracked_fingerprint(),
            Some(snapshot.current_node_id.clone()),
            self.clock.state(),
            self.session_policy,
            self.mode,
            self.last_vertex,
            self.analysis_enabled,
            self.analysis.clone(),
            self.analysis_best_move,
        );
    }

    fn persist_workspace_tabs(&mut self) -> Result<(), String> {
        self.persistence
            .persist_workspace_tabs(&self.workspace_tabs)
    }

    fn activate_workspace_tab(&mut self, tab_id: &str, cx: &mut Context<Self>) {
        if tab_id == self.workspace_tabs.active_tab_id() {
            return;
        }
        self.capture_active_workspace_tab();
        let tab = match self.workspace_tabs.tab_snapshot(tab_id) {
            Ok(tab) => tab,
            Err(error) => {
                self.status = format!("could not activate session: {error}").into();
                cx.notify();
                return;
            }
        };
        let tracked_path = tab.source_path.clone();
        let tab_title = tab.title.clone();
        let mut events = RecordingSink;
        match self.host.restore_workspace_tab_with_state(
            &tab.sgf,
            tab.source_path.clone(),
            tab.source_encoding,
            tab.is_dirty,
            tab.current_node_id.as_deref(),
            &mut events,
        ) {
            Ok(snapshot) => {
                // Commit the selection only after the SGF snapshot has been
                // parsed successfully; a corrupt background tab must not
                // desynchronize the workspace selection from the active host.
                let _ = self.workspace_tabs.activate(tab_id);
                self.disconnect_all_engine_sessions();
                self.external_file.detach_file();
                self.clock.apply_remote_clock(tab.clock);
                self.clock_last_updated = Instant::now();
                self.session_policy = tab.policy;
                self.mode = tab.mode;
                self.analysis_enabled = tab.analysis_enabled;
                self.analysis = tab.analysis;
                self.analysis_best_move = tab.analysis_best_move;
                self.board_size = snapshot.board.width;
                self.last_vertex = tab.last_vertex.or(snapshot.board.current_vertex);
                match (tracked_path, tab.source_fingerprint) {
                    (Some(path), Some(fingerprint)) => self
                        .external_file
                        .track_file_with_fingerprint(std::path::PathBuf::from(path), fingerprint),
                    (Some(path), None) => {
                        // Legacy snapshots predate the persisted baseline. Read
                        // the actual source when possible rather than treating
                        // dirty in-memory SGF as the last on-disk contents.
                        let path = std::path::PathBuf::from(path);
                        let baseline =
                            ryusei_host::GameFileAccess::read_game_file(&self.file_access, &path)
                                .map(|decoded| decoded.content)
                                .unwrap_or_else(|_| tab.sgf.clone());
                        self.external_file.track_file(path, &baseline);
                    }
                    (None, _) => {}
                }
                self.status = format!("已切换到会话: {tab_title}").into();
            }
            Err(error) => {
                self.status = format!("could not restore session: {error}").into();
                cx.notify();
                return;
            }
        }
        if self.analysis_enabled {
            self.start_analysis(cx);
        }
        if let Err(error) = self.persist_workspace_tabs() {
            self.status = format!("session state not persisted: {error}").into();
        }
        cx.notify();
    }

    fn create_workspace_session(&mut self, cx: &mut Context<Self>) {
        self.capture_active_workspace_tab();
        let (size, properties) = default_new_game_properties(&self.settings);
        let mut new_host = match ryusei_host::HostApplication::new(size, size) {
            Ok(host) => host,
            Err(error) => {
                self.status = format!("could not create session: {error}").into();
                cx.notify();
                return;
            }
        };
        let mut events = RecordingSink;
        if let Err(error) =
            new_host.create_new_with_properties(size, size, &properties, &mut events)
        {
            self.status = format!("could not initialize session: {error}").into();
            cx.notify();
            return;
        }
        self.disconnect_all_engine_sessions();
        self.external_file.detach_file();
        self.host = new_host;
        self.clock = ClockController::new(TimeControl::None);
        self.clock_last_updated = Instant::now();
        self.session_policy = SessionPolicy::new(SessionMode::Match, SessionSource::Local);
        self.board_size = size;
        self.last_vertex = None;
        let tab = self.workspace_tabs.create_tab(
            self.host.to_sgf(),
            "Untitled Game",
            ryusei_host::SourceEncoding::Utf8,
        );
        self.status = format!("新建会话: {}", tab.title).into();
        if let Err(error) = self.persist_workspace_tabs() {
            self.status = format!("session state not persisted: {error}").into();
        }
        cx.notify();
    }

    pub fn close_workspace_tab(&mut self, tab_id: &str, cx: &mut Context<Self>) {
        if self.workspace_tabs.tabs().len() <= 1 {
            self.new_game(cx);
            return;
        }
        match self.workspace_tabs.close(tab_id) {
            Ok(Some(next_tab)) => {
                let mut events = RecordingSink;
                if let Err(e) = self.host.restore_workspace_tab_with_state(
                    &next_tab.sgf,
                    next_tab.source_path.clone(),
                    next_tab.source_encoding,
                    next_tab.is_dirty,
                    next_tab.current_node_id.as_deref(),
                    &mut events,
                ) {
                    self.status = format!("error restoring tab: {e}").into();
                } else {
                    self.session_policy = next_tab.policy;
                    self.mode = next_tab.mode;
                    self.last_vertex = next_tab.last_vertex;
                    self.analysis = next_tab.analysis;
                    self.analysis_best_move = next_tab.analysis_best_move;
                    self.analysis_enabled = next_tab.analysis_enabled;
                    self.clock.apply_remote_clock(next_tab.clock);
                }
            }
            Ok(None) => {}
            Err(e) => {
                self.status = format!("close tab error: {e}").into();
            }
        }
        let _ = self.persist_workspace_tabs();
        cx.notify();
    }

    /// Applies a settings edit through the validated store and persists it.
    /// A persistence failure rolls the store back so the UI never shows a
    /// value that is not on disk.
    #[allow(dead_code)]
    fn apply_settings_edit(&mut self, edit: SettingEdit) {
        let key = edit.key().to_owned();
        let previous = self.settings.get(&key).cloned();
        match apply_setting_edit(&mut self.settings, edit) {
            Ok(()) => {
                match ryusei_host::persist_settings_store(
                    &self.settings,
                    &mut self.settings_persistence,
                ) {
                    Ok(()) => self.status = format!("setting {key} saved").into(),
                    Err(error) => {
                        match previous {
                            Some(value) => {
                                let _ = self.settings.set(&key, value);
                            }
                            None => {
                                self.settings.remove(&key);
                            }
                        }
                        self.status = format!("setting {key} not persisted: {error}").into();
                    }
                }
            }
            Err(error) => self.status = format!("setting {key} rejected: {error}").into(),
        }
    }

    #[allow(dead_code)]
    fn on_settings_toggle(
        &mut self,
        row: &SettingRow,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_settings_edit(toggle_boolean_edit(row));
        cx.notify();
    }

    /// Starts text editing for a non-boolean settings row: remembers the row,
    /// seeds the draft from the current value and focuses the input.
    #[allow(dead_code)]
    fn on_settings_row_clicked(
        &mut self,
        row: &SettingRow,
        _: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if row.kind == ryusei_host::SettingKind::Boolean {
            return;
        }
        self.settings_editing_key = Some(row.key.clone());
        self.text_inputs.settings_draft = row
            .value
            .as_ref()
            .map(|value| editable_setting_value(Some(value)))
            .unwrap_or_default()
            .into();
        window.focus(&self.text_inputs.settings_input_focus_handle);
        cx.notify();
    }

    #[allow(dead_code)]
    fn on_settings_input_focus(
        &mut self,
        _: &MouseDownEvent,
        window: &mut Window,
        _: &mut Context<Self>,
    ) {
        window.focus(&self.text_inputs.settings_input_focus_handle);
    }

    #[allow(dead_code)]
    fn on_settings_key_down(
        &mut self,
        event: &gpui::KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(key) = self.settings_editing_key.clone() else {
            return;
        };
        let Some(row) = panel_setting_rows(&self.settings)
            .into_iter()
            .find(|row| row.key == key)
        else {
            return;
        };
        let mut draft = self.text_inputs.settings_draft.to_string();
        match event.keystroke.key.as_str() {
            "backspace" => {
                draft.pop();
            }
            "enter" => {
                self.commit_settings_input(&row, &draft, cx);
                return;
            }
            "escape" => {
                self.settings_editing_key = None;
                self.text_inputs.settings_draft = "".into();
                cx.notify();
                return;
            }
            _ => {
                if let Some(key_char) = event.keystroke.key_char.as_ref() {
                    draft.push_str(key_char);
                }
            }
        }
        self.text_inputs.settings_draft = draft.into();
        cx.notify();
    }

    /// Commits the settings draft for the row: parses it by the host value
    /// kind, applies and persists it, then leaves the editing state.
    #[allow(dead_code)]
    fn commit_settings_input(&mut self, row: &SettingRow, text: &str, cx: &mut Context<Self>) {
        let edit = match row.kind {
            ryusei_host::SettingKind::Number => number_edit(&row.key, text),
            ryusei_host::SettingKind::StringArray => Ok(string_array_edit(&row.key, text)),
            ryusei_host::SettingKind::NullableString => {
                if text.trim().is_empty() {
                    Ok(SettingEdit::Clear {
                        key: row.key.clone(),
                    })
                } else {
                    Ok(SettingEdit::Set {
                        key: row.key.clone(),
                        value: serde_json::json!(text.trim()),
                    })
                }
            }
            _ => Ok(SettingEdit::Set {
                key: row.key.clone(),
                value: serde_json::json!(text),
            }),
        };
        match edit {
            Ok(edit) => self.apply_settings_edit(edit),
            Err(error) => self.status = format!("setting {} rejected: {error}", row.key).into(),
        }
        self.settings_editing_key = None;
        self.text_inputs.settings_draft = "".into();
        cx.notify();
    }

    /// Records a just-opened/saved path in recent files and updates the status
    /// bar with any persistence error.
    fn record_recent(&mut self, path: &std::path::Path) {
        match record_opened_file(&self.persistence, &mut self.recent_files, path.to_owned()) {
            Ok(()) => {}
            Err(error) => self.status = format!("recent-file persistence failed: {error}").into(),
        }
    }

    /// Snapshots the current dirty document as crash recovery.
    fn synchronize_recovery(&mut self) {
        self.capture_active_workspace_tab();
        if let Err(error) = self.persist_workspace_tabs() {
            self.status = format!("session state not persisted: {error}").into();
        }
        let snapshot = self.host.snapshot();
        if !snapshot.file_state.is_dirty {
            return;
        }
        let display_name = snapshot
            .file_state
            .path
            .as_deref()
            .and_then(|path| std::path::Path::new(path).file_name())
            .and_then(|name| name.to_str());
        match capture_autosave(
            &self.persistence,
            &mut self.autosave,
            &self.host,
            display_name,
        ) {
            Ok(info) if info.is_available => {}
            Ok(_) => {}
            Err(error) => self.status = format!("autosave failed: {error}").into(),
        }
    }

    /// Clears crash recovery after an explicit clean save.
    fn clear_recovery(&mut self) {
        if let Err(error) = clear_autosave(&self.persistence, &mut self.autosave) {
            self.status = format!("autosave clear failed: {error}").into();
        }
    }

    #[allow(dead_code)]
    fn on_restore_recovery(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(candidate) = self.autosave.resolve_restore() else {
            self.status = "no recovery to restore".into();
            cx.notify();
            return;
        };
        let mut events = RecordingSink;
        match self.host.restore_from_sgf(&candidate.sgf, &mut events) {
            Ok(_) => {
                self.status = "recovery restored".into();
                self.last_vertex = None;
                self.external_file.detach_file();
                self.disconnect_all_engine_sessions();
            }
            Err(error) => self.status = format!("restore failed: {error}").into(),
        }
        cx.notify();
    }

    #[allow(dead_code)]
    fn on_discard_recovery(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.autosave.resolve_discard();
        if let Err(error) = clear_autosave(&self.persistence, &mut self.autosave) {
            self.status = format!("autosave clear failed: {error}").into();
        } else {
            self.status = "recovery discarded".into();
        }
        cx.notify();
    }

    fn new_game(&mut self, cx: &mut Context<Self>) {
        let (size, _) = default_new_game_properties(&self.settings);
        self.new_game_at(size, cx);
    }

    /// Creates a clean document at `size` and applies the persisted new-game
    /// defaults. Used by both the New Game action and board-size buttons.
    fn new_game_at(&mut self, size: usize, cx: &mut Context<Self>) {
        let properties = default_new_game_properties_for_size(&self.settings, size);
        let mut events = RecordingSink;
        match self
            .host
            .create_new_with_properties(size, size, &properties, &mut events)
        {
            Ok(_) => {
                self.board_size = size;
                self.status = format!("new {size}x{size} game").into();
            }
            Err(error) => {
                self.status = format!("new game failed: {error}").into();
                cx.notify();
                return;
            }
        }
        self.last_vertex = None;
        self.clock = ClockController::new(TimeControl::from_sgf(&properties));
        self.clock_last_updated = Instant::now();
        if !matches!(self.clock.state().control, TimeControl::None) {
            self.clock.start(Color::Black);
        }
        self.external_file.detach_file();
        self.disconnect_all_engine_sessions();
        cx.notify();
    }

    pub fn start_new_match_from_setup(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // OGS remote matches start when the server finishes matchmaking; a
        // manual "开始对局" must not create a local game on top of the search.
        if self.session_policy.source == SessionSource::RemoteCompetition {
            if self.ogs_client.snapshot().matchmaking_status
                == ryusei_host::OgsMatchmakingStatus::Searching
            {
                self.show_toast("正在匹配 OGS 对手，匹配成功后对局自动开始".to_owned(), cx);
            } else {
                self.show_toast("OGS 远程对局由服务器自动开始，无需手动开始".to_owned(), cx);
            }
            self.close_drawer(event, window, cx);
            cx.notify();
            return;
        }

        // Preserve the match configuration the user chose in the setup drawer.
        // `create_workspace_session` / `new_game_at` both reset the policy to
        // defaults and the clock to `TimeControl::None`; restoring them here is
        // what keeps the chosen participants and clock active after the reset.
        let chosen_policy = self.session_policy;
        let chosen_control = self.clock.state().control;

        let has_moves = !self.host.snapshot().moves.is_empty();
        if has_moves {
            self.create_workspace_session(cx);
        } else {
            self.new_game_at(self.board_size, cx);
        }

        self.session_policy = chosen_policy;
        self.clock = ClockController::new(chosen_control);
        self.mode = GameMode::Play;
        self.active_tool = MarkupTool::Play;
        if !matches!(self.clock.state().control, TimeControl::None) {
            self.clock.start(Color::Black);
            self.clock_last_updated = Instant::now();
        }
        // Persist the restored policy/clock onto the new (or reused) workspace
        // tab so switching sessions does not fall back to the defaults.
        self.capture_active_workspace_tab();
        let _ = self.persist_workspace_tabs();

        self.request_configured_engine_turn(cx);
        self.close_drawer(event, window, cx);
        self.show_toast("对局已开始！".to_owned(), cx);
        cx.notify();
    }

    fn play_sound_if_enabled(&mut self, cue: SoundCue) {
        play_if_enabled(&self.settings, self.sound_sink.as_mut(), cue);
    }

    fn on_pass(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.session_policy.source == SessionSource::RemoteCompetition {
            self.ogs_pass(cx);
            return;
        }
        let color = self.host.snapshot().board.next_player;
        if self.configured_engine_role(color).is_some() {
            self.status = "当前轮到 AI 落子".into();
            self.show_toast("当前轮到 AI 落子".to_owned(), cx);
            cx.notify();
            return;
        }
        self.advance_clock(Instant::now(), cx);
        if self.clock.state().expired.is_some() {
            return;
        }
        let mut events = RecordingSink;
        match self.host.play_move(color, None, &mut events) {
            Ok(_) => {
                if !self.commit_clock_move(color, cx) {
                    return;
                }
                self.last_vertex = None;
                self.status = format!(
                    "{} passed",
                    if color == Color::Black {
                        "black"
                    } else {
                        "white"
                    }
                )
                .into();
                self.synchronize_recovery();
                self.play_sound_if_enabled(SoundCue::Pass);
                self.sync_engine_position(None, color, None, cx);
                // 双方连续停一手即终局（虚着结束），进入自由分析模式。
                let moves = self.host.snapshot().moves.clone();
                let double_pass = moves.len() >= 2
                    && moves[moves.len() - 1].vertex.is_none()
                    && moves[moves.len() - 2].vertex.is_none();
                if double_pass {
                    self.host.set_root_property("RE", vec!["0".to_owned()]);
                    self.finish_local_game("双方连续停一手", cx);
                    return;
                }
                self.maybe_background_review_current_position(cx);
                self.request_configured_engine_turn(cx);
            }
            Err(error) => self.status = format!("pass rejected: {error}").into(),
        }
        cx.notify();
    }

    /// 对局结束后切换到「自由分析模式」：打谱（Record）会话、允许自由落子与
    /// 手动 AI 分析，AI 不再自动应手，时钟停止。
    fn finish_local_game(&mut self, result: &str, cx: &mut Context<Self>) {
        self.session_policy = SessionPolicy::new(SessionMode::Record, SessionSource::Local);
        self.clock.pause();
        // Record 模式默认是手动分析，而非自动。
        self.analysis_enabled = false;
        self.restart_analysis_after_position_change = false;
        self.mode = GameMode::Play;
        self.active_tool = MarkupTool::Play;
        self.status = format!("对局结束（{result}），已进入自由分析模式").into();
        self.synchronize_recovery();
        self.show_toast(self.status.to_string(), cx);
        // 对局结束自动呈现胜率图（底部分析面板），方便查看复盘结果。
        self.switch_bottom_tab(crate::BottomDeckTab::WinrateGraph, cx);
        cx.notify();
    }

    /// 对局期间可选的后台逐手复盘：每手棋落下后自动用 80v 分析并持久化候选点
    /// 与胜率（`review.analyze_during_game`）。分析期间不打断对局，结束后胜率图
    /// 自动呈现。
    fn maybe_background_review_current_position(&mut self, cx: &mut Context<Self>) {
        if !self
            .settings
            .get_bool("review.analyze_during_game")
            .unwrap_or(false)
        {
            return;
        }
        // 已有分析/复盘在跑时跳过，避免并发冲突。
        if self.analysis_task.is_some() || self.background_review {
            return;
        }
        self.background_review = true;
        self.batch_review_profile = Some(ryusei_domain_core::ReviewProfile::Quick80);
        self.start_analysis(cx);
    }

    fn on_resign(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.session_policy.source == SessionSource::RemoteCompetition {
            self.ogs_resign(cx);
            return;
        }
        // Local resignation: the human concedes. In a human-vs-human game the
        // side to move is the one resigning; against an AI the human resigns.
        let snapshot = self.host.snapshot();
        let resigning = if self.session_policy.participants == MatchParticipants::human_vs_human() {
            snapshot.board.next_player
        } else if self.session_policy.participants.black == PlayerKind::Human {
            Color::Black
        } else {
            Color::White
        };
        let winner = resigning.opponent();
        let winner_code = if winner == Color::Black { "B" } else { "W" };
        self.host
            .set_root_property("RE", vec![format!("{winner_code}+R")]);
        let resigning_label = if resigning == Color::Black {
            "黑方"
        } else {
            "白方"
        };
        let winner_label = if winner == Color::Black {
            "黑方"
        } else {
            "白方"
        };
        self.synchronize_recovery();
        self.finish_local_game(&format!("{resigning_label} 认输，{winner_label} 获胜"), cx);
    }

    /// Starts a splitter drag from a divider's mouse-down position.
    fn begin_split_drag(&mut self, pane: SplitPane, position: f32, cx: &mut Context<Self>) {
        let start_size = match pane {
            SplitPane::Left => self.left_sidebar_width,
            SplitPane::Right => self.right_sidebar_width,
            SplitPane::PeerList => self.peer_list_height,
            SplitPane::WinrateGraph => self.winrate_graph_height,
            SplitPane::Properties => self.properties_height,
        };
        self.split_drag = Some(SplitDrag {
            pane,
            start_position: position,
            start_size,
        });
        cx.notify();
    }

    /// Applies a window-global mouse move to the active splitter drag.
    fn update_split_drag(&mut self, position: f32, window: &Window, cx: &mut Context<Self>) {
        let Some(drag) = self.split_drag else {
            return;
        };
        let (fallback_min, min_size_key, max_size) = match drag.pane {
            SplitPane::Left => (
                100.0,
                "view.leftsidebar_minwidth",
                f32::from(window.viewport_size().width) - 320.0,
            ),
            SplitPane::Right => (
                100.0,
                "view.sidebar_minwidth",
                f32::from(window.viewport_size().width) - 320.0,
            ),
            SplitPane::PeerList => (
                58.0,
                "view.peerlist_minheight",
                f32::from(window.viewport_size().height) - 180.0,
            ),
            SplitPane::WinrateGraph => (
                60.0,
                "view.winrategraph_minheight",
                self.settings
                    .get("view.winrategraph_maxheight")
                    .and_then(serde_json::Value::as_f64)
                    .map(|value| value as f32)
                    .unwrap_or(f32::from(window.viewport_size().height) - 180.0),
            ),
            SplitPane::Properties => (
                100.0,
                "view.properties_minheight",
                f32::from(window.viewport_size().height) - 180.0,
            ),
        };
        let min_size = self
            .settings
            .get(min_size_key)
            .and_then(serde_json::Value::as_f64)
            .map(|value| value as f32)
            .unwrap_or(fallback_min);
        let max_size = max_size.max(min_size);
        let next_size = clamp_pane_size(
            pane_size_for_drag(drag.start_size, drag.start_position, position, drag.pane),
            min_size,
            max_size,
        );
        match drag.pane {
            SplitPane::Left => self.left_sidebar_width = next_size,
            SplitPane::Right => self.right_sidebar_width = next_size,
            SplitPane::PeerList => self.peer_list_height = next_size,
            SplitPane::WinrateGraph => self.winrate_graph_height = next_size,
            SplitPane::Properties => self.properties_height = next_size,
        }
        cx.notify();
    }

    /// Finishes a splitter drag and persists the final pane width.
    fn finish_split_drag(&mut self, cx: &mut Context<Self>) {
        let Some(drag) = self.split_drag.take() else {
            return;
        };
        let (size_key, size, label) = match drag.pane {
            SplitPane::Left => (
                "view.leftsidebar_width",
                self.left_sidebar_width,
                "pane width",
            ),
            SplitPane::Right => ("view.sidebar_width", self.right_sidebar_width, "pane width"),
            SplitPane::PeerList => (
                "view.peerlist_height",
                self.peer_list_height,
                "peer list height",
            ),
            SplitPane::WinrateGraph => (
                "view.winrategraph_height",
                self.winrate_graph_height,
                "winrate graph height",
            ),
            SplitPane::Properties => (
                "view.properties_height",
                self.properties_height,
                "properties height",
            ),
        };
        if let Err(error) = self.settings.set(size_key, serde_json::json!(size)) {
            self.status = format!("{label} not saved: {error}").into();
        } else if let Err(error) =
            ryusei_host::persist_settings_store(&self.settings, &mut self.settings_persistence)
        {
            self.status = format!("{label} not persisted: {error}").into();
        }
        cx.notify();
    }

    fn on_split_drag_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(drag) = self.split_drag else {
            return;
        };
        let position = match drag.pane {
            SplitPane::Left | SplitPane::Right => f32::from(event.position.x),
            SplitPane::PeerList | SplitPane::WinrateGraph | SplitPane::Properties => {
                f32::from(event.position.y)
            }
        };
        self.update_split_drag(position, window, cx);
    }

    fn on_split_drag_mouse_up(
        &mut self,
        event: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.button == MouseButton::Left {
            self.finish_split_drag(cx);
        }
    }

    fn toggle_view_setting(&mut self, key: &str, label: &str, cx: &mut Context<Self>) {
        let current = self.settings.get_bool(key).unwrap_or(false);
        if let Err(error) = self.settings.set(key, serde_json::json!(!current)) {
            self.status = format!("{label} not accepted: {error}").into();
        } else if let Err(error) =
            ryusei_host::persist_settings_store(&self.settings, &mut self.settings_persistence)
        {
            self.status = format!("{label} not persisted: {error}").into();
        } else {
            self.status = format!("{label}: {}", if !current { "shown" } else { "hidden" }).into();
        }
        cx.notify();
    }

    fn toggle_sidebar_setting(&mut self, key: &str, label: &str, cx: &mut Context<Self>) {
        // Left sidebar defaults to hidden on first launch; right panes default to visible.
        let default_visible = key != "view.show_leftsidebar";
        let current = self.settings.get_bool(key).unwrap_or(default_visible);
        if let Err(error) = self.settings.set(key, serde_json::json!(!current)) {
            self.status = format!("{label} not accepted: {error}").into();
        } else if let Err(error) =
            ryusei_host::persist_settings_store(&self.settings, &mut self.settings_persistence)
        {
            self.status = format!("{label} not persisted: {error}").into();
        } else {
            self.status = format!("{label}: {}", if !current { "shown" } else { "hidden" }).into();
        }
        cx.notify();
    }

    fn on_toggle_left_sidebar(
        &mut self,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_sidebar_setting("view.show_leftsidebar", "engines sidebar", cx);
    }

    fn on_toggle_right_sidebar(
        &mut self,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_right_sidebar(cx);
    }

    fn toggle_right_sidebar(&mut self, cx: &mut Context<Self>) {
        // The right pane follows Sabaki's inferred visibility
        // (`show_graph || show_comments`). The toolbar toggle flips both
        // switches so the button always means "show/hide this pane".
        let show_graph = self.settings.get_bool("view.show_graph").unwrap_or(true);
        let show_comments = self.settings.get_bool("view.show_comments").unwrap_or(true);
        let show_analysis_preview = self
            .settings
            .get_bool("view.show_analysis_preview")
            .unwrap_or(true);
        let visible = right_pane_visible(show_graph, show_comments, show_analysis_preview);
        let target = !visible;
        let mut failed = false;
        for (key, value) in [
            ("view.show_graph", target),
            (
                "view.show_comments",
                if target { show_comments } else { false },
            ),
            ("view.show_analysis_preview", target),
        ] {
            if let Err(error) = self.settings.set(key, serde_json::json!(value)) {
                self.status = format!("panels sidebar not accepted: {error}").into();
                failed = true;
            }
        }
        if !failed {
            if let Err(error) =
                ryusei_host::persist_settings_store(&self.settings, &mut self.settings_persistence)
            {
                self.status = format!("panels sidebar not persisted: {error}").into();
            } else {
                self.status = format!(
                    "panels sidebar: {}",
                    if target { "shown" } else { "hidden" }
                )
                .into();
            }
        }
        cx.notify();
    }

    fn open(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.dialog_service.pick_open_path() else {
            self.status = "open cancelled".into();
            cx.notify();
            return;
        };
        let mut events = RecordingSink;
        match self.host.open(path.clone(), &self.file_access, &mut events) {
            Ok(_) => {
                self.disconnect_all_engine_sessions();
                self.status = format!("opened {}", path.display()).into();
                self.record_recent(&path);
                if let Err(error) = track_after_file_operation(&mut self.external_file, &path) {
                    self.status = format!("external-file tracking failed: {error}").into();
                }
            }
            Err(error) => self.status = format!("open failed: {error}").into(),
        }
        cx.notify();
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        self.save_game();
        cx.notify();
    }

    /// Saves the current document to its source location, or opens the Save As
    /// dialog when there is none. Returns `true` only when the document is now
    /// saved somewhere; failures and cancelled dialogs return `false`.
    fn save_game(&mut self) -> bool {
        let has_save_location = self.host.snapshot().file_state.path.is_some();
        if !has_save_location {
            return self.save_game_as();
        }
        let mut events = RecordingSink;
        match self.host.save(&mut self.file_access, &mut events) {
            Ok(_) => {
                self.status = "saved".into();
                self.clear_recovery();
                self.track_external_after_save();
                true
            }
            Err(error) => {
                self.status = format!("save failed: {error}").into();
                false
            }
        }
    }

    fn save_as(&mut self, cx: &mut Context<Self>) {
        self.save_game_as();
        cx.notify();
    }

    /// Saves the current document to a user-chosen location. Returns `true`
    /// only when the document is now saved; a cancelled dialog or a failed
    /// write returns `false`.
    fn save_game_as(&mut self) -> bool {
        let snapshot = self.host.snapshot();
        let suggested_name = snapshot
            .file_state
            .path
            .as_deref()
            .and_then(|path| std::path::Path::new(path).file_name())
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .unwrap_or("untitled.sgf")
            .to_owned();
        let Some(path) = self.dialog_service.pick_save_path(&suggested_name) else {
            self.status = "save cancelled".into();
            return false;
        };
        let mut events = RecordingSink;
        match self
            .host
            .save_at(path.clone(), &mut self.file_access, &mut events)
        {
            Ok(_) => {
                self.status = format!("saved {}", path.display()).into();
                self.record_recent(&path);
                self.clear_recovery();
                self.track_external_after_save();
                true
            }
            Err(error) => {
                self.status = format!("save failed: {error}").into();
                false
            }
        }
    }

    /// Rebases the external-file fingerprint after a successful save, so the
    /// saved document is the new baseline for change detection.
    fn track_external_after_save(&mut self) {
        let Some(path) = self.host.snapshot().file_state.path.clone() else {
            return;
        };
        let path = std::path::PathBuf::from(path);
        if let Err(error) = track_after_file_operation(&mut self.external_file, &path) {
            self.status = format!("external-file tracking failed: {error}").into();
        }
    }

    /// Runs one periodic external-file check and surfaces the outcome in the
    /// status bar. Clean documents changed on disk reload automatically;
    /// dirty documents keep the conflict status for the user to resolve.
    fn check_external_file_now(&mut self, cx: &mut Context<Self>) {
        let is_dirty = self.host.snapshot().file_state.is_dirty;
        match check_external_file(&mut self.external_file, &mut self.host, is_dirty) {
            ExternalCheckOutcome::Reloaded => {
                self.disconnect_all_engine_sessions();
                self.status = "external change reloaded".into();
                self.last_vertex = None;
            }
            ExternalCheckOutcome::Status(ryusei_host::ExternalFileStatus::Changed) => {
                self.status = "external change detected; save to keep local or reload".into();
            }
            ExternalCheckOutcome::Status(ryusei_host::ExternalFileStatus::Missing) => {
                self.status = "the source game file is missing".into();
            }
            ExternalCheckOutcome::Status(ryusei_host::ExternalFileStatus::Unreadable) => {
                self.status = "the source game file cannot be read".into();
            }
            ExternalCheckOutcome::Status(_) | ExternalCheckOutcome::Failed(_) => {}
        }
        cx.notify();
    }

    /// Explicitly reloads the document from its tracked source file, ignoring
    /// any pending external-file conflict.
    #[allow(dead_code)]
    fn on_reload_external(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.external_file.tracked_path() else {
            self.status = "no external file to reload".into();
            cx.notify();
            return;
        };
        let mut events = RecordingSink;
        match self.host.open(path.clone(), &self.file_access, &mut events) {
            Ok(_) => {
                self.disconnect_all_engine_sessions();
                self.status = format!("reloaded {}", path.display()).into();
                self.last_vertex = None;
                if let Err(error) = track_after_file_operation(&mut self.external_file, &path) {
                    self.status = format!("external-file tracking failed: {error}").into();
                }
            }
            Err(error) => {
                self.external_file
                    .set_status(ryusei_host::ExternalFileStatus::Unreadable);
                self.status = format!("reload failed: {error}").into();
            }
        }
        cx.notify();
    }

    /// Keeps the local modifications and drops the source identity, so the
    /// next save must go through Save As.
    #[allow(dead_code)]
    fn on_keep_local_external(
        &mut self,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.external_file.detach_file();
        self.status = "keeping local changes; use Save As".into();
        cx.notify();
    }

    /// Resolves the window-close decision for the current document. Clean
    /// documents close immediately; dirty documents ask the user to Save,
    /// Discard or Cancel through a native confirmation dialog, and the window
    /// stays open when the save fails or the user cancels.
    fn should_allow_window_close(&mut self) -> bool {
        let is_dirty = self.host.snapshot().file_state.is_dirty;
        let decision = ryusei_host::decide_close_request(is_dirty, false);
        if decision == ryusei_host::CloseRequestAction::Allow {
            return true;
        }
        let choice = rfd::MessageDialog::new()
            .set_title("Unsaved Changes")
            .set_description("The current game has unsaved changes. Save them before closing?")
            .set_buttons(rfd::MessageButtons::YesNoCancel)
            .show();
        let choice = match choice {
            rfd::MessageDialogResult::Yes => CloseChoice::Save,
            rfd::MessageDialogResult::No => CloseChoice::Discard,
            _ => CloseChoice::Cancel,
        };
        match choice {
            CloseChoice::Save => close_decision(choice, self.save_game()),
            _ => close_decision(choice, false),
        }
    }

    fn undo(&mut self, cx: &mut Context<Self>) {
        let mut events = RecordingSink;
        match self.host.undo(&mut events) {
            Ok(_) => self.status = "undo".into(),
            Err(error) => self.status = format!("undo failed: {error}").into(),
        }
        cx.notify();
    }

    fn redo(&mut self, cx: &mut Context<Self>) {
        let mut events = RecordingSink;
        match self.host.redo(&mut events) {
            Ok(_) => self.status = "redo".into(),
            Err(error) => self.status = format!("redo failed: {error}").into(),
        }
        cx.notify();
    }

    fn navigate(&mut self, direction: NavigationDirection, cx: &mut Context<Self>) {
        let snapshot = self.host.snapshot();
        let Some(target) = navigation_target(&snapshot, direction) else {
            self.status = format!("cannot navigate {direction:?}").into();
            cx.notify();
            return;
        };
        self.navigate_to_node(target, cx);
    }

    fn navigate_to_node(&mut self, target: ryusei_domain_core::NodeId, cx: &mut Context<Self>) {
        self.navigate_to_node_with_batch_policy(target, true, cx);
    }

    fn navigate_to_node_with_batch_policy(
        &mut self,
        target: ryusei_domain_core::NodeId,
        stop_batch: bool,
        cx: &mut Context<Self>,
    ) {
        if stop_batch
            && self
                .batch_review_progress
                .is_some_and(|progress| progress.is_running)
        {
            self.cancel_whole_game_review(false, cx);
        }
        let transaction = ryusei_domain_core::GameTransaction {
            schema_version: ryusei_domain_core::CURRENT_TRANSACTION_SCHEMA_VERSION,
            transaction_type: ryusei_domain_core::GameTransactionType::Navigate,
            color: None,
            vertex: None,
            node_id: Some(target.clone()),
            property: None,
            values: Vec::new(),
            marker: None,
            nodes: Vec::new(),
            score_override: None,
        };
        let mut events = RecordingSink;
        match self.host.apply_transaction(transaction, &mut events) {
            Ok(_) => self.status = format!("moved to {target}").into(),
            Err(error) => self.status = format!("navigation failed: {error}").into(),
        }
        // Restore persisted per-move analysis candidates when navigating to a
        // reviewed node, so the board shows its candidate points and winrate.
        self.restore_analysis_candidates_for_current_node();
        cx.notify();
    }

    /// Loads the persisted candidate list (`RYK`) for the current node back
    /// into the board analysis markers, clearing them when the node has none.
    fn restore_analysis_candidates_for_current_node(&mut self) {
        let snapshot = self.host.snapshot();
        let persisted = snapshot
            .nodes
            .iter()
            .find(|node| node.id == snapshot.current_node_id)
            .and_then(|node| node.properties.get(CANDIDATES_PROPERTY))
            .and_then(|values| values.first())
            .cloned();
        match persisted {
            Some(value) if !value.is_empty() => {
                let entries = deserialize_analysis_candidates(&value);
                self.analysis = entries.clone();
                let board_size = snapshot.board.width;
                self.analysis_best_move = best_analysis_move(&entries, board_size)
                    .map(|(column, row)| Vertex { column, row });
            }
            _ => {
                self.analysis.clear();
                self.analysis_best_move = None;
            }
        }
    }

    fn on_board_vertex_mouse_down(&mut self, vertex: Vertex, cx: &mut Context<Self>) {
        if self.session_policy.mode == SessionMode::Live {
            self.status = "实时会话为只读观察模式".into();
            cx.notify();
            return;
        }
        if self.session_policy.source == SessionSource::RemoteCompetition {
            if self.ogs_marking_dead {
                self.ogs_toggle_dead_stone(vertex, cx);
            } else {
                self.ogs_submit_move_at(vertex, cx);
            }
            return;
        }
        self.on_board_hover(vertex, cx);
        if self.active_tool.is_line_tool() && self.mode == GameMode::Edit {
            self.line_start = Some(vertex);
            self.status = "drag to draw; release at the end vertex".into();
            cx.notify();
        } else {
            self.on_board_vertex_clicked(vertex, cx);
        }
    }

    fn on_board_vertex_mouse_move(&mut self, vertex: Vertex, cx: &mut Context<Self>) {
        self.on_board_hover(vertex, cx);
    }

    fn on_board_vertex_mouse_up(&mut self, vertex: Vertex, cx: &mut Context<Self>) {
        if self.active_tool.is_line_tool() && self.mode == GameMode::Edit {
            self.line_at(vertex, cx);
            cx.notify();
        }
    }

    /// Board interaction entry point. The goban's per-vertex hit layer maps
    /// the click to a vertex before calling this, so the handler no longer
    /// depends on the board's window-global origin or the surrounding layout.
    fn on_board_vertex_clicked(&mut self, vertex: Vertex, cx: &mut Context<Self>) {
        if matches!(self.mode, GameMode::Scoring | GameMode::Estimator) {
            self.scoring_at(vertex, cx);
        } else if self.mode == GameMode::Find {
            self.find_move_at(vertex, cx);
        } else if self.mode == GameMode::Guess {
            self.guess_move_at(vertex, cx);
        } else if self.mode == GameMode::Autoplay {
            self.advance_autoplay(Some(vertex), cx);
        } else if self.active_tool.is_setup_tool() {
            self.setup_at(vertex, cx);
        } else if self.active_tool == MarkupTool::Play {
            self.play_at(vertex, cx);
        } else {
            self.markup_at(vertex, cx);
        }
        cx.notify();
    }

    fn play_at(&mut self, vertex: Vertex, cx: &mut Context<Self>) {
        let color = self.host.snapshot().board.next_player;
        if self.configured_engine_role(color).is_some() {
            self.status = "当前轮到 AI 落子".into();
            self.show_toast("当前轮到 AI 落子".to_owned(), cx);
            cx.notify();
            return;
        }
        self.advance_clock(Instant::now(), cx);
        if self.clock.state().expired.is_some() {
            return;
        }
        let mut events = RecordingSink;
        match self.host.play_move(color, Some(vertex), &mut events) {
            Ok(_) => {
                if !self.commit_clock_move(color, cx) {
                    return;
                }
                self.last_vertex = Some(vertex);
                self.status = format!("move at {},{}", vertex.column, vertex.row).into();
                self.synchronize_recovery();
                self.play_sound_if_enabled(SoundCue::StonePlaced);
                self.sync_engine_position(None, color, Some(vertex), cx);
                self.maybe_background_review_current_position(cx);

                self.request_configured_engine_turn(cx);
            }
            Err(error) => self.status = format!("move rejected: {error}").into(),
        }
        cx.notify();
    }

    /// Broadcasts a local move to every idle role session except the engine
    /// that generated it. A leased Analysis session is stopped and replayed on
    /// return, so it never receives commands concurrently with streaming.
    fn sync_engine_position(
        &mut self,
        source: Option<EngineRole>,
        color: Color,
        vertex: Option<Vertex>,
        cx: &mut Context<Self>,
    ) {
        self.hovered_candidate_vertex = None;
        self.trial_move = None;
        self.active_analysis_trial_move = None;
        self.analysis.clear();
        self.analysis_best_move = None;
        let start_idle_analysis = self.analysis_task.is_none() && self.analysis_enabled;
        if self.analysis_task.is_some() {
            self.restart_analysis_after_position_change = self.analysis_enabled;
            self.analysis_run.request_replay_and_stop();
        } else {
            self.restart_analysis_after_position_change = false;
        }
        let errors = self.engine_controller.synchronize_move(
            source,
            color,
            vertex.map(|vertex| (vertex.column, vertex.row)),
        );
        for (role, error) in errors {
            self.status = format!("{} engine sync failed: {error}", role.label()).into();
        }
        if start_idle_analysis {
            self.start_analysis(cx);
        }
    }

    fn find_move_at(&mut self, vertex: Vertex, cx: &mut Context<Self>) {
        let snapshot = self.host.snapshot();
        let matching_node = snapshot.nodes.iter().find(|node| {
            ["B", "W"]
                .into_iter()
                .filter_map(|property| node.properties.get(property)?.first())
                .filter_map(|value| crate::goban_view::parse_sgf_vertex(value))
                .any(|move_vertex| move_vertex == vertex)
        });
        match matching_node {
            Some(node) => {
                self.navigate_to_node(node.id.clone(), cx);
                self.status = format!("found move at {},{}", vertex.column, vertex.row).into();
            }
            None => {
                self.status = format!("no move at {},{}", vertex.column, vertex.row).into();
                cx.notify();
            }
        }
    }

    fn guess_move_at(&mut self, vertex: Vertex, cx: &mut Context<Self>) {
        let snapshot = self.host.snapshot();
        let expected_node = snapshot.nodes.iter().find(|node| {
            node.parent_id.as_deref() == Some(snapshot.current_node_id.as_str())
                && ["B", "W"]
                    .into_iter()
                    .filter_map(|property| node.properties.get(property)?.first())
                    .filter_map(|value| crate::goban_view::parse_sgf_vertex(value))
                    .any(|move_vertex| move_vertex == vertex)
        });
        match expected_node {
            Some(node) => {
                self.navigate_to_node(node.id.clone(), cx);
                self.status = "correct guess".into();
            }
            None => {
                self.status = "not the next move".into();
                cx.notify();
            }
        }
    }

    fn advance_autoplay(&mut self, selected_vertex: Option<Vertex>, cx: &mut Context<Self>) {
        let snapshot = self.host.snapshot();
        let expected_node = snapshot.nodes.iter().find(|node| {
            node.parent_id.as_deref() == Some(snapshot.current_node_id.as_str())
                && selected_vertex.is_none_or(|vertex| {
                    ["B", "W"]
                        .into_iter()
                        .filter_map(|property| node.properties.get(property)?.first())
                        .filter_map(|value| crate::goban_view::parse_sgf_vertex(value))
                        .any(|move_vertex| move_vertex == vertex)
                })
        });
        match expected_node {
            Some(node) => {
                self.navigate_to_node(node.id.clone(), cx);
                self.status = "autoplay advanced".into();
            }
            None => {
                self.status = "autoplay reached the end of this variation".into();
                cx.notify();
            }
        }
    }

    fn line_at(&mut self, vertex: Vertex, _cx: &mut Context<Self>) {
        let Some(start) = self.line_start.take() else {
            self.line_start = Some(vertex);
            self.status = "line: choose the end vertex".into();
            return;
        };
        if start == vertex {
            self.line_start = None;
            self.status = "line cancelled".into();
            return;
        }
        let snapshot = self.host.snapshot();
        let node_properties = snapshot
            .nodes
            .iter()
            .find(|node| node.id == snapshot.current_node_id)
            .map(|node| node.properties.clone())
            .unwrap_or_default();
        let Some(transaction) = create_line_transaction(
            &snapshot.current_node_id,
            start,
            vertex,
            self.active_tool,
            &node_properties,
        ) else {
            self.status = "no line transaction for this tool".into();
            return;
        };
        let mut events = RecordingSink;
        match self.host.apply_transaction(transaction, &mut events) {
            Ok(_) => {
                self.status = format!(
                    "{} from {},{} to {},{}",
                    self.active_tool.label(),
                    start.column,
                    start.row,
                    vertex.column,
                    vertex.row
                )
                .into();
                self.synchronize_recovery();
            }
            Err(error) => self.status = format!("line failed: {error}").into(),
        }
    }

    fn on_board_hover(&mut self, vertex: Vertex, cx: &mut Context<Self>) {
        let candidate_vertex = self.analysis.iter().find_map(|entry| {
            let parsed = entry
                .vertex
                .as_deref()
                .and_then(|value| parse_gtp_vertex(self.host.snapshot().board.width, value))?;
            (parsed == (vertex.column, vertex.row))
                .then(|| entry.vertex.clone())
                .flatten()
        });
        let changed = self.hovered_vertex != Some(vertex)
            || self.hovered_candidate_vertex != candidate_vertex;
        self.hovered_vertex = Some(vertex);
        self.hovered_candidate_vertex = candidate_vertex;
        if changed {
            cx.notify();
        }
    }

    fn markup_at(&mut self, vertex: Vertex, cx: &mut Context<Self>) {
        let snapshot = self.host.snapshot();
        let Some(transaction) =
            create_markup_transaction(&snapshot.current_node_id, vertex, self.active_tool, "A")
        else {
            self.status = "no markup transaction for this tool".into();
            cx.notify();
            return;
        };
        let mut events = RecordingSink;
        match self.host.apply_transaction(transaction, &mut events) {
            Ok(_) => {
                self.last_vertex = Some(vertex);
                self.status = format!(
                    "{} at {},{}",
                    self.active_tool.label(),
                    vertex.column,
                    vertex.row
                )
                .into();
                self.synchronize_recovery();
            }
            Err(error) => self.status = format!("markup failed: {error}").into(),
        }
        cx.notify();
    }

    /// Edits setup stones on the current node: `AB`/`AW` placement appends
    /// the clicked vertex to the node's setup property; clear removes it from
    /// both properties.
    fn setup_at(&mut self, vertex: Vertex, cx: &mut Context<Self>) {
        let snapshot = self.host.snapshot();
        let node_id = snapshot.current_node_id.clone();
        let node_properties = snapshot
            .nodes
            .iter()
            .find(|node| node.id == node_id)
            .map(|node| node.properties.clone())
            .unwrap_or_default();
        let transactions =
            create_setup_transactions(&node_id, vertex, self.active_tool, &node_properties);
        if transactions.is_empty() {
            self.status = "no setup change for this vertex".into();
            cx.notify();
            return;
        }
        let mut events = RecordingSink;
        let mut applied = 0;
        for transaction in transactions {
            match self.host.apply_transaction(transaction, &mut events) {
                Ok(_) => applied += 1,
                Err(error) => {
                    self.status = format!("setup failed: {error}").into();
                    break;
                }
            }
        }
        if applied > 0 {
            self.last_vertex = Some(vertex);
            self.status = format!(
                "{} at {},{}",
                self.active_tool.label(),
                vertex.column,
                vertex.row
            )
            .into();
            self.synchronize_recovery();
        }
        cx.notify();
    }

    /// Toggles the scoring override on the clicked vertex through the
    /// `ApplyScoringOverride` transaction, cycling none → alive black →
    /// alive white → clear.
    fn scoring_at(&mut self, vertex: Vertex, cx: &mut Context<Self>) {
        let current = self.host.snapshot().score_overrides.get(&vertex).copied();
        let transaction = create_scoring_transaction(vertex, next_scoring_override(current));
        let mut events = RecordingSink;
        match self.host.apply_transaction(transaction, &mut events) {
            Ok(_) => {
                self.last_vertex = Some(vertex);
                self.status =
                    format!("scoring override at {},{}", vertex.column, vertex.row).into();
                self.synchronize_recovery();
            }
            Err(error) => self.status = format!("scoring failed: {error}").into(),
        }
        cx.notify();
    }

    fn set_match_participants(&mut self, participants: MatchParticipants, cx: &mut Context<Self>) {
        if self.session_policy.source == SessionSource::RemoteCompetition {
            self.show_toast("OGS 远程对局期间不能修改参与者设置".to_owned(), cx);
            return;
        }
        self.session_policy.participants = participants;
        self.status = format!("对局模式: {}", participants.label()).into();
        self.show_toast(self.status.clone(), cx);
        self.request_configured_engine_turn(cx);
        cx.notify();
    }

    fn configured_engine_role(&self, color: Color) -> Option<EngineRole> {
        (self.session_policy.mode == SessionMode::Match
            && self.mode == GameMode::Play
            && self.session_policy.participants.player(color) == PlayerKind::Ai)
            .then_some(match color {
                Color::Black => EngineRole::Black,
                Color::White => EngineRole::White,
            })
    }

    fn request_configured_engine_turn(&mut self, cx: &mut Context<Self>) {
        let color = self.host.snapshot().board.next_player;
        let Some(role) = self.configured_engine_role(color) else {
            return;
        };

        // Prefer the dedicated black/white role. If it is not bound to its own
        // engine process, fall back to the shared Analysis engine, which owns
        // the KataGo session used for board feedback.
        let target_role = if self.engine_controller.is_attached(role) {
            role
        } else if self.engine_controller.is_attached(EngineRole::Analysis) {
            EngineRole::Analysis
        } else if self.engine_roles.get(role).is_some() {
            role
        } else {
            EngineRole::Analysis
        };

        if self.engine_controller.is_attached(target_role) {
            self.trigger_engine_genmove(target_role, color, cx);
            return;
        }

        // The engine session is still connecting. Queue the move so the
        // handshake completion callback plays it as soon as the role is ready.
        let role_generation = self
            .engine_generations
            .get(&target_role)
            .copied()
            .unwrap_or_default()
            .wrapping_add(1);
        self.pending_engine_move = Some(PendingEngineMove {
            role: target_role,
            role_generation,
            color,
        });
        self.on_engine_connect(target_role, cx);
    }

    fn set_opening_convention(&mut self, opening: OpeningConvention, cx: &mut Context<Self>) {
        let _ = self.settings.set(
            "game.opening_convention",
            serde_json::json!(opening.setting_value()),
        );
        let _ = ryusei_host::persist_settings_store(&self.settings, &mut self.settings_persistence);
        self.new_game_at(self.board_size, cx);
        self.status = format!("新局开局方式: {}", opening.label()).into();
        self.show_toast(self.status.clone(), cx);
        cx.notify();
    }

    /// Adds the persisted review summary and its five largest mistakes to SGF
    /// comments. This is explicit because comments may be user-authored.
    #[allow(dead_code)]
    pub fn write_review_comments(&mut self, cx: &mut Context<Self>) {
        if matches!(
            self.session_policy.source,
            SessionSource::LiveBroadcast | SessionSource::RemoteCompetition
        ) {
            self.show_toast("直播和远程竞赛棋谱不能写入复盘评论", cx);
            return;
        }
        let snapshot = self.host.snapshot();
        let evaluations = ryusei_host::compute_game_move_evaluations(&snapshot);
        let summary = ryusei_host::GameAnalyticsSummary::from_evaluations(&evaluations);
        if summary.top_blunders.is_empty() {
            self.show_toast("尚无可写入的问题手评价", cx);
            return;
        }
        let comment_for = |existing: Option<&String>, annotation: String| {
            existing
                .filter(|comment| !comment.trim().is_empty())
                .map(|comment| format!("{comment}\n\nRyusei Review\n{annotation}"))
                .unwrap_or_else(|| format!("Ryusei Review\n{annotation}"))
        };
        let root_comment = comment_for(
            snapshot
                .root_properties
                .get("C")
                .and_then(|values| values.first()),
            summary.verdict(),
        );
        let nodes = snapshot
            .nodes
            .iter()
            .map(|node| (node.id.clone(), node.properties.clone()))
            .collect::<BTreeMap<_, _>>();
        let mut events = RecordingSink;
        let mut written = 0;
        for evaluation in &summary.top_blunders {
            let existing = nodes
                .get(&evaluation.node_id)
                .and_then(|properties| properties.get("C"))
                .and_then(|values| values.first());
            let comment = comment_for(existing, evaluation.format_sgf_comment());
            if self
                .host
                .apply_transaction(
                    crate::node_inspector::create_property_transaction(
                        &evaluation.node_id,
                        "C",
                        vec![comment],
                    ),
                    &mut events,
                )
                .is_ok()
            {
                written += 1;
            }
        }
        self.host.set_root_property("C", vec![root_comment]);
        self.synchronize_recovery();
        self.status = format!("已写入 {written} 条问题手复盘评论").into();
        self.show_toast(self.status.clone(), cx);
        cx.notify();
    }

    /// Updates the active local record's SGF ruleset and restarts analysis so
    /// any new KataGo session receives the revised `kata-set-rules` command.
    pub fn set_current_game_ruleset(
        &mut self,
        ruleset: ryusei_host::GoRuleset,
        cx: &mut Context<Self>,
    ) {
        if matches!(
            self.session_policy.source,
            SessionSource::LiveBroadcast | SessionSource::RemoteCompetition
        ) {
            self.show_toast("直播和远程竞赛棋谱不能修改规则", cx);
            return;
        }

        self.host
            .set_root_property("RU", vec![ruleset.sgf_name().to_owned()]);
        let resume_analysis = self.analysis_enabled;
        self.disconnect_all_engine_sessions();
        self.synchronize_recovery();
        self.status = format!("当前棋局规则: {}", ruleset.label()).into();
        self.show_toast(self.status.clone(), cx);
        if resume_analysis {
            self.start_analysis(cx);
        }
        cx.notify();
    }

    pub fn apply_current_ruleset_default_komi(&mut self, cx: &mut Context<Self>) {
        if matches!(
            self.session_policy.source,
            SessionSource::LiveBroadcast | SessionSource::RemoteCompetition
        ) {
            self.show_toast("直播和远程竞赛棋谱不能修改贴目", cx);
            return;
        }
        let snapshot = self.host.snapshot();
        let ruleset = ryusei_host::GoRuleset::from_setting(
            snapshot
                .root_properties
                .get("RU")
                .and_then(|values| values.first())
                .map(String::as_str),
        );
        let handicap = snapshot
            .root_properties
            .get("HA")
            .and_then(|values| values.first())
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or_default();
        let komi = ruleset.default_komi(handicap);
        self.host
            .set_root_property("KM", vec![format!("{komi:.1}")]);
        let resume_analysis = self.analysis_enabled;
        self.disconnect_all_engine_sessions();
        self.synchronize_recovery();
        self.status = format!("已按 {} 设置默认贴目 {komi:.1}", ruleset.sgf_name()).into();
        self.show_toast(self.status.clone(), cx);
        if resume_analysis {
            self.start_analysis(cx);
        }
        cx.notify();
    }

    fn set_time_control(&mut self, control: TimeControl, cx: &mut Context<Self>) {
        if self.session_policy.source == SessionSource::RemoteCompetition {
            self.show_toast("OGS 远程对局使用服务器时钟，不能本地修改".to_owned(), cx);
            return;
        }
        self.clock = ClockController::new(control);
        self.clock_last_updated = Instant::now();
        // Do not start the clock here: the user is still configuring the match
        // in the setup drawer. The clock only starts when "开始对局" is pressed.
        match control.to_sgf() {
            Some((main_time, overtime)) => {
                self.host.set_root_property("TM", vec![main_time]);
                self.host.set_root_property("OT", vec![overtime]);
            }
            None => {
                self.host.set_root_property("TM", vec!["0".to_owned()]);
                self.host.set_root_property("OT", vec!["none".to_owned()]);
            }
        }
        self.status = match control {
            TimeControl::None => "time control disabled".into(),
            TimeControl::Absolute { main_time_secs } => {
                format!("absolute time: {} minutes", main_time_secs / 60).into()
            }
            TimeControl::ByoYomi {
                main_time_secs,
                period_time_secs,
                periods,
            } => format!(
                "time control: {} minutes + {} x {} seconds byo-yomi",
                main_time_secs / 60,
                periods,
                period_time_secs
            )
            .into(),
            TimeControl::Fischer {
                main_time_secs,
                increment_secs,
            } => format!(
                "time control: {} minutes + {}s increment (Fischer)",
                main_time_secs / 60,
                increment_secs
            )
            .into(),
        };
        self.synchronize_recovery();
        cx.notify();
    }

    fn advance_clock(&mut self, now: Instant, cx: &mut Context<Self>) {
        let elapsed = now.saturating_duration_since(self.clock_last_updated);
        self.clock_last_updated = now;
        if let Some(ClockEvent::Expired(loser)) = self.clock.tick(elapsed) {
            crate::sound_feedback::play_if_enabled(
                &self.settings,
                &mut *self.sound_sink,
                crate::sound_feedback::SoundCue::TimeExpired,
            );
            let winner = match loser {
                Color::Black => "W",
                Color::White => "B",
            };
            self.host
                .set_root_property("RE", vec![format!("{winner}+T")]);
            self.status = format!(
                "{} lost on time",
                if loser == Color::Black {
                    "black"
                } else {
                    "white"
                }
            )
            .into();
            self.synchronize_recovery();
            cx.notify();
        }
        self.play_byoyomi_countdown_cue();
    }

    /// Emits the byo-yomi countdown cue as the active player's period runs low
    /// (last 10 seconds), at most once per whole second (PRD §2 读秒语音反馈).
    fn play_byoyomi_countdown_cue(&mut self) {
        let state = self.clock.state();
        if !state.running || state.paused {
            self.last_byoyomi_tick_secs = None;
            return;
        }
        let Some(active) = state.active_color else {
            return;
        };
        let player = match active {
            Color::Black => state.black,
            Color::White => state.white,
        };
        if !matches!(player.phase, ryusei_domain_core::ClockPhase::ByoYomi) {
            self.last_byoyomi_tick_secs = None;
            return;
        }
        let secs = player.display_remaining().as_secs();
        if secs == 0 || secs > 10 {
            self.last_byoyomi_tick_secs = None;
            return;
        }
        if self.last_byoyomi_tick_secs == Some(secs) {
            return;
        }
        self.last_byoyomi_tick_secs = Some(secs);
        crate::sound_feedback::play_if_enabled(
            &self.settings,
            &mut *self.sound_sink,
            crate::sound_feedback::SoundCue::ByoYomiTick,
        );
    }

    fn commit_clock_move(&mut self, color: Color, cx: &mut Context<Self>) -> bool {
        if let ClockEvent::Expired(loser) = self.clock.on_move_committed(color, Duration::ZERO) {
            let winner = match loser {
                Color::Black => "W",
                Color::White => "B",
            };
            self.host
                .set_root_property("RE", vec![format!("{winner}+T")]);
            let result = format!(
                "{} 超时判负",
                if loser == Color::Black {
                    "黑方"
                } else {
                    "白方"
                }
            );
            self.synchronize_recovery();
            self.finish_local_game(&result, cx);
            return false;
        }
        self.clock_last_updated = Instant::now();
        true
    }

    pub fn set_left_sidebar_tab(&mut self, tab: LeftSidebarTab, cx: &mut Context<Self>) {
        self.left_sidebar_tab = tab;
        cx.notify();
    }

    fn set_session_mode(&mut self, mode: SessionMode, cx: &mut Context<Self>) {
        if self.session_policy.source == SessionSource::RemoteCompetition {
            self.show_toast("OGS 远程对局期间不能切换会话模式".to_owned(), cx);
            return;
        }
        let source = match mode {
            SessionMode::Live => SessionSource::LiveBroadcast,
            SessionMode::Match if self.session_policy.source == SessionSource::LiveBroadcast => {
                SessionSource::Local
            }
            SessionMode::Record if self.session_policy.source == SessionSource::LiveBroadcast => {
                SessionSource::Local
            }
            _ => self.session_policy.source,
        };
        self.session_policy = SessionPolicy::new(mode, source);
        self.analysis_enabled = self.session_policy.analysis == AnalysisPolicy::Continuous;
        if !self.analysis_enabled && self.analysis_task.is_some() {
            self.analysis_run.request_stop();
        }
        self.status = match mode {
            SessionMode::Match => "match session: AI analysis is off by default".into(),
            SessionMode::Record => "record session: start AI analysis manually when needed".into(),
            SessionMode::Live => "live session: continuous AI analysis is enabled".into(),
        };
        if self.analysis_enabled {
            self.start_analysis(cx);
        } else {
            self.request_configured_engine_turn(cx);
            cx.notify();
        }
    }

    /// Stops any running analysis and locks it off for a fair-play remote
    /// competition. Called both when entering OGS matchmaking and when the
    /// server confirms a new remote game, so an engine connected before the
    /// game started can never stream onto the OGS board.
    fn stop_analysis_for_fair_play(&mut self) {
        self.analysis_enabled = false;
        self.restart_analysis_after_position_change = false;
        self.restart_analysis_after_stop = false;
        self.pending_analysis_request = None;
        if self.analysis_task.is_some() {
            self.analysis_run.request_stop();
        }
        self.analysis.clear();
        self.analysis_best_move = None;
    }

    fn enter_ogs_remote_match(&mut self, cx: &mut Context<Self>) {
        // 未登录时先引导登录，不切换远程模式，避免残留 RemoteCompetition
        // 状态导致"新建对局"被误判为"已经在远程对局"。
        if self.ogs_auth_state != ryusei_host::OgsAuthState::Authenticated {
            self.status = "请先在 OGS 账户面板登录后再开始远程匹配".into();
            self.show_toast("请先登录 OGS 账户再发起自动匹配".to_owned(), cx);
            self.open_ogs_account(cx);
            cx.notify();
            return;
        }

        // 已匹配或匹配中则不要重复发起。
        if self.ogs_client.snapshot().matchmaking_status
            == ryusei_host::OgsMatchmakingStatus::Searching
        {
            self.status = "OGS 自动匹配已在寻找对手中…".into();
            self.show_toast("自动匹配已在寻找对手中，请稍候".to_owned(), cx);
            cx.notify();
            return;
        }

        // 切换到远程竞赛模式并锁定公平竞赛。
        self.session_policy =
            SessionPolicy::new(SessionMode::Match, SessionSource::RemoteCompetition)
                .lock_fair_play(true);
        self.stop_analysis_for_fair_play();
        self.status = "OGS 远程对局模式：公平竞赛锁定，正在寻找对手…".into();
        self.synchronize_recovery();
        // 发起 OGS 自动匹配（寻找对手并连接）。
        self.ogs_start_automatch(cx);
        self.show_toast("已进入 OGS 远程对局模式，正在自动匹配对手…".to_owned(), cx);
        cx.notify();
    }

    fn leave_remote_match(&mut self, cx: &mut Context<Self>) {
        // 匹配中退出时，必须取消 OGS 自动匹配，避免对手继续等待/匹配上。
        if self.ogs_client.snapshot().matchmaking_status
            == ryusei_host::OgsMatchmakingStatus::Searching
        {
            self.ogs_cancel_automatch(cx);
        }
        // 清理投影状态，下次匹配新对局时能正确触发投影。
        self.ogs_projected_moves = 0;
        self.ogs_projected_game_id = None;
        // 退出远程对局后进入自由分析模式（与对局结束一致）。
        self.finish_local_game("退出 OGS 远程对局", cx);
    }

    fn set_mode(&mut self, mode: GameMode, cx: &mut Context<Self>) {
        if self.session_policy.source == SessionSource::RemoteCompetition && mode != GameMode::Play
        {
            self.show_toast("OGS 远程对局期间不能切换本地点目或编辑工具".to_owned(), cx);
            return;
        }
        self.mode = mode;
        self.line_start = None;
        if mode == GameMode::Play {
            self.active_tool = MarkupTool::Play;
        }
        self.status = match mode {
            GameMode::Play => "play mode".into(),
            GameMode::Edit => "edit mode: choose a tool".into(),
            GameMode::Scoring => "scoring mode: click stones to cycle dead/alive".into(),
            GameMode::Estimator => {
                "estimator mode: heuristic area estimate (Monte Carlo not wired)".into()
            }
            GameMode::Find => "find mode: not implemented yet".into(),
            GameMode::Guess => "guess mode: not implemented yet".into(),
            GameMode::Autoplay => "autoplay mode: not implemented yet".into(),
        };
        cx.notify();
    }

    /// Toggles the scoring mode: while active, board clicks cycle scoring
    /// overrides instead of placing moves.
    #[allow(dead_code)]
    fn on_scoring_mode_toggle(
        &mut self,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_mode(
            if self.mode == GameMode::Scoring {
                GameMode::Play
            } else {
                GameMode::Scoring
            },
            cx,
        );
    }

    fn on_comment_focus(&mut self, _: &MouseDownEvent, window: &mut Window, _: &mut Context<Self>) {
        window.focus(&self.text_inputs.comment_focus_handle);
        self.active_text_input = Some(ActiveTextInput::Comment);
        let metadata = current_node_metadata(&self.host.snapshot());
        self.text_inputs.comment_input.set_text(metadata.comment);
    }

    #[allow(dead_code)]
    fn on_node_title_focus(
        &mut self,
        _: &MouseDownEvent,
        window: &mut Window,
        _: &mut Context<Self>,
    ) {
        window.focus(&self.text_inputs.node_title_focus_handle);
        self.active_text_input = Some(ActiveTextInput::NodeTitle);
        self.text_inputs.node_title_input.set_text(
            self.host
                .snapshot()
                .nodes
                .iter()
                .find(|node| node.id == self.host.snapshot().current_node_id)
                .and_then(|node| node.properties.get("N"))
                .and_then(|values| values.first())
                .cloned()
                .unwrap_or_default(),
        );
    }

    fn handle_text_input_key(
        input: &mut NativeTextInput,
        event: &gpui::KeyDownEvent,
    ) -> InputKeyResult {
        if event.keystroke.modifiers.secondary() {
            match event.keystroke.key.as_str() {
                "a" => {
                    input.select_all();
                    return InputKeyResult::Changed;
                }
                "z" if event.keystroke.modifiers.shift => {
                    return if input.redo() {
                        InputKeyResult::Changed
                    } else {
                        InputKeyResult::Ignored
                    };
                }
                "z" => {
                    return if input.undo() {
                        InputKeyResult::Changed
                    } else {
                        InputKeyResult::Ignored
                    };
                }
                _ => {}
            }
        }
        // Printable characters are inserted exactly once by the native platform
        // text input bridge (Window::dispatch_input -> replace_text_in_range),
        // which is registered by NativeInputBinding. Inserting them here as well
        // duplicates every keystroke (e.g. typing "f" produces "ff").
        if event.keystroke.key_char.is_some() {
            return InputKeyResult::Ignored;
        }
        input.handle_key(event.keystroke.key.as_str(), None)
    }

    #[allow(dead_code)]
    fn on_node_title_key_down(
        &mut self,
        event: &gpui::KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match Self::handle_text_input_key(&mut self.text_inputs.node_title_input, event) {
            InputKeyResult::Submit => {
                let metadata = current_node_metadata(&self.host.snapshot());
                let title = self.text_inputs.node_title_input.text().trim().to_owned();
                let mut events = RecordingSink;
                match self.host.apply_transaction(
                    crate::node_inspector::create_property_transaction(
                        &metadata.node_id,
                        "N",
                        if !title.is_empty() {
                            vec![title]
                        } else {
                            Default::default()
                        },
                    ),
                    &mut events,
                ) {
                    Ok(_) => {
                        self.text_inputs.node_title_input.set_text("");
                        self.status = "node title saved".into();
                        self.synchronize_recovery();
                    }
                    Err(error) => self.status = format!("node title failed: {error}").into(),
                }
            }
            InputKeyResult::Cancel => self.text_inputs.node_title_input.set_text(
                self.host
                    .snapshot()
                    .nodes
                    .iter()
                    .find(|node| node.id == self.host.snapshot().current_node_id)
                    .and_then(|node| node.properties.get("N"))
                    .and_then(|values| values.first())
                    .cloned()
                    .unwrap_or_default(),
            ),
            InputKeyResult::Changed | InputKeyResult::Ignored => {}
        }
        cx.notify();
    }

    fn on_comment_key_down(
        &mut self,
        event: &gpui::KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match Self::handle_text_input_key(&mut self.text_inputs.comment_input, event) {
            InputKeyResult::Submit => {
                let comment = self.text_inputs.comment_input.text().to_owned();
                self.save_comment(&comment, cx);
                return;
            }
            InputKeyResult::Cancel => {
                self.text_inputs
                    .comment_input
                    .set_text(current_node_metadata(&self.host.snapshot()).comment);
            }
            InputKeyResult::Changed | InputKeyResult::Ignored => {}
        }
        cx.notify();
    }

    fn save_comment(&mut self, comment: &str, cx: &mut Context<Self>) {
        let metadata = current_node_metadata(&self.host.snapshot());
        let transaction = create_comment_transaction(&metadata.node_id, comment);
        let mut events = RecordingSink;
        match self.host.apply_transaction(transaction, &mut events) {
            Ok(_) => {
                self.status = "comment saved".into();
                self.text_inputs.comment_input.set_text("");
                self.synchronize_recovery();
            }
            Err(error) => self.status = format!("comment failed: {error}").into(),
        }
        cx.notify();
    }

    /// Clears every markup, label, line and arrow annotation from the current
    /// node with a single click. The document is only changed when at least one
    /// annotation property exists, so clicking on an already-clean node stays a
    /// no-op (matching the prototype's conditional clear button).
    fn clear_current_node_markups(&mut self, cx: &mut Context<Self>) {
        let metadata = current_node_metadata(&self.host.snapshot());
        let transactions = create_clear_markup_transactions(&metadata.node_id);
        let mut changed = false;
        for transaction in transactions {
            let mut events = RecordingSink;
            match self.host.apply_transaction(transaction, &mut events) {
                Ok(_) => changed = true,
                Err(error) => self.status = format!("clear markups failed: {error}").into(),
            }
        }
        if changed {
            self.status = "markups cleared".into();
            self.synchronize_recovery();
            cx.notify();
        }
    }

    /// Toggles a timed playback loop through the active main line. Each step
    /// advances one node every 850ms and stops cleanly at the branch end. The
    /// loop re-checks navigation availability per step, so inserting variations
    /// or reaching a leaf never leaves a stale timer behind.
    fn toggle_autoplay(&mut self, cx: &mut Context<Self>) {
        if self.autoplay_task.take().is_some() {
            self.status = "autoplay stopped".into();
            cx.notify();
            return;
        }
        self.autoplay_task = Some(cx.spawn(
            move |weak: gpui::WeakEntity<ShellApp>, cx: &mut gpui::AsyncApp| {
                let mut cx = cx.clone();
                async move {
                    loop {
                        cx.background_executor()
                            .timer(Duration::from_millis(850))
                            .await;
                        let should_continue = weak
                            .update(&mut cx, |shell, cx| {
                                let snapshot = shell.host.snapshot();
                                if !navigation_availability(&snapshot).can_go_next {
                                    shell.autoplay_task = None;
                                    shell.status = "autoplay finished".into();
                                    cx.notify();
                                    return false;
                                }
                                shell.navigate(NavigationDirection::Next, cx);
                                play_if_enabled(
                                    &shell.settings,
                                    shell.sound_sink.as_mut(),
                                    SoundCue::StonePlaced,
                                );
                                true
                            })
                            .unwrap_or(false);
                        if !should_continue {
                            break;
                        }
                    }
                }
            },
        ));
        self.status = "autoplay started".into();
        cx.notify();
    }
    /// Starts or stops the animated principal-variation playback. Each visible
    /// step appears on the board every 400ms; the run stops automatically after
    /// the candidate's PV is exhausted. The document is never mutated, matching
    /// the prototype's transient 推演 PV animation.
    fn toggle_pv_animation(&mut self, vertex: String, cx: &mut Context<Self>) {
        if let Some((current, _)) = self.pv_animation.as_ref()
            && current == &vertex
        {
            self.pv_animation_task = None;
            self.pv_animation = None;
            self.status = "PV 推演已停止".into();
            cx.notify();
            return;
        }

        self.pv_animation_task = None;
        self.pv_animation = Some((vertex.clone(), 1));
        self.pv_animation_task = Some(cx.spawn(
            move |weak: gpui::WeakEntity<ShellApp>, cx: &mut gpui::AsyncApp| {
                let mut cx = cx.clone();
                async move {
                    loop {
                        cx.background_executor()
                            .timer(Duration::from_millis(400))
                            .await;
                        let should_continue = weak
                            .update(&mut cx, |shell, cx| {
                                let Some((animation_vertex, step)) = shell.pv_animation.clone()
                                else {
                                    return false;
                                };
                                let total_steps = shell
                                    .analysis
                                    .iter()
                                    .find(|entry| {
                                        entry.vertex.as_deref() == Some(animation_vertex.as_str())
                                    })
                                    .map(|entry| entry.pv.len())
                                    .unwrap_or(0);
                                if step >= total_steps {
                                    shell.pv_animation = None;
                                    shell.pv_animation_task = None;
                                    shell.status = "PV 推演完成".into();
                                    cx.notify();
                                    return false;
                                }
                                shell.pv_animation = Some((animation_vertex, step + 1));
                                play_if_enabled(
                                    &shell.settings,
                                    shell.sound_sink.as_mut(),
                                    SoundCue::StonePlaced,
                                );
                                cx.notify();
                                true
                            })
                            .unwrap_or(false);
                        if !should_continue {
                            break;
                        }
                    }
                }
            },
        ));
        self.status = format!("正在推演 PV: {vertex}").into();
        cx.notify();
    }

    fn set_hovered_candidate(&mut self, vertex: Option<String>, cx: &mut Context<Self>) {
        if self.hovered_candidate_vertex != vertex {
            self.hovered_candidate_vertex = vertex;
            cx.notify();
        }
    }

    fn set_winrate_hover(&mut self, index: Option<usize>, cx: &mut Context<Self>) {
        if self.winrate_hover_index != index {
            self.winrate_hover_index = index;
            cx.notify();
        }
    }

    /// Ignore delayed leave events from a different candidate row. GPUI hover
    /// transitions can overlap while moving from one row into another's action.
    #[allow(dead_code)]
    fn clear_hovered_candidate_if(&mut self, vertex: &str, cx: &mut Context<Self>) {
        if self.hovered_candidate_vertex.as_deref() == Some(vertex) {
            self.set_hovered_candidate(None, cx);
        }
    }

    #[allow(dead_code)]
    fn on_trial_candidate(&mut self, vertex: &str, cx: &mut Context<Self>) {
        let snapshot = self.host.snapshot();
        let Some((column, row)) = parse_gtp_vertex(snapshot.board.width, vertex) else {
            self.status = format!("cannot trial invalid candidate {vertex}").into();
            cx.notify();
            return;
        };
        let trial_move = MoveDto {
            color: snapshot.board.next_player,
            vertex: Some(Vertex { column, row }),
        };
        self.trial_move = Some(trial_move);
        self.hovered_candidate_vertex = None;
        self.analysis.clear();
        self.analysis_best_move = None;
        self.analysis_enabled = true;
        self.status = format!("试下 {vertex}：正在分析 AI 应对…").into();
        if self.analysis_task.is_some() {
            // The existing worker owns the engine. Stop it first, then replay
            // this ephemeral move once the lease has returned.
            self.restart_analysis_after_position_change = true;
            self.analysis_run.request_replay_and_stop();
        } else {
            self.start_analysis(cx);
        }
    }

    #[allow(dead_code)]
    fn clear_trial_move(&mut self, cx: &mut Context<Self>) {
        if self.trial_move.take().is_some() {
            self.active_analysis_trial_move = None;
            self.last_analysis_trial_move = None;
            self.analysis.clear();
            self.analysis_best_move = None;
            if self.analysis_task.is_some() {
                // Invalidate the ephemeral run before restoring the real
                // position, so a late trial batch cannot repopulate it.
                self.restart_analysis_after_position_change = self.analysis_enabled;
                self.analysis_run.request_replay_and_stop();
                self.status = "已退出试下局面，正在恢复当前局面分析…".into();
            } else {
                self.restart_analysis_after_position_change = false;
                self.status = "已退出试下局面".into();
                if self.analysis_enabled {
                    self.start_analysis(cx);
                }
            }
            cx.notify();
        }
    }

    #[allow(dead_code)]
    fn on_branch_candidate_pv(&mut self, pv: &[String], cx: &mut Context<Self>) {
        if pv.is_empty() {
            return;
        }
        let board_size = self.host.snapshot().board.width;
        let mut color = self.host.snapshot().board.next_player;
        let mut played_count = 0;
        let mut events = RecordingSink;

        for move_str in pv {
            if let Some(coords) = crate::engine_console::parse_gtp_vertex(board_size, move_str) {
                let vertex = Vertex {
                    column: coords.0,
                    row: coords.1,
                };
                if self
                    .host
                    .play_move(color, Some(vertex), &mut events)
                    .is_ok()
                {
                    played_count += 1;
                    color = match color {
                        Color::Black => Color::White,
                        Color::White => Color::Black,
                    };
                } else {
                    break;
                }
            }
        }

        if played_count > 0 {
            self.last_vertex = None;
            self.synchronize_recovery();
            self.show_toast(
                format!("🌿 已成功在棋谱树上生成 AI 推荐变化分支 (共 {played_count} 手)"),
                cx,
            );
        }
        cx.notify();
    }

    pub fn set_winrate_metric(&mut self, metric: WinrateGraphMetric, cx: &mut Context<Self>) {
        let val = match metric {
            WinrateGraphMetric::Winrate => "winrate",
            WinrateGraphMetric::ScoreLead => "scorelead",
        };
        let _ = self
            .settings
            .set("board.analysis_type", serde_json::json!(val));
        cx.notify();
    }

    fn export_current_position_png(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.host.snapshot();
        let ownership =
            best_analysis_entry(&self.analysis).and_then(|entry| entry.ownership.clone());
        let options = ryusei_host::PositionPngOptions {
            image_size: 720,
            show_coordinates: self
                .settings
                .get_bool("view.show_coordinates")
                .unwrap_or(true),
            ownership,
        };
        match ryusei_host::export_position_to_png(&snapshot.board, &options) {
            Ok(png_bytes) => {
                let suggested = format!("ryusei-position-{}.png", std::process::id());
                let Some(destination) = self.dialog_service.pick_save_png_path(&suggested) else {
                    self.status = "PNG export cancelled".into();
                    cx.notify();
                    return;
                };
                match self
                    .persistence
                    .persist_png_export(&destination, &png_bytes)
                {
                    Ok(()) => {
                        self.status =
                            format!("current position PNG exported: {}", destination.display())
                                .into();
                        self.show_toast(
                            format!("已导出当前局面 PNG: {}", destination.display()),
                            cx,
                        );
                    }
                    Err(error) => {
                        self.status = format!("PNG export failed: {error}").into();
                        self.show_toast(self.status.clone(), cx);
                    }
                }
            }
            Err(error) => {
                self.status = format!("PNG export failed: {error}").into();
                self.show_toast(self.status.clone(), cx);
            }
        }
        cx.notify();
    }

    fn on_export_gif_action(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let snapshot = self.host.snapshot();
        let options = ryusei_host::GifExportOptions::default();
        match ryusei_host::export_sgf_to_gif(&snapshot, &options) {
            Ok(gif_bytes) => {
                let suggested = format!("saba_game_{}.gif", std::process::id());
                let dest = self
                    .dialog_service
                    .pick_save_gif_path(&suggested)
                    .unwrap_or_else(|| std::env::temp_dir().join(&suggested));
                match self.persistence.persist_gif_export(&dest, &gif_bytes) {
                    Ok(()) => {
                        self.show_toast(
                            format!("🎬 成功导出 GIF 动画棋谱: {}", dest.display()),
                            cx,
                        );
                    }
                    Err(error) => {
                        self.show_toast(format!("GIF 保存失败: {error}"), cx);
                    }
                }
            }
            Err(e) => {
                self.show_toast(format!("GIF 导出失败: {e}"), cx);
            }
        }
        cx.notify();
    }

    fn open_drawer(&mut self, drawer: ActiveDrawer, status: &str, cx: &mut Context<Self>) {
        self.active_drawer = Some(drawer);
        self.status = status.to_owned().into();
        cx.notify();
    }

    fn open_library(&mut self, cx: &mut Context<Self>) {
        self.refresh_library_form();
        self.open_drawer(ActiveDrawer::Library, "棋谱库已打开", cx);
        self.refresh_library_entries(cx);
    }

    fn refresh_library_entries(&mut self, cx: &mut Context<Self>) {
        if self.library_task.is_some() || self.library_sources.is_empty() {
            return;
        }
        let sources = self.library_sources.clone();
        let base = match crate::file_workflow::current_user_config_directory() {
            Ok(base) => base.join("libraries"),
            Err(error) => {
                self.library_status = format!("无法确定数据目录：{error}").into();
                return;
            }
        };
        self.library_status = "正在读取本地棋谱索引…".into();
        let weak = cx.entity().downgrade();
        self.library_task = Some(cx.spawn(
            move |_: gpui::WeakEntity<ShellApp>, cx: &mut gpui::AsyncApp| {
                let mut cx = cx.clone();
                async move {
                    let result = cx
                        .background_executor()
                        .spawn(async move {
                            let mut entries = Vec::new();
                            for source in sources {
                                entries.extend(
                                    ryusei_host::scan_sgf_library(
                                        &source.id,
                                        &base.join(&source.id),
                                    )
                                    .map_err(|error| error.to_string())?,
                                );
                            }
                            Ok::<_, String>(entries)
                        })
                        .await;
                    let _ = weak.update(&mut cx, |shell, cx| {
                        shell.library_task = None;
                        match result {
                            Ok(entries) => {
                                shell.library_entries = entries;
                                shell.library_status =
                                    format!("已载入 {} 个本地 SGF", shell.library_entries.len())
                                        .into();
                            }
                            Err(error) => {
                                shell.library_status = format!("索引读取失败：{error}").into();
                            }
                        }
                        cx.notify();
                    });
                }
            },
        ));
    }

    fn refresh_library_form(&mut self) {
        let source = self
            .library_selected_source
            .as_deref()
            .and_then(|selected| {
                self.library_sources
                    .iter()
                    .find(|source| source.id == selected)
            })
            .or_else(|| self.library_sources.first());
        self.text_inputs.library_id_input.set_text(
            source
                .map(|source| source.id.as_str())
                .unwrap_or("local-sgf"),
        );
        self.text_inputs.library_name_input.set_text(
            source
                .map(|source| source.name.as_str())
                .unwrap_or("授权 SGF 棋谱库"),
        );
        self.text_inputs.library_github_url_input.set_text(
            source
                .map(|source| source.github_url.as_str())
                .unwrap_or(""),
        );
        self.text_inputs.library_reference_input.set_text(
            source
                .map(|source| source.reference.as_str())
                .unwrap_or("main"),
        );
        self.text_inputs.library_license_name_input.set_text(
            source
                .and_then(|source| source.license_name.as_deref())
                .unwrap_or(""),
        );
        self.text_inputs.library_license_url_input.set_text(
            source
                .and_then(|source| source.license_url.as_deref())
                .unwrap_or(""),
        );
        self.library_rights_confirmed = source
            .is_some_and(|source| source.rights == ryusei_host::RedistributionRights::Permitted);
    }

    fn toggle_library_rights(&mut self, checked: bool, cx: &mut Context<Self>) {
        self.library_rights_confirmed = checked;
        cx.notify();
    }

    fn select_library_source(&mut self, source_id: &str, cx: &mut Context<Self>) {
        if self
            .library_sources
            .iter()
            .any(|source| source.id == source_id)
        {
            self.library_selected_source = Some(source_id.to_owned());
            self.refresh_library_form();
            self.library_status = format!("正在编辑来源 {source_id}").into();
            cx.notify();
        }
    }

    fn new_library_source(&mut self, cx: &mut Context<Self>) {
        self.library_selected_source = None;
        self.text_inputs.library_id_input.set_text("");
        self.text_inputs.library_name_input.set_text("");
        self.text_inputs.library_github_url_input.set_text("");
        self.text_inputs.library_reference_input.set_text("main");
        self.text_inputs.library_license_name_input.set_text("");
        self.text_inputs.library_license_url_input.set_text("");
        self.library_rights_confirmed = false;
        self.library_status = "填写新来源并确认再分发权".into();
        cx.notify();
    }

    fn remove_selected_library_source(&mut self, cx: &mut Context<Self>) {
        let Some(source_id) = self.library_selected_source.clone() else {
            return;
        };
        self.library_sources.retain(|source| source.id != source_id);
        self.library_entries
            .retain(|entry| entry.source_id != source_id);
        self.library_selected_source = self.library_sources.first().map(|source| source.id.clone());
        let serialized = serde_json::to_string(&self.library_sources).unwrap_or_default();
        let result = self
            .settings
            .set("library.sources", serialized.into())
            .map_err(|error| error.to_string())
            .and_then(|_| {
                ryusei_host::persist_settings_store(&self.settings, &mut self.settings_persistence)
            });
        match result {
            Ok(()) => self.library_status = format!("已移除来源配置 {source_id}").into(),
            Err(error) => self.library_status = format!("来源移除未持久化：{error}").into(),
        }
        self.refresh_library_form();
        cx.notify();
    }

    fn library_source_from_form(&self) -> Result<ryusei_host::SgfLibrarySource, String> {
        let source = ryusei_host::SgfLibrarySource {
            id: self.text_inputs.library_id_input.text().trim().to_owned(),
            name: self.text_inputs.library_name_input.text().trim().to_owned(),
            github_url: self
                .text_inputs
                .library_github_url_input
                .text()
                .trim()
                .to_owned(),
            reference: self
                .text_inputs
                .library_reference_input
                .text()
                .trim()
                .to_owned(),
            rights: if self.library_rights_confirmed {
                ryusei_host::RedistributionRights::Permitted
            } else {
                ryusei_host::RedistributionRights::Unknown
            },
            license_name: Some(
                self.text_inputs
                    .library_license_name_input
                    .text()
                    .trim()
                    .to_owned(),
            ),
            license_url: Some(
                self.text_inputs
                    .library_license_url_input
                    .text()
                    .trim()
                    .to_owned(),
            ),
        };
        source
            .validate_for_sync()
            .map_err(|error| error.to_string())?;
        Ok(source)
    }

    fn persist_library_source(
        &mut self,
        source: &ryusei_host::SgfLibrarySource,
    ) -> Result<(), String> {
        if let Some(selected) = self.library_selected_source.as_deref()
            && selected != source.id
        {
            self.library_sources
                .retain(|existing| existing.id != selected);
            self.library_entries
                .retain(|entry| entry.source_id != selected);
        }
        if let Some(existing) = self
            .library_sources
            .iter_mut()
            .find(|existing| existing.id == source.id)
        {
            *existing = source.clone();
        } else {
            self.library_sources.push(source.clone());
        }
        self.library_selected_source = Some(source.id.clone());
        self.settings
            .set(
                "library.sources",
                serde_json::to_string(&self.library_sources)
                    .unwrap_or_default()
                    .into(),
            )
            .map_err(|error| error.to_string())?;
        ryusei_host::persist_settings_store(&self.settings, &mut self.settings_persistence)
    }

    fn sync_library(&mut self, cx: &mut Context<Self>) {
        if self.library_task.is_some() {
            return;
        }
        let source = match self.library_source_from_form() {
            Ok(source) => source,
            Err(error) => {
                self.library_status = format!("配置无效：{error}").into();
                self.show_toast(format!("棋谱库配置无效：{error}"), cx);
                cx.notify();
                return;
            }
        };
        if let Err(error) = self.persist_library_source(&source) {
            self.library_status = format!("配置保存失败：{error}").into();
            self.show_toast(format!("棋谱库配置保存失败：{error}"), cx);
            cx.notify();
            return;
        }
        let base = match crate::file_workflow::current_user_config_directory() {
            Ok(base) => base,
            Err(error) => {
                self.library_status = format!("无法确定数据目录：{error}").into();
                cx.notify();
                return;
            }
        };
        let destination = base.join("libraries").join(&source.id);
        let source_id = source.id.clone();
        self.library_syncing_source = Some(source.id.clone());
        self.library_status = "正在同步 Git 棋谱库…".into();
        let weak = cx.entity().downgrade();
        self.library_task = Some(cx.spawn(
            move |_: gpui::WeakEntity<ShellApp>, cx: &mut gpui::AsyncApp| {
                let mut cx = cx.clone();
                async move {
                    let result = cx
                        .background_executor()
                        .spawn(async move {
                            let mut adapter = ryusei_host::ProcessGitSyncAdapter;
                            let report =
                                ryusei_host::sync_sgf_library(&source, &destination, &mut adapter)
                                    .map_err(|error| error.to_string())?;
                            let entries = ryusei_host::scan_sgf_library(&source_id, &destination)
                                .map_err(|error| error.to_string())?;
                            Ok::<_, String>((report, entries))
                        })
                        .await;
                    let _ = weak.update(&mut cx, |shell, cx| {
                        shell.library_task = None;
                        shell.library_syncing_source = None;
                        match result {
                            Ok((report, entries)) => {
                                shell.library_entries = entries;
                                shell.library_status = format!(
                                    "同步完成：{} 个 SGF（{:?}）",
                                    shell.library_entries.len(),
                                    report.operation
                                )
                                .into();
                                shell.show_toast("授权 SGF 棋谱库同步完成".to_owned(), cx);
                            }
                            Err(error) => {
                                shell.library_status = format!("同步失败：{error}").into();
                                shell.show_toast(format!("棋谱库同步失败：{error}"), cx);
                            }
                        }
                        cx.notify();
                    });
                }
            },
        ));
        cx.notify();
    }

    fn open_recent_file(&mut self, identifier: &str, cx: &mut Context<Self>) {
        let Some(path) = self.recent_files.resolve_path(identifier) else {
            self.status = "最近文件记录已失效".into();
            cx.notify();
            return;
        };
        if !path.is_file() {
            self.status = format!("文件不存在: {}", path.display()).into();
            cx.notify();
            return;
        }
        self.open_record_path(path, SessionSource::Local, "最近棋谱", cx);
    }

    fn open_library_entry(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.open_record_path(path, SessionSource::Library, "棋谱库", cx);
    }

    fn open_record_path(
        &mut self,
        path: PathBuf,
        source: SessionSource,
        label: &str,
        cx: &mut Context<Self>,
    ) {
        let mut events = RecordingSink;
        match self.host.open(path.clone(), &self.file_access, &mut events) {
            Ok(_) => {
                self.disconnect_all_engine_sessions();
                self.stop_ogs_public_poll();
                self.live_source_url = None;
                self.live_ogs_state = None;
                self.session_policy = SessionPolicy::new(SessionMode::Record, source);
                self.status = format!("已打开{label}：{}", path.display()).into();
                self.record_recent(&path);
                if let Err(error) = track_after_file_operation(&mut self.external_file, &path) {
                    self.status = format!("外部文件跟踪失败：{error}").into();
                }
                self.synchronize_recovery();
                self.active_drawer = None;
            }
            Err(error) => self.show_toast(format!("棋谱库棋谱打开失败：{error}"), cx),
        }
        cx.notify();
    }

    fn open_live_capture(&mut self, cx: &mut Context<Self>) {
        self.open_drawer(ActiveDrawer::LiveCapture, "公共直播导入已打开", cx);
    }

    fn open_ogs_account(&mut self, cx: &mut Context<Self>) {
        self.refresh_ogs_account_state(cx);
        self.open_drawer(ActiveDrawer::OgsAccount, "OGS 账户已打开", cx);
    }

    /// Mirrors the OGS client snapshot into `ogs_auth_state` so the toolbar
    /// label stays correct without the shell owning a second source of truth.
    fn refresh_ogs_account_state(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.ogs_client.snapshot();
        self.ogs_auth_state = if snapshot.user.is_some() {
            ryusei_host::OgsAuthState::Authenticated
        } else {
            ryusei_host::OgsAuthState::SignedOut
        };
        // 对手取消 / 匹配超时：服务器把匹配状态重置为 Idle，且无进行中对局。
        if self.ogs_was_searching
            && snapshot.matchmaking_status == ryusei_host::OgsMatchmakingStatus::Idle
            && snapshot.online_game.is_none()
        {
            self.ogs_was_searching = false;
            self.finish_local_game("OGS 自动匹配已取消", cx);
            return;
        }
        if let Some(game) = snapshot.online_game.as_ref() {
            // Project whenever a new game becomes connected, even when its move
            // list is still empty: matching a fresh opponent must switch the
            // board to the new (empty) game instead of leaving the old board.
            let new_game = self.ogs_projected_game_id != Some(game.game_id);
            let new_moves = game.moves.len() as u32 != self.ogs_projected_moves;
            if game.connected
                && let (Some(control), Some(server_clock)) = (game.time_control, game.clock)
            {
                // OGS is authoritative for remote clocks. Replace the local
                // prediction on every realtime update, including clock-only
                // frames between moves.
                self.clock
                    .apply_remote_clock(server_clock.to_clock_state(control));
                self.clock_last_updated = Instant::now();
            }
            if game.connected && (new_game || new_moves) {
                self.project_ogs_server_moves(game);
                if new_game {
                    self.session_policy =
                        SessionPolicy::new(SessionMode::Match, SessionSource::RemoteCompetition)
                            .lock_fair_play(true);
                    // Defense in depth: even when matchmaking started through a
                    // path that skipped the guarded entry, a live analysis
                    // stream must not survive onto the remote board.
                    self.stop_analysis_for_fair_play();
                    self.mode = GameMode::Play;
                    self.active_tool = MarkupTool::Play;
                    let move_desc = if game.move_number == 0 {
                        "开局".to_owned()
                    } else {
                        format!("第 {} 手", game.move_number)
                    };
                    self.status = format!(
                        "OGS 对局已开始：{} vs {}（{move_desc}）",
                        game.black_name, game.white_name
                    )
                    .into();
                    self.show_toast(self.status.to_string(), cx);
                } else if new_moves {
                    // 服务器确认的新一手（自己或对手）落盘：与本地落子一致，
                    // 落子播 StonePlaced、停一手播 Pass，让远程对局也有音效反馈。
                    self.play_sound_if_enabled(if game.last_move.as_deref() == Some("..") {
                        SoundCue::Pass
                    } else {
                        SoundCue::StonePlaced
                    });
                    // 每手棋投影后，按选项做 80v 后台复盘（OGS 对局期间不上屏
                    // 只持久化，对局结束后胜率图自动呈现）。
                    self.maybe_background_review_current_position(cx);
                }
            }
            if game.last_move.as_deref() == Some("..")
                && !game.last_move_was_ours
                && game.move_number != self.ogs_last_pass_notified_move
            {
                self.ogs_last_pass_notified_move = game.move_number;
                self.status = format!("对手第 {} 手停一手（pass）", game.move_number).into();
                self.show_toast(self.status.clone(), cx);
            }
            // OGS 对局结束（服务器把 phase 切到 finished）：投影最终局面并进入
            // 自由分析模式，与本地对局结束行为一致。
            if game.phase == "finished"
                && self.session_policy.source == SessionSource::RemoteCompetition
            {
                self.project_ogs_server_moves(game);
                self.finish_local_game(
                    &format!("OGS 对局结束：{} vs {}", game.black_name, game.white_name),
                    cx,
                );
            }
        }
    }

    /// Rebuilds the board as a read-only projection of the server-confirmed
    /// move list. This is separate from local editing: the local document is
    /// replaced, never marked editable, and analysis stays fair-play locked.
    fn project_ogs_server_moves(&mut self, game: &ryusei_host::OgsOnlineGame) {
        let width = if game.width > 0 { game.width } else { 19 };
        let height = if game.height > 0 { game.height } else { 19 };
        // Rectangular boards use `SZ[width:height]`; square boards use `SZ[n]`.
        let size = if height != width {
            format!("{width}:{height}")
        } else {
            format!("{width}")
        };
        let mut sgf = format!(
            "(;GM[1]FF[4]SZ[{size}]PB[{}]PW[{}]",
            game.black_name, game.white_name
        );
        // Handicap games start with white to move and carry setup stones.
        if let Some(handicap) = game.handicap.filter(|h| *h > 0) {
            sgf.push_str(&format!("HA[{handicap}]"));
        }
        if let Some(rules) = game.rules.as_deref() {
            sgf.push_str(&format!("RU[{rules}]"));
        }
        if let Some(komi) = game.komi {
            sgf.push_str(&format!("KM[{komi}]"));
        }
        if let Some(time_control) = game.time_control
            && let Some((main_time, overtime)) = time_control.to_sgf()
        {
            sgf.push_str(&format!("TM[{main_time}]OT[{overtime}]"));
        }
        if game.initial_player == "white" {
            sgf.push_str("PL[W]");
        }
        for coord in &game.initial_black {
            if let Some(vertex) = crate::goban_view::parse_sgf_vertex(coord) {
                sgf.push_str(&format!(
                    "AB[{}]",
                    crate::goban_view::format_sgf_vertex(vertex)
                ));
            }
        }
        for coord in &game.initial_white {
            if let Some(vertex) = crate::goban_view::parse_sgf_vertex(coord) {
                sgf.push_str(&format!(
                    "AW[{}]",
                    crate::goban_view::format_sgf_vertex(vertex)
                ));
            }
        }
        let mut color_is_black = game.initial_player != "white";
        for coord in &game.moves {
            if coord == ".." {
                sgf.push_str(if color_is_black { ";B[]" } else { ";W[]" });
            } else if let Some(vertex) = crate::goban_view::parse_sgf_vertex(coord) {
                let point = crate::goban_view::format_sgf_vertex(vertex);
                sgf.push_str(if color_is_black { ";B[" } else { ";W[" });
                sgf.push_str(&point);
                sgf.push(']');
            } else {
                continue;
            }
            color_is_black = !color_is_black;
        }
        sgf.push(')');
        let mut events = RecordingSink;
        if self.host.restore_from_sgf(&sgf, &mut events).is_ok() {
            self.ogs_projected_moves = game.moves.len() as u32;
            self.ogs_projected_game_id = Some(game.game_id);
        }
    }

    fn ogs_pass(&mut self, cx: &mut Context<Self>) {
        let Some(game_id) = self.ogs_client.competition_game_id() else {
            self.show_toast("尚未连接 OGS 对局".to_owned(), cx);
            return;
        };
        let client = Arc::clone(&self.ogs_client);
        let weak = cx.entity().downgrade();
        cx.spawn(
            move |_: gpui::WeakEntity<ShellApp>, cx: &mut gpui::AsyncApp| {
                let mut cx = cx.clone();
                async move {
                    let result = cx
                        .background_executor()
                        .spawn(async move { client.pass(game_id) })
                        .await;
                    weak.update(&mut cx, |shell, cx| {
                        if let Err(error) = result {
                            shell.show_toast(format!("停一手失败：{error}"), cx);
                        }
                        cx.notify();
                    })
                    .ok();
                }
            },
        )
        .detach();
        cx.notify();
    }

    fn ogs_resign(&mut self, cx: &mut Context<Self>) {
        let Some(game_id) = self.ogs_client.competition_game_id() else {
            self.show_toast("尚未连接 OGS 对局".to_owned(), cx);
            return;
        };
        let client = Arc::clone(&self.ogs_client);
        let weak = cx.entity().downgrade();
        cx.spawn(
            move |_: gpui::WeakEntity<ShellApp>, cx: &mut gpui::AsyncApp| {
                let mut cx = cx.clone();
                async move {
                    let result = cx
                        .background_executor()
                        .spawn(async move { client.resign(game_id) })
                        .await;
                    weak.update(&mut cx, |shell, cx| {
                        if let Err(error) = result {
                            shell.show_toast(format!("认输失败：{error}"), cx);
                        }
                        cx.notify();
                    })
                    .ok();
                }
            },
        )
        .detach();
        cx.notify();
    }

    /// Submits a board click to the connected OGS game instead of playing
    /// locally. The server confirms the move before it is projected.
    fn ogs_submit_move_at(&mut self, vertex: Vertex, cx: &mut Context<Self>) {
        let Some(game_id) = self.ogs_client.competition_game_id() else {
            self.show_toast("尚未连接 OGS 对局".to_owned(), cx);
            return;
        };
        let coordinate = crate::goban_view::format_sgf_vertex(vertex);
        let client = Arc::clone(&self.ogs_client);
        let weak = cx.entity().downgrade();
        self.status = format!("正在提交落子 {coordinate}…").into();
        cx.spawn(
            move |_: gpui::WeakEntity<ShellApp>, cx: &mut gpui::AsyncApp| {
                let mut cx = cx.clone();
                async move {
                    let result = cx
                        .background_executor()
                        .spawn(async move { client.play_move(game_id, Some(coordinate)) })
                        .await;
                    weak.update(&mut cx, |shell, cx| {
                        if let Err(error) = result {
                            shell.show_toast(format!("提交落子失败：{error}"), cx);
                        }
                        cx.notify();
                    })
                    .ok();
                }
            },
        )
        .detach();
        cx.notify();
    }

    fn ogs_send_chat(&mut self, cx: &mut Context<Self>) {
        let body = self.text_inputs.ogs_chat_input.text().trim().to_owned();
        if body.is_empty() {
            return;
        }
        let Some(game) = self.ogs_client.snapshot().online_game else {
            self.show_toast("尚未连接 OGS 对局".to_owned(), cx);
            return;
        };
        let client = Arc::clone(&self.ogs_client);
        let game_id = game.game_id;
        let move_number = game.move_number;
        self.text_inputs.ogs_chat_input.set_text("");
        cx.spawn(
            move |_: gpui::WeakEntity<ShellApp>, cx: &mut gpui::AsyncApp| {
                let cx = cx.clone();
                async move {
                    let _ = cx
                        .background_executor()
                        .spawn(async move { client.send_chat(game_id, move_number, &body) })
                        .await;
                }
            },
        )
        .detach();
        cx.notify();
    }

    fn ogs_removed_stones_string(&self) -> String {
        self.ogs_removed_stones.iter().cloned().collect::<String>()
    }

    /// Toggles one board stone as dead and sends the updated marking.
    fn ogs_toggle_dead_stone(&mut self, vertex: Vertex, cx: &mut Context<Self>) {
        let Some(game_id) = self.ogs_client.competition_game_id() else {
            self.show_toast("尚未连接 OGS 对局".to_owned(), cx);
            return;
        };
        let coordinate = crate::goban_view::format_sgf_vertex(vertex);
        let now_removed = if self.ogs_removed_stones.remove(&coordinate) {
            false
        } else {
            self.ogs_removed_stones.insert(coordinate.clone());
            true
        };
        let stones = self.ogs_removed_stones_string();
        let client = Arc::clone(&self.ogs_client);
        self.status = format!(
            "死子标记：{coordinate} {}（共 {} 子）",
            if now_removed {
                "标记为死"
            } else {
                "取消标记"
            },
            self.ogs_removed_stones.len()
        )
        .into();
        cx.spawn(
            move |_: gpui::WeakEntity<ShellApp>, cx: &mut gpui::AsyncApp| {
                let cx = cx.clone();
                async move {
                    let _ = cx
                        .background_executor()
                        .spawn(
                            async move { client.set_removed_stones(game_id, &stones, now_removed) },
                        )
                        .await;
                }
            },
        )
        .detach();
        cx.notify();
    }

    fn ogs_toggle_dead_marking(&mut self, cx: &mut Context<Self>) {
        self.ogs_marking_dead = !self.ogs_marking_dead;
        if !self.ogs_marking_dead {
            self.ogs_removed_stones.clear();
        }
        self.status = if self.ogs_marking_dead {
            "死子标记模式：点击棋盘切换死子".into()
        } else {
            "已退出死子标记模式".into()
        };
        self.show_toast(self.status.clone(), cx);
        cx.notify();
    }

    fn ogs_clear_dead_marking(&mut self, cx: &mut Context<Self>) {
        let Some(game_id) = self.ogs_client.competition_game_id() else {
            self.show_toast("尚未连接 OGS 对局".to_owned(), cx);
            return;
        };
        let stones = self.ogs_removed_stones_string();
        self.ogs_removed_stones.clear();
        let client = Arc::clone(&self.ogs_client);
        self.status = "已清空死子标记".into();
        cx.spawn(
            move |_: gpui::WeakEntity<ShellApp>, cx: &mut gpui::AsyncApp| {
                let cx = cx.clone();
                async move {
                    let _ = cx
                        .background_executor()
                        .spawn(async move { client.set_removed_stones(game_id, &stones, false) })
                        .await;
                }
            },
        )
        .detach();
        cx.notify();
    }

    fn ogs_accept_removed_stones(&mut self, cx: &mut Context<Self>) {
        let Some(game_id) = self.ogs_client.competition_game_id() else {
            self.show_toast("尚未连接 OGS 对局".to_owned(), cx);
            return;
        };
        let stones = self.ogs_removed_stones_string();
        let client = Arc::clone(&self.ogs_client);
        self.ogs_marking_dead = false;
        cx.spawn(
            move |_: gpui::WeakEntity<ShellApp>, cx: &mut gpui::AsyncApp| {
                let cx = cx.clone();
                async move {
                    let _ = cx
                        .background_executor()
                        .spawn(async move { client.accept_removed_stones(game_id, &stones) })
                        .await;
                }
            },
        )
        .detach();
        cx.notify();
    }

    fn ogs_start_automatch(&mut self, cx: &mut Context<Self>) {
        self.ogs_was_searching = true;
        let client = Arc::clone(&self.ogs_client);
        let weak = cx.entity().downgrade();
        cx.spawn(
            move |_: gpui::WeakEntity<ShellApp>, cx: &mut gpui::AsyncApp| {
                let mut cx = cx.clone();
                async move {
                    let result = cx
                        .background_executor()
                        .spawn(async move {
                            client.start_automatch(&serde_json::json!({
                                "size_speed_options": [
                                    {"size": "19x19", "speed": "live", "system": "byoyomi"}
                                ],
                                "lower_rank_diff": 3,
                                "upper_rank_diff": 3,
                                "rules": {"condition": "preferred", "value": "japanese"},
                                "handicap": {"condition": "preferred", "value": "disabled"},
                            }))
                        })
                        .await;
                    weak.update(&mut cx, |shell, cx| {
                        if let Err(error) = result {
                            shell.show_toast(format!("自动匹配失败：{error}"), cx);
                        }
                        cx.notify();
                    })
                    .ok();
                }
            },
        )
        .detach();
        cx.notify();
    }

    fn ogs_cancel_automatch(&mut self, cx: &mut Context<Self>) {
        self.ogs_was_searching = false;
        let client = Arc::clone(&self.ogs_client);
        cx.spawn(
            move |_: gpui::WeakEntity<ShellApp>, cx: &mut gpui::AsyncApp| {
                let cx = cx.clone();
                async move {
                    let _ = cx
                        .background_executor()
                        .spawn(async move { client.cancel_automatch() })
                        .await;
                }
            },
        )
        .detach();
        cx.notify();
    }

    /// App-internal OGS login. The password is consumed by the REST call on a
    /// background executor and never persisted.
    fn ogs_login(&mut self, cx: &mut Context<Self>) {
        if self.ogs_login_in_progress {
            return;
        }
        let username = self.text_inputs.ogs_username_input.text().trim().to_owned();
        let password = self.text_inputs.ogs_password_input.text().to_owned();
        if username.is_empty() || password.is_empty() {
            self.show_toast("请输入 OGS 用户名和密码".to_owned(), cx);
            return;
        }
        self.ogs_login_in_progress = true;
        self.status = "正在登录 OGS…".into();
        let client = Arc::clone(&self.ogs_client);
        let weak = cx.entity().downgrade();
        cx.spawn(
            move |_: gpui::WeakEntity<ShellApp>, cx: &mut gpui::AsyncApp| {
                let mut cx = cx.clone();
                async move {
                    let result = cx
                        .background_executor()
                        .spawn(async move { client.login(&username, &password) })
                        .await;
                    weak.update(&mut cx, |shell, cx| {
                        shell.ogs_login_in_progress = false;
                        shell.text_inputs.ogs_password_input.set_text("");
                        match result {
                            Ok(user) => {
                                shell.ogs_auth_state = ryusei_host::OgsAuthState::Authenticated;
                                let name = user
                                    .get("username")
                                    .and_then(serde_json::Value::as_str)
                                    .unwrap_or("OGS user");
                                let persistence_warning = shell.ogs_client.snapshot().last_error;
                                shell.status = match persistence_warning {
                                    Some(warning) => format!("OGS 已登录：{name}。{warning}"),
                                    None => format!("OGS 已登录：{name}"),
                                }
                                .into();
                                shell.show_toast(shell.status.clone(), cx);
                            }
                            Err(error) => {
                                shell.ogs_auth_state = ryusei_host::OgsAuthState::SignedOut;
                                shell.status = format!("OGS 登录失败：{error}").into();
                                shell.show_toast(shell.status.clone(), cx);
                            }
                        }
                        cx.notify();
                    })
                    .ok();
                }
            },
        )
        .detach();
        cx.notify();
    }

    fn ogs_logout(&mut self, cx: &mut Context<Self>) {
        let client = Arc::clone(&self.ogs_client);
        let weak = cx.entity().downgrade();
        cx.spawn(
            move |_: gpui::WeakEntity<ShellApp>, cx: &mut gpui::AsyncApp| {
                let mut cx = cx.clone();
                async move {
                    // Revoke the server session first (it reads the cookie that
                    // `logout` clears), then tear down local state.
                    let _ = cx
                        .background_executor()
                        .spawn(async move {
                            let _ = client.revoke_server_session();
                            client.logout();
                        })
                        .await;
                    weak.update(&mut cx, |shell, cx| {
                        shell.ogs_auth_state = ryusei_host::OgsAuthState::SignedOut;
                        shell.status = "OGS 已登出".into();
                        shell.show_toast(shell.status.clone(), cx);
                        cx.notify();
                    })
                    .ok();
                }
            },
        )
        .detach();
        cx.notify();
    }

    /// Connects to an OGS game and switches the board into fair-play-locked
    /// remote competition mode.
    fn connect_ogs_game(&mut self, cx: &mut Context<Self>) {
        let text = self.text_inputs.ogs_game_id_input.text().trim().to_owned();
        let Ok(game_id) = text.parse::<u64>() else {
            self.show_toast("请输入有效的 OGS 对局 ID".to_owned(), cx);
            return;
        };
        if self.ogs_client.snapshot().user.is_none() {
            self.show_toast("请先登录 OGS".to_owned(), cx);
            return;
        }
        // The remote projection replaces the local document, so refuse to
        // connect while there are unsaved local changes that would be lost.
        if self.host.snapshot().file_state.is_dirty {
            self.show_toast(
                "当前棋谱有未保存的修改，请先保存或放弃后再连接 OGS 对局".to_owned(),
                cx,
            );
            return;
        }
        let client = Arc::clone(&self.ogs_client);
        let weak = cx.entity().downgrade();
        cx.spawn(
            move |_: gpui::WeakEntity<ShellApp>, cx: &mut gpui::AsyncApp| {
                let mut cx = cx.clone();
                async move {
                    let result = cx
                        .background_executor()
                        .spawn(async move { client.connect_game(game_id) })
                        .await;
                    weak.update(&mut cx, |shell, cx| {
                        match result {
                            Ok(()) => {
                                shell.enter_ogs_remote_match(cx);
                                shell.status = format!("正在连接 OGS 对局 #{game_id}…").into();
                            }
                            Err(error) => {
                                shell.show_toast(format!("连接 OGS 对局失败：{error}"), cx);
                            }
                        }
                        cx.notify();
                    })
                    .ok();
                }
            },
        )
        .detach();
        cx.notify();
    }

    fn open_profile(&mut self, cx: &mut Context<Self>) {
        self.open_drawer(ActiveDrawer::Profile, "Profile 已打开", cx);
    }

    #[allow(dead_code)]
    fn open_goals(&mut self, cx: &mut Context<Self>) {
        self.open_drawer(ActiveDrawer::Goals, "目标与计划已打开", cx);
    }

    fn open_preferences(&mut self, cx: &mut Context<Self>) {
        self.open_drawer(ActiveDrawer::Preferences, "设置已打开", cx);
    }

    fn open_review(&mut self, cx: &mut Context<Self>) {
        self.open_drawer(ActiveDrawer::Review, "全谱 AI 复盘", cx);
    }

    fn open_export(&mut self, cx: &mut Context<Self>) {
        self.open_drawer(ActiveDrawer::Export, "导出与分享棋谱", cx);
    }

    fn set_goal_from_active_session(&mut self, cx: &mut Context<Self>) {
        let title = self.workspace_tabs.active_tab().title.clone();
        self.apply_settings_edit(SettingEdit::Set {
            key: "profile.current_goal".to_owned(),
            value: serde_json::Value::String(format!("完成 {title}")),
        });
        self.apply_settings_edit(SettingEdit::Set {
            key: "profile.current_plan".to_owned(),
            value: serde_json::Value::String(format!("打谱、复盘并保存 {title}")),
        });
        self.status = format!("已将 {title} 设为当前目标").into();
        cx.notify();
    }

    fn complete_current_plan(&mut self, cx: &mut Context<Self>) {
        let plan = self
            .settings
            .get_str("profile.current_plan")
            .filter(|plan| !plan.trim().is_empty())
            .unwrap_or("当前计划")
            .to_owned();
        self.apply_settings_edit(SettingEdit::Set {
            key: "profile.current_plan".to_owned(),
            value: serde_json::Value::String(format!("已完成: {plan}")),
        });
        self.status = "计划已标记为完成".into();
        cx.notify();
    }

    /// Opens the plugin pinning manager from the player bar hamburger menu.
    fn open_side_menu(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.toggle_plugin_popover("all", cx);
    }

    pub fn is_bottom_deck_open(&self) -> bool {
        self.gtp_terminal_open || self.active_plugin_popover.is_some()
    }

    pub fn active_bottom_tab(&self) -> BottomDeckTab {
        if self.gtp_terminal_open {
            BottomDeckTab::GtpTerminal
        } else if let Some(target) = self.active_plugin_popover.as_deref() {
            match target {
                "winrate-graph" => BottomDeckTab::WinrateGraph,
                "variation-tree" => BottomDeckTab::VariationTree,
                "all" => BottomDeckTab::PluginManager,
                // Built-in engine/tool panels moved to the left engine sidebar;
                // they no longer resolve to a deck tab, so stale popover ids
                // fall through to the winrate graph instead of double-rendering.
                other if Self::is_engine_sidebar_popover(other) => BottomDeckTab::WinrateGraph,
                other => BottomDeckTab::Generic(other.to_owned()),
            }
        } else {
            BottomDeckTab::WinrateGraph
        }
    }

    /// Ids that used to open a deck tab but now live in the left engine
    /// sidebar's "引擎与工具" section.
    fn is_engine_sidebar_popover(id: &str) -> bool {
        Self::engine_sidebar_panel_for(id).is_some()
    }

    pub fn switch_bottom_tab(&mut self, tab: BottomDeckTab, cx: &mut Context<Self>) {
        match tab {
            BottomDeckTab::WinrateGraph => {
                self.gtp_terminal_open = false;
                self.active_plugin_popover = Some("winrate-graph".to_owned());
            }
            BottomDeckTab::VariationTree => {
                self.gtp_terminal_open = false;
                self.active_plugin_popover = Some("variation-tree".to_owned());
            }
            BottomDeckTab::GtpTerminal => {
                self.gtp_terminal_open = true;
                self.active_plugin_popover = None;
                self.active_text_input = Some(ActiveTextInput::GtpInput);
            }
            // Built-in engine/tool panels live only in the left engine sidebar;
            // switching to them reroutes there instead of opening a deck tab.
            BottomDeckTab::KataGo => {
                self.open_engine_config_panel(EngineConfigPanel::KataGo, cx);
            }
            BottomDeckTab::FoxSync => {
                self.open_engine_config_panel(EngineConfigPanel::FoxSync, cx);
            }
            BottomDeckTab::PositionSgf => {
                self.open_engine_config_panel(EngineConfigPanel::PositionSgf, cx);
            }
            BottomDeckTab::PluginManager => {
                self.gtp_terminal_open = false;
                self.active_plugin_popover = Some("all".to_owned());
            }
            BottomDeckTab::Engines => {
                self.open_engine_config_panel(EngineConfigPanel::Engines, cx);
            }
            BottomDeckTab::Generic(id) => {
                self.gtp_terminal_open = false;
                self.active_plugin_popover = Some(id);
            }
        }
        cx.notify();
    }

    #[allow(dead_code)]
    pub fn toggle_bottom_deck_tab(&mut self, tab: BottomDeckTab, cx: &mut Context<Self>) {
        if self.is_bottom_deck_open() && self.active_bottom_tab() == tab {
            self.close_bottom_deck(cx);
        } else {
            self.switch_bottom_tab(tab, cx);
        }
    }

    pub fn close_bottom_deck(&mut self, cx: &mut Context<Self>) {
        self.gtp_terminal_open = false;
        self.active_plugin_popover = None;
        cx.notify();
    }

    /// Expands an engine-configuration panel in the left engine sidebar,
    /// opening the sidebar first if it is hidden. Used by the deck-slimming
    /// migration: KataGo setup and the engine manager now live there.
    fn open_engine_config_panel(&mut self, panel: EngineConfigPanel, cx: &mut Context<Self>) {
        if !self
            .settings
            .get_bool("view.show_leftsidebar")
            .unwrap_or(false)
        {
            self.settings
                .set("view.show_leftsidebar", serde_json::Value::Bool(true))
                .ok();
        }
        self.engine_config_panel = Some(panel);
        match panel {
            EngineConfigPanel::KataGo => {
                if self.katago_release.is_none() {
                    self.refresh_katago_panel(cx);
                }
            }
            EngineConfigPanel::Engines => {
                self.active_text_input = Some(ActiveTextInput::EngineSpec);
            }
            EngineConfigPanel::FoxSync => {
                self.active_text_input = Some(ActiveTextInput::FoxQuery);
            }
            EngineConfigPanel::PositionSgf => {}
        }
        cx.notify();
    }

    fn toggle_engine_config_panel(&mut self, panel: EngineConfigPanel, cx: &mut Context<Self>) {
        if self.engine_config_panel == Some(panel) {
            self.engine_config_panel = None;
        } else {
            self.open_engine_config_panel(panel, cx);
        }
        cx.notify();
    }

    fn toggle_plugin_popover(&mut self, id: &str, cx: &mut Context<Self>) {
        // Engine/tool panels moved to the left engine sidebar: reroute those
        // ids there so the panel has a single render path (no deck/sidebar
        // double render with divergent form state).
        if let Some(panel) = Self::engine_sidebar_panel_for(id) {
            self.toggle_engine_config_panel(panel, cx);
            return;
        }
        if self.active_plugin_popover.as_deref() == Some(id) {
            self.active_plugin_popover = None;
        } else {
            self.active_plugin_popover = Some(id.to_owned());
            self.gtp_terminal_open = false;
        }
        cx.notify();
    }

    /// Maps a legacy engine/tool popover id to its left-sidebar panel.
    fn engine_sidebar_panel_for(id: &str) -> Option<EngineConfigPanel> {
        Some(match id {
            "org.ryusei.katago-setup-hub" => EngineConfigPanel::KataGo,
            "org.ryusei.fox-kifu-sync" => EngineConfigPanel::FoxSync,
            "org.ryusei.position-to-sgf" => EngineConfigPanel::PositionSgf,
            "engines" => EngineConfigPanel::Engines,
            _ => return None,
        })
    }

    fn close_plugin_popover(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.active_plugin_popover = None;
        cx.notify();
    }

    fn toggle_plugin_pinned(&mut self, plugin_id: &str, cx: &mut Context<Self>) {
        let pinned = self.pinned_plugin_ids();
        let mut new_pinned = pinned.clone();
        if new_pinned.contains(&plugin_id.to_owned()) {
            new_pinned.retain(|id| id != plugin_id);
        } else {
            new_pinned.push(plugin_id.to_owned());
        }
        let json_arr = serde_json::Value::Array(
            new_pinned
                .into_iter()
                .map(serde_json::Value::String)
                .collect(),
        );
        let _ = self.settings.set("plugins.pinned", json_arr);
        let _ = ryusei_host::persist_settings_store(&self.settings, &mut self.settings_persistence);
        cx.notify();
    }

    pub fn pinned_plugin_ids(&self) -> Vec<String> {
        self.settings
            .get("plugins.pinned")
            .and_then(serde_json::Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(ToOwned::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    }

    fn open_game_info(&mut self, cx: &mut Context<Self>) {
        self.open_drawer(ActiveDrawer::GameInfo, "game info opened", cx);
    }

    fn open_score(&mut self, cx: &mut Context<Self>) {
        self.open_drawer(ActiveDrawer::Score, "score opened", cx);
    }

    fn open_about(&mut self, cx: &mut Context<Self>) {
        self.open_drawer(ActiveDrawer::About, "about opened", cx);
    }

    fn open_match_setup(&mut self, cx: &mut Context<Self>) {
        self.open_drawer(ActiveDrawer::MatchSetup, "match setup opened", cx);
    }

    fn close_drawer(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.active_drawer = None;
        cx.notify();
    }

    fn open_game_graph_context_menu(
        &mut self,
        node_id: ryusei_domain_core::NodeId,
        cx: &mut Context<Self>,
    ) {
        self.game_graph_context_node = Some(node_id);
        cx.notify();
    }

    fn close_game_graph_context_menu(
        &mut self,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.game_graph_context_node = None;
        cx.notify();
    }

    fn navigate_game_graph_context_node(
        &mut self,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(node_id) = self.game_graph_context_node.clone() {
            self.navigate_to_node(node_id, cx);
        }
        self.game_graph_context_node = None;
        cx.notify();
    }

    fn toggle_game_graph_context_hotspot(
        &mut self,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(node_id) = self.game_graph_context_node.clone() else {
            return;
        };
        let enabled = self
            .host
            .snapshot()
            .nodes
            .iter()
            .find(|node| node.id == node_id)
            .is_some_and(|node| node.properties.contains_key("HO"));
        let mut events = RecordingSink;
        match self
            .host
            .apply_transaction(create_hotspot_transaction(&node_id, !enabled), &mut events)
        {
            Ok(_) => {
                self.status = if enabled {
                    "game graph hotspot removed".into()
                } else {
                    "game graph hotspot added".into()
                };
                self.synchronize_recovery();
            }
            Err(error) => self.status = format!("game graph hotspot failed: {error}").into(),
        }
        self.game_graph_context_node = None;
        cx.notify();
    }

    /// Applies a variation-structure transaction (promote to main line / delete
    /// branch) from the game-graph context menu, then closes the menu.
    fn apply_game_graph_structure_transaction(
        &mut self,
        transaction_type: ryusei_domain_core::GameTransactionType,
        status_ok: &'static str,
        status_err: &'static str,
        cx: &mut Context<Self>,
    ) {
        let Some(node_id) = self.game_graph_context_node.clone() else {
            return;
        };
        let transaction = ryusei_domain_core::GameTransaction {
            schema_version: ryusei_domain_core::CURRENT_TRANSACTION_SCHEMA_VERSION,
            transaction_type,
            color: None,
            vertex: None,
            node_id: Some(node_id),
            property: None,
            values: Vec::new(),
            marker: None,
            nodes: Vec::new(),
            score_override: None,
        };
        let mut events = RecordingSink;
        match self.host.apply_transaction(transaction, &mut events) {
            Ok(_) => {
                self.status = status_ok.into();
                self.synchronize_recovery();
            }
            Err(error) => self.status = format!("{status_err}: {error}").into(),
        }
        self.game_graph_context_node = None;
        cx.notify();
    }

    fn promote_game_graph_context_variation(
        &mut self,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_game_graph_structure_transaction(
            ryusei_domain_core::GameTransactionType::PromoteVariation,
            "variation promoted to main line",
            "promote variation failed",
            cx,
        );
    }

    fn delete_game_graph_context_variation(
        &mut self,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_game_graph_structure_transaction(
            ryusei_domain_core::GameTransactionType::RemoveVariation,
            "variation branch deleted",
            "delete variation failed",
            cx,
        );
    }

    #[allow(dead_code)]
    fn on_navigate_first(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.navigate(NavigationDirection::First, cx);
    }

    fn on_navigate_previous(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.navigate(NavigationDirection::Previous, cx);
    }

    fn on_navigate_next(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.navigate(NavigationDirection::Next, cx);
    }

    #[allow(dead_code)]
    fn on_navigate_last(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.navigate(NavigationDirection::Last, cx);
    }
}

impl Render for ShellApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let snapshot = self.host.snapshot();
        let theme_color = self.theme.background_color().rgb_u32();
        let status = match self.last_vertex {
            Some(Vertex { column, row }) => format!("last move: {column},{row}"),
            None => "click the board or use the File menu".to_owned(),
        };
        let file_state = &snapshot.file_state;
        let _dirty_label = if file_state.is_dirty {
            "modified"
        } else {
            "saved"
        };
        let _path_label = file_state.path.as_deref().unwrap_or("no source file");
        let _availability = navigation_availability(&snapshot);
        let _position = position_label(&snapshot);
        let variation_layout = build_variation_tree_layout(&snapshot);
        // `best_analysis_winrate` already returns Black perspective; the graph
        // history builder performs the same conversion for White-to-play data,
        // so feed it the engine's raw player-to-move fraction to avoid a double
        // conversion.
        let live_player_winrate =
            best_analysis_entry(&self.analysis).map(|entry| entry.winrate.clamp(0.0, 1.0));
        let live_score_lead = best_analysis_entry(&self.analysis)
            .and_then(|entry| entry.score_lead)
            .filter(|lead| lead.is_finite());
        let winrate_points = winrate_history(
            &snapshot,
            live_player_winrate,
            live_score_lead,
            snapshot.board.next_player,
        );
        let winrate_metric =
            WinrateGraphMetric::from_setting(self.settings.get_str("board.analysis_type"));
        let _winrate_plot_points = graph_plot_points(
            &winrate_points,
            winrate_metric,
            self.settings
                .get_bool("view.winrategraph_invert")
                .unwrap_or(false),
            self.settings
                .get("view.winrategraph_blunderthreshold")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(5.0),
            self.settings
                .get("view.winrategraph_blunderthreshold_scorelead")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(2.0),
        );
        let inspector_metadata = current_node_metadata(&snapshot);
        let _settings_rows = panel_setting_rows(&self.settings);
        let external_status = self.external_file.status();
        let _external_conflict = matches!(
            external_status.status,
            ryusei_host::ExternalFileStatus::Changed
                | ryusei_host::ExternalFileStatus::Missing
                | ryusei_host::ExternalFileStatus::Unreadable
        );
        // Left engines sidebar defaults to collapsed on first launch (matching reference screenshot);
        // right sidebar (game graph & comments) defaults to expanded.
        let show_left_sidebar = self
            .settings
            .get_bool("view.show_leftsidebar")
            .unwrap_or(false);
        let show_graph = self.settings.get_bool("view.show_graph").unwrap_or(true);
        let show_comments = self.settings.get_bool("view.show_comments").unwrap_or(true);
        let show_analysis_preview = self
            .settings
            .get_bool("view.show_analysis_preview")
            .unwrap_or(true);
        let show_right_sidebar =
            right_pane_visible(show_graph, show_comments, show_analysis_preview);
        let palette = self.palette;
        let weak_shell = cx.entity().downgrade();
        let on_node_clicked = Rc::new(
            move |node_id: &ryusei_domain_core::NodeId, _window: &mut Window, cx: &mut App| {
                weak_shell
                    .update(cx, |shell, cx| shell.navigate_to_node(node_id.clone(), cx))
                    .ok();
            },
        );

        let weak_shell_for_context = cx.entity().downgrade();
        let on_node_context_requested = Rc::new(
            move |node_id: &ryusei_domain_core::NodeId, _window: &mut Window, cx: &mut App| {
                weak_shell_for_context
                    .update(cx, |shell, cx| {
                        shell.open_game_graph_context_menu(node_id.clone(), cx)
                    })
                    .ok();
            },
        );

        // Adaptive goban: fill the center pane as a square, bounded by the
        // available width (after sidebars) and height (after the player bar).
        let window_bounds = window.bounds();
        let window_width = f32::from(window_bounds.size.width);
        let window_height = f32::from(window_bounds.size.height);
        // Responsive breakpoint (design: ≤1024px narrows the sidebars to
        // 210/260). The cap never overrides a narrower user-dragged width.
        let (left_cap, right_cap) = if window_width <= 1024.0 {
            (210.0, 260.0)
        } else {
            (f32::MAX, f32::MAX)
        };
        let effective_left = self.left_sidebar_width.min(left_cap);
        let effective_right = self.right_sidebar_width.min(right_cap);
        let side_panels = NAVIGATION_RAIL_WIDTH
            + if show_left_sidebar {
                effective_left
            } else {
                0.0
            }
            + if show_right_sidebar {
                effective_right
            } else {
                0.0
            };
        let bottom_panel_height = if self.is_bottom_deck_open() {
            panels::BOTTOM_DECK_HEIGHT
        } else {
            0.0
        };
        // Center-pane chrome the board must clear: the unified floating toolbar
        // capsule, the floating playback capsule, and the column gaps between them.
        let center_chrome_height = 34.0 + 34.0 + 16.0;
        let available_width = (window_width - side_panels - 16.0).max(160.0);
        let available_height =
            (window_height - 44.0 - 36.0 - bottom_panel_height - center_chrome_height - 16.0)
                .max(160.0);
        // Always fit the center pane: a too-small window shrinks the board
        // instead of pushing it into a scroll container (which clipped the
        // bottom rows off-screen until the user scrolled).
        let board_pixel_size = available_width.min(available_height);

        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(theme_color))
            .text_color(rgb(palette.text))
            // Keyboard focus navigation: the shell is one tab group, so Tab /
            // Shift-Tab cycle the text inputs marked with `tab_index` inside it.
            .tab_group()
            .on_action(|_: &FocusNext, window, _cx| window.focus_next())
            .on_action(|_: &FocusPrev, window, _cx| window.focus_prev())
            .child(panels::render_titlebar(
                show_left_sidebar,
                show_right_sidebar,
                &snapshot,
                window_width,
                window.is_fullscreen(),
                self,
                cx,
            ))
            .child(
                div()
                    .id("workspace")
                    .debug_selector(|| "workspace".to_owned())
                    .flex_1()
                    .h_0()
                    .min_h_0()
                    .flex()
                    .child(navigation_rail::render_navigation_rail(self, cx))
                    .child(if show_left_sidebar {
                        div()
                            .id("left-sidebar")
                            .debug_selector(|| "left-sidebar".to_owned())
                            .flex_none()
                            .w(px(effective_left))
                            .h_full()
                            .min_h_0()
                            .flex()
                            .flex_col()
                            .bg(rgb(palette.input))
                            .border_r_1()
                            .border_color(rgb(palette.border))
                            .child(panels::render_left_engine_sidebar(self, cx))
                    } else {
                        div()
                            .id("left-sidebar-hidden")
                            .debug_selector(|| "left-sidebar-hidden".to_owned())
                    })
                    .child(if show_left_sidebar {
                        panels::render_split_handle(SplitPane::Left, palette, cx)
                    } else {
                        div()
                    })
                    .child(
                        div()
                            .id("center-pane")
                            .debug_selector(|| "center-pane".to_owned())
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .min_h_0()
                            .overflow_hidden()
                            .flex()
                            .flex_col()
                            .child(
                                // Board region: toolbar capsule + goban + playback
                                // capsule, centered in the remaining column space.
                                div()
                                    .id("center-board-region")
                                    .flex_1()
                                    .min_h_0()
                                    .w_full()
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .justify_center()
                                    .gap_2()
                                    .child(panels::render_session_toolbar(self, cx))
                                    .child(panels::render_goban_area(
                                        &snapshot,
                                        &self.theme,
                                        self.analysis_best_move,
                                        board_pixel_size,
                                        self,
                                        cx,
                                    ))
                                    .child(panels::render_floating_playback_bar(
                                        &snapshot, self, cx,
                                    )),
                            )
                            // Design: the pull-up analysis deck attaches to the
                            // center board column only — it no longer spans the
                            // left/right sidebars.
                            .children(if self.is_bottom_deck_open() {
                                Some(panels::render_bottom_deck_panel(&snapshot, self, cx))
                            } else {
                                None
                            }),
                    )
                    .child(if show_right_sidebar {
                        panels::render_split_handle(SplitPane::Right, palette, cx)
                    } else {
                        div()
                    })
                    .child(if show_right_sidebar {
                        div()
                            .id("right-sidebar")
                            .debug_selector(|| "right-sidebar".to_owned())
                            .flex_none()
                            .w(px(effective_right))
                            .h_full()
                            .min_h_0()
                            .flex()
                            .flex_col()
                            .pr_1()
                            .border_l_1()
                            .border_color(rgb(palette.border))
                            .bg(rgb(palette.panel))
                            .overflow_y_scroll()
                            .child(if show_analysis_preview {
                                panels::render_analysis_preview_panel(
                                    &snapshot,
                                    &self.theme,
                                    self,
                                    cx,
                                )
                            } else {
                                div().id("analysis-preview-panel-hidden")
                            })
                            .child(div().id("winrate-graph-panel-hidden"))
                            .child(
                                div()
                                    .id("game-graph-region")
                                    .debug_selector(|| "game-graph-region".to_owned())
                                    .flex_1()
                                    .min_h(px(140.0))
                                    .overflow_x_scroll()
                                    .overflow_y_scroll()
                                    .child(if show_graph {
                                        panels::render_variation_tree_panel(
                                            "variation-tree-panel",
                                            &variation_layout,
                                            self.settings
                                                .get("graph.grid_size")
                                                .and_then(serde_json::Value::as_f64)
                                                .map(|value| value as f32)
                                                .unwrap_or(26.0),
                                            self.settings
                                                .get("graph.node_size")
                                                .and_then(serde_json::Value::as_f64)
                                                .map(|value| value as f32)
                                                .unwrap_or(4.0),
                                            palette,
                                            self,
                                            cx,
                                            move |node_id, window, cx| {
                                                on_node_clicked(node_id, window, cx)
                                            },
                                            {
                                                let handler = on_node_context_requested.clone();
                                                move |node_id, window, cx| {
                                                    handler(node_id, window, cx)
                                                }
                                            },
                                        )
                                    } else {
                                        div().id("variation-tree-panel-hidden")
                                    }),
                            )
                            .child(if show_comments {
                                panels::render_right_sidebar_split_handle(
                                    SplitPane::Properties,
                                    palette,
                                    cx,
                                )
                            } else {
                                div().id("properties-splitter-hidden")
                            })
                            .child(
                                div()
                                    .id("properties-region")
                                    .debug_selector(|| "properties-region".to_owned())
                                    .flex_none()
                                    .h(px(self.properties_height))
                                    .min_h_0()
                                    .overflow_y_scroll()
                                    .flex()
                                    .flex_col()
                                    .gap_3()
                                    .child(panels::render_node_inspector_panel(
                                        &inspector_metadata,
                                        self,
                                        cx,
                                    )),
                            )
                    } else {
                        div()
                            .id("right-sidebar-hidden")
                            .debug_selector(|| "right-sidebar-hidden".to_owned())
                    }),
            )
            .child(
                div()
                    .id("bottom-dock-container")
                    .debug_selector(|| "bottom-dock-container".to_owned())
                    .flex_none()
                    .flex()
                    .flex_col()
                    .child(panels::render_player_bar(
                        &snapshot, &status, palette, self, cx,
                    )),
            )
            .child(match self.active_drawer {
                Some(ActiveDrawer::Preferences) => panels::render_preferences_drawer(self, cx),
                Some(ActiveDrawer::Library) => panels::render_library_drawer(self, cx),
                Some(ActiveDrawer::Profile) => panels::render_profile_drawer(self, cx),
                Some(ActiveDrawer::Goals) => panels::render_goals_drawer(self, cx),
                Some(ActiveDrawer::LiveCapture) => panels::render_live_capture_drawer(self, cx),
                Some(ActiveDrawer::GameInfo) => {
                    panels::render_game_info_drawer(&snapshot, self, cx)
                }
                Some(ActiveDrawer::Score) => panels::render_score_drawer(&snapshot, self, cx),
                Some(ActiveDrawer::About) => panels::render_about_drawer(self, cx),
                Some(ActiveDrawer::OgsAccount) => panels::render_ogs_account_drawer(self, cx),
                Some(ActiveDrawer::Review) => panels::render_review_drawer(self, cx),
                Some(ActiveDrawer::Export) => panels::render_export_drawer(&snapshot, self, cx),
                Some(ActiveDrawer::MatchSetup) => panels::render_match_setup_drawer(self, cx),
                None => div().id("drawer-hidden"),
            })
            .child(if self.split_drag.is_some() {
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .when(
                        self.split_drag.is_some_and(|drag| {
                            matches!(
                                drag.pane,
                                SplitPane::PeerList
                                    | SplitPane::WinrateGraph
                                    | SplitPane::Properties
                            )
                        }),
                        |this| this.cursor_row_resize(),
                    )
                    .when(
                        self.split_drag.is_some_and(|drag| {
                            matches!(drag.pane, SplitPane::Left | SplitPane::Right)
                        }),
                        |this| this.cursor_col_resize(),
                    )
                    .on_mouse_move(cx.listener(ShellApp::on_split_drag_mouse_move))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(ShellApp::on_split_drag_mouse_up),
                    )
            } else {
                div()
            })
            .children(self.toast.as_ref().map(|message| {
                div()
                    .id("toast")
                    .debug_selector(|| "toast".to_owned())
                    .absolute()
                    .bottom(px(56.0))
                    .left_0()
                    .w_full()
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .px_4()
                            .py_2p5()
                            .rounded_full()
                            .bg(rgb(palette.text))
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(palette.panel))
                            .child(message.clone())
                            // Design toastSlideUp: fade in while rising 10px over
                            // the base motion duration with an ease-out curve.
                            .with_animation(
                                "toast-slide-up",
                                Animation::new(Duration::from_millis(
                                    crate::theme::motion::BASE_MS,
                                ))
                                .with_easing(ease_out_quint()),
                                move |element, delta| {
                                    element.opacity(delta).mt(px((1.0 - delta) * 10.0))
                                },
                            ),
                    )
            }))
    }
}

impl ShellApp {
    fn active_text_input_mut(&mut self) -> Option<&mut NativeTextInput> {
        self.text_inputs.active_mut(self.active_text_input)
    }

    fn active_text_input(&self) -> Option<&NativeTextInput> {
        self.text_inputs.active(self.active_text_input)
    }
}

impl gpui::EntityInputHandler for ShellApp {
    fn text_for_range(
        &mut self,
        range: std::ops::Range<usize>,
        adjusted_range: &mut Option<std::ops::Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let input = self.active_text_input_mut()?;
        *adjusted_range = Some(range.clone());
        Some(input.text_for_utf16_range(range))
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<gpui::UTF16Selection> {
        Some(gpui::UTF16Selection {
            range: self.active_text_input_mut()?.utf16_selection(),
            reversed: false,
        })
    }

    fn marked_text_range(
        &self,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<std::ops::Range<usize>> {
        self.active_text_input()
            .and_then(|input| input.marked_text_utf16_range())
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(input) = self.active_text_input_mut() {
            input.unmark_text();
            cx.notify();
        }
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<std::ops::Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(input) = self.active_text_input_mut() {
            // `insertText:` commits any pending IME composition, so replace
            // the marked range rather than appending at the cursor.
            input.commit_marked_text(range, text);
            cx.notify();
        }
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<std::ops::Range<usize>>,
        new_text: &str,
        _: Option<std::ops::Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(input) = self.active_text_input_mut() {
            input.replace_marked_text(range, new_text);
            cx.notify();
        }
    }

    fn bounds_for_range(
        &mut self,
        _: std::ops::Range<usize>,
        element_bounds: Bounds<gpui::Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<gpui::Pixels>> {
        // The native text bridge needs a non-zero caret rectangle. If the
        // binding element collapsed (e.g. inside a block layout), fall back to
        // a minimum-height rect so the I-beam cursor remains visible.
        self.active_text_input.as_ref()?;
        let height = element_bounds.size.height.max(gpui::px(14.0));
        let width = element_bounds.size.width.max(gpui::px(1.0));
        Some(Bounds {
            origin: element_bounds.origin,
            size: gpui::size(width, height),
        })
    }

    fn character_index_for_point(
        &mut self,
        _: gpui::Point<gpui::Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        self.active_text_input_mut()
            .map(|input| input.utf16_selection().start)
    }
}

/// Renders one settings row: a toggle pill for booleans, a click-to-edit text
/// row for every other kind. The editing row shows the draft in a focused
/// input box; Enter commits, Esc reverts.

#[derive(Default)]
struct RecordingSink;

impl ryusei_host::HostEventSink for RecordingSink {
    fn emit(&mut self, _event: ryusei_host::HostEvent) {}
}

type MouseClickHandler = dyn Fn(&MouseDownEvent, &mut Window, &mut App);

#[allow(dead_code)]
fn navigation_bar<A, B, C, D>(
    availability: crate::navigation::NavigationAvailability,
    position: &str,
    palette: UiPalette,
    on_first: A,
    on_previous: B,
    on_next: C,
    on_last: D,
) -> Div
where
    A: Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    B: Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    C: Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    D: Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
{
    let button_style = |label: &str, enabled: bool, on_click: Box<MouseClickHandler>| {
        let mut btn = div()
            .w(px(28.0))
            .h(px(28.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded_md()
            .text_xs()
            .font_weight(FontWeight::MEDIUM)
            .bg(if enabled {
                rgb(palette.button)
            } else {
                rgb(palette.panel)
            })
            .border_1()
            .border_color(rgb(if enabled {
                palette.accent
            } else {
                palette.panel
            }))
            .text_color(if enabled {
                rgb(palette.text)
            } else {
                rgb(palette.subtle)
            })
            .child(label.to_owned());

        if enabled {
            btn = btn
                .cursor_pointer()
                .hover(|style| style.bg(rgb(palette.button_active)))
                .on_mouse_down(MouseButton::Left, on_click);
        }
        btn
    };

    div()
        .flex()
        .items_center()
        .gap_1()
        .p_1()
        .rounded_lg()
        .bg(rgb(palette.panel))
        .border_1()
        .border_color(rgb(palette.border))
        .child(button_style(
            "⏮",
            availability.can_go_first,
            Box::new(on_first),
        ))
        .child(button_style(
            "◀",
            availability.can_go_previous,
            Box::new(on_previous),
        ))
        .child(
            div()
                .px_2()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(palette.text))
                .child(position.to_owned()),
        )
        .child(button_style(
            "▶",
            availability.can_go_next,
            Box::new(on_next),
        ))
        .child(button_style(
            "⏭",
            availability.can_go_last,
            Box::new(on_last),
        ))
}

fn shell_menus() -> Vec<Menu> {
    vec![
        Menu {
            name: "Ryusei".into(),
            items: vec![
                MenuItem::action("About Ryusei", OpenAbout),
                MenuItem::separator(),
                MenuItem::action("Quit", Quit),
            ],
        },
        Menu {
            name: "File".into(),
            items: vec![
                MenuItem::action("New Game", NewGame),
                MenuItem::action("Open…", OpenGame),
                MenuItem::separator(),
                MenuItem::action("Save", SaveGame),
                MenuItem::action("Save As…", SaveGameAs),
                MenuItem::separator(),
                MenuItem::action("Export Animated GIF…", ExportGif),
                MenuItem::action("Export Current Position PNG…", ExportPositionPng),
            ],
        },
        Menu {
            name: "Edit".into(),
            items: vec![
                MenuItem::action("Undo", UndoMove),
                MenuItem::action("Redo", RedoMove),
            ],
        },
        Menu {
            name: "View".into(),
            items: vec![
                MenuItem::action("Game Info", OpenGameInfo),
                MenuItem::action("Score Summary", OpenScore),
                MenuItem::separator(),
                MenuItem::action("Toggle Game Graph", ToggleGameGraph),
                MenuItem::action("Toggle Comments", ToggleComments),
                MenuItem::action("Toggle Coordinates", ToggleCoordinates),
                MenuItem::action("Toggle Move Numbers", ToggleMoveNumbers),
            ],
        },
        Menu {
            name: "Board & Theme".into(),
            items: vec![
                MenuItem::action("19 × 19 Board", SetBoardSize19),
                MenuItem::action("13 × 13 Board", SetBoardSize13),
                MenuItem::action("9 × 9 Board", SetBoardSize9),
                MenuItem::separator(),
                MenuItem::action("Theme: Classic Light", SetThemeClassic),
                MenuItem::action("Theme: Dark Slate", SetThemeDark),
                MenuItem::action("Theme: Mist", SetThemeMist),
                MenuItem::separator(),
                MenuItem::action("Coordinates: A1 Style", SetCoordsA1),
                MenuItem::action("Coordinates: 1-1 Style", SetCoords1_1),
            ],
        },
        Menu {
            name: "Session".into(),
            items: vec![
                MenuItem::action("Match", SetSessionMatch),
                MenuItem::action("Record", SetSessionRecord),
                MenuItem::action("Live", SetSessionLive),
                MenuItem::separator(),
                MenuItem::action("人人对弈", SetPlayersHumanVsHuman),
                MenuItem::action("人机对弈（黑方人类）", SetPlayersHumanVsAi),
                MenuItem::action("人机对弈（白方人类）", SetPlayersAiVsHuman),
                MenuItem::action("AI 对弈", SetPlayersAiVsAi),
                MenuItem::separator(),
                MenuItem::action("Free Opening", SetOpeningFree),
                MenuItem::action("Chinese Ancient: Seat Stones", SetOpeningAncientSeatStones),
                MenuItem::separator(),
                MenuItem::action("No Clock", SetTimeNone),
                MenuItem::action("10 Minutes Absolute", SetTimeAbsolute600),
                MenuItem::action("10m + 5 x 30s Byo-yomi", SetTimeByoYomi),
            ],
        },
        Menu {
            name: "Board Tool".into(),
            items: vec![
                MenuItem::action("Play", SetPlayMode),
                MenuItem::action("Edit", SetEditMode),
                MenuItem::action("Score", SetScoringMode),
                MenuItem::action("Estimate", SetEstimatorMode),
            ],
        },
        Menu {
            name: "Engines".into(),
            items: vec![
                MenuItem::action("Show Engines Sidebar", ToggleEnginesSidebar),
                MenuItem::action("Toggle GTP Terminal", ToggleGtpTerminal),
                MenuItem::separator(),
                MenuItem::action("Start Analysis", StartAnalysis),
                MenuItem::action("Stop Analysis", StopAnalysis),
                MenuItem::action("Whole Game Review: Quick (50 visits)", StartReviewQuick),
                MenuItem::action(
                    "Whole Game Review: Preliminary (800 visits)",
                    StartReviewPreliminary,
                ),
                MenuItem::action(
                    "Whole Game Review: Intermediate (2500 visits)",
                    StartReviewIntermediate,
                ),
                MenuItem::action(
                    "Whole Game Review: Advanced (10000 visits)",
                    StartReviewAdvanced,
                ),
                MenuItem::action("Generate Engine Move", GenerateEngineMove),
                MenuItem::separator(),
                MenuItem::action("Visits Limit: 100", SetVisits100),
                MenuItem::action("Visits Limit: 500", SetVisits500),
                MenuItem::action("Visits Limit: 1000", SetVisits1000),
                MenuItem::action("Visits Limit: Unlimited", SetVisitsUnlimited),
            ],
        },
        Menu {
            name: "Plugins".into(),
            items: vec![
                MenuItem::action("KataGo 一键配置与环境诊断", PluginKataGoSetup),
                MenuItem::action("野狐对局与棋谱同步", PluginFoxSync),
                MenuItem::action("局面转 SGF 剪贴板导出", PluginPositionToSgf),
                MenuItem::separator(),
                MenuItem::action("插件管理器 (固定到栏)", TogglePluginMenu),
                MenuItem::action("+ 从 ZIP 安装插件…", PluginInstallZip),
            ],
        },
        Menu {
            name: "Navigate".into(),
            items: vec![
                MenuItem::action("First Node", GoToFirstNode),
                MenuItem::action("Previous Node", GoToPreviousNode),
                MenuItem::action("Next Node", GoToNextNode),
                MenuItem::action("Last Node", GoToLastNode),
            ],
        },
        Menu {
            name: "Help".into(),
            items: vec![MenuItem::action("About Ryusei", OpenAbout)],
        },
    ]
}

/// How the user answered the close confirmation for a dirty document.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CloseChoice {
    Save,
    Discard,
    Cancel,
}

/// Maps a user's close-confirmation choice to the final allow-close decision.
/// A successful save (or an explicit discard) closes the window; a cancelled
/// or failed save keeps it open. Pure so the flow is unit-testable without a
/// native dialog.
pub fn close_decision(choice: CloseChoice, save_succeeded: bool) -> bool {
    match choice {
        CloseChoice::Discard => true,
        CloseChoice::Save => save_succeeded,
        CloseChoice::Cancel => false,
    }
}

/// Minimum interval between periodic external-file checks.
const EXTERNAL_CHECK_INTERVAL: Duration = Duration::from_secs(1);

/// Schedules one external-file check on the next rendered frame, re-arming
/// itself. Frames only occur while the window is active, so this effectively
/// checks shortly after the window regains focus (the platform activation
/// handler schedules a refresh frame); the interval throttles repeated checks
/// during active use. Idle or inactive windows produce no frames and no reads.
fn schedule_external_check<V: gpui::Render + 'static>(
    window: &mut Window,
    _cx: &mut App,
    window_handle: gpui::WindowHandle<V>,
    shell: Entity<ShellApp>,
    last_check: Rc<Cell<Option<Instant>>>,
) {
    window.on_next_frame(move |window, cx| {
        let due = match last_check.get() {
            Some(previous) => previous.elapsed() >= EXTERNAL_CHECK_INTERVAL,
            None => true,
        };
        if window_handle.is_active(cx).unwrap_or(false) {
            shell.update(cx, |shell, cx| shell.advance_clock(Instant::now(), cx));
            if due {
                last_check.set(Some(Instant::now()));
                shell.update(cx, |shell, cx| shell.check_external_file_now(cx));
            }
        }
        schedule_external_check(window, cx, window_handle, shell, last_check);
    });
}

fn main() {
    let startup_file = std::env::args().nth(1).map(PathBuf::from);
    Application::new()
        .with_assets(icons::EmbeddedIcons)
        .run(move |cx: &mut App| {
        gpui_component::init(cx);
        // The gpui-component theme is synced to the shell palette's luminance
        // once the shell exists (see `ShellApp::sync_component_theme`), so
        // component controls match the active light/dark board theme instead of
        // being forced dark.

        let settings_persistence = match NativeSettingsPersistence::for_current_user() {
            Ok(persistence) => persistence,
            Err(error) => {
                eprintln!("settings persistence unavailable ({error}); using a temp directory");
                NativeSettingsPersistence::new(std::env::temp_dir().join("ryusei-gpui-config"))
            }
        };
        let mut initial_status = "new game".to_owned();
        let settings = match ryusei_host::load_settings_store(&settings_persistence) {
            Ok(loaded) => {
                for invalid in &loaded.validation.invalid_values {
                    initial_status = format!("ignored setting {}: {}", invalid.key, invalid);
                }
                // Design §8.1: user styles.css is not executed; report which
                // color rules could migrate to theme tokens.
                if !loaded.store.user_styles().trim().is_empty() {
                    let report = ryusei_host::analyze_legacy_styles(loaded.store.user_styles());
                    initial_status = format!(
                        "styles.css migration: {} color rule(s) migratable to theme tokens, {} rule(s) not migrated",
                        report.migrated_color_rules.len(),
                        report.ignored_rule_count
                    );
                }
                loaded.store
            }
            Err(error) => {
                initial_status = format!("could not load settings: {error}");
                ryusei_host::SettingsStore::default()
            }
        };
        let default_size = (1240.0, 800.0);
        let (initial_width, initial_height) =
            window_bounds_from_settings(&settings).unwrap_or(default_size);
        let bounds = Bounds::centered(
            None,
            size(px(initial_width as f32), px(initial_height as f32)),
            cx,
        );
        let window_bounds = if window_maximized_from_settings(&settings) {
            WindowBounds::Maximized(bounds)
        } else {
            WindowBounds::Windowed(bounds)
        };
        let mut shell_slot: Option<Entity<ShellApp>> = None;
        let shell_slot_ref = &mut shell_slot as *mut Option<Entity<ShellApp>>;
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(window_bounds),
                    window_min_size: Some(size(px(960.0), px(640.0))),
                    titlebar: Some(TitlebarOptions {
                        title: None,
                        appears_transparent: true,
                        traffic_light_position: Some(point(px(9.0), px(9.0))),
                    }),
                    ..Default::default()
                },
                move |window, cx| {
                    let startup_file = startup_file.clone();
                    let initial_status = initial_status.clone();
                    let settings = settings.clone();
                    let settings_persistence = settings_persistence.clone();
                    let host_persistence = NativeHostPersistence::for_current_user()
                        .unwrap_or_else(|_| {
                            NativeHostPersistence::new(
                                std::env::temp_dir().join("ryusei-gpui-config"),
                            )
                        });
                    let plugin_persistence = NativePluginPersistence::for_current_user()
                        .unwrap_or_else(|_| {
                            NativePluginPersistence::new(
                                std::env::temp_dir().join("ryusei-gpui-config"),
                            )
                        });
                    let shell = cx.new(move |cx| {
                        ShellApp::new(
                            settings,
                            settings_persistence,
                            host_persistence,
                            plugin_persistence,
                            initial_status,
                            startup_file,
                            Box::new(RfdDialogService),
                            cx,
                        )
                    });
                    unsafe {
                        *shell_slot_ref = Some(shell.clone());
                    }
                    cx.new(move |cx| gpui_component::Root::new(shell, window, cx))
                },
            )
            .unwrap();
        let shell: Entity<ShellApp> = shell_slot.unwrap();
        shell.update(cx, |shell, cx| {
            shell.sync_component_theme(cx);
            if shell.analysis_enabled {
                shell.start_analysis(cx);
            }
        });

        let shell_for_close = shell.clone();
        window
            .update(cx, |_, window, cx| {
                window.on_window_should_close(&*cx, move |window, cx| {
                    let bounds = window.bounds();
                    let maximized = window.is_maximized();
                    let mut allow_close = false;
                    shell_for_close.update(cx, |shell, _| {
                        shell.remember_window_bounds(
                            bounds.size.width.to_f64(),
                            bounds.size.height.to_f64(),
                            maximized,
                        );
                        allow_close = shell.should_allow_window_close();
                    });
                    allow_close
                });
            })
            .unwrap();

        let shell_for_external_check = shell.clone();
        let last_external_check: Rc<Cell<Option<Instant>>> = Rc::new(Cell::new(None));
        let external_check_window = window;
        window
            .update(cx, |_, window, cx| {
                schedule_external_check(
                    window,
                    cx,
                    external_check_window,
                    shell_for_external_check,
                    last_external_check,
                );
            })
            .unwrap();

        let shell_new_game = shell.clone();
        cx.on_action(move |_: &NewGame, cx| {
            shell_new_game.update(cx, |shell, cx| shell.new_game(cx));
        });
        let shell_open = shell.clone();
        cx.on_action(move |_: &OpenGame, cx| {
            shell_open.update(cx, |shell, cx| shell.open(cx));
        });
        let shell_toggle_engines = shell.clone();
        cx.on_action(move |_: &ToggleEnginesSidebar, cx| {
            shell_toggle_engines.update(cx, |shell, cx| {
                shell.toggle_sidebar_setting("view.show_leftsidebar", "engines sidebar", cx)
            });
        });
        let shell_game_info = shell.clone();
        cx.on_action(move |_: &OpenGameInfo, cx| {
            shell_game_info.update(cx, |shell, cx| shell.open_game_info(cx));
        });
        let shell_score = shell.clone();
        cx.on_action(move |_: &OpenScore, cx| {
            shell_score.update(cx, |shell, cx| shell.open_score(cx));
        });
        let shell_about = shell.clone();
        cx.on_action(move |_: &OpenAbout, cx| {
            shell_about.update(cx, |shell, cx| shell.open_about(cx));
        });
        let shell_toggle_graph = shell.clone();
        cx.on_action(move |_: &ToggleGameGraph, cx| {
            shell_toggle_graph.update(cx, |shell, cx| {
                shell.toggle_sidebar_setting("view.show_graph", "game graph", cx)
            });
        });
        let shell_toggle_comments = shell.clone();
        cx.on_action(move |_: &ToggleComments, cx| {
            shell_toggle_comments.update(cx, |shell, cx| {
                shell.toggle_sidebar_setting("view.show_comments", "comments", cx)
            });
        });
        let shell_toggle_coords = shell.clone();
        cx.on_action(move |_: &ToggleCoordinates, cx| {
            shell_toggle_coords.update(cx, |shell, cx| {
                shell.toggle_view_setting("view.show_coordinates", "board coordinates", cx)
            });
        });
        let shell_toggle_move_nums = shell.clone();
        cx.on_action(move |_: &ToggleMoveNumbers, cx| {
            shell_toggle_move_nums.update(cx, |shell, cx| {
                shell.toggle_view_setting("view.show_move_numbers", "move numbers", cx)
            });
        });
        let shell_session_match = shell.clone();
        cx.on_action(move |_: &SetSessionMatch, cx| {
            shell_session_match.update(cx, |shell, cx| shell.set_session_mode(SessionMode::Match, cx));
        });
        let shell_session_record = shell.clone();
        cx.on_action(move |_: &SetSessionRecord, cx| {
            shell_session_record.update(cx, |shell, cx| shell.set_session_mode(SessionMode::Record, cx));
        });
        let shell_session_live = shell.clone();
        cx.on_action(move |_: &SetSessionLive, cx| {
            shell_session_live.update(cx, |shell, cx| shell.set_session_mode(SessionMode::Live, cx));
        });
        let shell_players_hvh = shell.clone();
        cx.on_action(move |_: &SetPlayersHumanVsHuman, cx| {
            shell_players_hvh.update(cx, |shell, cx| {
                shell.set_match_participants(MatchParticipants::human_vs_human(), cx)
            });
        });
        let shell_players_hvai = shell.clone();
        cx.on_action(move |_: &SetPlayersHumanVsAi, cx| {
            shell_players_hvai.update(cx, |shell, cx| {
                shell.set_match_participants(MatchParticipants::human_vs_ai(), cx)
            });
        });
        let shell_players_aihv = shell.clone();
        cx.on_action(move |_: &SetPlayersAiVsHuman, cx| {
            shell_players_aihv.update(cx, |shell, cx| {
                shell.set_match_participants(
                    MatchParticipants {
                        black: PlayerKind::Ai,
                        white: PlayerKind::Human,
                    },
                    cx,
                )
            });
        });
        let shell_players_aiai = shell.clone();
        cx.on_action(move |_: &SetPlayersAiVsAi, cx| {
            shell_players_aiai.update(cx, |shell, cx| {
                shell.set_match_participants(MatchParticipants::ai_vs_ai(), cx)
            });
        });
        let shell_opening_free = shell.clone();
        cx.on_action(move |_: &SetOpeningFree, cx| {
            shell_opening_free.update(cx, |shell, cx| {
                shell.set_opening_convention(OpeningConvention::Free, cx)
            });
        });
        let shell_opening_ancient = shell.clone();
        cx.on_action(move |_: &SetOpeningAncientSeatStones, cx| {
            shell_opening_ancient.update(cx, |shell, cx| {
                shell.set_opening_convention(OpeningConvention::ChineseAncientSeatStones, cx)
            });
        });
        let shell_time_none = shell.clone();
        cx.on_action(move |_: &SetTimeNone, cx| {
            shell_time_none.update(cx, |shell, cx| shell.set_time_control(TimeControl::None, cx));
        });
        let shell_time_absolute = shell.clone();
        cx.on_action(move |_: &SetTimeAbsolute600, cx| {
            shell_time_absolute.update(cx, |shell, cx| {
                shell.set_time_control(TimeControl::Absolute { main_time_secs: 600 }, cx)
            });
        });
        let shell_time_byoyomi = shell.clone();
        cx.on_action(move |_: &SetTimeByoYomi, cx| {
            shell_time_byoyomi.update(cx, |shell, cx| {
                shell.set_time_control(
                    TimeControl::ByoYomi {
                        main_time_secs: 600,
                        period_time_secs: 30,
                        periods: 5,
                    },
                    cx,
                )
            });
        });
        let shell_play_mode = shell.clone();
        cx.on_action(move |_: &SetPlayMode, cx| {
            shell_play_mode.update(cx, |shell, cx| shell.set_mode(GameMode::Play, cx));
        });
        let shell_edit_mode = shell.clone();
        cx.on_action(move |_: &SetEditMode, cx| {
            shell_edit_mode.update(cx, |shell, cx| shell.set_mode(GameMode::Edit, cx));
        });
        let shell_scoring_mode = shell.clone();
        cx.on_action(move |_: &SetScoringMode, cx| {
            shell_scoring_mode.update(cx, |shell, cx| shell.set_mode(GameMode::Scoring, cx));
        });
        let shell_estimator_mode = shell.clone();
        cx.on_action(move |_: &SetEstimatorMode, cx| {
            shell_estimator_mode.update(cx, |shell, cx| shell.set_mode(GameMode::Estimator, cx));
        });
        let shell_start_analysis = shell.clone();
        cx.on_action(move |_: &StartAnalysis, cx| {
            shell_start_analysis.update(cx, |shell, cx| shell.start_analysis(cx));
        });
        let shell_stop_analysis = shell.clone();
        cx.on_action(move |_: &StopAnalysis, cx| {
            shell_stop_analysis.update(cx, |shell, cx| shell.stop_analysis(cx));
        });
        let shell_genmove = shell.clone();
        cx.on_action(move |_: &GenerateEngineMove, cx| {
            shell_genmove.update(cx, |shell, cx| shell.generate_engine_move(cx));
        });
        let shell_save = shell.clone();
        cx.on_action(move |_: &SaveGame, cx| {
            shell_save.update(cx, |shell, cx| shell.save(cx));
        });
        let shell_save_as = shell.clone();
        cx.on_action(move |_: &SaveGameAs, cx| {
            shell_save_as.update(cx, |shell, cx| shell.save_as(cx));
        });
        let shell_undo = shell.clone();
        cx.on_action(move |_: &UndoMove, cx| {
            shell_undo.update(cx, |shell, cx| shell.undo(cx));
        });
        let shell_redo = shell.clone();
        cx.on_action(move |_: &RedoMove, cx| {
            shell_redo.update(cx, |shell, cx| shell.redo(cx));
        });
        let shell_first = shell.clone();
        cx.on_action(move |_: &GoToFirstNode, cx| {
            shell_first.update(cx, |shell, cx| {
                shell.navigate(NavigationDirection::First, cx)
            });
        });
        let shell_previous = shell.clone();
        cx.on_action(move |_: &GoToPreviousNode, cx| {
            shell_previous.update(cx, |shell, cx| {
                shell.navigate(NavigationDirection::Previous, cx)
            });
        });
        let shell_next = shell.clone();
        cx.on_action(move |_: &GoToNextNode, cx| {
            shell_next.update(cx, |shell, cx| {
                shell.navigate(NavigationDirection::Next, cx)
            });
        });
        let shell_preferences = shell.clone();
        cx.on_action(move |_: &OpenPreferences, cx| {
            shell_preferences.update(cx, |shell, cx| shell.open_preferences(cx));
        });
        let shell_th_classic = shell.clone();
        cx.on_action(move |_: &SetThemeClassic, cx| {
            shell_th_classic.update(cx, |shell, cx| shell.on_theme_selected(ThemeChoice::Classic, cx));
        });
        let shell_th_dark = shell.clone();
        cx.on_action(move |_: &SetThemeDark, cx| {
            shell_th_dark.update(cx, |shell, cx| shell.on_theme_selected(ThemeChoice::Dark, cx));
        });
        let shell_th_mist = shell.clone();
        cx.on_action(move |_: &SetThemeMist, cx| {
            shell_th_mist.update(cx, |shell, cx| shell.on_theme_selected(ThemeChoice::Mist, cx));
        });
        let shell_bs19 = shell.clone();
        cx.on_action(move |_: &SetBoardSize19, cx| {
            shell_bs19.update(cx, |shell, cx| shell.on_board_size_selected(19, cx));
        });
        let shell_bs13 = shell.clone();
        cx.on_action(move |_: &SetBoardSize13, cx| {
            shell_bs13.update(cx, |shell, cx| shell.on_board_size_selected(13, cx));
        });
        let shell_bs9 = shell.clone();
        cx.on_action(move |_: &SetBoardSize9, cx| {
            shell_bs9.update(cx, |shell, cx| shell.on_board_size_selected(9, cx));
        });
        let shell_ca1 = shell.clone();
        cx.on_action(move |_: &SetCoordsA1, cx| {
            shell_ca1.update(cx, |shell, cx| {
                let _ = shell.settings.set("view.coordinates_type", serde_json::json!("A1"));
                cx.notify();
            });
        });
        let shell_c11 = shell.clone();
        cx.on_action(move |_: &SetCoords1_1, cx| {
            shell_c11.update(cx, |shell, cx| {
                let _ = shell.settings.set("view.coordinates_type", serde_json::json!("1-1"));
                cx.notify();
            });
        });
        let shell_v100 = shell.clone();
        cx.on_action(move |_: &SetVisits100, cx| {
            shell_v100.update(cx, |shell, cx| shell.apply_analysis_visits(100, cx));
        });
        let shell_v500 = shell.clone();
        cx.on_action(move |_: &SetVisits500, cx| {
            shell_v500.update(cx, |shell, cx| shell.apply_analysis_visits(500, cx));
        });
        let shell_v1000 = shell.clone();
        cx.on_action(move |_: &SetVisits1000, cx| {
            shell_v1000.update(cx, |shell, cx| shell.apply_analysis_visits(1000, cx));
        });
        let shell_v0 = shell.clone();
        cx.on_action(move |_: &SetVisitsUnlimited, cx| {
            shell_v0.update(cx, |shell, cx| shell.apply_analysis_visits(0, cx));
        });
        let shell_review_quick = shell.clone();
        cx.on_action(move |_: &StartReviewQuick, cx| {
            shell_review_quick.update(cx, |shell, cx| {
                shell.start_review_profile_action(ryusei_domain_core::ReviewProfile::Quick, cx)
            });
        });
        let shell_review_preliminary = shell.clone();
        cx.on_action(move |_: &StartReviewPreliminary, cx| {
            shell_review_preliminary.update(cx, |shell, cx| {
                shell.start_review_profile_action(ryusei_domain_core::ReviewProfile::Preliminary, cx)
            });
        });
        let shell_review_intermediate = shell.clone();
        cx.on_action(move |_: &StartReviewIntermediate, cx| {
            shell_review_intermediate.update(cx, |shell, cx| {
                shell.start_review_profile_action(ryusei_domain_core::ReviewProfile::Intermediate, cx)
            });
        });
        let shell_review_advanced = shell.clone();
        cx.on_action(move |_: &StartReviewAdvanced, cx| {
            shell_review_advanced.update(cx, |shell, cx| {
                shell.start_review_profile_action(ryusei_domain_core::ReviewProfile::Advanced, cx)
            });
        });
        let shell_p_katago = shell.clone();
        cx.on_action(move |_: &PluginKataGoSetup, cx| {
            shell_p_katago.update(cx, |shell, cx| {
                shell.open_engine_config_panel(EngineConfigPanel::KataGo, cx);
            });
        });
        let shell_p_fox = shell.clone();
        cx.on_action(move |_: &PluginFoxSync, cx| {
            shell_p_fox.update(cx, |shell, cx| {
                shell.open_engine_config_panel(EngineConfigPanel::FoxSync, cx);
            });
        });
        let shell_p_pos = shell.clone();
        cx.on_action(move |_: &PluginPositionToSgf, cx| {
            shell_p_pos.update(cx, |shell, cx| {
                shell.open_engine_config_panel(EngineConfigPanel::PositionSgf, cx);
            });
        });
        let shell_p_zip = shell.clone();
        cx.on_action(move |_: &PluginInstallZip, cx| {
            shell_p_zip.update(cx, |shell, cx| {
                shell.install_plugin_zip(cx);
            });
        });
        let shell_p_menu = shell.clone();
        cx.on_action(move |_: &TogglePluginMenu, cx| {
            shell_p_menu.update(cx, |shell, cx| {
                shell.toggle_plugin_popover("all", cx);
            });
        });
        let shell_last = shell.clone();
        cx.on_action(move |_: &GoToLastNode, cx| {
            shell_last.update(cx, |shell, cx| {
                shell.navigate(NavigationDirection::Last, cx)
            });
        });
        let shell_gtp_terminal = shell.clone();
        cx.on_action(move |_: &ToggleGtpTerminal, cx| {
            shell_gtp_terminal.update(cx, |shell, cx| {
                shell.toggle_bottom_deck_tab(BottomDeckTab::GtpTerminal, cx);
            });
        });
        let shell_bottom_deck = shell.clone();
        cx.on_action(move |_: &ToggleBottomDeck, cx| {
            shell_bottom_deck.update(cx, |shell, cx| {
                if shell.is_bottom_deck_open() {
                    shell.close_bottom_deck(cx);
                } else {
                    shell.switch_bottom_tab(BottomDeckTab::WinrateGraph, cx);
                }
            });
        });
        let shell_review = shell.clone();
        cx.on_action(move |_: &StartWholeGameReview, cx| {
            shell_review.update(cx, |shell, cx| shell.start_whole_game_review(cx));
        });
        let shell_export_gif = shell.clone();
        cx.on_action(move |_: &ExportGif, cx| {
            shell_export_gif.update(cx, |shell, cx| {
                let snapshot = shell.host.snapshot();
                let options = ryusei_host::GifExportOptions::default();
                match ryusei_host::export_sgf_to_gif(&snapshot, &options) {
                    Ok(gif_bytes) => {
                        let suggested = format!("saba_game_{}.gif", std::process::id());
                        let dest = shell
                            .dialog_service
                            .pick_save_gif_path(&suggested)
                            .unwrap_or_else(|| std::env::temp_dir().join(&suggested));
                        if std::fs::write(&dest, &gif_bytes).is_ok() {
                            shell.show_toast(
                                format!("🎬 成功导出 GIF 动画棋谱: {}", dest.display()),
                                cx,
                            );
                        }
                    }
                    Err(e) => {
                        shell.show_toast(format!("GIF 导出失败: {e}"), cx);
                    }
                }
                cx.notify();
            });
        });
        let shell_export_png = shell.clone();
        cx.on_action(move |_: &ExportPositionPng, cx| {
            shell_export_png.update(cx, |shell, cx| shell.export_current_position_png(cx));
        });
        cx.on_action(|_: &Quit, cx| cx.quit());

        cx.bind_keys([
            KeyBinding::new("cmd-n", NewGame, None),
            KeyBinding::new("cmd-o", OpenGame, None),
            KeyBinding::new("cmd-s", SaveGame, None),
            KeyBinding::new("cmd-shift-s", SaveGameAs, None),
            KeyBinding::new("cmd-comma", OpenPreferences, None),
            KeyBinding::new("space", StartAnalysis, None),
            KeyBinding::new("cmd-1", SetPlayMode, None),
            KeyBinding::new("cmd-2", SetEditMode, None),
            KeyBinding::new("cmd-3", SetScoringMode, None),
            KeyBinding::new("cmd-4", SetEstimatorMode, None),
            KeyBinding::new("cmd-shift-b", ToggleEnginesSidebar, None),
            KeyBinding::new("cmd-shift-c", ToggleCoordinates, None),
            KeyBinding::new("cmd-shift-m", ToggleMoveNumbers, None),
            KeyBinding::new("cmd-t", ToggleGtpTerminal, None),
            KeyBinding::new("cmd-j", ToggleBottomDeck, None),
            KeyBinding::new("cmd-r", StartWholeGameReview, None),
            KeyBinding::new("cmd-e", ExportGif, None),
            KeyBinding::new("cmd-g", GenerateEngineMove, None),
            KeyBinding::new("cmd-z", UndoMove, None),
            KeyBinding::new("cmd-shift-z", RedoMove, None),
            KeyBinding::new("cmd-left", GoToFirstNode, None),
            KeyBinding::new("left", GoToPreviousNode, None),
            KeyBinding::new("right", GoToNextNode, None),
            KeyBinding::new("cmd-right", GoToLastNode, None),
            // Keyboard focus navigation (Tab / Shift-Tab cycles tab stops).
            KeyBinding::new("tab", FocusNext, None),
            KeyBinding::new("shift-tab", FocusPrev, None),
            KeyBinding::new("cmd-q", Quit, None),
        ]);
        if shell
            .read(cx)
            .settings
            .get_bool("view.show_menubar")
            .unwrap_or(true)
        {
            cx.set_menus(shell_menus());
        } else {
            cx.set_menus(Vec::new());
        }
        cx.activate(true);
    });
}

#[cfg(test)]
mod tests {
    use super::{
        CloseChoice, close_decision, default_new_game_properties,
        default_new_game_properties_for_size,
    };
    use ryusei_host::{CloseRequestAction, SettingsStore, decide_close_request};
    use serde_json::json;

    #[test]
    fn shell_menus_cover_file_edit_view_session_engine_and_navigation() {
        let names = super::shell_menus()
            .into_iter()
            .map(|menu| menu.name.to_string())
            .collect::<Vec<_>>();
        for expected in [
            "File", "Edit", "View", "Session", "Engines", "Navigate", "Help",
        ] {
            assert!(
                names.iter().any(|name| name == expected),
                "missing {expected} menu"
            );
        }
    }

    #[test]
    fn clean_documents_skip_the_close_confirmation() {
        assert_eq!(
            decide_close_request(false, false),
            CloseRequestAction::Allow
        );
    }

    #[test]
    fn dirty_documents_require_a_confirmation() {
        assert_eq!(
            decide_close_request(true, false),
            CloseRequestAction::ConfirmDiscard
        );
    }

    #[test]
    fn a_successful_save_or_explicit_discard_allows_the_close() {
        assert!(close_decision(CloseChoice::Save, true));
        assert!(close_decision(CloseChoice::Discard, false));
    }

    #[test]
    fn a_cancelled_or_failed_save_keeps_the_window_open() {
        assert!(!close_decision(CloseChoice::Cancel, false));
        assert!(!close_decision(CloseChoice::Save, false));
        assert!(!close_decision(CloseChoice::Cancel, true));
    }

    #[test]
    fn new_game_defaults_fall_back_to_upstream_values() {
        let settings = SettingsStore::default();
        let (size, properties) = default_new_game_properties(&settings);

        assert_eq!(size, 19);
        assert_eq!(properties.get("KM").unwrap(), &vec!["6.5".to_owned()]);
        assert!(!properties.contains_key("RU"));
        assert!(!properties.contains_key("HA"));
    }

    #[test]
    fn new_game_defaults_can_select_a_ruleset_for_katago_and_sgf() {
        let mut settings = SettingsStore::default();
        settings
            .set("game.default_ruleset", json!("Japanese"))
            .unwrap();
        let (_, properties) = default_new_game_properties(&settings);
        assert_eq!(properties.get("RU").unwrap(), &vec!["Japanese".to_owned()]);
    }

    #[test]
    fn new_game_defaults_apply_size_komi_and_standard_handicap_stones() {
        let mut settings = SettingsStore::default();
        settings.set("game.default_board_size", json!(13)).unwrap();
        settings.set("game.default_komi", json!(0.5)).unwrap();
        settings.set("game.default_handicap", json!(4)).unwrap();

        let (size, properties) = default_new_game_properties(&settings);
        assert_eq!(size, 13);
        assert_eq!(properties.get("KM").unwrap(), &vec!["0.5".to_owned()]);
        assert_eq!(properties.get("HA").unwrap(), &vec!["4".to_owned()]);
        assert_eq!(
            properties.get("AB").unwrap(),
            &vec![
                "dj".to_owned(),
                "jd".to_owned(),
                "jj".to_owned(),
                "dd".to_owned()
            ]
        );

        // Selecting a board-size button must place stones for that size,
        // not for the default size stored in settings.
        let size_nine_properties = default_new_game_properties_for_size(&settings, 9);
        assert_eq!(
            size_nine_properties.get("HA").unwrap(),
            &vec!["4".to_owned()]
        );
        assert_eq!(
            size_nine_properties.get("AB").unwrap(),
            &vec![
                "cg".to_owned(),
                "gc".to_owned(),
                "gg".to_owned(),
                "cc".to_owned()
            ]
        );
    }

    #[test]
    fn ancient_seat_stones_preserve_rules_and_komi_while_adding_setup_stones() {
        let mut settings = SettingsStore::default();
        settings
            .set("game.opening_convention", json!("chineseAncientSeatStones"))
            .unwrap();
        let (size, properties) = default_new_game_properties(&settings);
        assert_eq!(size, 19);
        assert_eq!(properties.get("KM").unwrap(), &vec!["6.5".to_owned()]);
        assert!(!properties.contains_key("RU"));
        assert_eq!(properties.get("AB").unwrap().len(), 4);
        assert!(properties.get("AB").unwrap().contains(&"dd".to_owned()));
        assert!(properties.get("AB").unwrap().contains(&"pp".to_owned()));
    }
}

/// Headless application-logic smoke tests (Beta gate #10, partial).
///
/// gpui 0.2.2 cannot render offscreen frames, but `App::headless()` lets the
/// full `ShellApp` entity run without a window, so the core interaction
/// state machine (open/play/score/theme) is exercised headlessly on macOS
/// and Linux. Windows ignores the headless flag (it would open a real
/// window), so the tests skip there.
#[cfg(test)]
mod headless_smoke {
    use super::ShellApp;
    use crate::dialog_service::MockDialogService;
    use gpui::{AppContext, Entity};
    use ryusei_domain_core::{Color, GameMode, Vertex};
    use ryusei_host::SettingsStore;
    use std::path::PathBuf;

    use crate::RecordingSink;
    use crate::file_workflow::{
        NativeHostPersistence, NativePluginPersistence, NativeSettingsPersistence,
    };

    fn temp_config(test_name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ryusei-headless-{test_name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp config dir is created");
        dir
    }

    /// Runs the closure with a TestAppContext (gpui test-support): the full
    /// ShellApp entity runs headlessly without windows or GPU. Skipped on
    /// Windows where the test platform is unavailable.
    fn with_headless_shell(test_name: &str, run: impl FnOnce(&mut ShellApp)) {
        with_headless_shell_cx(test_name, |shell, _| run(shell));
    }

    fn with_headless_shell_cx(
        test_name: &str,
        run: impl FnOnce(&mut ShellApp, &mut gpui::Context<ShellApp>),
    ) {
        use gpui::{TestAppContext, TestDispatcher};
        use rand::SeedableRng;
        let config = temp_config(test_name);
        let dispatcher = TestDispatcher::new(rand::rngs::StdRng::seed_from_u64(0));
        let mut cx = TestAppContext::build(dispatcher, None);
        let shell_entity: Entity<ShellApp> = cx.new(|cx| {
            ShellApp::new(
                SettingsStore::default(),
                NativeSettingsPersistence::new(config.clone()),
                NativeHostPersistence::new(config.clone()),
                NativePluginPersistence::new(config.clone()),
                "headless smoke".to_owned(),
                None,
                Box::new(MockDialogService::default()),
                cx,
            )
        });
        shell_entity.update(&mut cx, |shell, cx| run(shell, cx));
        let _ = std::fs::remove_dir_all(&config);
    }
    #[test]
    fn headless_shell_opens_and_plays_moves() {
        with_headless_shell("open-play", |shell| {
            let snapshot = shell.host.snapshot();
            assert_eq!(snapshot.board.width, 19);
            // ShellApp::new now creates a clean default game instead of
            // seeding demonstration moves.
            let initial_moves = snapshot.moves.len();
            assert_eq!(initial_moves, 0);
            assert_eq!(
                snapshot
                    .root_properties
                    .get("KM")
                    .and_then(|values| values.first()),
                Some(&"6.5".to_owned())
            );

            let mut events = RecordingSink;
            shell
                .host
                .play_move(
                    Color::Black,
                    Some(Vertex {
                        column: 16,
                        row: 16,
                    }),
                    &mut events,
                )
                .expect("the move is legal");
            assert_eq!(shell.host.snapshot().moves.len(), initial_moves + 1);
            assert_eq!(
                shell.host.snapshot().board.sign_map[16][16],
                1,
                "black stone must land on the board"
            );
        });
    }

    #[test]
    fn headless_shell_toggles_scoring_mode_and_applies_an_override() {
        with_headless_shell("scoring", |shell| {
            shell.mode = GameMode::Scoring;
            let vertex = Vertex { column: 3, row: 3 };
            let current = shell.host.snapshot().score_overrides.get(&vertex).copied();
            let transaction = crate::markup::create_scoring_transaction(
                vertex,
                crate::markup::next_scoring_override(current),
            );
            let mut events = RecordingSink;
            shell
                .host
                .apply_transaction(transaction, &mut events)
                .expect("the scoring override applies");
            assert_eq!(
                shell.host.snapshot().score_overrides.get(&vertex),
                Some(&1),
                "first cycle marks the vertex alive black"
            );
        });
    }

    #[test]
    fn headless_shell_applies_an_installed_theme_tokens() {
        with_headless_shell("theme", |shell| {
            let tokens = crate::theme::ThemeTokens::default();
            shell.theme = tokens;
            assert_eq!(shell.theme.board_wood_color().rgb_u32(), 0xd9a866);
        });
    }

    #[test]
    fn headless_release_fixture_workflow_opens_edits_saves_and_reopens() {
        let fixture = std::env::temp_dir().join(format!(
            "ryusei-release-fixture-workflow-{}.sgf",
            std::process::id()
        ));
        std::fs::write(
            &fixture,
            "(;GM[1]FF[4]SZ[9]KM[6.5]PB[Black]PW[White];B[dd];W[ee])",
        )
        .expect("fixture is written");

        with_headless_shell("release-fixture", |shell| {
            let mut events = RecordingSink;
            shell
                .host
                .open(fixture.clone(), &shell.file_access, &mut events)
                .expect("fixture opens through the native file port");
            assert_eq!(shell.host.snapshot().moves.len(), 2);

            let next = shell.host.snapshot().board.next_player;
            shell
                .host
                .play_move(next, Some(Vertex { column: 2, row: 2 }), &mut events)
                .expect("a legal edit applies after opening");
            let node_id = shell.host.snapshot().current_node_id;
            shell
                .host
                .apply_transaction(
                    crate::node_inspector::create_comment_transaction(
                        &node_id,
                        "release workflow comment",
                    ),
                    &mut events,
                )
                .expect("comment transaction applies");
            shell
                .host
                .save(&mut shell.file_access, &mut events)
                .expect("edited fixture saves atomically");

            shell
                .host
                .open(fixture.clone(), &shell.file_access, &mut events)
                .expect("saved fixture reopens");
            let snapshot = shell.host.snapshot();
            assert_eq!(snapshot.moves.len(), 3);
            assert!(snapshot.nodes.iter().any(|node| node.properties.get("C")
                == Some(&vec!["release workflow comment".to_owned()])));
        });
        let _ = std::fs::remove_file(&fixture);
    }

    #[test]
    fn headless_shell_reports_analysis_command_from_settings() {
        with_headless_shell("analysis-cmd", |shell| {
            let (command, arguments) =
                crate::engine_console::analysis_command_from_settings(&shell.settings);
            assert_eq!(command, "kata-analyze");
            assert_eq!(arguments, vec!["B", "100", "rootInfo", "true"]);
        });
    }

    #[test]
    fn fresh_profile_enables_analysis_markers_and_engine_sidebar() {
        with_headless_shell("analysis-visible-defaults", |shell| {
            assert_eq!(shell.settings.get_bool("board.show_analysis"), Some(true));
            assert_eq!(shell.settings.get_bool("view.show_leftsidebar"), Some(true));
        });
    }

    #[test]
    fn live_session_is_read_only_at_the_board_boundary() {
        with_headless_shell_cx("live-read-only", |shell, cx| {
            shell.set_session_mode(ryusei_domain_core::SessionMode::Live, cx);
            assert_eq!(
                shell.session_policy.source,
                ryusei_domain_core::SessionSource::LiveBroadcast
            );
            let before = shell.host.snapshot();
            shell.on_board_vertex_mouse_down(Vertex { column: 3, row: 3 }, cx);
            let after = shell.host.snapshot();
            assert_eq!(after.moves.len(), before.moves.len());
            assert_eq!(after.current_node_id, before.current_node_id);
        });
    }

    #[test]
    fn remote_competition_board_clicks_do_not_play_locally() {
        with_headless_shell_cx("remote-board-readonly", |shell, cx| {
            shell.ogs_auth_state = ryusei_host::OgsAuthState::Authenticated;
            shell.enter_ogs_remote_match(cx);
            let before = shell.host.snapshot();
            shell.on_board_vertex_mouse_down(Vertex { column: 3, row: 3 }, cx);
            let after = shell.host.snapshot();
            assert_eq!(after.moves.len(), before.moves.len());
        });
    }

    #[test]
    fn ogs_dead_stone_marking_toggles_and_serializes() {
        with_headless_shell_cx("ogs-dead-stones", |shell, cx| {
            assert!(!shell.ogs_marking_dead);
            shell.ogs_toggle_dead_marking(cx);
            assert!(shell.ogs_marking_dead);
            shell.ogs_removed_stones.insert("dd".to_owned());
            shell.ogs_removed_stones.insert("pp".to_owned());
            assert_eq!(shell.ogs_removed_stones_string(), "ddpp");
            shell.ogs_toggle_dead_marking(cx);
            assert!(!shell.ogs_marking_dead);
            assert!(shell.ogs_removed_stones.is_empty());
        });
    }

    #[test]
    fn human_sl_profile_picker_only_saves_valid_profiles() {
        with_headless_shell_cx("human-sl-profile", |shell, cx| {
            shell.set_human_sl_profile("rank_3d", cx);
            assert_eq!(
                shell.settings.get_str("katago.human_sl_profile"),
                Some("rank_3d")
            );
            shell.set_human_sl_profile("invalid", cx);
            assert_eq!(
                shell.settings.get_str("katago.human_sl_profile"),
                Some("rank_3d")
            );
        });
    }

    #[test]
    fn engine_manager_saves_records_and_assigns_roles() {
        with_headless_shell_cx("engine-manager", |shell, cx| {
            // The engine manager moved to the left engine sidebar; switching to
            // its deck tab reroutes there instead of opening a deck panel.
            shell.switch_bottom_tab(super::BottomDeckTab::Engines, cx);
            assert_eq!(
                shell.engine_config_panel,
                Some(super::EngineConfigPanel::Engines)
            );

            shell
                .text_inputs
                .engine_spec_input
                .set_text("GNU Go | /usr/local/bin/gnugo | --mode gtp | level 10");
            shell.save_engine_spec(cx);
            assert!(
                shell
                    .engine_store
                    .list()
                    .iter()
                    .any(|record| record.name == "GNU Go")
            );

            shell.assign_engine_role(crate::engine_console::EngineRole::Black, "GNU Go", cx);
            assert_eq!(
                shell
                    .engine_roles
                    .get(crate::engine_console::EngineRole::Black),
                Some("GNU Go")
            );
        });
    }

    #[test]
    fn current_game_ruleset_updates_local_sgf_and_blocks_live_sources() {
        with_headless_shell_cx("current-ruleset", |shell, cx| {
            shell.set_current_game_ruleset(ryusei_host::GoRuleset::Japanese, cx);
            assert_eq!(
                shell
                    .host
                    .snapshot()
                    .root_properties
                    .get("RU")
                    .and_then(|values| values.first()),
                Some(&"Japanese".to_owned())
            );
            shell.apply_current_ruleset_default_komi(cx);
            assert_eq!(
                shell
                    .host
                    .snapshot()
                    .root_properties
                    .get("KM")
                    .and_then(|values| values.first()),
                Some(&"6.5".to_owned())
            );

            shell.set_session_mode(ryusei_domain_core::SessionMode::Live, cx);
            shell.set_current_game_ruleset(ryusei_host::GoRuleset::Korean, cx);
            assert_eq!(
                shell
                    .host
                    .snapshot()
                    .root_properties
                    .get("RU")
                    .and_then(|values| values.first()),
                Some(&"Japanese".to_owned())
            );
        });
    }

    #[test]
    fn review_comment_writeback_preserves_existing_comments() {
        with_headless_shell_cx("review-comments", |shell, cx| {
            let mut events = RecordingSink;
            shell
                .host
                .restore_from_sgf(
                    "(;GM[1]FF[4]SZ[19]C[Original root]SBKV[90]SBKS[8];B[dd]C[Original move]SBKV[30]SBKS[-1])",
                    &mut events,
                )
                .expect("review fixture loads");
            shell.write_review_comments(cx);
            let snapshot = shell.host.snapshot();
            assert!(
                snapshot
                    .root_properties
                    .get("C")
                    .and_then(|values| values.first())
                    .is_some_and(|comment| comment.contains("Original root")
                        && comment.contains("Ryusei Review"))
            );
            assert!(
                snapshot
                    .nodes
                    .iter()
                    .find(|node| node.id == snapshot.current_node_id)
                    .and_then(|node| node.properties.get("C"))
                    .and_then(|values| values.first())
                    .is_some_and(|comment| comment.contains("Original move")
                        && comment.contains("Ryusei Review"))
            );
        });
    }

    #[test]
    fn remote_match_exposes_fair_play_lock_and_can_return_local() {
        with_headless_shell_cx("remote-fair-play", |shell, cx| {
            shell.ogs_auth_state = ryusei_host::OgsAuthState::Authenticated;
            shell.enter_ogs_remote_match(cx);
            assert_eq!(
                shell.session_policy.mode,
                ryusei_domain_core::SessionMode::Match
            );
            assert_eq!(
                shell.session_policy.source,
                ryusei_domain_core::SessionSource::RemoteCompetition
            );
            assert_eq!(
                shell.session_policy.analysis,
                ryusei_domain_core::AnalysisPolicy::FairPlayLockedOff
            );
            assert!(!shell.analysis_enabled);

            shell.leave_remote_match(cx);
            assert_eq!(
                shell.session_policy.source,
                ryusei_domain_core::SessionSource::Local
            );
            // Exiting a remote match now lands in free-analysis (Record) mode.
            assert_eq!(
                shell.session_policy.mode,
                ryusei_domain_core::SessionMode::Record
            );
            assert_eq!(
                shell.session_policy.analysis,
                ryusei_domain_core::AnalysisPolicy::Manual
            );
        });
    }

    #[test]
    fn remote_competition_stops_inherited_analysis_stream() {
        with_headless_shell_cx("remote-analysis-stop", |shell, cx| {
            // An analysis engine connected before the OGS game must not keep
            // streaming onto the remote board.
            shell.ogs_auth_state = ryusei_host::OgsAuthState::Authenticated;
            shell.analysis_enabled = true;
            shell.analysis = vec![ryusei_host::AnalysisEntry {
                id: Some(1),
                vertex: Some("D4".to_owned()),
                visits: 100,
                winrate: 0.55,
                score_lead: None,
                pv: Vec::new(),
                is_during_search: false,
                ownership: None,
                prior: None,
            }];
            shell.analysis_best_move = Some(Vertex { column: 3, row: 3 });

            shell.enter_ogs_remote_match(cx);
            assert_eq!(
                shell.session_policy.analysis,
                ryusei_domain_core::AnalysisPolicy::FairPlayLockedOff
            );
            assert!(!shell.analysis_enabled);
            assert!(shell.analysis.is_empty());
            assert_eq!(shell.analysis_best_move, None);
        });
    }

    #[test]
    fn remote_competition_drops_streamed_analysis_batches() {
        with_headless_shell_cx("remote-analysis-batches", |shell, cx| {
            shell.session_policy = ryusei_domain_core::SessionPolicy::new(
                ryusei_domain_core::SessionMode::Match,
                ryusei_domain_core::SessionSource::RemoteCompetition,
            )
            .lock_fair_play(true);
            let node_id = shell.host.snapshot().current_node_id.clone();
            let run = shell
                .analysis_run
                .begin(node_id, ryusei_domain_core::Color::Black);
            let entry = ryusei_host::AnalysisEntry {
                id: Some(1),
                vertex: Some("D4".to_owned()),
                visits: 100,
                winrate: 0.55,
                score_lead: None,
                pv: Vec::new(),
                is_during_search: false,
                ownership: None,
                prior: None,
            };

            // A leftover interactive stream must be dropped, never displayed.
            shell.push_analysis_batch(&run, vec![entry.clone()], cx);
            assert!(shell.analysis.is_empty());

            // The opted-in per-move background review is still allowed.
            shell.background_review = true;
            shell.push_analysis_batch(&run, vec![entry], cx);
            assert_eq!(shell.analysis.len(), 1);
        });
    }

    #[test]
    fn markup_tools_apply_markers_through_the_shell() {
        with_headless_shell_cx("markup-tools", |shell, cx| {
            shell.active_tool = crate::markup::MarkupTool::Triangle;
            shell.set_mode(ryusei_domain_core::GameMode::Edit, cx);
            shell.on_board_vertex_mouse_down(Vertex { column: 3, row: 3 }, cx);
            let snapshot = shell.host.snapshot();
            let node = snapshot
                .nodes
                .iter()
                .find(|node| node.id == snapshot.current_node_id)
                .expect("current node exists");
            assert!(
                node.properties.contains_key("TR"),
                "triangle markup must be applied, got {:?}",
                node.properties
            );
            assert!(snapshot.board.markers[3][3].is_some());
        });
    }

    #[test]
    fn estimate_mode_toggles_off_when_clicked_again() {
        with_headless_shell_cx("estimate-toggle", |shell, cx| {
            shell.set_mode(ryusei_domain_core::GameMode::Estimator, cx);
            assert_eq!(shell.mode, ryusei_domain_core::GameMode::Estimator);
            // Second click of the 估目 button returns to play mode, so no
            // separate exit button is required.
            if shell.mode == ryusei_domain_core::GameMode::Estimator {
                shell.set_mode(ryusei_domain_core::GameMode::Play, cx);
            } else {
                shell.set_mode(ryusei_domain_core::GameMode::Estimator, cx);
            }
            assert_eq!(shell.mode, ryusei_domain_core::GameMode::Play);
            assert_eq!(shell.active_tool, crate::markup::MarkupTool::Play);
        });
    }

    #[test]
    fn ogs_account_starts_signed_out_and_tracks_client_state() {
        with_headless_shell_cx("ogs-account", |shell, cx| {
            assert_eq!(shell.ogs_auth_state, ryusei_host::OgsAuthState::SignedOut);
            assert_eq!(
                shell.ogs_client.snapshot().socket_status,
                ryusei_host::OgsSocketStatus::Disconnected
            );
            shell.refresh_ogs_account_state(cx);
            assert_eq!(shell.ogs_auth_state, ryusei_host::OgsAuthState::SignedOut);

            // Invalid game id must not enter remote mode.
            shell.text_inputs.ogs_game_id_input.set_text("not-a-number");
            shell.connect_ogs_game(cx);
            assert_ne!(
                shell.session_policy.source,
                ryusei_domain_core::SessionSource::RemoteCompetition
            );
        });
    }

    #[test]
    fn library_sources_round_trip_as_persisted_authorized_configurations() {
        with_headless_shell("library-sources", |shell| {
            let source = ryusei_host::SgfLibrarySource {
                id: "licensed-games".to_owned(),
                name: "Licensed Games".to_owned(),
                github_url: "https://github.com/example/licensed-games".to_owned(),
                reference: "main".to_owned(),
                rights: ryusei_host::RedistributionRights::Permitted,
                license_name: Some("CC BY 4.0".to_owned()),
                license_url: Some("https://creativecommons.org/licenses/by/4.0/".to_owned()),
            };
            shell
                .persist_library_source(&source)
                .expect("source persists");
            let encoded = shell
                .settings
                .get_str("library.sources")
                .expect("source setting exists");
            let decoded: Vec<ryusei_host::SgfLibrarySource> =
                serde_json::from_str(encoded).expect("source setting decodes");
            assert_eq!(decoded, vec![source]);
        });
    }

    #[test]
    fn pinned_plugins_can_be_queried_and_toggled() {
        with_headless_shell_cx("pinned-plugins", |shell, cx| {
            assert!(shell.pinned_plugin_ids().is_empty());
            shell.toggle_plugin_pinned("org.ryusei.fox-kifu-sync", cx);
            assert_eq!(
                shell.pinned_plugin_ids(),
                vec!["org.ryusei.fox-kifu-sync".to_owned()]
            );
            shell.toggle_plugin_pinned("org.ryusei.fox-kifu-sync", cx);
            assert!(shell.pinned_plugin_ids().is_empty());
        });
    }
}

#[cfg(test)]
mod frontend_smoke {
    use super::*;
    use crate::dialog_service::MockDialogService;
    use crate::file_workflow::{
        NativeHostPersistence, NativePluginPersistence, NativeSettingsPersistence,
    };
    use gpui::{TestAppContext, TestDispatcher, VisualTestContext, px};
    use rand::SeedableRng;
    use ryusei_host::SettingsStore;
    use std::ops::Deref;
    use std::path::PathBuf;

    fn temp_config(test_name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ryusei-frontend-{test_name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp config dir is created");
        dir
    }

    #[derive(Default)]
    struct BackgroundResponsivenessProbe {
        foreground_steps: usize,
        background_finished: bool,
        task: Option<gpui::Task<()>>,
    }

    impl BackgroundResponsivenessProbe {
        fn start(&mut self, cx: &mut gpui::Context<Self>) {
            self.foreground_steps += 1;
            let weak = cx.entity().downgrade();
            self.task = Some(cx.spawn(
                move |_: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
                    let mut cx = cx.clone();
                    async move {
                        cx.background_executor()
                            .spawn(async {
                                std::thread::sleep(std::time::Duration::from_millis(25));
                            })
                            .await;
                        let _ = weak.update(&mut cx, |probe, _| {
                            probe.background_finished = true;
                        });
                    }
                },
            ));
        }
    }

    #[test]
    fn delayed_analysis_like_background_work_does_not_block_foreground_updates() {
        let dispatcher = TestDispatcher::new(rand::rngs::StdRng::seed_from_u64(17));
        let mut cx = TestAppContext::build(dispatcher, None);
        let probe = cx.new(|_| BackgroundResponsivenessProbe::default());
        probe.update(&mut cx, |probe, cx| probe.start(cx));
        probe.update(&mut cx, |probe, _| probe.foreground_steps += 1);
        probe.update(&mut cx, |probe, _| {
            assert_eq!(probe.foreground_steps, 2);
        });
        cx.run_until_parked();
        probe.update(&mut cx, |probe, _| {
            assert!(probe.background_finished);
        });
    }

    #[test]
    fn keyboard_layout_notification_does_not_abort_during_an_app_action() {
        let cx = TestAppContext::single();
        // `rfd` runs a nested native modal loop while an OpenGame action holds
        // GPUI's App borrow. macOS may emit a keyboard-input-source change in
        // that loop; its observer must not turn the reentrant borrow into an
        // abort (the exact native crash reported by release testing).
        let _action_borrow = cx.app.borrow_mut();
        cx.simulate_keyboard_layout_change();
    }

    #[test]
    fn keyboard_layout_notification_still_reaches_observers_when_idle() {
        let cx = TestAppContext::single();
        let notifications = std::rc::Rc::new(std::cell::RefCell::new(0));
        let subscription = cx.update(|app| {
            let notifications = notifications.clone();
            app.on_keyboard_layout_change(move |_| *notifications.borrow_mut() += 1)
        });

        cx.simulate_keyboard_layout_change();
        assert_eq!(*notifications.borrow(), 1);
        drop(subscription);
    }

    /// Renders the full window with the gpui test platform and simulates a
    /// real left click on a rendered intersection. This catches the original
    /// Beta frontend regression: the goban hitbox was zero-sized and the
    /// board visuals were offset by half the board margin.
    #[test]
    fn fresh_settings_render_visible_engine_sidebar_and_compact_status_bar() {
        let config = temp_config("fresh-defaults");
        let dispatcher = TestDispatcher::new(rand::rngs::StdRng::seed_from_u64(11));
        let mut cx = TestAppContext::build(dispatcher, None);
        cx.update(|cx| {
            gpui_component::init(cx);
            gpui_component::Theme::change(gpui_component::ThemeMode::Dark, None, cx);
        });
        let mut shell_slot: Option<Entity<ShellApp>> = None;
        let shell_ptr = &mut shell_slot as *mut Option<Entity<ShellApp>>;
        let window_handle = cx.add_window(|window, cx| {
            let app = cx.new(|cx| {
                ShellApp::new(
                    SettingsStore::default(),
                    NativeSettingsPersistence::new(config.clone()),
                    NativeHostPersistence::new(config.clone()),
                    NativePluginPersistence::new(config.clone()),
                    "fresh defaults".to_owned(),
                    None,
                    Box::new(MockDialogService::default()),
                    cx,
                )
            });
            unsafe {
                *shell_ptr = Some(app.clone());
            }
            gpui_component::Root::new(app, window, cx)
        });
        let shell = shell_slot.unwrap();
        let vcx = VisualTestContext::from_window(*window_handle.deref(), &cx).into_mut();
        vcx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        vcx.run_until_parked();

        // A fresh profile exposes the engines and analysis controls rather
        // than making a connected KataGo process look inert.
        let navigation_rail = vcx
            .debug_bounds("navigation-rail")
            .expect("a compact global navigation rail is always present");
        let left_sidebar = vcx
            .debug_bounds("left-sidebar")
            .expect("fresh profile renders the engine sidebar");
        assert_eq!(navigation_rail.size.width, px(NAVIGATION_RAIL_WIDTH));
        assert!(navigation_rail.right() <= left_sidebar.origin.x);
        assert!(vcx.debug_bounds("right-sidebar").is_some());
        shell.update(&mut vcx.cx, |shell, cx| {
            shell.toggle_sidebar_setting("view.show_leftsidebar", "engines sidebar", cx);
        });
        assert_eq!(
            shell.read_with(&vcx.cx, |shell, _| {
                shell.settings.get_bool("view.show_leftsidebar")
            }),
            Some(false),
            "the visible first-launch left pane must persist hidden after toggling"
        );
        shell.update(&mut vcx.cx, |shell, cx| {
            shell.toggle_right_sidebar(cx);
        });
        assert_eq!(
            shell.read_with(&vcx.cx, |shell, _| {
                (
                    shell.settings.get_bool("view.show_graph"),
                    shell.settings.get_bool("view.show_comments"),
                )
            }),
            (Some(false), Some(false)),
            "first-launch panels toggle must persist both inferred pane sources hidden"
        );
        let status = vcx
            .debug_bounds("player-bar")
            .expect("fresh window has a compact player bar");
        assert!(
            f32::from(status.size.height) < 48.0,
            "release player bar should remain a single compact row, got {:?}",
            status.size
        );
        shell.update(&mut vcx.cx, |shell, cx| {
            let first_sgf = shell.host.to_sgf();
            shell.mode = GameMode::Edit;
            shell.last_vertex = Some(Vertex { column: 3, row: 3 });
            shell.analysis_enabled = true;
            shell.analysis = vec![ryusei_host::AnalysisEntry {
                id: Some(7),
                vertex: Some("D4".to_owned()),
                visits: 800,
                winrate: 0.61,
                score_lead: Some(2.5),
                pv: vec!["D4".to_owned()],
                is_during_search: false,
                ownership: None,
                prior: None,
            }];
            shell.analysis_best_move = Some(Vertex { column: 3, row: 3 });
            shell.create_workspace_session(cx);
            assert_eq!(shell.workspace_tabs.tabs().len(), 2);
            assert_eq!(shell.workspace_tabs.active_tab_id(), "session-2");
            shell.activate_workspace_tab("session-1", cx);
            assert_eq!(shell.host.to_sgf(), first_sgf);
            assert_eq!(shell.workspace_tabs.active_tab_id(), "session-1");
            assert_eq!(shell.mode, GameMode::Edit);
            assert!(shell.analysis_enabled);
            assert_eq!(shell.analysis.len(), 1);
            assert_eq!(shell.analysis_best_move, Some(Vertex { column: 3, row: 3 }));
            assert!(
                shell
                    .persistence
                    .load_workspace_tabs()
                    .expect("workspace sessions persist")
                    .is_some()
            );
        });
        let _ = std::fs::remove_dir_all(&config);
    }

    #[test]
    fn goban_hit_layer_places_a_stone_and_panes_do_not_overlap() {
        let config = temp_config("board-click");
        let dispatcher = TestDispatcher::new(rand::rngs::StdRng::seed_from_u64(7));
        let mut cx = TestAppContext::build(dispatcher, None);
        let mut settings = SettingsStore::default();
        settings
            .set("view.show_leftsidebar", serde_json::json!(true))
            .unwrap();
        settings
            .set("view.show_graph", serde_json::json!(true))
            .unwrap();
        settings
            .set("view.show_comments", serde_json::json!(true))
            .unwrap();
        settings
            .set("view.show_winrategraph", serde_json::json!(true))
            .unwrap();
        settings
            .set("view.winrategraph_height", serde_json::json!(90.0))
            .unwrap();
        settings
            .set("view.properties_height", serde_json::json!(180.0))
            .unwrap();
        settings
            .set("view.leftsidebar_width", serde_json::json!(250.0))
            .unwrap();
        settings
            .set("view.peerlist_height", serde_json::json!(130.0))
            .unwrap();
        settings
            .set("view.sidebar_width", serde_json::json!(200.0))
            .unwrap();
        cx.update(|cx| {
            gpui_component::init(cx);
            gpui_component::Theme::change(gpui_component::ThemeMode::Dark, None, cx);
        });
        let mut shell_slot: Option<Entity<ShellApp>> = None;
        let shell_ptr = &mut shell_slot as *mut Option<Entity<ShellApp>>;
        let window_handle = cx.add_window(|window, cx| {
            let app = cx.new(|cx| {
                ShellApp::new(
                    settings.clone(),
                    NativeSettingsPersistence::new(config.clone()),
                    NativeHostPersistence::new(config.clone()),
                    NativePluginPersistence::new(config.clone()),
                    "frontend smoke".to_owned(),
                    None,
                    Box::new(MockDialogService::default()),
                    cx,
                )
            });
            unsafe {
                *shell_ptr = Some(app.clone());
            }
            gpui_component::Root::new(app, window, cx)
        });
        let shell = shell_slot.unwrap();
        let vcx = VisualTestContext::from_window(*window_handle.deref(), &cx).into_mut();
        vcx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        vcx.run_until_parked();

        // The match controls now live in a floating capsule above the goban
        // (single 44px titlebar replaces the old 40px + 42px chrome stack).
        let session_toolbar = vcx
            .debug_bounds("session-toolbar")
            .expect("the floating match-control capsule must remain visible");
        assert!(
            f32::from(session_toolbar.size.height) <= 44.0,
            "floating match capsule must be compact, got {:?}",
            session_toolbar.size.height
        );
        let goban_bounds = vcx
            .debug_bounds("goban")
            .expect("the rendered goban must have a debug selector");
        // The goban is now adaptive: it must render as a square that fills the
        // center pane. In the test window it should be at least the old fixed
        // baseline and never exceed the available pane width.
        assert_eq!(
            goban_bounds.size.width, goban_bounds.size.height,
            "goban must remain square, got {:?}",
            goban_bounds.size
        );
        assert!(
            f32::from(goban_bounds.size.width) >= BOARD_PIXEL_SIZE,
            "goban must fill available space, got {:?}",
            goban_bounds.size
        );
        let wood_bounds = vcx
            .debug_bounds("goban-wood")
            .expect("the wood background must have a debug selector");
        assert!(
            (f32::from(wood_bounds.origin.x) - f32::from(goban_bounds.origin.x) - 14.0).abs() < 0.5,
            "wood {:?} must sit one half-margin inside goban {:?}",
            wood_bounds,
            goban_bounds
        );
        assert!(
            (f32::from(wood_bounds.origin.y) - f32::from(goban_bounds.origin.y) - 14.0).abs() < 0.5,
            "wood {:?} must sit one half-margin inside goban {:?}",
            wood_bounds,
            goban_bounds
        );
        assert_eq!(
            wood_bounds.size.width,
            px(f32::from(goban_bounds.size.width) - 28.0)
        );

        let left_sidebar = vcx
            .debug_bounds("left-sidebar")
            .expect("the left sidebar must have a debug selector");
        let center_pane = vcx
            .debug_bounds("center-pane")
            .expect("the center pane must have a debug selector");
        let right_sidebar = vcx
            .debug_bounds("right-sidebar")
            .expect("the right sidebar must have a debug selector");
        assert!(
            left_sidebar.right() <= center_pane.origin.x,
            "left sidebar {:?} must not overlap center {:?}",
            left_sidebar,
            center_pane
        );
        assert!(
            center_pane.right() <= right_sidebar.origin.x,
            "center {:?} must not overlap right sidebar {:?}",
            center_pane,
            right_sidebar
        );
        assert_eq!(left_sidebar.size.width, px(250.0));
        assert_eq!(right_sidebar.size.width, px(200.0));
        let panels = [
            (
                "variation-tree-panel",
                vcx.debug_bounds("variation-tree-panel")
                    .expect("variation panel must have a debug selector"),
            ),
            (
                "node-inspector-panel",
                vcx.debug_bounds("node-inspector-panel")
                    .expect("node inspector panel must have a debug selector"),
            ),
        ];
        for (name, bounds) in &panels {
            assert!(
                f32::from(bounds.origin.x) >= f32::from(right_sidebar.origin.x) - 0.5
                    && f32::from(bounds.right()) <= f32::from(right_sidebar.right()) + 0.5,
                "{name} {:?} must stay inside the right sidebar {:?}",
                bounds,
                right_sidebar
            );
        }
        assert!(vcx.debug_bounds("bottom-deck-panel").is_none());
        shell.update(&mut vcx.cx, |shell, cx| {
            shell.toggle_plugin_popover("all", cx);
        });
        vcx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("bottom-deck-panel").is_some());
        let plugin_close = vcx
            .debug_bounds("plugin-menu-close")
            .expect("Plugin menu must expose a close control");
        vcx.simulate_click(plugin_close.center(), gpui::Modifiers::none());
        vcx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        vcx.run_until_parked();
        assert_eq!(
            shell.read_with(&vcx.cx, |shell, _| shell.active_plugin_popover.clone()),
            None,
            "closing plugin menu must clear popover state"
        );
        shell.update(&mut vcx.cx, |shell, cx| shell.open_game_info(cx));
        vcx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        let game_info_drawer = vcx
            .debug_bounds("game-info-drawer")
            .expect("Game Info action must open a drawer");
        assert_eq!(game_info_drawer.size.width, px(380.0));
        let game_info_close = vcx
            .debug_bounds("game-info-drawer-close")
            .expect("Game Info drawer must expose a close control");
        vcx.simulate_click(game_info_close.center(), gpui::Modifiers::none());
        shell.update(&mut vcx.cx, |shell, cx| shell.open_score(cx));
        vcx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        let score_drawer = vcx
            .debug_bounds("score-drawer")
            .expect("Score action must open a drawer");
        assert_eq!(score_drawer.size.width, px(380.0));
        let score_close = vcx
            .debug_bounds("score-drawer-close")
            .expect("Score drawer must expose a close control");
        vcx.simulate_click(score_close.center(), gpui::Modifiers::none());
        vcx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        assert_eq!(
            shell.read_with(&vcx.cx, |shell, _| shell.active_drawer),
            None,
            "closing read-only drawers must clear drawer state"
        );
        let engine_sidebar = vcx
            .debug_bounds("engine-sidebar")
            .expect("engine analysis sidebar must have a debug selector");
        assert!(
            f32::from(engine_sidebar.origin.x) >= f32::from(left_sidebar.origin.x) - 0.5
                && f32::from(engine_sidebar.right()) <= f32::from(left_sidebar.right()) + 0.5,
            "engine sidebar {:?} must live in the left sidebar {:?}",
            engine_sidebar,
            left_sidebar
        );
        let game_graph_region = vcx
            .debug_bounds("game-graph-region")
            .expect("game graph region must have a debug selector");
        let properties_region = vcx
            .debug_bounds("properties-region")
            .expect("properties region must have a debug selector");
        assert!(
            f32::from(game_graph_region.bottom()) <= f32::from(properties_region.origin.y) + 0.5,
            "game graph region {:?} must sit above properties region {:?}",
            game_graph_region,
            properties_region
        );
        let properties_splitter = vcx
            .debug_bounds("properties-splitter")
            .expect("properties internal splitter must render");
        let comment_box = vcx
            .debug_bounds("node-comment-input-box")
            .expect("enabled CommentBox must render");
        assert!(
            game_graph_region.origin.y <= properties_splitter.origin.y
                && properties_splitter.origin.y <= properties_region.origin.y,
            "GameGraph {:?}, properties splitter {:?}, and CommentBox {:?} must be vertically ordered",
            game_graph_region,
            properties_splitter,
            properties_region
        );
        assert!(
            f32::from(comment_box.origin.x) >= f32::from(properties_region.origin.x) - 0.5
                && f32::from(comment_box.right()) <= f32::from(properties_region.right()) + 0.5,
            "CommentBox {:?} must stay inside properties {:?}",
            comment_box,
            properties_region
        );

        let board = shell.read_with(&vcx.cx, |shell, _| shell.host.snapshot().board.clone());
        let goban_size = f32::from(goban_bounds.size.width);
        let (x, y) = crate::goban_view::intersection_position(&board, goban_size, 16, 16);
        let click = gpui::point(
            px(f32::from(goban_bounds.origin.x) + x),
            px(f32::from(goban_bounds.origin.y) + y),
        );
        let before = shell.read_with(&vcx.cx, |shell, _| shell.host.snapshot().moves.len());
        vcx.simulate_click(click, gpui::Modifiers::none());
        let after = shell.read_with(&vcx.cx, |shell, _| {
            let snap = shell.host.snapshot();
            (snap.moves.len(), snap.moves.last().and_then(|m| m.vertex))
        });
        assert_eq!(
            after.0,
            before + 1,
            "clicking the rendered intersection must place the next black stone"
        );
        assert!(after.1.is_some(), "placed move must have a vertex");
        vcx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        vcx.run_until_parked();
        assert!(
            vcx.debug_bounds("pass-button").is_some(),
            "the pass button must have a debug selector"
        );
        vcx.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.on_pass(&MouseDownEvent::default(), window, cx);
            });
        });
        let after_pass = shell.read_with(&vcx.cx, |shell, _| {
            let snapshot = shell.host.snapshot();
            (
                snapshot.moves.len(),
                snapshot.moves.last().map(|m| m.vertex),
            )
        });
        assert_eq!(after_pass.0, after.0 + 1);
        assert!(
            matches!(after_pass.1, Some(None)),
            "pass must append a pass move"
        );
        vcx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        vcx.run_until_parked();
        let root_graph_node = vcx
            .debug_bounds("game-graph-node-0")
            .expect("GameGraph root must be rendered as an interactive node");
        vcx.simulate_click(root_graph_node.center(), gpui::Modifiers::none());
        let current_node_after_graph_click =
            shell.read_with(&vcx.cx, |shell, _| shell.host.snapshot().current_node_id);
        assert_eq!(
            current_node_after_graph_click, "root",
            "clicking the GameGraph root must navigate to the root node"
        );
        vcx.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.open_game_graph_context_menu("root".to_owned(), cx);
                shell.toggle_game_graph_context_hotspot(&MouseDownEvent::default(), window, cx);
            });
        });
        assert!(
            shell.read_with(&vcx.cx, |shell, _| {
                shell
                    .host
                    .snapshot()
                    .nodes
                    .iter()
                    .find(|node| node.id == "root")
                    .is_some_and(|node| node.properties.contains_key("HO"))
            }),
            "context-menu hotspot action must write HO on the selected node"
        );

        let splitter = vcx
            .debug_bounds("left-splitter")
            .expect("the left splitter must have a debug selector");
        let drag_start = splitter.center();
        let drag_end = gpui::point(
            px(f32::from(drag_start.x) + 60.0),
            px(f32::from(drag_start.y)),
        );
        vcx.simulate_mouse_down(drag_start, gpui::MouseButton::Left, gpui::Modifiers::none());
        vcx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        vcx.run_until_parked();
        vcx.simulate_mouse_move(
            drag_end,
            Some(gpui::MouseButton::Left),
            gpui::Modifiers::none(),
        );
        vcx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        vcx.simulate_mouse_up(drag_end, gpui::MouseButton::Left, gpui::Modifiers::none());
        vcx.update(|window, cx| {
            let _ = window.draw(cx);
        });

        let resized_left = vcx
            .debug_bounds("left-sidebar")
            .expect("the left sidebar must still have a debug selector");
        assert!(
            (f32::from(resized_left.size.width) - 310.0).abs() < 0.75,
            "dragging the left divider 60px should resize the left pane to 310px, got {:?}",
            resized_left.size
        );
        let persisted_width = shell.read_with(&vcx.cx, |shell, _| {
            shell
                .settings
                .get("view.leftsidebar_width")
                .and_then(serde_json::Value::as_f64)
        });
        assert!(
            persisted_width.is_some_and(|width| (width - 310.0).abs() < 0.75),
            "finishing the drag must persist the pane width, got {persisted_width:?}"
        );
        let _ = std::fs::remove_dir_all(&config);
    }

    #[test]
    fn tab_key_cycles_focus_through_registered_text_inputs() {
        let config = temp_config("tab-focus");
        let dispatcher = TestDispatcher::new(rand::rngs::StdRng::seed_from_u64(13));
        let mut cx = TestAppContext::build(dispatcher, None);
        let mut settings = SettingsStore::default();
        settings
            .set("view.show_comments", serde_json::json!(true))
            .unwrap();
        cx.update(|cx| {
            gpui_component::init(cx);
            gpui_component::Theme::change(gpui_component::ThemeMode::Dark, None, cx);
        });
        let mut shell_slot: Option<Entity<ShellApp>> = None;
        let shell_ptr = &mut shell_slot as *mut Option<Entity<ShellApp>>;
        let window_handle = cx.add_window(|window, cx| {
            let app = cx.new(|cx| {
                ShellApp::new(
                    settings.clone(),
                    NativeSettingsPersistence::new(config.clone()),
                    NativeHostPersistence::new(config.clone()),
                    NativePluginPersistence::new(config.clone()),
                    "tab focus smoke".to_owned(),
                    None,
                    Box::new(MockDialogService::default()),
                    cx,
                )
            });
            unsafe {
                *shell_ptr = Some(app.clone());
            }
            gpui_component::Root::new(app, window, cx)
        });
        let shell = shell_slot.unwrap();
        let vcx = VisualTestContext::from_window(*window_handle.deref(), &cx).into_mut();
        vcx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        vcx.run_until_parked();

        // Focus the comment box directly, then Tab forward: focus must leave
        // the comment box and move to another registered tab stop (proves the
        // tab-stop ring is wired, without depending on the first-stop order).
        let comment_handle = shell.read_with(&vcx.cx, |shell, _| {
            shell.text_inputs.comment_focus_handle.clone()
        });
        vcx.update(|window, _cx| window.focus(&comment_handle));
        vcx.update(|window, _cx| {
            assert!(
                comment_handle.is_focused(window),
                "precondition: the comment box accepts direct focus"
            );
        });
        vcx.update(|window, _cx| window.focus_next());
        let moved_off_comment = vcx.update(|window, _cx| !comment_handle.is_focused(window));
        assert!(
            moved_off_comment,
            "Tab must move focus from the comment box to the next registered tab stop"
        );
        let _ = std::fs::remove_dir_all(&config);
    }
}
