mod benchmark;
mod dialog_service;
mod engine_console;
mod external_file;
mod file_workflow;
mod goban_view;
mod layout;
mod markup;
mod navigation;
mod node_inspector;
mod panels;
mod plugin_contribution;
mod plugin_panel;
mod settings;
mod settings_form;
mod theme;
mod variation_tree;

use std::{
    cell::Cell,
    collections::BTreeMap,
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use gpui::{
    App, Application, Bounds, Context, Div, Entity, FocusHandle, InteractiveElement, KeyBinding,
    Menu, MenuItem, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, SharedString, Task,
    Window, WindowBounds, WindowOptions, actions, div, prelude::*, px, rgb, size,
};
use sabaki_domain_core::gtp::AnalysisStream;
use sabaki_domain_core::legacy::handicap_placement;
use sabaki_domain_core::{Color, Vertex};
use sabaki_host::{HostPersistence, replay_position_stream};

use crate::benchmark::{LargeGameBenchmark, SnapshotBenchmark};
use crate::dialog_service::{DialogService, NativeGameFileAccess, RfdDialogService};
use crate::engine_console::{
    EngineLogEntry, GtpEngine, MockGtpEngine, analysis_command_from_settings, best_analysis_move,
    entry_for_response, format_console_command, format_gtp_vertex, merge_analysis_entries,
    parse_engine_spec, parse_gtp_vertex, parse_stream_line,
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
    MarkupTool, create_markup_transaction, create_scoring_transaction, create_setup_transactions,
    next_scoring_override,
};
use crate::navigation::{
    NavigationDirection, navigation_availability, navigation_target, position_label,
};
use crate::node_inspector::{
    VariationAction, create_comment_transaction, create_variation_transaction,
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
use crate::theme::{ThemeTokens, UiPalette, ui_palette};
use crate::variation_tree::build_variation_tree_layout;

const BOARD_PIXEL_SIZE: f32 = 420.0;

actions!(
    sabaki_gpui,
    [
        NewGame,
        OpenGame,
        SaveGame,
        SaveGameAs,
        UndoMove,
        RedoMove,
        GoToFirstNode,
        GoToPreviousNode,
        GoToNextNode,
        GoToLastNode,
        Quit,
    ]
);

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
    engine_store: sabaki_host::EngineStore,
    engine_session: Option<sabaki_host::EngineSession<sabaki_host::ProcessGtpTransport>>,
    analysis: Vec<sabaki_host::AnalysisEntry>,
    analysis_best_move: Option<Vertex>,
    analysis_task: Option<Task<()>>,
    analysis_stop_flag: Arc<AtomicBool>,
    analysis_generation: Arc<AtomicUsize>,
    engine: MockGtpEngine,
    engine_log: Vec<EngineLogEntry>,
    engine_input_focus_handle: FocusHandle,
    engine_draft: SharedString,
    engine_spec_draft: SharedString,
    engine_spec_focus_handle: FocusHandle,
    theme_choice: ThemeChoice,
    theme: ThemeTokens,
    palette: UiPalette,
    left_sidebar_width: f32,
    right_sidebar_width: f32,
    split_drag: Option<SplitDrag>,
    installed_themes: Vec<sabaki_host::InstalledTheme>,
    legacy_asar_themes: Vec<std::path::PathBuf>,
    board_size: usize,
    settings_editing_key: Option<String>,
    settings_draft: SharedString,
    settings_input_focus_handle: FocusHandle,
    plugin_store: sabaki_host::PluginStore,
    plugin_persistence: NativePluginPersistence,
    plugin_supervisors: std::collections::BTreeMap<String, sabaki_host::PluginSupervisor>,
    installed_plugins: Vec<PluginPanelEntry>,
    last_vertex: Option<Vertex>,
    active_tool: MarkupTool,
    scoring_mode: bool,
    comment_focus_handle: FocusHandle,
    comment_draft: SharedString,
    benchmark: SharedString,
    large_game_benchmark: SharedString,
    status: SharedString,
}

/// Replays the current position into a connected engine session before a
/// streaming analysis command, using the session's own GTP transport.
fn replay_position_session(
    session: &mut sabaki_host::EngineSession<sabaki_host::ProcessGtpTransport>,
    board_size: usize,
    moves: &[sabaki_domain_core::MoveDto],
) -> Result<(), sabaki_domain_core::gtp::GtpError> {
    session.send_command("boardsize", vec![board_size.to_string()])?;
    session.send_command("clear_board", Vec::new())?;
    for move_dto in moves {
        let color = match move_dto.color {
            sabaki_domain_core::Color::Black => "B",
            sabaki_domain_core::Color::White => "W",
        };
        let vertex = match move_dto.vertex {
            Some(vertex) => {
                crate::engine_console::format_gtp_vertex(board_size, vertex.column, vertex.row)
            }
            None => "pass".to_owned(),
        };
        session.play(color, &vertex)?;
    }
    Ok(())
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
    fn new(
        settings: sabaki_host::SettingsStore,
        settings_persistence: NativeSettingsPersistence,
        persistence: NativeHostPersistence,
        plugin_persistence: NativePluginPersistence,
        initial_status: String,
        startup_file: Option<PathBuf>,
        dialog_service: Box<dyn DialogService>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut host = sabaki_host::HostApplication::default();
        let file_access = NativeGameFileAccess::default();
        let mut events = RecordingSink::default();
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
        let benchmark_result = SnapshotBenchmark::new_with_moves(19, 19, 120).run(2_000);
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
        let plugin_store =
            match sabaki_host::PluginStore::restore(&plugin_persistence, &plugin_install_root) {
                Ok(store) => store,
                Err(error) => {
                    status = format!("plugin scan failed: {error}");
                    sabaki_host::PluginStore::default()
                }
            };
        let installed_plugins = plugin_store.list().iter().map(entry_from_record).collect();
        let engine_store = sabaki_host::EngineStore::from_settings(&settings).unwrap_or_default();

        Self {
            host,
            file_access,
            dialog_service,
            persistence,
            recent_files,
            autosave,
            settings,
            settings_persistence,
            external_file: sabaki_host::ExternalFileStore::default(),
            engine_store,
            engine_session: None,
            analysis: Vec::new(),
            analysis_best_move: None,
            analysis_task: None,
            analysis_stop_flag: Arc::new(AtomicBool::new(false)),
            analysis_generation: Arc::new(AtomicUsize::new(0)),
            engine: MockGtpEngine::default(),
            engine_log: Vec::new(),
            engine_input_focus_handle: cx.focus_handle(),
            engine_draft: "".into(),
            engine_spec_draft: "".into(),
            engine_spec_focus_handle: cx.focus_handle(),
            theme_choice,
            theme,
            palette,
            left_sidebar_width,
            right_sidebar_width,
            split_drag: None,
            installed_themes,
            legacy_asar_themes,
            board_size,
            settings_editing_key: None,
            settings_draft: "".into(),
            settings_input_focus_handle: cx.focus_handle(),
            plugin_store,
            plugin_persistence,
            plugin_supervisors: std::collections::BTreeMap::new(),
            installed_plugins,
            last_vertex: None,
            active_tool: MarkupTool::Play,
            scoring_mode: false,
            comment_focus_handle: cx.focus_handle(),
            comment_draft: "".into(),
            benchmark: benchmark_result.summary().into(),
            large_game_benchmark: LargeGameBenchmark::professional_game(19, 19, 300)
                .run(200, 200)
                .summary()
                .into(),
            status: status.into(),
        }
    }

    /// Parses a GTP command line into `(name, arguments)`.
    fn parse_engine_command_line(draft: &str) -> (String, Vec<String>) {
        let mut tokens = draft.split_whitespace();
        let name = tokens.next().unwrap_or_default().to_owned();
        let arguments = tokens.map(ToOwned::to_owned).collect();
        (name, arguments)
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
        let (name, arguments) = Self::parse_engine_command_line(draft);
        let formatted = format_console_command(&name, &arguments);
        if let Some(session) = &mut self.engine_session {
            match session.send_command(&name, arguments) {
                Ok(response) => {
                    self.record_engine_log(entry_for_response(formatted.clone(), &response));
                    if response.success && name == "boardsize" {
                        if let Some(size) = draft
                            .split_whitespace()
                            .nth(1)
                            .and_then(|value| value.parse().ok())
                        {
                            self.board_size = size;
                        }
                    }
                    self.status = format!("engine: {formatted}").into();
                }
                Err(error) => {
                    self.record_engine_log(EngineLogEntry {
                        command: formatted.clone(),
                        success: false,
                        response: format!("protocol error: {error}"),
                    });
                    self.status = format!("engine failed: {error}").into();
                }
            }
        } else {
            let result = self.engine.send(&name, arguments);
            match result {
                Ok(response) => {
                    self.record_engine_log(entry_for_response(formatted.clone(), &response));
                    if response.success && name == "boardsize" {
                        if let Some(size) = draft
                            .split_whitespace()
                            .nth(1)
                            .and_then(|value| value.parse().ok())
                        {
                            self.board_size = size;
                        }
                    }
                    self.status = format!("engine: {formatted}").into();
                }
                Err(error) => {
                    self.record_engine_log(EngineLogEntry {
                        command: formatted.clone(),
                        success: false,
                        response: format!("protocol error: {error}"),
                    });
                    self.status = format!("engine failed: {error}").into();
                }
            }
        }
        self.engine_draft = "".into();
        cx.notify();
    }

    /// Starts a real engine session for the named engine: spawns the process,
    /// runs the host handshake/probe/startup/board-setup sequence, and replays
    /// the current position into the engine so it tracks the board.
    fn on_engine_connect(&mut self, name: &str, cx: &mut Context<Self>) {
        if self.engine_session.is_some() {
            self.status = "an engine is already connected".into();
            cx.notify();
            return;
        }
        let Some(record) = self
            .engine_store
            .list()
            .iter()
            .find(|record| record.name == name)
            .cloned()
        else {
            self.status = format!("engine {name} is not configured").into();
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
        let mut session = match sabaki_host::EngineSession::start(transport, &record, board_size) {
            Ok(session) => session,
            Err(error) => {
                self.status = format!("engine start failed: {error}").into();
                cx.notify();
                return;
            }
        };
        let replay_ok = self.host.snapshot().moves.iter().all(|move_dto| {
            let color = match move_dto.color {
                Color::Black => "B",
                Color::White => "W",
            };
            let vertex = match &move_dto.vertex {
                Some(vertex) => format_gtp_vertex(board_size, vertex.column, vertex.row),
                None => "pass".to_owned(),
            };
            session.play(color, &vertex).is_ok()
        });
        if !replay_ok {
            let _ = session.stop();
            self.status = "engine failed to replay the current position".into();
            cx.notify();
            return;
        }
        self.engine_session = Some(session);
        self.status = format!("engine {name} connected").into();
        cx.notify();
    }

    fn on_engine_disconnect(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(mut session) = self.engine_session.take() {
            let _ = session.stop();
            self.status = "engine disconnected".into();
        } else {
            self.status = "no engine connected".into();
        }
        self.analysis.clear();
        self.analysis_best_move = None;
        cx.notify();
    }

    /// Requests a position analysis from the connected engine (or the mock
    /// engine when none is connected), stores the entries and marks the best
    /// candidate on the board.
    fn on_analyze(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(task) = self.analysis_task.take() {
            task.detach();
        }
        self.analysis_stop_flag.store(false, Ordering::Relaxed);
        let generation = self.analysis_generation.fetch_add(1, Ordering::SeqCst) + 1;
        let generation_flag = self.analysis_generation.clone();
        let (command, command_arguments) = analysis_command_from_settings(&self.settings);

        // Bounded `analyze` responses go through the connected session.
        if command == "analyze" {
            if let Some(session) = &mut self.engine_session {
                match session.analyze(&command, vec!["".to_owned()]) {
                    Ok(entries) => self.set_analysis(entries, cx),
                    Err(error) => {
                        self.status = format!("analysis failed: {error}").into();
                        cx.notify();
                        return;
                    }
                }
                cx.notify();
                return;
            }
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
        if let Some(session) = self.engine_session.as_mut() {
            if let Err(error) = replay_position_session(session, board_size, &moves) {
                self.status = format!("analysis session setup failed: {error}").into();
                cx.notify();
                return;
            }
            match session.stream_analyze(&command, command_arguments.clone()) {
                Ok(()) => session_mode = true,
                Err(sabaki_domain_core::gtp::GtpError::UnsupportedStreaming) => {
                    // Connected engine cannot stream; fall through.
                }
                Err(error) => {
                    self.status = format!("analysis command failed: {error}").into();
                    cx.notify();
                    return;
                }
            }
        }

        let stop_flag = self.analysis_stop_flag.clone();
        let task_command = command.clone();

        if session_mode {
            let mut session = self
                .engine_session
                .take()
                .expect("session mode implies a connected session");
            self.status = format!("analysis: streaming {command} on connected session").into();
            self.analysis_task = Some(cx.spawn(
                move |shell_weak: gpui::WeakEntity<ShellApp>, cx: &mut gpui::AsyncApp| {
                    let mut cx = cx.clone();
                    async move {
                        let mut pending: Vec<sabaki_host::AnalysisEntry> = Vec::new();
                        let mut last_flush = Instant::now();
                        loop {
                            if stop_flag.load(Ordering::Relaxed)
                                || generation_flag.load(Ordering::SeqCst) != generation
                            {
                                if stop_flag.load(Ordering::Relaxed) {
                                    let _ = session.stop_analysis();
                                }
                                break;
                            }
                            match session.recv_analysis_line(Duration::from_millis(50)) {
                                Some(line) => {
                                    let line = line.trim();
                                    if line.is_empty() {
                                        break;
                                    }
                                    if let Some(entry) = parse_stream_line(&task_command, line) {
                                        pending.push(entry);
                                    }
                                    if task_command == "kata-analyze"
                                        && pending
                                            .last()
                                            .is_some_and(|entry| !entry.is_during_search)
                                    {
                                        break;
                                    }
                                }
                                None => {}
                            }
                            if last_flush.elapsed() >= Duration::from_millis(120)
                                && !pending.is_empty()
                            {
                                let batch = std::mem::take(&mut pending);
                                let _ = shell_weak.update(&mut cx, |shell, cx| {
                                    shell.push_analysis_batch(batch, cx)
                                });
                                last_flush = Instant::now();
                            }
                        }
                        if !pending.is_empty() {
                            let batch = std::mem::take(&mut pending);
                            let _ = shell_weak
                                .update(&mut cx, |shell, cx| shell.push_analysis_batch(batch, cx));
                        }
                        // Give the session back to the shell so later
                        // analysis and play requests reuse it.
                        let _ = shell_weak.update(&mut cx, |shell, cx| {
                            shell.engine_session = Some(session);
                            shell.analysis_finished(cx);
                        });
                    }
                },
            ));
            cx.notify();
            return;
        }

        let Some(record) = self.engine_store.list().first().cloned() else {
            self.status = "no engine configured for analysis".into();
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
        self.analysis_task = Some(cx.spawn(
            move |shell_weak: gpui::WeakEntity<ShellApp>, cx: &mut gpui::AsyncApp| {
                let mut cx = cx.clone();
                async move {
                    let mut pending: Vec<sabaki_host::AnalysisEntry> = Vec::new();
                    let mut last_flush = Instant::now();
                    loop {
                        if stop_flag.load(Ordering::Relaxed)
                            || generation_flag.load(Ordering::SeqCst) != generation
                        {
                            if stop_flag.load(Ordering::Relaxed) {
                                let _ = stream.send_command("stop");
                            }
                            break;
                        }
                        match stream.recv_line_timeout(Duration::from_millis(50)) {
                            Some(line) => {
                                let line = line.trim();
                                if line.is_empty() {
                                    break;
                                }
                                if let Some(entry) = parse_stream_line(&task_command, line) {
                                    pending.push(entry);
                                }
                                if task_command == "kata-analyze"
                                    && pending.last().is_some_and(|entry| !entry.is_during_search)
                                {
                                    break;
                                }
                            }
                            None => {}
                        }
                        if last_flush.elapsed() >= Duration::from_millis(120) && !pending.is_empty()
                        {
                            let batch = std::mem::take(&mut pending);
                            let _ = shell_weak
                                .update(&mut cx, |shell, cx| shell.push_analysis_batch(batch, cx));
                            last_flush = Instant::now();
                        }
                    }
                    if !pending.is_empty() {
                        let batch = std::mem::take(&mut pending);
                        let _ = shell_weak
                            .update(&mut cx, |shell, cx| shell.push_analysis_batch(batch, cx));
                    }
                    let _ = shell_weak.update(&mut cx, |shell, cx| shell.analysis_finished(cx));
                }
            },
        ));
        cx.notify();
    }

    /// Replaces the analysis set with a merged batch from the streaming task
    /// and refreshes the best-move marker.
    fn push_analysis_batch(
        &mut self,
        entries: Vec<sabaki_host::AnalysisEntry>,
        cx: &mut Context<Self>,
    ) {
        self.analysis = merge_analysis_entries(&self.analysis, entries);
        self.set_analysis(self.analysis.clone(), cx);
    }

    /// Stores an analysis set and refreshes the best-move marker and status.
    fn set_analysis(&mut self, entries: Vec<sabaki_host::AnalysisEntry>, cx: &mut Context<Self>) {
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

    /// Clears the running-analysis state once the streaming task ends.
    fn analysis_finished(&mut self, cx: &mut Context<Self>) {
        self.analysis_task = None;
        self.analysis_stop_flag.store(false, Ordering::Relaxed);
        self.status = "analysis finished".into();
        cx.notify();
    }

    /// Requests the streaming analysis task to stop and emit its final
    /// candidates.
    fn on_analysis_stop(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.analysis_task.is_some() {
            self.analysis_stop_flag.store(true, Ordering::Relaxed);
            self.status = "stopping analysis".into();
        } else {
            self.status = "no analysis running".into();
        }
        cx.notify();
    }

    /// Asks the connected engine for a move for the current player and plays
    /// it on the board through the host.
    fn on_engine_move(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = &mut self.engine_session else {
            self.status = "no engine connected".into();
            cx.notify();
            return;
        };
        let snapshot = self.host.snapshot();
        let color = snapshot.board.next_player;
        let color_str = match color {
            Color::Black => "B",
            Color::White => "W",
        };
        match session.generate_move(color_str) {
            Ok(response) => {
                self.record_engine_log(entry_for_response(
                    format!("genmove {color_str}"),
                    &response,
                ));
                if !response.success {
                    self.status = format!("engine genmove failed: {}", response.content).into();
                    cx.notify();
                    return;
                }
                let board_size = snapshot.board.width;
                let vertex = parse_gtp_vertex(board_size, response.content.trim())
                    .map(|(column, row)| Vertex { column, row });
                let mut events = RecordingSink::default();
                match self.host.play_move(color, vertex, &mut events) {
                    Ok(_) => {
                        self.last_vertex = vertex;
                        self.status = format!("engine played {}", response.content.trim()).into();
                        self.synchronize_recovery();
                    }
                    Err(error) => self.status = format!("engine move rejected: {error}").into(),
                }
            }
            Err(error) => self.status = format!("engine genmove failed: {error}").into(),
        }
        cx.notify();
    }

    /// Removes an engine from the configured list and persists the change
    /// through the settings store.
    fn on_engine_remove(&mut self, name: &str, cx: &mut Context<Self>) {
        if !self.engine_store.remove(name) {
            self.status = format!("engine {name} is not configured").into();
            cx.notify();
            return;
        }
        match self.engine_store.save(&mut self.settings) {
            Ok(()) => match sabaki_host::persist_settings_store(
                &self.settings,
                &mut self.settings_persistence,
            ) {
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

    /// Toggles a plugin between enabled and disabled through the host registry,
    /// then persists the registry and refreshes the rendered panel entries.
    fn on_plugin_toggle(&mut self, plugin_id: &str) {
        let currently_enabled = self
            .plugin_store
            .list()
            .iter()
            .find(|record| record.manifest.id == plugin_id)
            .map(|record| record.enabled)
            .unwrap_or(false);
        let result = if currently_enabled {
            self.plugin_store.disable(plugin_id)
        } else {
            self.plugin_store.enable(plugin_id)
        };
        match result {
            Ok(()) => {
                if currently_enabled {
                    if let Some(supervisor) = self.plugin_supervisors.get_mut(plugin_id) {
                        supervisor.stop();
                    }
                    self.plugin_supervisors.remove(plugin_id);
                } else {
                    self.start_plugin_supervisor(plugin_id);
                }
                self.persist_plugin_registry(
                    plugin_id,
                    if currently_enabled {
                        "disabled"
                    } else {
                        "enabled"
                    },
                );
            }
            Err(error) => self.status = format!("plugin toggle failed: {error}").into(),
        }
    }

    /// Grants the plugin's requested permissions and enables it, then
    /// persists the registry.
    fn on_plugin_grant(&mut self, plugin_id: &str, cx: &mut Context<Self>) {
        let permissions: Vec<_> = self
            .plugin_store
            .list()
            .iter()
            .find(|record| record.manifest.id == plugin_id)
            .map(|record| record.manifest.permissions.iter().cloned().collect())
            .unwrap_or_default();
        match self.plugin_store.grant_permissions(plugin_id, permissions) {
            Ok(()) => match self.plugin_store.enable(plugin_id) {
                Ok(()) => self.persist_plugin_registry(plugin_id, "granted and enabled"),
                Err(error) => self.status = format!("plugin enable failed: {error}").into(),
            },
            Err(error) => self.status = format!("permission grant failed: {error}").into(),
        }
        cx.notify();
    }

    /// Starts the supervised native process for a plugin when it is a
    /// native-runtime plugin, enabled and authorized; failures land in the
    /// status bar and leave the plugin unsupervised.
    fn start_plugin_supervisor(&mut self, plugin_id: &str) {
        let Some(record) = self
            .plugin_store
            .list()
            .iter()
            .find(|record| record.manifest.id == plugin_id)
        else {
            return;
        };
        if !matches!(
            record.manifest.runtime,
            sabaki_plugin_runtime::PluginRuntime::Native
        ) {
            return;
        }
        let mut supervisor = sabaki_host::PluginSupervisor::new(plugin_id);
        match supervisor.start(record) {
            Ok(()) => {
                self.plugin_supervisors
                    .insert(plugin_id.to_owned(), supervisor);
            }
            Err(error) => {
                self.status = format!("plugin process start failed: {error}").into();
            }
        }
    }

    /// Refreshes the supervised-process state shown in the plugin panel:
    /// polls for crashes and overlays the live info onto the entries.
    fn refresh_plugin_processes(&mut self) {
        for supervisor in self.plugin_supervisors.values_mut() {
            supervisor.poll();
        }
        for entry in &mut self.installed_plugins {
            if let Some(supervisor) = self.plugin_supervisors.get(&entry.plugin_id) {
                apply_process_info(entry, &supervisor.info());
            } else {
                entry.process_status = None;
                entry.process_logs.clear();
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
        match self.plugin_store.authorize_native(plugin_id) {
            Ok(()) => {
                let permissions: Vec<_> = self
                    .plugin_store
                    .list()
                    .iter()
                    .find(|record| record.manifest.id == plugin_id)
                    .map(|record| record.manifest.permissions.iter().cloned().collect())
                    .unwrap_or_default();
                if let Err(error) = self.plugin_store.grant_permissions(plugin_id, permissions) {
                    self.status = format!("permission grant failed: {error}").into();
                    cx.notify();
                    return;
                }
                match self.plugin_store.enable(plugin_id) {
                    Ok(()) => self.persist_plugin_registry(plugin_id, "authorized and enabled"),
                    Err(error) => self.status = format!("plugin enable failed: {error}").into(),
                }
            }
            Err(error) => self.status = format!("native authorization failed: {error}").into(),
        }
        cx.notify();
    }

    /// Dispatches a plugin command. WASM plugins are invoked in-process
    /// through the sandboxed runtime; declarative plugins have no execution
    /// body yet and are recorded in the status bar.
    /// Dispatches a command to a native plugin through its supervised
    /// JSON-RPC process. The supervisor is started on demand; a crashed
    /// process triggers one automatic restart attempt, and further failures
    /// surface the host's crash diagnostics (design §10.4: bounded restarts,
    /// auto-disable after the budget).
    fn dispatch_native_plugin_command(&mut self, plugin_id: &str, command_id: &str) {
        if !self.plugin_supervisors.contains_key(plugin_id) {
            self.start_plugin_supervisor(plugin_id);
        }
        let Some(supervisor) = self.plugin_supervisors.get_mut(plugin_id) else {
            self.status = format!("plugin {plugin_id} process unavailable").into();
            return;
        };
        let request = serde_json::json!({"command": command_id});
        match supervisor.request(command_id, request.clone()) {
            Ok(result) => {
                self.status = format!("plugin {plugin_id} command {command_id} → {result}").into();
            }
            Err(sabaki_plugin_runtime::PluginError::ProcessExited { .. }) => {
                // One automatic restart, then report the crash diagnostic.
                match supervisor.restart() {
                    Ok(()) => match supervisor.request(command_id, request) {
                        Ok(result) => {
                            self.status = format!(
                                "plugin {plugin_id} restarted; command {command_id} → {result}"
                            )
                            .into();
                        }
                        Err(error) => {
                            self.status =
                                format!("plugin {plugin_id} command failed: {error}").into();
                        }
                    },
                    Err(error) => {
                        self.status = format!("plugin {plugin_id} disabled: {error}").into();
                    }
                }
            }
            Err(error) => {
                self.status = format!("plugin {plugin_id} command failed: {error}").into();
            }
        }
    }

    fn on_plugin_command(&mut self, plugin_id: &str, command_id: &str, cx: &mut Context<Self>) {
        let Some(record) = self
            .plugin_store
            .list()
            .iter()
            .find(|record| record.manifest.id == plugin_id)
            .cloned()
        else {
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
                        let mut events = RecordingSink::default();
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

    /// Persists the plugin registry and refreshes the rendered panel entries.
    fn persist_plugin_registry(&mut self, plugin_id: &str, action: &str) {
        match self.plugin_store.persist(&self.plugin_persistence) {
            Ok(()) => self.status = format!("plugin {plugin_id} {action}").into(),
            Err(error) => self.status = format!("plugin not persisted: {error}").into(),
        }
        self.installed_plugins = self
            .plugin_store
            .list()
            .iter()
            .map(entry_from_record)
            .collect();
        self.refresh_plugin_processes();
    }

    /// Resets the board to the given size as a fresh game using the persisted
    /// new-game defaults for komi and handicap.
    fn on_board_size_selected(&mut self, size: usize, cx: &mut Context<Self>) {
        self.new_game_at(size, cx);
    }

    /// Stops and drops the connected engine session, if any.
    fn disconnect_engine_session(&mut self) {
        if let Some(mut session) = self.engine_session.take() {
            let _ = session.stop();
        }
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
        let mut events = RecordingSink::default();
        match self.host.restore_from_sgf(&candidate.sgf, &mut events) {
            Ok(_) => {
                self.status = "recovery restored".into();
                self.last_vertex = None;
                self.external_file.detach_file();
                self.disconnect_engine_session();
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
        let mut events = RecordingSink::default();
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
        self.disconnect_engine_session();
        cx.notify();
    }

    fn on_pass(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let color = self.host.snapshot().board.next_player;
        let mut events = RecordingSink::default();
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
                self.sync_engine_position(color, None);
            }
            Err(error) => self.status = format!("pass rejected: {error}").into(),
        }
        cx.notify();
    }

    /// Starts a splitter drag from a divider's mouse-down position.
    fn begin_split_drag(&mut self, pane: SplitPane, position: f32, cx: &mut Context<Self>) {
        let start_size = match pane {
            SplitPane::Left => self.left_sidebar_width,
            SplitPane::Right => self.right_sidebar_width,
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
        let (fallback_min, min_width_key) = match drag.pane {
            SplitPane::Left => (100.0, "view.leftsidebar_minwidth"),
            SplitPane::Right => (100.0, "view.sidebar_minwidth"),
        };
        let min_size = self
            .settings
            .get(min_width_key)
            .and_then(serde_json::Value::as_f64)
            .map(|value| value as f32)
            .unwrap_or(fallback_min);
        let max_size = (f32::from(window.viewport_size().width) - 320.0).max(min_size);
        let next_size = clamp_pane_size(
            pane_size_for_drag(drag.start_size, drag.start_position, position, drag.pane),
            min_size,
            max_size,
        );
        match drag.pane {
            SplitPane::Left => self.left_sidebar_width = next_size,
            SplitPane::Right => self.right_sidebar_width = next_size,
        }
        cx.notify();
    }

    /// Finishes a splitter drag and persists the final pane width.
    fn finish_split_drag(&mut self, cx: &mut Context<Self>) {
        let Some(drag) = self.split_drag.take() else {
            return;
        };
        let (width_key, width) = match drag.pane {
            SplitPane::Left => ("view.leftsidebar_width", self.left_sidebar_width),
            SplitPane::Right => ("view.sidebar_width", self.right_sidebar_width),
        };
        if let Err(error) = self.settings.set(width_key, serde_json::json!(width)) {
            self.status = format!("pane width not saved: {error}").into();
        } else if let Err(error) =
            sabaki_host::persist_settings_store(&self.settings, &mut self.settings_persistence)
        {
            self.status = format!("pane width not persisted: {error}").into();
        }
        cx.notify();
    }

    fn on_split_drag_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.update_split_drag(f32::from(event.position.x), window, cx);
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
        let current = self.settings.get_bool(key).unwrap_or(false);
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
        // The right pane follows Sabaki's inferred visibility
        // (`show_graph || show_comments`). The toolbar toggle flips both
        // switches so the button always means "show/hide this pane".
        let show_graph = self.settings.get_bool("view.show_graph").unwrap_or(false);
        let show_comments = self
            .settings
            .get_bool("view.show_comments")
            .unwrap_or(false);
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
        let mut events = RecordingSink::default();
        match self.host.open(path.clone(), &self.file_access, &mut events) {
            Ok(_) => {
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
        let mut events = RecordingSink::default();
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
        let mut events = RecordingSink::default();
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
        let mut events = RecordingSink::default();
        match self.host.open(path.clone(), &self.file_access, &mut events) {
            Ok(_) => {
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
        let mut events = RecordingSink::default();
        match self.host.undo(&mut events) {
            Ok(_) => self.status = "undo".into(),
            Err(error) => self.status = format!("undo failed: {error}").into(),
        }
        cx.notify();
    }

    fn redo(&mut self, cx: &mut Context<Self>) {
        let mut events = RecordingSink::default();
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
        let mut events = RecordingSink::default();
        match self.host.apply_transaction(transaction, &mut events) {
            Ok(_) => self.status = format!("moved to {target}").into(),
            Err(error) => self.status = format!("navigation failed: {error}").into(),
        }
        cx.notify();
    }

    /// Board interaction entry point. The goban's per-vertex hit layer maps
    /// the click to a vertex before calling this, so the handler no longer
    /// depends on the board's window-global origin or the surrounding layout.
    fn on_board_vertex_clicked(&mut self, vertex: Vertex, cx: &mut Context<Self>) {
        if self.scoring_mode {
            self.scoring_at(vertex, cx);
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
        let mut events = RecordingSink::default();
        match self.host.play_move(color, Some(vertex), &mut events) {
            Ok(_) => {
                self.last_vertex = Some(vertex);
                self.status = format!("move at {},{}", vertex.column, vertex.row).into();
                self.synchronize_recovery();
                self.sync_engine_position(color, Some(vertex));
            }
            Err(error) => self.status = format!("move rejected: {error}").into(),
        }
        cx.notify();
    }

    /// Sends the just-played move to the connected engine so its position
    /// stays in sync with the board. Sync failures surface in the status bar
    /// but never undo the local move.
    fn sync_engine_position(&mut self, color: Color, vertex: Option<Vertex>) {
        let Some(session) = &mut self.engine_session else {
            return;
        };
        let color_str = match color {
            Color::Black => "B",
            Color::White => "W",
        };
        let vertex_str = match vertex {
            Some(vertex) => format_gtp_vertex(session.board_size(), vertex.column, vertex.row),
            None => "pass".to_owned(),
        };
        if let Err(error) = session.play(color_str, &vertex_str) {
            self.status = format!("engine sync failed: {error}").into();
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
        let mut events = RecordingSink::default();
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
        let mut events = RecordingSink::default();
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
        let mut events = RecordingSink::default();
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

    /// Toggles the scoring mode: while active, board clicks cycle scoring
    /// overrides instead of placing moves.
    fn on_scoring_mode_toggle(
        &mut self,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.scoring_mode = !self.scoring_mode;
        self.status = if self.scoring_mode {
            "scoring mode: click stones to cycle dead/alive".into()
        } else {
            "scoring mode off".into()
        };
        cx.notify();
    }

    fn on_tool_selected(&mut self, tool: MarkupTool, cx: &mut Context<Self>) {
        self.active_tool = tool;
        self.status = format!("tool: {}", tool.label()).into();
        cx.notify();
    }

    fn on_comment_focus(&mut self, _: &MouseDownEvent, window: &mut Window, _: &mut Context<Self>) {
        window.focus(&self.comment_focus_handle);
        let metadata = current_node_metadata(&self.host.snapshot());
        self.comment_draft = metadata.comment.into();
    }

    fn on_comment_key_down(
        &mut self,
        event: &gpui::KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut draft = self.comment_draft.to_string();
        match event.keystroke.key.as_str() {
            "backspace" => {
                draft.pop();
            }
            "enter" => {
                self.save_comment(&draft, cx);
                return;
            }
            "escape" => {
                let metadata = current_node_metadata(&self.host.snapshot());
                self.comment_draft = metadata.comment.into();
                cx.notify();
                return;
            }
            _ => {
                if let Some(key_char) = event.keystroke.key_char.as_ref() {
                    draft.push_str(key_char);
                }
            }
        }
        self.comment_draft = draft.into();
        cx.notify();
    }

    fn save_comment(&mut self, comment: &str, cx: &mut Context<Self>) {
        let metadata = current_node_metadata(&self.host.snapshot());
        let transaction = create_comment_transaction(&metadata.node_id, comment);
        let mut events = RecordingSink::default();
        match self.host.apply_transaction(transaction, &mut events) {
            Ok(_) => {
                self.status = "comment saved".into();
                self.comment_draft = "".into();
                self.synchronize_recovery();
            }
            Err(error) => self.status = format!("comment failed: {error}").into(),
        }
        cx.notify();
    }

    fn on_variation_promote(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let metadata = current_node_metadata(&self.host.snapshot());
        let transaction = create_variation_transaction(&metadata.node_id, VariationAction::Promote);
        let mut events = RecordingSink::default();
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
        let mut events = RecordingSink::default();
        match self.host.apply_transaction(transaction, &mut events) {
            Ok(_) => {
                self.status = "variation removed".into();
                self.synchronize_recovery();
            }
            Err(error) => self.status = format!("remove failed: {error}").into(),
        }
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let snapshot = self.host.snapshot();
        let theme_color = self.theme.background_color().rgb_u32();
        let status = match self.last_vertex {
            Some(Vertex { column, row }) => format!("last move: {column},{row}"),
            None => "click the board or use the File menu".to_owned(),
        };
        let file_state = &snapshot.file_state;
        let dirty_label = if file_state.is_dirty {
            "modified"
        } else {
            "saved"
        };
        let path_label = file_state.path.as_deref().unwrap_or("no source file");
        let availability = navigation_availability(&snapshot);
        let position = position_label(&snapshot);
        let variation_layout = build_variation_tree_layout(&snapshot);
        let inspector_metadata = current_node_metadata(&snapshot);
        let settings_rows = panel_setting_rows(&self.settings);
        let external_status = self.external_file.status();
        let external_conflict = matches!(
            external_status.status,
            sabaki_host::ExternalFileStatus::Changed
                | sabaki_host::ExternalFileStatus::Missing
                | sabaki_host::ExternalFileStatus::Unreadable
        );
        let show_left_sidebar = self
            .settings
            .get_bool("view.show_leftsidebar")
            .unwrap_or(false);
        let show_graph = self.settings.get_bool("view.show_graph").unwrap_or(false);
        let show_comments = self
            .settings
            .get_bool("view.show_comments")
            .unwrap_or(false);
        let show_right_sidebar = right_pane_visible(show_graph, show_comments);
        let palette = self.palette;
        let weak_shell = cx.entity().downgrade();
        let on_node_clicked =
            move |node_id: &sabaki_domain_core::NodeId, _window: &mut Window, cx: &mut App| {
                weak_shell
                    .update(cx, |shell, cx| shell.navigate_to_node(node_id.clone(), cx))
                    .ok();
            };

        let root = div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(theme_color))
            .text_color(rgb(palette.text))
            .child(panels::render_header(&snapshot, &status, palette))
            .child(
                div()
                    .id("workspace")
                    .debug_selector(|| "workspace".to_owned())
                    .flex_1()
                    .flex()
                    .min_h_0()
                    .p_4()
                    .child(if show_left_sidebar {
                        div()
                            .id("left-sidebar")
                            .debug_selector(|| "left-sidebar".to_owned())
                            .flex_none()
                            .w(px(self.left_sidebar_width))
                            .h_full()
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .gap_3()
                            .pr_1()
                            .child(panels::render_engine_panel(self, cx))
                    } else {
                        div().id("left-sidebar-hidden")
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
                            .gap_3()
                            .child(panels::render_goban_area(
                                &snapshot,
                                &self.theme,
                                self.analysis_best_move,
                                self,
                                cx,
                            ))
                            .child(panels::render_toolbar_row(
                                self.active_tool,
                                availability,
                                &position,
                                show_left_sidebar,
                                show_right_sidebar,
                                palette,
                                cx,
                            ))
                            .child(panels::render_recovery_buttons(self, cx))
                            .child(panels::render_external_conflict_buttons(
                                external_conflict,
                                palette,
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
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .gap_3()
                            .pr_1()
                            .child(panels::render_plugins_panel(self, cx))
                            .child(if show_graph {
                                panels::render_variation_tree_panel(
                                    &variation_layout,
                                    palette,
                                    on_node_clicked,
                                )
                            } else {
                                div().id("variation-tree-panel-hidden")
                            })
                            .child(panels::render_node_inspector_panel(
                                &inspector_metadata,
                                self,
                                cx,
                            ))
                            .child(panels::render_settings_panel(&settings_rows, self, cx))
                    } else {
                        div().id("right-sidebar-hidden")
                    }),
            )
            .child(panels::render_status_bar(
                self,
                &status,
                dirty_label,
                path_label,
                &external_status,
            ))
            .child(if self.split_drag.is_some() {
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .cursor_col_resize()
                    .on_mouse_move(cx.listener(ShellApp::on_split_drag_mouse_move))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(ShellApp::on_split_drag_mouse_up),
                    )
            } else {
                div()
            });

        root
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
    let button_style =
        |label: &str,
         enabled: bool,
         on_click: Box<dyn Fn(&MouseDownEvent, &mut Window, &mut App)>| {
            div()
                .px_2()
                .py_1()
                .border_1()
                .border_color(rgb(palette.accent))
                .rounded(px(4.0))
                .bg(if enabled {
                    rgb(palette.button)
                } else {
                    rgb(palette.button_active)
                })
                .text_color(if enabled {
                    rgb(palette.text)
                } else {
                    rgb(palette.subtle)
                })
                .child(label.to_owned())
                .on_mouse_down(MouseButton::Left, on_click)
        };

    div()
        .flex()
        .items_center()
        .gap_2()
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
                .text_sm()
                .text_color(rgb(palette.muted))
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
            name: "Sabaki".into(),
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
            name: "Navigate".into(),
            items: vec![
                MenuItem::action("First Node", GoToFirstNode),
                MenuItem::action("Previous Node", GoToPreviousNode),
                MenuItem::action("Next Node", GoToNextNode),
                MenuItem::action("Last Node", GoToLastNode),
            ],
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
        schedule_external_check(window, cx, window_handle.clone(), shell, last_check);
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
        let external_check_window = window.clone();
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
            KeyBinding::new("cmd-z", UndoMove, None),
            KeyBinding::new("cmd-shift-z", RedoMove, None),
            KeyBinding::new("cmd-left", GoToFirstNode, None),
            KeyBinding::new("left", GoToPreviousNode, None),
            KeyBinding::new("right", GoToNextNode, None),
            KeyBinding::new("cmd-right", GoToLastNode, None),
            KeyBinding::new("cmd-q", Quit, None),
        ]);
        cx.set_menus(shell_menus());
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
    use sabaki_domain_core::{Color, Vertex};
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

            let mut events = RecordingSink::default();
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
            shell.scoring_mode = true;
            let vertex = Vertex { column: 3, row: 3 };
            let current = shell.host.snapshot().score_overrides.get(&vertex).copied();
            let transaction = crate::markup::create_scoring_transaction(
                vertex,
                crate::markup::next_scoring_override(current),
            );
            let mut events = RecordingSink::default();
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
    fn headless_shell_reports_analysis_command_from_settings() {
        with_headless_shell("analysis-cmd", |shell| {
            let (command, arguments) =
                crate::engine_console::analysis_command_from_settings(&shell.settings);
            assert_eq!(command, "lz-analyze");
            assert!(arguments.is_empty());
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

    /// Renders the full window with the gpui test platform and simulates a
    /// real left click on a rendered intersection. This catches the original
    /// Beta frontend regression: the goban hitbox was zero-sized and the
    /// board visuals were offset by half the board margin.
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
            .set("view.leftsidebar_width", serde_json::json!(250.0))
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
        assert_eq!(goban_bounds.size.width, px(BOARD_PIXEL_SIZE));
        assert_eq!(goban_bounds.size.height, px(BOARD_PIXEL_SIZE));
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
        assert_eq!(wood_bounds.size.width, px(BOARD_PIXEL_SIZE - 28.0));

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
                "plugins-panel",
                vcx.debug_bounds("plugins-panel")
                    .expect("plugins panel must have a debug selector"),
            ),
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
            (
                "settings-panel",
                vcx.debug_bounds("settings-panel")
                    .expect("settings panel must have a debug selector"),
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
        let engine_bounds = vcx
            .debug_bounds("engine-panel")
            .expect("engine panel must have a debug selector");
        assert!(
            f32::from(engine_bounds.origin.x) >= f32::from(left_sidebar.origin.x) - 0.5
                && f32::from(engine_bounds.right()) <= f32::from(left_sidebar.right()) + 0.5,
            "engine panel {:?} must live in the left sidebar {:?}",
            engine_bounds,
            left_sidebar
        );
        for pair in panels.windows(2) {
            assert!(
                f32::from(pair[0].1.bottom()) <= f32::from(pair[1].1.origin.y) + 0.5,
                "{} {:?} must be stacked above {} {:?}",
                pair[0].0,
                pair[0].1,
                pair[1].0,
                pair[1].1
            );
        }

        let board = window_handle
            .read_with(&vcx.cx, |shell, _| shell.host.snapshot().board.clone())
            .unwrap();
        let spacing = crate::goban_view::board_spacing(&board, BOARD_PIXEL_SIZE);
        let click = gpui::point(
            px(f32::from(goban_bounds.origin.x) + 28.0 + 16.0 * spacing),
            px(f32::from(goban_bounds.origin.y) + 28.0 + 16.0 * spacing),
        );
        let before = window_handle
            .read_with(&vcx.cx, |shell, _| shell.host.snapshot().moves.len())
            .unwrap();
        vcx.simulate_click(click, gpui::Modifiers::none());
        let after = window_handle
            .read_with(&vcx.cx, |shell, _| {
                (
                    shell.host.snapshot().moves.len(),
                    shell.host.snapshot().board.sign_map[16][16],
                )
            })
            .unwrap();
        assert_eq!(
            (after.0, after.1),
            (before + 1, 1),
            "clicking the rendered 17th intersection must place the next black stone"
        );
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
        let _ = std::fs::remove_dir_all(&config);
    }
}
