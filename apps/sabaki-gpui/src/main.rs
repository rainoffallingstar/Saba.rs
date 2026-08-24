#[allow(dead_code)]
mod benchmark;
mod dialog_service;
mod engine_console;
mod external_file;
mod file_workflow;
mod goban_view;
mod layout;
mod markup;
mod mode_bar;
mod native_text_input;
mod navigation;
mod node_inspector;
mod panels;
mod plugin_contribution;
mod plugin_panel;
mod settings;
mod settings_form;
mod sound_feedback;
mod theme;
mod variation_tree;
mod winrate_graph;

use std::{
    cell::Cell,
    collections::BTreeMap,
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, Instant},
};

use gpui::{
    App, Application, Bounds, Context, Div, Entity, FocusHandle, FontWeight, InteractiveElement,
    KeyBinding, Menu, MenuItem, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    SharedString, Task, TitlebarOptions, Window, WindowBounds, WindowOptions, actions, div, point,
    prelude::*, px, rgb, size,
};
use sabaki_domain_core::gtp::AnalysisStream;
use sabaki_domain_core::legacy::handicap_placement;
use sabaki_domain_core::{Color, GameMode, Vertex};
use sabaki_host::{HostPersistence, replay_position_stream};

use crate::dialog_service::{DialogService, NativeGameFileAccess, RfdDialogService};
use crate::engine_console::{
    EngineLogEntry, EngineRole, EngineRoleAssignments, analysis_command_from_settings,
    best_analysis_move, best_analysis_winrate, entry_for_response, format_console_command,
    merge_analysis_entries, parse_engine_spec, parse_gtp_vertex, parse_stream_line,
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
    MarkupTool, create_line_transaction, create_markup_transaction, create_scoring_transaction,
    create_setup_transactions, next_scoring_override,
};
use crate::native_text_input::{InputKeyResult, NativeTextInput};
use crate::navigation::{
    NavigationDirection, navigation_availability, navigation_target, position_label,
};
use crate::node_inspector::{
    AnnotationGroup, NodeAnnotation, VariationAction, create_annotation_transactions,
    create_comment_transaction, create_hotspot_transaction, create_variation_transaction,
    current_node_metadata,
};
use crate::plugin_panel::{PluginPanelEntry, apply_process_info, entry_from_record};
use crate::settings::{
    BOARD_SIZE_OPTIONS, THEME_CHOICES, ThemeChoice, theme_from_setting,
    window_bounds_from_settings, window_maximized_from_settings,
};
use crate::settings_form::{
    SettingEdit, SettingRow, apply_setting_edit, display_setting_value, number_edit,
    panel_setting_rows, string_array_edit, toggle_boolean_edit,
};
use crate::sound_feedback::{SoundCue, SoundSink, platform_sound_sink, play_if_enabled};
use crate::theme::{ThemeTokens, UiPalette, ui_palette};
use crate::variation_tree::build_variation_tree_layout;
use crate::winrate_graph::{
    WinrateGraphMetric, analysis_sgf_properties, graph_plot_points, winrate_history,
};

const BOARD_PIXEL_SIZE: f32 = 420.0;

actions!(
    sabaki_gpui,
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
        ToggleWinrateGraph,
        ToggleCoordinates,
        ToggleMoveNumbers,
        SetPlayMode,
        SetEditMode,
        SetScoringMode,
        SetEstimatorMode,
        SetFindMode,
        SetGuessMode,
        SetAutoplayMode,
        StartAnalysis,
        StopAnalysis,
        GenerateEngineMove,
        Quit,
    ]
);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ActiveDrawer {
    Preferences,
    GameInfo,
    Score,
    About,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ActiveTextInput {
    Comment,
    NodeTitle,
}

struct ShellApp {
    host: sabaki_host::HostApplication,
    file_access: NativeGameFileAccess,
    dialog_service: Box<dyn DialogService>,
    external_file: sabaki_host::ExternalFileStore,
    persistence: NativeHostPersistence,
    recent_files: sabaki_host::RecentFilesStore,
    autosave: sabaki_host::AutosaveStore,
    settings: sabaki_host::SettingsStore,
    settings_persistence: NativeSettingsPersistence,
    sound_sink: Box<dyn SoundSink>,
    engine_store: sabaki_host::EngineStore,
    engine_controller: sabaki_host::EngineController<EngineRole, sabaki_host::ProcessGtpTransport>,
    active_console_role: Option<EngineRole>,
    engine_roles: EngineRoleAssignments,
    analysis: Vec<sabaki_host::AnalysisEntry>,
    analysis_best_move: Option<Vertex>,
    analysis_run: sabaki_host::AnalysisRunController,
    analysis_task: Option<Task<()>>,
    engine_log: Vec<EngineLogEntry>,
    engine_input_focus_handle: FocusHandle,
    engine_draft: SharedString,
    engine_spec_draft: SharedString,
    engine_spec_focus_handle: FocusHandle,
    fox_query: SharedString,
    fox_query_focus_handle: FocusHandle,
    theme_choice: ThemeChoice,
    theme: ThemeTokens,
    palette: UiPalette,
    left_sidebar_width: f32,
    right_sidebar_width: f32,
    peer_list_height: f32,
    winrate_graph_height: f32,
    properties_height: f32,
    split_drag: Option<SplitDrag>,
    active_drawer: Option<ActiveDrawer>,
    plugin_menu_open: bool,
    game_graph_context_node: Option<sabaki_domain_core::NodeId>,
    installed_themes: Vec<sabaki_host::InstalledTheme>,
    legacy_asar_themes: Vec<std::path::PathBuf>,
    board_size: usize,
    settings_editing_key: Option<String>,
    settings_draft: SharedString,
    settings_input_focus_handle: FocusHandle,
    plugin_controller: sabaki_host::PluginController<NativePluginPersistence>,
    installed_plugins: Vec<PluginPanelEntry>,
    last_vertex: Option<Vertex>,
    active_tool: MarkupTool,
    mode: GameMode,
    line_start: Option<Vertex>,
    hovered_vertex: Option<Vertex>,
    comment_focus_handle: FocusHandle,
    comment_input: NativeTextInput,
    node_title_focus_handle: FocusHandle,
    node_title_input: NativeTextInput,
    active_text_input: Option<ActiveTextInput>,
    status: SharedString,
    /// Prominent transient notification shown as a centered toast overlay.
    toast: Option<SharedString>,
}

/// Active splitter drag state. Window-global mouse move/up listeners are
/// registered while this is `Some` so the drag continues outside the handle.
#[derive(Clone, Copy)]
struct SplitDrag {
    pane: SplitPane,
    start_position: f32,
    start_size: f32,
}

/// Resolves the new-game defaults stored in the settings store and returns
/// the board size plus root SGF properties (`KM`, `HA`, and standard `AB`
/// handicap stones). Missing values fall back to the upstream Sabaki defaults.
fn default_board_size(settings: &sabaki_host::SettingsStore) -> usize {
    settings
        .get("game.default_board_size")
        .and_then(serde_json::Value::as_f64)
        .map(|value| value.round() as i64)
        .filter(|value| (2..=25).contains(value))
        .unwrap_or(19) as usize
}

fn default_new_game_properties_for_size(
    settings: &sabaki_host::SettingsStore,
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
    properties
}

/// Resolves the new-game defaults stored in the settings store and returns
/// the board size plus root SGF properties (`KM`, `HA`, and standard `AB`
/// handicap stones). Missing values fall back to the upstream Sabaki defaults.
fn default_new_game_properties(
    settings: &sabaki_host::SettingsStore,
) -> (usize, BTreeMap<String, Vec<String>>) {
    let size = default_board_size(settings);
    let properties = default_new_game_properties_for_size(settings, size);
    (size, properties)
}

impl ShellApp {
    #[expect(
        clippy::too_many_arguments,
        reason = "P2 will replace direct shell construction dependencies with dedicated controllers"
    )]
    fn new(
        mut settings: sabaki_host::SettingsStore,
        settings_persistence: NativeSettingsPersistence,
        persistence: NativeHostPersistence,
        plugin_persistence: NativePluginPersistence,
        initial_status: String,
        startup_file: Option<PathBuf>,
        dialog_service: Box<dyn DialogService>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut host = sabaki_host::HostApplication::default();
        let file_access = NativeGameFileAccess;
        let mut events = RecordingSink;
        let (default_size, default_properties) = default_new_game_properties(&settings);
        let left_sidebar_width = pane_size_from_settings(
            &settings,
            "view.leftsidebar_width",
            "view.leftsidebar_minwidth",
            250.0,
        );
        let right_sidebar_width = pane_size_from_settings(
            &settings,
            "view.sidebar_width",
            "view.sidebar_minwidth",
            200.0,
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
        let board_size = host.snapshot().board.width;

        let recent_files = persistence.load_recent_files().unwrap_or_default();
        let autosave = persistence.load_autosave();
        let theme_choice = theme_from_setting(settings.get_str("theme.current"));
        let theme = theme_choice.tokens();
        let palette = ui_palette(&theme);
        let (installed_themes, legacy_asar_themes) = match file_workflow::theme_root() {
            Ok(theme_root) => match sabaki_host::scan_theme_root(&theme_root) {
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
                std::env::temp_dir().join("sabaki-gpui-plugins")
            }
        };
        let plugin_controller = match sabaki_host::PluginController::restore(
            plugin_persistence,
            &plugin_install_root,
        ) {
            Ok(controller) => controller,
            Err(error) => {
                status = format!("plugin scan failed: {error}");
                let fallback_persistence = NativePluginPersistence::for_current_user()
                    .unwrap_or_else(|_| NativePluginPersistence::new(std::env::temp_dir()));
                sabaki_host::PluginController::from_store(
                    sabaki_host::PluginStore::default(),
                    fallback_persistence,
                )
            }
        };
        let installed_plugins = plugin_controller
            .records()
            .iter()
            .map(entry_from_record)
            .collect();
        let engine_store = sabaki_host::EngineStore::from_settings(&settings).unwrap_or_default();
        let engine_roles = EngineRoleAssignments::from_settings(&settings);
        // Analysis is the principal board feedback loop. The legacy setting is
        // optional, so a fresh Saba.rs profile must expose analysis markers
        // instead of silently hiding a connected KataGo session.
        if settings.get_bool("board.show_analysis").is_none() {
            let _ = settings.set("board.show_analysis", serde_json::json!(true));
        }
        if settings.get_bool("view.show_leftsidebar").is_none() {
            let _ = settings.set("view.show_leftsidebar", serde_json::json!(true));
        }
        let active_console_role = EngineRole::ALL
            .into_iter()
            .find(|role| engine_roles.get(*role).is_some());

        Self {
            host,
            file_access,
            dialog_service,
            persistence,
            recent_files,
            autosave,
            settings,
            settings_persistence,
            sound_sink: platform_sound_sink(),
            external_file: sabaki_host::ExternalFileStore::default(),
            engine_store,
            engine_controller: sabaki_host::EngineController::default(),
            active_console_role,
            engine_roles,
            analysis: Vec::new(),
            analysis_best_move: None,
            analysis_run: sabaki_host::AnalysisRunController::default(),
            analysis_task: None,
            engine_log: Vec::new(),
            engine_input_focus_handle: cx.focus_handle(),
            engine_draft: "".into(),
            engine_spec_draft: "".into(),
            engine_spec_focus_handle: cx.focus_handle(),
            fox_query: "".into(),
            fox_query_focus_handle: cx.focus_handle(),
            theme_choice,
            theme,
            palette,
            left_sidebar_width,
            right_sidebar_width,
            peer_list_height,
            winrate_graph_height,
            properties_height,
            split_drag: None,
            active_drawer: None,
            plugin_menu_open: false,
            game_graph_context_node: None,
            installed_themes,
            legacy_asar_themes,
            board_size,
            settings_editing_key: None,
            settings_draft: "".into(),
            settings_input_focus_handle: cx.focus_handle(),
            plugin_controller,
            installed_plugins,
            last_vertex: None,
            active_tool: MarkupTool::Play,
            mode: GameMode::Play,
            line_start: None,
            hovered_vertex: None,
            comment_focus_handle: cx.focus_handle(),
            comment_input: NativeTextInput::new(""),
            node_title_focus_handle: cx.focus_handle(),
            node_title_input: NativeTextInput::new(""),
            active_text_input: None,
            status: status.into(),
            toast: None,
        }
    }

    /// Parses a GTP command line into `(name, arguments)`.
    fn parse_engine_command_line(draft: &str) -> (String, Vec<String>) {
        let mut tokens = draft.split_whitespace();
        let name = tokens.next().unwrap_or_default().to_owned();
        let arguments = tokens.map(ToOwned::to_owned).collect();
        (name, arguments)
    }

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
        window.focus(&self.engine_input_focus_handle);
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
            sabaki_host::persist_settings_store(&self.settings, &mut self.settings_persistence)
        {
            self.settings = previous;
            return Err(error);
        }
        Ok(())
    }

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
        _: &mut Context<Self>,
    ) {
        window.focus(&self.fox_query_focus_handle);
    }

    fn on_fox_query_key_down(
        &mut self,
        event: &gpui::KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut query = self.fox_query.to_string();
        match event.keystroke.key.as_str() {
            "backspace" => {
                query.pop();
            }
            "enter" => {
                self.fetch_fox_query(cx);
                return;
            }
            "escape" => {
                self.fox_query = "".into();
            }
            _ => {
                if let Some(key_char) = event.keystroke.key_char.as_ref() {
                    query.push_str(key_char);
                }
            }
        }
        self.fox_query = query.into();
        cx.notify();
    }

    fn fetch_fox_query(&mut self, cx: &mut Context<Self>) {
        let query = self.fox_query.to_string();
        let query = query.trim().to_owned();
        if query.is_empty() {
            self.status = "输入野狐用户名或 ID 后按 Enter 查询".into();
            cx.notify();
            return;
        }
        self.show_toast(format!("🦊 正在查询野狐用户 {query} 的最新棋谱..."), cx);
        let weak = cx.entity().downgrade();
        cx.spawn(
            move |_: gpui::WeakEntity<ShellApp>, cx: &mut gpui::AsyncApp| {
                let mut cx = cx.clone();
                async move {
                    let result: Result<(sabaki_host::FoxGameSummary, String), String> = cx
                        .background_executor()
                        .spawn(async move {
                            let games = sabaki_host::fetch_user_recent_games(&query)?;
                            let game = games
                                .first()
                                .ok_or_else(|| "未查询到近期对局记录".to_owned())?;
                            let sgf = sabaki_host::fetch_game_sgf(&game.chess_id)?;
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
        result: Result<(sabaki_host::FoxGameSummary, String), String>,
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
        _: &mut Context<Self>,
    ) {
        window.focus(&self.engine_input_focus_handle);
    }

    fn on_engine_key_down(
        &mut self,
        event: &gpui::KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut draft = self.engine_draft.to_string();
        match event.keystroke.key.as_str() {
            "backspace" => {
                draft.pop();
            }
            "enter" => {
                self.send_engine_command(&draft, cx);
                return;
            }
            "escape" => {
                self.engine_draft = "".into();
                cx.notify();
                return;
            }
            _ => {
                if let Some(key_char) = event.keystroke.key_char.as_ref() {
                    draft.push_str(key_char);
                }
            }
        }
        self.engine_draft = draft.into();
        cx.notify();
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
        let Some(role) = self.active_console_role else {
            self.status = "select an engine role for the GTP console".into();
            cx.notify();
            return;
        };
        let (name, arguments) = Self::parse_engine_command_line(draft);
        let formatted = format_console_command(&name, &arguments);
        let result = match self.engine_controller.send(role, &name, arguments) {
            Ok(response) => Ok(response),
            Err(sabaki_host::EngineControllerError::Detached) => {
                self.status = format!("{} engine is detached", role.label()).into();
                self.engine_draft = "".into();
                cx.notify();
                return;
            }
            Err(error) => Err(error.to_string()),
        };
        match result {
            Ok(response) => {
                self.record_engine_log(entry_for_response(formatted.clone(), &response));
                if response.success
                    && name == "boardsize"
                    && let Some(size) = draft
                        .split_whitespace()
                        .nth(1)
                        .and_then(|value| value.parse().ok())
                {
                    self.board_size = size;
                }
                self.status = format!("{} engine: {formatted}", role.label()).into();
            }
            Err(error) => {
                self.record_engine_log(EngineLogEntry {
                    command: formatted.clone(),
                    success: false,
                    response: format!("protocol error: {error}"),
                });
                self.status = format!("{} engine failed: {error}", role.label()).into();
            }
        }
        self.engine_draft = "".into();
        cx.notify();
    }

    /// Starts a role-specific engine session: spawns the process,
    /// runs the host handshake/probe/startup/board-setup sequence, and replays
    /// the current position into the engine so it tracks the board.
    fn on_engine_connect(&mut self, role: EngineRole, cx: &mut Context<Self>) {
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
        let Some(name) = self.engine_roles.get(role) else {
            self.status = format!("select a configured {} engine first", role.label()).into();
            cx.notify();
            return;
        };
        let Some(record) = self
            .engine_store
            .list()
            .iter()
            .find(|record| record.name == name)
            .cloned()
        else {
            self.status =
                format!("selected {} engine {name} is not configured", role.label()).into();
            cx.notify();
            return;
        };
        let arguments: Vec<String> = record
            .args
            .split_whitespace()
            .map(ToOwned::to_owned)
            .collect();
        let board_size = self.host.snapshot().board.width;
        let transport = match sabaki_host::ProcessGtpTransport::start(&record.path, &arguments) {
            Ok(transport) => transport,
            Err(error) => {
                self.status = format!("engine process failed: {error}").into();
                cx.notify();
                return;
            }
        };
        let moves = self.host.snapshot().moves.clone();
        if let Err(error) = self
            .engine_controller
            .attach(role, transport, &record, board_size, &moves)
        {
            self.status = format!("engine attach failed: {error}").into();
            cx.notify();
            return;
        }
        self.active_console_role = Some(role);
        self.status = format!("{} engine {name} attached", role.label()).into();
        if role == EngineRole::Analysis {
            self.start_analysis(cx);
        } else {
            cx.notify();
        }
    }

    fn disconnect_engine_role(&mut self, role: EngineRole) {
        if role == EngineRole::Analysis && self.analysis_task.is_some() {
            self.analysis_run.cancel_and_dispose();
            if let Some(task) = self.analysis_task.take() {
                task.detach();
            }
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
            self.analysis.clear();
            self.analysis_best_move = None;
        }
        self.status = if attached {
            format!("{} engine detached", role.label())
        } else {
            format!("{} engine is already detached", role.label())
        }
        .into();
        cx.notify();
    }

    fn on_analyze(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.start_analysis(cx);
    }

    /// Requests analysis from the role-specific Analysis engine and marks the
    /// best candidate on the board.
    fn start_analysis(&mut self, cx: &mut Context<Self>) {
        if self.analysis_task.is_some() {
            self.status = "analysis is already running; stop it before starting another run".into();
            cx.notify();
            return;
        }
        let analysis_snapshot = self.host.snapshot();
        let (command, command_arguments) = analysis_command_from_settings(&self.settings);
        if !self.engine_controller.is_attached(EngineRole::Analysis) {
            let name = self
                .engine_roles
                .get(EngineRole::Analysis)
                .unwrap_or_default();
            self.status = format!("attach selected analysis engine {name} before analyzing").into();
            cx.notify();
            return;
        }

        let run = self.analysis_run.begin(
            analysis_snapshot.current_node_id.clone(),
            analysis_snapshot.board.next_player,
        );

        // Bounded `analyze` responses go through the attached Analysis session.
        if command == "analyze" && self.engine_controller.is_attached(EngineRole::Analysis) {
            match self.engine_controller.analyze(
                EngineRole::Analysis,
                &command,
                vec!["".to_owned()],
            ) {
                Ok(entries) => {
                    self.set_analysis(entries, cx);
                    self.analysis_run.finish(&run);
                }
                Err(error) => {
                    self.status = format!("analysis failed: {error}").into();
                    cx.notify();
                    return;
                }
            }
            cx.notify();
            return;
        }

        // Streaming commands (kata-analyze / lz-analyze): reuse the already
        // connected engine session when it supports streaming (no second
        // process), otherwise fall back to a fresh analysis process with the
        // current position replayed into it.
        let board_size = self.host.snapshot().board.width;
        let moves = self.host.snapshot().moves.clone();

        // Session mode: replay the position into the connected session and
        // stream from it. On any failure we fall back to a fresh process.
        let mut session_mode = false;
        if self.engine_controller.is_attached(EngineRole::Analysis) {
            if let Err(error) =
                self.engine_controller
                    .replay(EngineRole::Analysis, board_size, &moves)
            {
                self.status = format!("analysis session setup failed: {error}").into();
                cx.notify();
                return;
            }
            match self.engine_controller.start_analysis(
                EngineRole::Analysis,
                &command,
                command_arguments.clone(),
            ) {
                Ok(()) => session_mode = true,
                Err(sabaki_host::EngineControllerError::Transport(
                    sabaki_domain_core::gtp::GtpError::UnsupportedStreaming,
                )) => {
                    // Connected engine cannot stream; fall through.
                }
                Err(error) => {
                    self.status = format!("analysis command failed: {error}").into();
                    cx.notify();
                    return;
                }
            }
        }

        let task_run = run.clone();
        let task_command = command.clone();

        if session_mode {
            let mut session = self
                .engine_controller
                .lease_for_analysis(EngineRole::Analysis)
                .expect("session mode implies an attached Analysis session");
            self.status =
                format!("analysis: streaming {command} on attached Analysis engine").into();
            let session_run = task_run.clone();
            self.analysis_task = Some(cx.spawn(
                move |shell_weak: gpui::WeakEntity<ShellApp>, cx: &mut gpui::AsyncApp| {
                    let mut cx = cx.clone();
                    async move {
                        let mut pending: Vec<sabaki_host::AnalysisEntry> = Vec::new();
                        let mut last_flush = Instant::now();
                        loop {
                            if session_run.should_stop() {
                                if session_run.is_current() {
                                    let _ = sabaki_host::EngineController::<
                                        EngineRole,
                                        sabaki_host::ProcessGtpTransport,
                                    >::stop_leased_analysis(
                                        &mut session
                                    );
                                }
                                break;
                            }
                            if let Some(line) = sabaki_host::EngineController::<
                                EngineRole,
                                sabaki_host::ProcessGtpTransport,
                            >::recv_analysis_line(
                                &mut session, Duration::from_millis(50)
                            ) {
                                let line = line.trim();
                                if line.is_empty() {
                                    break;
                                }
                                if let Some(entry) = parse_stream_line(&task_command, line) {
                                    let proxy_completion = task_command == "kata-analyze"
                                        && !entry.is_during_search
                                        && line.trim_start().starts_with('{');
                                    pending.push(entry);
                                    // Official KataGo GTP `kata-analyze` emits
                                    // continuous `info move` records without a
                                    // completion sentinel. JSON proxy adapters
                                    // retain their explicit completion record.
                                    if proxy_completion {
                                        break;
                                    }
                                }
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
                        // Return the leased Analysis session only when this is
                        // still the active run; detached or stale sessions stop.
                        let _ = shell_weak.update(&mut cx, |shell, cx| {
                            shell.finish_streaming_analysis(&session_run, session, cx);
                        });
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
        let arguments: Vec<String> = record
            .args
            .split_whitespace()
            .map(ToOwned::to_owned)
            .collect();
        let mut stream = match AnalysisStream::start(&record.path, &arguments) {
            Ok(stream) => stream,
            Err(error) => {
                self.status = format!("analysis process failed: {error}").into();
                cx.notify();
                return;
            }
        };
        if let Err(error) = replay_position_stream(&mut stream, board_size, &moves) {
            self.status = format!("analysis setup failed: {error}").into();
            cx.notify();
            return;
        }
        let full_command = if command_arguments.is_empty() {
            command.clone()
        } else {
            format!("{} {}", command, command_arguments.join(" "))
        };
        if let Err(error) = stream.send_command(&full_command) {
            self.status = format!("analysis command failed: {error}").into();
            cx.notify();
            return;
        }
        self.status = format!("analysis: streaming {command}").into();
        let stream_run = task_run.clone();
        self.analysis_task = Some(cx.spawn(
            move |shell_weak: gpui::WeakEntity<ShellApp>, cx: &mut gpui::AsyncApp| {
                let mut cx = cx.clone();
                async move {
                    let mut pending: Vec<sabaki_host::AnalysisEntry> = Vec::new();
                    let mut last_flush = Instant::now();
                    loop {
                        if stream_run.should_stop() {
                            if stream_run.is_current() {
                                let _ = stream.send_command("stop");
                            }
                            break;
                        }
                        if let Some(line) = stream.recv_line_timeout(Duration::from_millis(50)) {
                            let line = line.trim();
                            if line.is_empty() {
                                break;
                            }
                            if let Some(entry) = parse_stream_line(&task_command, line) {
                                let proxy_completion = task_command == "kata-analyze"
                                    && !entry.is_during_search
                                    && line.trim_start().starts_with('{');
                                pending.push(entry);
                                // Official KataGo GTP `kata-analyze` emits
                                // continuous `info move` records without a
                                // completion sentinel. JSON proxy adapters
                                // retain their explicit completion record.
                                if proxy_completion {
                                    break;
                                }
                            }
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
                    let _ = shell_weak.update(&mut cx, |shell, cx| {
                        shell.analysis_finished(&stream_run, cx)
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
        run: &sabaki_host::AnalysisRunTicket,
        entries: Vec<sabaki_host::AnalysisEntry>,
        cx: &mut Context<Self>,
    ) {
        if !run.is_current() {
            return;
        }
        self.analysis = merge_analysis_entries(&self.analysis, entries);
        self.set_analysis(self.analysis.clone(), cx);
    }

    /// Stores the strongest completed Analysis-role candidate as upstream
    /// compatible `SBKV` (Black percent) and finite `SBKS` (Black score lead).
    /// The tracked node/player gate prevents a late streaming batch from
    /// annotating a node reached after the analysis request started.
    fn persist_analysis_snapshot(&mut self) {
        let snapshot = self.host.snapshot();
        let Some(player) = self.analysis_run.player_for_node(&snapshot.current_node_id) else {
            return;
        };
        let Some(entry) = self
            .analysis
            .iter()
            .filter(|entry| !entry.is_during_search)
            .max_by_key(|entry| entry.visits)
        else {
            return;
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
                return;
            }
        }
        self.synchronize_recovery();
    }

    /// Stores an analysis set and refreshes the best-move marker and status.
    fn set_analysis(&mut self, entries: Vec<sabaki_host::AnalysisEntry>, cx: &mut Context<Self>) {
        self.analysis = entries;
        let board_size = self.host.snapshot().board.width;
        self.analysis_best_move = best_analysis_move(&self.analysis, board_size)
            .map(|(column, row)| Vertex { column, row });
        self.persist_analysis_snapshot();
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
        run: &sabaki_host::AnalysisRunTicket,
        mut session: sabaki_host::EngineSession<sabaki_host::ProcessGtpTransport>,
        cx: &mut Context<Self>,
    ) {
        if self.analysis_run.should_dispose(run) {
            let _ = session.stop();
            self.analysis_finished(run, cx);
            return;
        }
        if self.analysis_run.replay_required(run) {
            let board_size = self.host.snapshot().board.width;
            let moves = self.host.snapshot().moves.clone();
            if let Err(error) = sabaki_host::EngineController::<
                EngineRole,
                sabaki_host::ProcessGtpTransport,
            >::replay_leased(&mut session, board_size, &moves)
            {
                let _ = session.stop();
                self.analysis_run.clear_replay(run);
                self.analysis_finished(run, cx);
                self.status = format!("analysis engine replay failed: {error}").into();
                return;
            }
        }
        self.analysis_run.clear_replay(run);
        self.engine_controller
            .return_analysis_lease(EngineRole::Analysis, session);
        self.analysis_finished(run, cx);
    }

    /// Clears the running-analysis state once the matching streaming task ends.
    fn analysis_finished(&mut self, run: &sabaki_host::AnalysisRunTicket, cx: &mut Context<Self>) {
        if !self.analysis_run.finish(run) {
            return;
        }
        self.analysis_task = None;
        self.status = "analysis finished".into();
        cx.notify();
    }

    fn on_analysis_stop(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.stop_analysis(cx);
    }

    /// Requests the streaming analysis task to stop and emit its final
    /// candidates.
    fn stop_analysis(&mut self, cx: &mut Context<Self>) {
        if self.analysis_task.is_some() {
            self.analysis_run.request_stop();
            self.status = "stopping analysis".into();
        } else {
            self.status = "no analysis running".into();
        }
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

    /// Asks the engine attached to the specified role for a move, places it,
    /// and synchronizes all sessions.
    fn trigger_engine_genmove(&mut self, role: EngineRole, color: Color, cx: &mut Context<Self>) {
        let color_str = match color {
            Color::Black => "B",
            Color::White => "W",
        };
        let response = match self.engine_controller.request_move(role, color) {
            Ok(response) => Ok(response),
            Err(sabaki_host::EngineControllerError::Detached) => {
                let name = self.engine_roles.get(role).unwrap_or_default();
                self.status = format!(
                    "attach selected {} engine {name} before generating a move",
                    role.label()
                )
                .into();
                cx.notify();
                return;
            }
            Err(error) => Err(error.to_string()),
        };
        match response {
            Ok(response) => {
                self.record_engine_log(entry_for_response(
                    format!("{}: genmove {color_str}", role.label()),
                    &response,
                ));
                if !response.success {
                    self.status = format!(
                        "{} engine genmove failed: {}",
                        role.label(),
                        response.content
                    )
                    .into();
                    cx.notify();
                    return;
                }
                let board_size = self.host.snapshot().board.width;
                let vertex = parse_gtp_vertex(board_size, response.content.trim())
                    .map(|(column, row)| Vertex { column, row });
                let mut events = RecordingSink;
                match self.host.play_move(color, vertex, &mut events) {
                    Ok(_) => {
                        self.last_vertex = vertex;
                        self.status =
                            format!("{} AI played {}", role.label(), response.content.trim())
                                .into();
                        self.synchronize_recovery();
                        self.play_sound_if_enabled(if vertex.is_some() {
                            SoundCue::StonePlaced
                        } else {
                            SoundCue::Pass
                        });
                        self.sync_engine_position(Some(role), color, vertex);
                    }
                    Err(error) => self.status = format!("engine move rejected: {error}").into(),
                }
            }
            Err(error) => {
                self.status = format!("{} engine genmove failed: {error}", role.label()).into();
            }
        }
        cx.notify();
    }

    fn on_engine_move(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.generate_engine_move(cx);
    }

    /// Removes an engine from the configured list and persists the change
    /// through the settings store.
    fn on_engine_remove(&mut self, name: &str, cx: &mut Context<Self>) {
        if !self.engine_store.remove(name) {
            self.status = format!("engine {name} is not configured").into();
            cx.notify();
            return;
        }
        for role in EngineRole::ALL {
            if self.engine_roles.get(role) == Some(name) {
                self.disconnect_engine_role(role);
            }
        }
        self.engine_roles.clear_engine(name);
        match self.engine_store.save(&mut self.settings) {
            Ok(()) => match self.persist_engine_roles() {
                Ok(()) => self.status = format!("engine {name} removed").into(),
                Err(error) => self.status = format!("engine not persisted: {error}").into(),
            },
            Err(error) => self.status = format!("engine list rejected: {error}").into(),
        }
        cx.notify();
    }

    /// Adds an engine from the spec input (`Name | path | args | commands`)
    /// and persists the configured list.
    fn commit_engine_spec(&mut self, cx: &mut Context<Self>) {
        let spec = self.engine_spec_draft.to_string();
        self.engine_spec_draft = "".into();
        let record = match parse_engine_spec(&spec) {
            Ok(record) => record,
            Err(error) => {
                self.status = format!("engine spec rejected: {error}").into();
                cx.notify();
                return;
            }
        };
        match self.engine_store.add(record.clone()) {
            Ok(()) => match self.engine_store.save(&mut self.settings) {
                Ok(()) => match sabaki_host::persist_settings_store(
                    &self.settings,
                    &mut self.settings_persistence,
                ) {
                    Ok(()) => self.status = format!("engine {} added", record.name).into(),
                    Err(error) => self.status = format!("engine not persisted: {error}").into(),
                },
                Err(error) => self.status = format!("engine list rejected: {error}").into(),
            },
            Err(error) => self.status = format!("engine not added: {error}").into(),
        }
        cx.notify();
    }

    fn on_engine_spec_focus(
        &mut self,
        _: &MouseDownEvent,
        window: &mut Window,
        _: &mut Context<Self>,
    ) {
        window.focus(&self.engine_spec_focus_handle);
    }

    fn on_engine_spec_key_down(
        &mut self,
        event: &gpui::KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut draft = self.engine_spec_draft.to_string();
        match event.keystroke.key.as_str() {
            "backspace" => {
                draft.pop();
            }
            "enter" => {
                self.commit_engine_spec(cx);
                return;
            }
            "escape" => {
                self.engine_spec_draft = "".into();
                cx.notify();
                return;
            }
            _ => {
                if let Some(key_char) = event.keystroke.key_char.as_ref() {
                    draft.push_str(key_char);
                }
            }
        }
        self.engine_spec_draft = draft.into();
        cx.notify();
    }

    /// Applies an installed theme package: swaps the active tokens and
    /// records the choice as `theme:<id>` under `theme.current`.
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
            Ok(_) => match sabaki_host::persist_settings_store(
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
    fn refresh_installed_themes(&mut self) {
        match file_workflow::theme_root() {
            Ok(theme_root) => match sabaki_host::scan_theme_root(&theme_root) {
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
        match sabaki_host::install_theme(&path, &theme_root) {
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
    fn on_theme_uninstall(&mut self, theme_id: &str, cx: &mut Context<Self>) {
        let theme_root = match file_workflow::theme_root() {
            Ok(root) => root,
            Err(error) => {
                self.status = format!("theme directory unavailable: {error}").into();
                cx.notify();
                return;
            }
        };
        match sabaki_host::uninstall_theme(&theme_root, theme_id) {
            Ok(()) => {
                self.refresh_installed_themes();
                self.status = format!("uninstalled theme {theme_id}").into();
            }
            Err(error) => self.status = format!("theme uninstall failed: {error}").into(),
        }
        cx.notify();
    }

    /// Selects a theme, swaps the active tokens and persists the choice under
    /// the `theme.current` setting key through the host settings workflow.
    fn on_theme_selected(&mut self, choice: ThemeChoice, cx: &mut Context<Self>) {
        self.theme_choice = choice;
        self.theme = choice.tokens();
        self.palette = ui_palette(&self.theme);
        match self
            .settings
            .set("theme.current", serde_json::json!(choice.setting_value()))
        {
            Ok(_) => match sabaki_host::persist_settings_store(
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
            sabaki_host::persist_settings_store(&self.settings, &mut self.settings_persistence)
        {
            self.status = format!("window size not persisted: {error}").into();
        }
    }

    /// Installs a plugin from a user-selected `.zip` archive.
    fn on_install_plugin_zip(
        &mut self,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
                self.status = outcome.message.into();
                self.installed_plugins = self
                    .plugin_controller
                    .records()
                    .iter()
                    .map(entry_from_record)
                    .collect();
            }
            Err(error) => self.status = format!("plugin installation failed: {error}").into(),
        }
        cx.notify();
    }

    /// Delegates enablement, persistence and native-process lifecycle to the
    /// host plugin Module, then refreshes the UI projection.
    fn on_plugin_toggle(&mut self, plugin_id: &str) {
        match self.plugin_controller.toggle(plugin_id) {
            Ok(outcome) => self.status = outcome.message.into(),
            Err(error) => self.status = format!("plugin toggle failed: {error}").into(),
        }
        self.refresh_plugin_processes();
    }

    /// Grants the manifest permissions and enables the plugin through the
    /// controller's single persisted lifecycle operation.
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
        tier: sabaki_host::KataGoModelTier,
        starting_message: &'static str,
        success_message: &'static str,
        cx: &mut Context<Self>,
    ) {
        self.show_toast(starting_message, cx);
        let base_dir = base_dir.to_path_buf();
        let weak = cx.entity().downgrade();
        cx.spawn(
            move |_: gpui::WeakEntity<ShellApp>, cx: &mut gpui::AsyncApp| {
                let mut cx = cx.clone();
                async move {
                    let result = cx
                        .background_executor()
                        .spawn(async move { sabaki_host::install_katago_model(&base_dir, tier) })
                        .await;
                    weak.update(&mut cx, |shell, cx| match result {
                        Ok(path) => {
                            shell.status =
                                format!("KataGo model installed: {}", path.display()).into();
                            shell.show_toast(success_message, cx);
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

    fn on_plugin_command(&mut self, plugin_id: &str, command_id: &str, cx: &mut Context<Self>) {
        let builtin = sabaki_host::BuiltinPluginCommandRegistry::resolve(plugin_id, command_id);
        if builtin.is_some_and(|command| command.is_katago()) {
            let backend = sabaki_host::HardwareBackend::detect_current_platform();
            let base_dir = match file_workflow::plugin_install_root() {
                Ok(root) => root,
                Err(_) => std::env::temp_dir(),
            };

            match builtin.expect("KataGo command was classified") {
                sabaki_host::BuiltinPluginCommand::KataGoSetup => {
                    match sabaki_host::ensure_katago_environment(
                        &base_dir,
                        sabaki_host::KataGoModelTier::Balanced,
                        None,
                    ) {
                        Ok(env) => {
                            let engine_name = env.engine_record.name.clone();
                            self.engine_store.upsert(env.engine_record.clone());
                            if self.engine_roles.get(EngineRole::Analysis).is_none() {
                                self.engine_roles.assign(EngineRole::Analysis, &engine_name);
                            }
                            if self.engine_roles.get(EngineRole::White).is_none() {
                                self.engine_roles.assign(EngineRole::White, &engine_name);
                            }
                            let _ = self.engine_store.save(&mut self.settings);
                            let _ = self.persist_engine_roles();
                            let _ = sabaki_host::persist_settings_store(
                                &self.settings,
                                &mut self.settings_persistence,
                            );

                            let msg = if env.executable_exists {
                                format!(
                                    "⚡ KataGo 引擎已成功配置并就绪 ({})！\n已自动设为默认分析引擎",
                                    backend.label()
                                )
                            } else {
                                "⚡ KataGo 配置已生成 (GTP 规则已写入)！\n提示: 请确保已安装 katago (macOS: brew install katago)".to_string()
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
                sabaki_host::BuiltinPluginCommand::KataGoDownload(
                    sabaki_host::KataGoModelTier::Balanced,
                ) => self.download_katago_model(
                    &base_dir,
                    sabaki_host::KataGoModelTier::Balanced,
                    "⭐ 开始下载 10B 推荐模型 (94MB)...",
                    "⭐ 10B 推荐权重模型下载成功并就绪！",
                    cx,
                ),
                sabaki_host::BuiltinPluginCommand::KataGoDownload(
                    sabaki_host::KataGoModelTier::Lightweight,
                ) => self.download_katago_model(
                    &base_dir,
                    sabaki_host::KataGoModelTier::Lightweight,
                    "⚡ 开始下载 38MB 轻量分析模型...",
                    "⚡ 38MB 轻量分析模型下载成功！",
                    cx,
                ),
                sabaki_host::BuiltinPluginCommand::KataGoDownload(
                    sabaki_host::KataGoModelTier::Strongest,
                ) => self.download_katago_model(
                    &base_dir,
                    sabaki_host::KataGoModelTier::Strongest,
                    "🏆 开始下载 240MB 最强模型...",
                    "🏆 240MB 专家模型下载成功！",
                    cx,
                ),
                _ => unreachable!("registry only classifies KataGo commands here"),
            }
            return;
        }

        if builtin == Some(sabaki_host::BuiltinPluginCommand::FoxFetchLatest) {
            // A command click is a convenience path only. It uses the visible
            // user query if present; otherwise it tells the user exactly how to
            // select a game instead of downloading an unrelated hard-coded SGF.
            self.fetch_fox_query(cx);
            return;
        }

        if builtin == Some(sabaki_host::BuiltinPluginCommand::PositionCheck) {
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
            let msg = format!(
                "📊 局面检查完成: 黑子 {black_stones} 颗, 白子 {white_stones} 颗, 当前手序: 第 {} 手",
                snap.moves.len()
            );
            self.status = msg.clone().into();
            self.show_toast(msg, cx);
            return;
        }

        if builtin == Some(sabaki_host::BuiltinPluginCommand::SgfExport) {
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
            sabaki_plugin_runtime::PluginRuntime::Native
        ) {
            self.dispatch_native_plugin_command(plugin_id, command_id);
        } else if matches!(
            record.manifest.runtime,
            sabaki_plugin_runtime::PluginRuntime::Wasm
        ) {
            let snapshot_json =
                serde_json::to_string(&self.host.snapshot()).unwrap_or_else(|_| "{}".to_owned());
            match sabaki_host::load_wasm_module(&record)
                .and_then(|module| {
                    sabaki_host::invoke_wasm_command(
                        &record,
                        &module,
                        command_id,
                        serde_json::json!({}),
                        Some(&snapshot_json),
                    )
                })
                .map_err(sabaki_host::WasmWorkflowError::into_plugin_error)
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
                            sabaki_domain_core::GameTransaction,
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
        if self.analysis_task.is_some() {
            self.analysis_run.cancel_and_dispose();
            if let Some(task) = self.analysis_task.take() {
                task.detach();
            }
        }
        self.engine_controller.detach_all();
        self.active_console_role = None;
        self.analysis.clear();
        self.analysis_best_move = None;
    }

    /// Applies a settings edit through the validated store and persists it.
    /// A persistence failure rolls the store back so the UI never shows a
    /// value that is not on disk.
    fn apply_settings_edit(&mut self, edit: SettingEdit) {
        let key = edit.key().to_owned();
        let previous = self.settings.get(&key).cloned();
        match apply_setting_edit(&mut self.settings, edit) {
            Ok(()) => {
                match sabaki_host::persist_settings_store(
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
    fn on_settings_row_clicked(
        &mut self,
        row: &SettingRow,
        _: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if row.kind == sabaki_host::SettingKind::Boolean {
            return;
        }
        self.settings_editing_key = Some(row.key.clone());
        self.settings_draft = display_setting_value(row.value.as_ref()).into();
        window.focus(&self.settings_input_focus_handle);
        cx.notify();
    }

    fn on_settings_input_focus(
        &mut self,
        _: &MouseDownEvent,
        window: &mut Window,
        _: &mut Context<Self>,
    ) {
        window.focus(&self.settings_input_focus_handle);
    }

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
        let mut draft = self.settings_draft.to_string();
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
                self.settings_draft = "".into();
                cx.notify();
                return;
            }
            _ => {
                if let Some(key_char) = event.keystroke.key_char.as_ref() {
                    draft.push_str(key_char);
                }
            }
        }
        self.settings_draft = draft.into();
        cx.notify();
    }

    /// Commits the settings draft for the row: parses it by the host value
    /// kind, applies and persists it, then leaves the editing state.
    fn commit_settings_input(&mut self, row: &SettingRow, text: &str, cx: &mut Context<Self>) {
        let edit = match row.kind {
            sabaki_host::SettingKind::Number => number_edit(&row.key, text),
            sabaki_host::SettingKind::StringArray => Ok(string_array_edit(&row.key, text)),
            sabaki_host::SettingKind::NullableString => {
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
        self.settings_draft = "".into();
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
        self.external_file.detach_file();
        self.disconnect_all_engine_sessions();
        cx.notify();
    }

    fn play_sound_if_enabled(&mut self, cue: SoundCue) {
        play_if_enabled(&self.settings, self.sound_sink.as_mut(), cue);
    }

    fn on_pass(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let color = self.host.snapshot().board.next_player;
        let mut events = RecordingSink;
        match self.host.play_move(color, None, &mut events) {
            Ok(_) => {
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
                self.sync_engine_position(None, color, None);
            }
            Err(error) => self.status = format!("pass rejected: {error}").into(),
        }
        cx.notify();
    }

    fn on_resign(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.status = "resign is not implemented yet".into();
        cx.notify();
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
            sabaki_host::persist_settings_store(&self.settings, &mut self.settings_persistence)
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

    fn toggle_sidebar_setting(&mut self, key: &str, label: &str, cx: &mut Context<Self>) {
        // Left sidebar defaults to hidden on first launch; right panes default to visible.
        let default_visible = key != "view.show_leftsidebar";
        let current = self.settings.get_bool(key).unwrap_or(default_visible);
        if let Err(error) = self.settings.set(key, serde_json::json!(!current)) {
            self.status = format!("{label} not accepted: {error}").into();
        } else if let Err(error) =
            sabaki_host::persist_settings_store(&self.settings, &mut self.settings_persistence)
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
        let visible = right_pane_visible(show_graph, show_comments);
        let target = !visible;
        let mut failed = false;
        for (key, value) in [
            ("view.show_graph", target),
            (
                "view.show_comments",
                if target { show_comments } else { false },
            ),
        ] {
            if let Err(error) = self.settings.set(key, serde_json::json!(value)) {
                self.status = format!("panels sidebar not accepted: {error}").into();
                failed = true;
            }
        }
        if !failed {
            if let Err(error) =
                sabaki_host::persist_settings_store(&self.settings, &mut self.settings_persistence)
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
            ExternalCheckOutcome::Status(sabaki_host::ExternalFileStatus::Changed) => {
                self.status = "external change detected; save to keep local or reload".into();
            }
            ExternalCheckOutcome::Status(sabaki_host::ExternalFileStatus::Missing) => {
                self.status = "the source game file is missing".into();
            }
            ExternalCheckOutcome::Status(sabaki_host::ExternalFileStatus::Unreadable) => {
                self.status = "the source game file cannot be read".into();
            }
            ExternalCheckOutcome::Status(_) | ExternalCheckOutcome::Failed(_) => {}
        }
        cx.notify();
    }

    /// Explicitly reloads the document from its tracked source file, ignoring
    /// any pending external-file conflict.
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
                    .set_status(sabaki_host::ExternalFileStatus::Unreadable);
                self.status = format!("reload failed: {error}").into();
            }
        }
        cx.notify();
    }

    /// Keeps the local modifications and drops the source identity, so the
    /// next save must go through Save As.
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
        let decision = sabaki_host::decide_close_request(is_dirty, false);
        if decision == sabaki_host::CloseRequestAction::Allow {
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

    fn navigate_to_node(&mut self, target: sabaki_domain_core::NodeId, cx: &mut Context<Self>) {
        let transaction = sabaki_domain_core::GameTransaction {
            schema_version: sabaki_domain_core::CURRENT_TRANSACTION_SCHEMA_VERSION,
            transaction_type: sabaki_domain_core::GameTransactionType::Navigate,
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
        cx.notify();
    }

    fn on_board_vertex_mouse_down(&mut self, vertex: Vertex, cx: &mut Context<Self>) {
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
        let mut events = RecordingSink;
        match self.host.play_move(color, Some(vertex), &mut events) {
            Ok(_) => {
                self.last_vertex = Some(vertex);
                self.status = format!("move at {},{}", vertex.column, vertex.row).into();
                self.synchronize_recovery();
                self.play_sound_if_enabled(SoundCue::StonePlaced);
                self.sync_engine_position(None, color, Some(vertex));

                // Auto-reply if next player has an attached engine session in Play mode (Play vs AI)
                if self.mode == GameMode::Play {
                    let next_color = self.host.snapshot().board.next_player;
                    let next_role = match next_color {
                        Color::Black => EngineRole::Black,
                        Color::White => EngineRole::White,
                    };
                    if self.engine_controller.is_attached(next_role) {
                        self.trigger_engine_genmove(next_role, next_color, cx);
                    }
                }
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
    ) {
        if self.analysis_task.is_some() {
            self.analysis_run.request_replay_and_stop();
        }
        let errors = self.engine_controller.synchronize_move(
            source,
            color,
            vertex.map(|vertex| (vertex.column, vertex.row)),
        );
        for (role, error) in errors {
            self.status = format!("{} engine sync failed: {error}", role.label()).into();
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

    fn on_mode_action(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.mode == GameMode::Autoplay {
            self.advance_autoplay(None, cx);
        } else {
            self.status = match self.mode {
                GameMode::Find => "click an intersection to find its first occurrence".into(),
                GameMode::Guess => "click the next move to test your guess".into(),
                _ => self.status.clone(),
            };
            cx.notify();
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
        if self.hovered_vertex != Some(vertex) {
            self.hovered_vertex = Some(vertex);
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

    fn set_mode(&mut self, mode: GameMode, cx: &mut Context<Self>) {
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

    fn on_mode_selected(
        &mut self,
        mode: GameMode,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_mode(mode, cx);
    }

    /// Toggles the scoring mode: while active, board clicks cycle scoring
    /// overrides instead of placing moves.
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

    fn on_tool_selected(&mut self, tool: MarkupTool, cx: &mut Context<Self>) {
        self.active_tool = tool;
        self.line_start = None;
        if tool != MarkupTool::Play {
            self.mode = GameMode::Edit;
        }
        self.status = format!("tool: {}", tool.label()).into();
        cx.notify();
    }

    fn on_comment_focus(&mut self, _: &MouseDownEvent, window: &mut Window, _: &mut Context<Self>) {
        window.focus(&self.comment_focus_handle);
        self.active_text_input = Some(ActiveTextInput::Comment);
        let metadata = current_node_metadata(&self.host.snapshot());
        self.comment_input.set_text(metadata.comment);
    }

    fn on_node_title_focus(
        &mut self,
        _: &MouseDownEvent,
        window: &mut Window,
        _: &mut Context<Self>,
    ) {
        window.focus(&self.node_title_focus_handle);
        self.active_text_input = Some(ActiveTextInput::NodeTitle);
        self.node_title_input.set_text(
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
        input.handle_key(
            event.keystroke.key.as_str(),
            event.keystroke.key_char.as_deref(),
        )
    }

    fn on_node_title_key_down(
        &mut self,
        event: &gpui::KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match Self::handle_text_input_key(&mut self.node_title_input, event) {
            InputKeyResult::Submit => {
                let metadata = current_node_metadata(&self.host.snapshot());
                let title = self.node_title_input.text().trim().to_owned();
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
                        self.node_title_input.set_text("");
                        self.status = "node title saved".into();
                        self.synchronize_recovery();
                    }
                    Err(error) => self.status = format!("node title failed: {error}").into(),
                }
            }
            InputKeyResult::Cancel => self.node_title_input.set_text(
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
        match Self::handle_text_input_key(&mut self.comment_input, event) {
            InputKeyResult::Submit => {
                let comment = self.comment_input.text().to_owned();
                self.save_comment(&comment, cx);
                return;
            }
            InputKeyResult::Cancel => {
                self.comment_input
                    .set_text(current_node_metadata(&self.host.snapshot()).comment);
            }
            InputKeyResult::Changed | InputKeyResult::Ignored => {}
        }
        cx.notify();
    }

    fn on_node_annotation(&mut self, annotation: NodeAnnotation, cx: &mut Context<Self>) {
        let metadata = current_node_metadata(&self.host.snapshot());
        let active = match annotation.group() {
            AnnotationGroup::Move => metadata.move_annotation,
            AnnotationGroup::Position => metadata.position_annotation,
        };
        let selected = (active != Some(annotation)).then_some(annotation);
        let transactions =
            create_annotation_transactions(&metadata.node_id, selected, annotation.group());
        let mut events = RecordingSink;
        for transaction in transactions {
            if let Err(error) = self.host.apply_transaction(transaction, &mut events) {
                self.status = format!("annotation failed: {error}").into();
                cx.notify();
                return;
            }
        }
        self.status = format!(
            "{} annotation {}",
            annotation.label(),
            if selected.is_some() { "set" } else { "cleared" }
        )
        .into();
        self.synchronize_recovery();
        cx.notify();
    }

    fn on_hotspot_toggle(&mut self, cx: &mut Context<Self>) {
        let metadata = current_node_metadata(&self.host.snapshot());
        let mut events = RecordingSink;
        match self.host.apply_transaction(
            create_hotspot_transaction(&metadata.node_id, !metadata.hotspot),
            &mut events,
        ) {
            Ok(_) => {
                self.status = if metadata.hotspot {
                    "hotspot cleared"
                } else {
                    "hotspot set"
                }
                .into();
                self.synchronize_recovery();
            }
            Err(error) => self.status = format!("hotspot failed: {error}").into(),
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
                self.comment_input.set_text("");
                self.synchronize_recovery();
            }
            Err(error) => self.status = format!("comment failed: {error}").into(),
        }
        cx.notify();
    }

    fn on_variation_promote(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let metadata = current_node_metadata(&self.host.snapshot());
        let transaction = create_variation_transaction(&metadata.node_id, VariationAction::Promote);
        let mut events = RecordingSink;
        match self.host.apply_transaction(transaction, &mut events) {
            Ok(_) => {
                self.status = "variation promoted".into();
                self.synchronize_recovery();
            }
            Err(error) => self.status = format!("promote failed: {error}").into(),
        }
        cx.notify();
    }

    fn on_variation_remove(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let metadata = current_node_metadata(&self.host.snapshot());
        let transaction = create_variation_transaction(&metadata.node_id, VariationAction::Remove);
        let mut events = RecordingSink;
        match self.host.apply_transaction(transaction, &mut events) {
            Ok(_) => {
                self.status = "variation removed".into();
                self.synchronize_recovery();
            }
            Err(error) => self.status = format!("remove failed: {error}").into(),
        }
        cx.notify();
    }

    fn open_drawer(&mut self, drawer: ActiveDrawer, status: &str, cx: &mut Context<Self>) {
        self.active_drawer = Some(drawer);
        self.status = status.to_owned().into();
        cx.notify();
    }

    fn open_preferences(&mut self, cx: &mut Context<Self>) {
        self.open_drawer(ActiveDrawer::Preferences, "preferences opened", cx);
    }

    /// Opens the Preferences drawer from the player bar hamburger menu.
    fn open_side_menu(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.open_drawer(ActiveDrawer::Preferences, "preferences opened", cx);
    }

    fn set_plugin_menu_open(&mut self, open: bool, cx: &mut Context<Self>) {
        self.plugin_menu_open = open;
        cx.notify();
    }

    fn toggle_plugin_menu(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.set_plugin_menu_open(!self.plugin_menu_open, cx);
    }

    fn close_plugin_menu(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.set_plugin_menu_open(false, cx);
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

    fn close_drawer(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.active_drawer = None;
        cx.notify();
    }

    fn open_game_graph_context_menu(
        &mut self,
        node_id: sabaki_domain_core::NodeId,
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

    fn on_navigate_first(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.navigate(NavigationDirection::First, cx);
    }

    fn on_navigate_previous(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.navigate(NavigationDirection::Previous, cx);
    }

    fn on_navigate_next(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.navigate(NavigationDirection::Next, cx);
    }

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
        let live_winrate = (!self.analysis.is_empty())
            .then(|| best_analysis_winrate(&self.analysis, snapshot.board.next_player));
        let winrate_points = winrate_history(&snapshot, live_winrate, snapshot.board.next_player);
        let winrate_metric =
            WinrateGraphMetric::from_setting(self.settings.get_str("board.analysis_type"));
        let winrate_plot_points = graph_plot_points(
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
        let blunder_evals: Vec<sabaki_host::ReviewedPosition> = winrate_points
            .iter()
            .enumerate()
            .map(|(idx, pt)| {
                let winrate = pt.black_winrate.unwrap_or(0.5);
                let score = pt.black_score_lead;
                let node_id = pt.node_id.clone();
                let player = if idx % 2 == 1 {
                    Color::Black
                } else {
                    Color::White
                };
                (idx, node_id, player, None, winrate, score, None, vec![])
            })
            .collect();
        let blunders = sabaki_host::find_blunders(
            &blunder_evals,
            10.0,
            self.settings
                .get("view.winrategraph_blunderthreshold")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(5.0),
        );

        let inspector_metadata = current_node_metadata(&snapshot);
        let settings_rows = panel_setting_rows(&self.settings);
        let external_status = self.external_file.status();
        let _external_conflict = matches!(
            external_status.status,
            sabaki_host::ExternalFileStatus::Changed
                | sabaki_host::ExternalFileStatus::Missing
                | sabaki_host::ExternalFileStatus::Unreadable
        );
        // Left engines sidebar defaults to collapsed on first launch (matching reference screenshot);
        // right sidebar (game graph & comments) defaults to expanded.
        let show_left_sidebar = self
            .settings
            .get_bool("view.show_leftsidebar")
            .unwrap_or(false);
        let show_graph = self.settings.get_bool("view.show_graph").unwrap_or(true);
        let show_comments = self.settings.get_bool("view.show_comments").unwrap_or(true);
        let show_right_sidebar = right_pane_visible(show_graph, show_comments);
        let palette = self.palette;
        let weak_shell = cx.entity().downgrade();
        let on_node_clicked = Rc::new(
            move |node_id: &sabaki_domain_core::NodeId, _window: &mut Window, cx: &mut App| {
                weak_shell
                    .update(cx, |shell, cx| shell.navigate_to_node(node_id.clone(), cx))
                    .ok();
            },
        );

        let weak_shell_for_context = cx.entity().downgrade();
        let on_node_context_requested = Rc::new(
            move |node_id: &sabaki_domain_core::NodeId, _window: &mut Window, cx: &mut App| {
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
        let side_panels = if show_left_sidebar {
            self.left_sidebar_width
        } else {
            0.0
        } + if show_right_sidebar {
            self.right_sidebar_width
        } else {
            0.0
        };
        let available_width = (window_width - side_panels - 32.0).max(240.0);
        let available_height = (window_height - 40.0 - 36.0 - 16.0).max(240.0);
        let board_pixel_size = available_width.min(available_height).max(BOARD_PIXEL_SIZE);

        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(theme_color))
            .text_color(rgb(palette.text))
            .child(panels::render_titlebar(
                show_left_sidebar,
                show_right_sidebar,
                palette,
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
                    .child(if show_left_sidebar {
                        div()
                            .id("left-sidebar")
                            .debug_selector(|| "left-sidebar".to_owned())
                            .flex_none()
                            .w(px(self.left_sidebar_width))
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
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .items_center()
                            .justify_center()
                            .child(panels::render_goban_area(
                                &snapshot,
                                &self.theme,
                                self.analysis_best_move,
                                board_pixel_size,
                                self,
                                cx,
                            )),
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
                            .w(px(self.right_sidebar_width))
                            .h_full()
                            .min_h_0()
                            .flex()
                            .flex_col()
                            .pr_1()
                            .border_l_1()
                            .border_color(rgb(palette.border))
                            .bg(rgb(palette.panel))
                            .child(
                                if self
                                    .settings
                                    .get_bool("view.show_winrategraph")
                                    .unwrap_or(true)
                                {
                                    panels::render_winrate_graph_panel(
                                        &winrate_plot_points,
                                        winrate_metric,
                                        self.winrate_graph_height,
                                        palette,
                                        {
                                            let handler = on_node_clicked.clone();
                                            move |node_id, window, cx| handler(node_id, window, cx)
                                        },
                                    )
                                } else {
                                    div().id("winrate-graph-panel-hidden")
                                },
                            )
                            .child(panels::render_blunder_list_panel(&blunders, palette, cx))
                            .child(
                                if self
                                    .settings
                                    .get_bool("view.show_winrategraph")
                                    .unwrap_or(true)
                                {
                                    panels::render_right_sidebar_split_handle(
                                        SplitPane::WinrateGraph,
                                        palette,
                                        cx,
                                    )
                                } else {
                                    div().id("winrate-graph-splitter-hidden")
                                },
                            )
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
            .child(panels::render_player_bar(
                &snapshot, &status, palette, self, cx,
            ))
            .child(if self.plugin_menu_open {
                panels::render_plugin_menu(self, cx)
            } else {
                div().id("plugin-menu-hidden")
            })
            .child(match self.active_drawer {
                Some(ActiveDrawer::Preferences) => {
                    panels::render_preferences_drawer(&settings_rows, self, cx)
                }
                Some(ActiveDrawer::GameInfo) => {
                    panels::render_game_info_drawer(&snapshot, self, cx)
                }
                Some(ActiveDrawer::Score) => panels::render_score_drawer(&snapshot, self, cx),
                Some(ActiveDrawer::About) => panels::render_about_drawer(self, cx),
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
                            .rounded_lg()
                            .bg(rgb(0x1c1c1e))
                            .border_1()
                            .border_color(rgb(0x3a3a3c))
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(0xf5f5f7))
                            .child(message.clone()),
                    )
            }))
    }
}

impl ShellApp {
    fn active_text_input_mut(&mut self) -> Option<&mut NativeTextInput> {
        match self.active_text_input {
            Some(ActiveTextInput::Comment) => Some(&mut self.comment_input),
            Some(ActiveTextInput::NodeTitle) => Some(&mut self.node_title_input),
            None => None,
        }
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
        None
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {}

    fn replace_text_in_range(
        &mut self,
        range: Option<std::ops::Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(input) = self.active_text_input_mut() {
            input.replace_utf16_range(range, text);
            cx.notify();
        }
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<std::ops::Range<usize>>,
        new_text: &str,
        _: Option<std::ops::Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replace_text_in_range(range, new_text, window, cx);
    }

    fn bounds_for_range(
        &mut self,
        _: std::ops::Range<usize>,
        element_bounds: Bounds<gpui::Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<gpui::Pixels>> {
        self.active_text_input.map(|_| element_bounds)
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

impl sabaki_host::HostEventSink for RecordingSink {
    fn emit(&mut self, _event: sabaki_host::HostEvent) {}
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
            name: "Saba.rs".into(),
            items: vec![
                MenuItem::action("New Game", NewGame),
                MenuItem::action("Open…", OpenGame),
                MenuItem::separator(),
                MenuItem::action("Save", SaveGame),
                MenuItem::action("Save As…", SaveGameAs),
                MenuItem::separator(),
                MenuItem::action("Undo", UndoMove),
                MenuItem::action("Redo", RedoMove),
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
                MenuItem::action("Score", OpenScore),
                MenuItem::action("Preferences", OpenPreferences),
                MenuItem::separator(),
                MenuItem::action("Toggle Game Graph", ToggleGameGraph),
                MenuItem::action("Toggle Comments", ToggleComments),
                MenuItem::action("Toggle Winrate Graph", ToggleWinrateGraph),
                MenuItem::action("Toggle Coordinates", ToggleCoordinates),
                MenuItem::action("Toggle Move Numbers", ToggleMoveNumbers),
            ],
        },
        Menu {
            name: "Mode".into(),
            items: vec![
                MenuItem::action("Play", SetPlayMode),
                MenuItem::action("Edit", SetEditMode),
                MenuItem::action("Score", SetScoringMode),
                MenuItem::action("Estimate", SetEstimatorMode),
                MenuItem::action("Find", SetFindMode),
                MenuItem::action("Guess", SetGuessMode),
                MenuItem::action("Autoplay", SetAutoplayMode),
            ],
        },
        Menu {
            name: "Engines".into(),
            items: vec![
                MenuItem::action("Show Engines Sidebar", ToggleEnginesSidebar),
                MenuItem::separator(),
                MenuItem::action("Generate Engine Move", GenerateEngineMove),
                MenuItem::action("Start Analysis", StartAnalysis),
                MenuItem::action("Stop Analysis", StopAnalysis),
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
            items: vec![MenuItem::action("About Saba.rs", OpenAbout)],
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
fn schedule_external_check(
    window: &mut Window,
    _cx: &mut App,
    window_handle: gpui::WindowHandle<ShellApp>,
    shell: Entity<ShellApp>,
    last_check: Rc<Cell<Option<Instant>>>,
) {
    window.on_next_frame(move |window, cx| {
        let due = match last_check.get() {
            Some(previous) => previous.elapsed() >= EXTERNAL_CHECK_INTERVAL,
            None => true,
        };
        if due && window_handle.is_active(cx).unwrap_or(false) {
            last_check.set(Some(Instant::now()));
            shell.update(cx, |shell, cx| shell.check_external_file_now(cx));
        }
        schedule_external_check(window, cx, window_handle, shell, last_check);
    });
}

fn main() {
    let startup_file = std::env::args().nth(1).map(PathBuf::from);
    Application::new().run(move |cx: &mut App| {
        let settings_persistence = match NativeSettingsPersistence::for_current_user() {
            Ok(persistence) => persistence,
            Err(error) => {
                eprintln!("settings persistence unavailable ({error}); using a temp directory");
                NativeSettingsPersistence::new(std::env::temp_dir().join("sabaki-gpui-config"))
            }
        };
        let mut initial_status = "new game".to_owned();
        let settings = match sabaki_host::load_settings_store(&settings_persistence) {
            Ok(loaded) => {
                for invalid in &loaded.validation.invalid_values {
                    initial_status = format!("ignored setting {}: {}", invalid.key, invalid);
                }
                // Design §8.1: user styles.css is not executed; report which
                // color rules could migrate to theme tokens.
                if !loaded.store.user_styles().trim().is_empty() {
                    let report = sabaki_host::analyze_legacy_styles(loaded.store.user_styles());
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
                sabaki_host::SettingsStore::default()
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
                move |_, cx| {
                    let startup_file = startup_file.clone();
                    let initial_status = initial_status.clone();
                    let settings = settings.clone();
                    let settings_persistence = settings_persistence.clone();
                    let host_persistence = NativeHostPersistence::for_current_user()
                        .unwrap_or_else(|_| {
                            NativeHostPersistence::new(
                                std::env::temp_dir().join("sabaki-gpui-config"),
                            )
                        });
                    let plugin_persistence = NativePluginPersistence::for_current_user()
                        .unwrap_or_else(|_| {
                            NativePluginPersistence::new(
                                std::env::temp_dir().join("sabaki-gpui-config"),
                            )
                        });
                    cx.new(move |cx| {
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
                    })
                },
            )
            .unwrap();
        let shell: Entity<ShellApp> = window.update(cx, |_, _, cx| cx.entity()).unwrap();

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
        let shell_toggle_winrate = shell.clone();
        cx.on_action(move |_: &ToggleWinrateGraph, cx| {
            shell_toggle_winrate.update(cx, |shell, cx| {
                shell.toggle_sidebar_setting("view.show_winrategraph", "winrate graph", cx)
            });
        });
        let shell_toggle_coords = shell.clone();
        cx.on_action(move |_: &ToggleCoordinates, cx| {
            shell_toggle_coords.update(cx, |shell, cx| {
                shell.toggle_sidebar_setting("view.show_coordinates", "board coordinates", cx)
            });
        });
        let shell_toggle_move_nums = shell.clone();
        cx.on_action(move |_: &ToggleMoveNumbers, cx| {
            shell_toggle_move_nums.update(cx, |shell, cx| {
                shell.toggle_sidebar_setting("view.show_move_numbers", "move numbers", cx)
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
        let shell_find_mode = shell.clone();
        cx.on_action(move |_: &SetFindMode, cx| {
            shell_find_mode.update(cx, |shell, cx| shell.set_mode(GameMode::Find, cx));
        });
        let shell_guess_mode = shell.clone();
        cx.on_action(move |_: &SetGuessMode, cx| {
            shell_guess_mode.update(cx, |shell, cx| shell.set_mode(GameMode::Guess, cx));
        });
        let shell_autoplay_mode = shell.clone();
        cx.on_action(move |_: &SetAutoplayMode, cx| {
            shell_autoplay_mode.update(cx, |shell, cx| shell.set_mode(GameMode::Autoplay, cx));
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
        let shell_last = shell.clone();
        cx.on_action(move |_: &GoToLastNode, cx| {
            shell_last.update(cx, |shell, cx| {
                shell.navigate(NavigationDirection::Last, cx)
            });
        });
        cx.on_action(|_: &Quit, cx| cx.quit());

        cx.bind_keys([
            KeyBinding::new("cmd-n", NewGame, None),
            KeyBinding::new("cmd-o", OpenGame, None),
            KeyBinding::new("cmd-s", SaveGame, None),
            KeyBinding::new("cmd-shift-s", SaveGameAs, None),
            KeyBinding::new("cmd-comma", OpenPreferences, None),
            KeyBinding::new("cmd-1", SetPlayMode, None),
            KeyBinding::new("cmd-2", SetEditMode, None),
            KeyBinding::new("cmd-3", SetScoringMode, None),
            KeyBinding::new("cmd-4", SetEstimatorMode, None),
            KeyBinding::new("cmd-shift-b", ToggleEnginesSidebar, None),
            KeyBinding::new("cmd-shift-c", ToggleCoordinates, None),
            KeyBinding::new("cmd-shift-m", ToggleMoveNumbers, None),
            KeyBinding::new("cmd-g", GenerateEngineMove, None),
            KeyBinding::new("cmd-z", UndoMove, None),
            KeyBinding::new("cmd-shift-z", RedoMove, None),
            KeyBinding::new("cmd-left", GoToFirstNode, None),
            KeyBinding::new("left", GoToPreviousNode, None),
            KeyBinding::new("right", GoToNextNode, None),
            KeyBinding::new("cmd-right", GoToLastNode, None),
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
    use sabaki_host::{CloseRequestAction, SettingsStore, decide_close_request};
    use serde_json::json;

    #[test]
    fn shell_menus_cover_file_edit_view_mode_engine_and_navigation() {
        let names = super::shell_menus()
            .into_iter()
            .map(|menu| menu.name.to_string())
            .collect::<Vec<_>>();
        for expected in [
            "File", "Edit", "View", "Mode", "Engines", "Navigate", "Help",
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
        assert!(!properties.contains_key("HA"));
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
    use sabaki_domain_core::{Color, GameMode, Vertex};
    use sabaki_host::SettingsStore;
    use std::path::PathBuf;

    use crate::RecordingSink;
    use crate::file_workflow::{
        NativeHostPersistence, NativePluginPersistence, NativeSettingsPersistence,
    };

    fn temp_config(test_name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sabaki-headless-{test_name}-{}",
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
        shell_entity.update(&mut cx, |shell, _cx| run(shell));
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
            "sabaki-release-fixture-workflow-{}.sgf",
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
            assert_eq!(arguments, vec!["B", "10", "rootInfo", "true"]);
        });
    }

    #[test]
    fn fresh_profile_enables_analysis_markers_and_engine_sidebar() {
        with_headless_shell("analysis-visible-defaults", |shell| {
            assert_eq!(shell.settings.get_bool("board.show_analysis"), Some(true));
            assert_eq!(shell.settings.get_bool("view.show_leftsidebar"), Some(true));
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
    use sabaki_host::SettingsStore;
    use std::ops::Deref;
    use std::path::PathBuf;

    fn temp_config(test_name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sabaki-frontend-{test_name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp config dir is created");
        dir
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
        let window_handle = cx.add_window(|_, cx| {
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
        let vcx = VisualTestContext::from_window(*window_handle.deref(), &cx).into_mut();
        vcx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        vcx.run_until_parked();

        // A fresh profile exposes the engines and analysis controls rather
        // than making a connected KataGo process look inert.
        assert!(vcx.debug_bounds("left-sidebar").is_some());
        assert!(vcx.debug_bounds("right-sidebar").is_some());
        window_handle
            .update(&mut vcx.cx, |shell, _window, cx| {
                shell.toggle_sidebar_setting("view.show_leftsidebar", "engines sidebar", cx);
            })
            .expect("left-pane toggle persists through the same handler");
        assert_eq!(
            window_handle
                .read_with(&vcx.cx, |shell, _| {
                    shell.settings.get_bool("view.show_leftsidebar")
                })
                .expect("shell remains alive"),
            Some(false),
            "the visible first-launch left pane must persist hidden after toggling"
        );
        window_handle
            .update(&mut vcx.cx, |shell, _window, cx| {
                shell.toggle_right_sidebar(cx)
            })
            .expect("right-pane toggle persists through its handler");
        assert_eq!(
            window_handle
                .read_with(&vcx.cx, |shell, _| {
                    (
                        shell.settings.get_bool("view.show_graph"),
                        shell.settings.get_bool("view.show_comments"),
                    )
                })
                .expect("shell remains alive"),
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
        let window_handle = cx.add_window(|_, cx| {
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
        let vcx = VisualTestContext::from_window(*window_handle.deref(), &cx).into_mut();
        vcx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        vcx.run_until_parked();

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
        assert!(vcx.debug_bounds("plugin-menu").is_none());
        window_handle
            .update(&mut vcx.cx, |shell, _window, cx| {
                shell.set_plugin_menu_open(true, cx);
            })
            .expect("plugin button opens compact overlay menu");
        vcx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("plugin-menu").is_some());
        window_handle
            .update(&mut vcx.cx, |shell, _window, cx| shell.open_preferences(cx))
            .expect("Preferences action must update the shell");
        vcx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        vcx.run_until_parked();
        let preferences_drawer = vcx
            .debug_bounds("preferences-drawer")
            .expect("Preferences action must open a drawer");
        let settings_drawer_panel = vcx
            .debug_bounds("settings-panel")
            .expect("settings must render inside Preferences");
        assert_eq!(
            preferences_drawer.size.width,
            px(380.0),
            "Preferences drawer must keep its stable desktop width"
        );
        assert!(
            settings_drawer_panel.origin.x >= preferences_drawer.origin.x
                && settings_drawer_panel.right() <= preferences_drawer.right(),
            "settings {:?} must stay inside Preferences {:?}",
            settings_drawer_panel,
            preferences_drawer
        );
        let preferences_close = vcx
            .debug_bounds("preferences-close")
            .expect("Preferences drawer must expose a close control");
        vcx.simulate_click(preferences_close.center(), gpui::Modifiers::none());
        vcx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        vcx.run_until_parked();
        assert_eq!(
            window_handle
                .read_with(&vcx.cx, |shell, _| shell.active_drawer)
                .expect("shell remains available"),
            None,
            "closing Preferences must clear drawer state"
        );
        window_handle
            .update(&mut vcx.cx, |shell, _window, cx| shell.open_game_info(cx))
            .expect("Game Info action must update the shell");
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
        window_handle
            .update(&mut vcx.cx, |shell, _window, cx| shell.open_score(cx))
            .expect("Score action must update the shell");
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
            window_handle
                .read_with(&vcx.cx, |shell, _| shell.active_drawer)
                .expect("shell remains available"),
            None,
            "closing read-only drawers must clear drawer state"
        );
        let engine_roster = vcx
            .debug_bounds("engine-roster")
            .expect("engine roster must have a debug selector");
        let gtp_console = vcx
            .debug_bounds("gtp-console")
            .expect("GTP console must have a debug selector");
        let peer_list_splitter = vcx
            .debug_bounds("peer-list-splitter")
            .expect("engine sidebar splitter must have a debug selector");
        for (name, bounds) in [
            ("engine roster", engine_roster),
            ("GTP console", gtp_console),
        ] {
            assert!(
                f32::from(bounds.origin.x) >= f32::from(left_sidebar.origin.x) - 0.5
                    && f32::from(bounds.right()) <= f32::from(left_sidebar.right()) + 0.5,
                "{name} {:?} must live in the left sidebar {:?}",
                bounds,
                left_sidebar
            );
        }
        assert!(
            engine_roster.bottom() <= peer_list_splitter.origin.y
                && peer_list_splitter.bottom() <= gtp_console.origin.y,
            "engine roster {:?}, splitter {:?}, and console {:?} must be vertically ordered",
            engine_roster,
            peer_list_splitter,
            gtp_console
        );
        assert!(
            (f32::from(engine_roster.size.height) - 130.0).abs() < 0.75,
            "peer list height must use the persisted setting, got {:?}",
            engine_roster.size
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
        let winrate_graph = vcx
            .debug_bounds("winrate-graph-panel")
            .expect("enabled WinrateGraph must render");
        let winrate_splitter = vcx
            .debug_bounds("winrate-graph-splitter")
            .expect("WinrateGraph internal splitter must render");
        let properties_splitter = vcx
            .debug_bounds("properties-splitter")
            .expect("properties internal splitter must render");
        let comment_annotations = vcx
            .debug_bounds("commentbox-annotations")
            .expect("enabled CommentBox annotations must render");
        assert!(
            winrate_graph.bottom() <= winrate_splitter.origin.y
                && winrate_splitter.bottom() <= game_graph_region.origin.y,
            "winrate graph {:?}, splitter {:?}, and GameGraph {:?} must be vertically ordered",
            winrate_graph,
            winrate_splitter,
            game_graph_region
        );
        assert!(
            game_graph_region.origin.y <= properties_splitter.origin.y
                && properties_splitter.origin.y <= properties_region.origin.y,
            "GameGraph {:?}, properties splitter {:?}, and CommentBox {:?} must be vertically ordered",
            game_graph_region,
            properties_splitter,
            properties_region
        );
        assert!(
            f32::from(comment_annotations.origin.x) >= f32::from(properties_region.origin.x) - 0.5
                && f32::from(comment_annotations.right())
                    <= f32::from(properties_region.right()) + 0.5,
            "CommentBox annotation controls {:?} must stay inside properties {:?}",
            comment_annotations,
            properties_region
        );

        let board = window_handle
            .read_with(&vcx.cx, |shell, _| shell.host.snapshot().board.clone())
            .unwrap();
        let goban_size = f32::from(goban_bounds.size.width);
        let (x, y) = crate::goban_view::intersection_position(&board, goban_size, 16, 16);
        let click = gpui::point(
            px(f32::from(goban_bounds.origin.x) + x),
            px(f32::from(goban_bounds.origin.y) + y),
        );
        let before = window_handle
            .read_with(&vcx.cx, |shell, _| shell.host.snapshot().moves.len())
            .unwrap();
        vcx.simulate_click(click, gpui::Modifiers::none());
        let after = window_handle
            .read_with(&vcx.cx, |shell, _| {
                let snap = shell.host.snapshot();
                (snap.moves.len(), snap.moves.last().and_then(|m| m.vertex))
            })
            .unwrap();
        assert_eq!(
            after.0,
            before + 1,
            "clicking the rendered intersection must place the next black stone"
        );
        assert!(after.1.is_some(), "placed move must have a vertex");
        let pass_bounds = vcx
            .debug_bounds("pass-button")
            .expect("the pass button must have a debug selector");
        vcx.simulate_click(pass_bounds.center(), gpui::Modifiers::none());
        let after_pass = window_handle
            .read_with(&vcx.cx, |shell, _| {
                let snapshot = shell.host.snapshot();
                (
                    snapshot.moves.len(),
                    snapshot.moves.last().map(|m| m.vertex),
                )
            })
            .unwrap();
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
        let current_node_after_graph_click = window_handle
            .read_with(&vcx.cx, |shell, _| shell.host.snapshot().current_node_id)
            .unwrap();
        assert_eq!(
            current_node_after_graph_click, "root",
            "clicking the GameGraph root must navigate to the root node"
        );
        window_handle
            .update(&mut vcx.cx, |shell, _window, cx| {
                shell.open_game_graph_context_menu("root".to_owned(), cx);
                shell.toggle_game_graph_context_hotspot(&MouseDownEvent::default(), _window, cx);
            })
            .expect("context-menu hotspot action must succeed");
        assert!(
            window_handle
                .read_with(&vcx.cx, |shell, _| {
                    shell
                        .host
                        .snapshot()
                        .nodes
                        .iter()
                        .find(|node| node.id == "root")
                        .is_some_and(|node| node.properties.contains_key("HO"))
                })
                .unwrap(),
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
        let persisted_width = window_handle
            .read_with(&vcx.cx, |shell, _| {
                shell
                    .settings
                    .get("view.leftsidebar_width")
                    .and_then(serde_json::Value::as_f64)
            })
            .unwrap();
        assert!(
            persisted_width.is_some_and(|width| (width - 310.0).abs() < 0.75),
            "finishing the drag must persist the pane width, got {persisted_width:?}"
        );

        let peer_splitter = vcx
            .debug_bounds("peer-list-splitter")
            .expect("the engine sidebar splitter must still have a debug selector");
        let peer_drag_start = peer_splitter.center();
        let peer_drag_end = gpui::point(
            px(f32::from(peer_drag_start.x)),
            px(f32::from(peer_drag_start.y) + 40.0),
        );
        vcx.simulate_mouse_down(
            peer_drag_start,
            gpui::MouseButton::Left,
            gpui::Modifiers::none(),
        );
        vcx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        vcx.run_until_parked();
        vcx.simulate_mouse_move(
            peer_drag_end,
            Some(gpui::MouseButton::Left),
            gpui::Modifiers::none(),
        );
        vcx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        vcx.simulate_mouse_up(
            peer_drag_end,
            gpui::MouseButton::Left,
            gpui::Modifiers::none(),
        );
        vcx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        let resized_roster = vcx
            .debug_bounds("engine-roster")
            .expect("the engine roster must still have a debug selector");
        assert!(
            (f32::from(resized_roster.size.height) - 170.0).abs() < 0.75,
            "dragging the engine splitter 40px should resize the roster to 170px, got {:?}",
            resized_roster.size
        );
        let persisted_height = window_handle
            .read_with(&vcx.cx, |shell, _| {
                shell
                    .settings
                    .get("view.peerlist_height")
                    .and_then(serde_json::Value::as_f64)
            })
            .unwrap();
        assert!(
            persisted_height.is_some_and(|height| (height - 170.0).abs() < 0.75),
            "finishing the vertical drag must persist the peer list height, got {persisted_height:?}"
        );
        let _ = std::fs::remove_dir_all(&config);
    }
}
