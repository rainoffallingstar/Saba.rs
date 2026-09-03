pub mod analysis;
pub mod analysis_controller;
pub mod autosave;
pub mod close_flow;
pub mod engine_controller;
pub mod engine_session;
pub mod engine_workflow;
pub mod external_file;
pub mod file_codec;
pub mod fox_kifu;
pub mod gif_exporter;
pub mod katago_setup;
pub mod legacy_styles;
pub mod move_grading;
pub mod ogs;
pub mod ogs_auth;
pub mod ogs_client;
pub mod ogs_credentials;
pub mod ogs_rest;
pub mod ogs_socket;
pub mod persistence;
pub mod plugin_commands;
pub mod plugin_controller;
pub mod plugin_supervisor;
pub mod plugin_wasm;
pub mod plugin_workflow;
pub mod position_exporter;
pub mod recent_files;
pub mod rules_sync;
pub mod settings;
pub mod sgf_library;
pub mod starriver_capture;
pub mod territory_estimator;
pub mod theme_workflow;
pub mod whole_game_review;
pub mod workspace_tabs;

use std::path::PathBuf;

use ryusei_domain_core::{Color, GameDocument, GameSnapshot, GameTransaction, Properties, Vertex};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use analysis::{
    AnalysisCommandSink, AnalysisEntry, parse_analysis_response, parse_kata_analysis_line,
    parse_lz_analysis_entries, parse_lz_analysis_line, replay_position_stream,
    replay_position_stream_commands,
};
pub use analysis_controller::{AnalysisRunController, AnalysisRunOutcome, AnalysisRunTicket};
pub use autosave::{AutosaveCandidate, AutosaveInfo, AutosaveStore};
pub use close_flow::{CloseRequestAction, decide_close_request};
pub use engine_controller::{EngineController, EngineControllerError};
pub use engine_session::{
    EngineCommandTimeouts, EngineSession, EngineSessionError, EngineSessionState, GtpTransport,
    ProcessGtpTransport,
};
pub use engine_workflow::{
    EngineRecord, EngineStore, engine_list_from_value, engine_list_to_value,
    parse_engine_arguments, validate_engine_list_value, validate_engine_record,
};
pub use external_file::{
    ExternalFileDecision, ExternalFileObservation, ExternalFileReadError, ExternalFileReader,
    ExternalFileStatus, ExternalFileStatusDto, ExternalFileStore,
};
pub use file_codec::{
    FileCodecError, decode_legacy_bytes, decode_sgf_bytes, detect_sgf_encoding, encode_sgf,
};
pub use fox_kifu::{
    CurlFoxHttpAdapter, FOX_CGI_FETCH_CHESS_URL, FOX_CHESS_LIST_URL, FOX_FETCH_CHESS_URL,
    FOX_QUERY_USER_URL, FoxGameSummary, FoxHttpAdapter, FoxKifuClient, FoxUserSummary,
    build_fetch_chess_list_url, build_fetch_chess_url, build_query_user_url, fetch_game_sgf,
    fetch_user_recent_games, parse_fox_chess_list_response, parse_fox_sgf_response,
    parse_query_user_response, sanitize_fox_sgf,
};
pub use gif_exporter::{GifExportOptions, export_sgf_to_gif};
pub use katago_setup::{
    CurlKataGoModelDownloadAdapter, HardwareBackend, KATAGO_HUMAN_SL_CONFIG_NAME,
    KATAGO_LATEST_WEIGHT_DISPLAY_LIMIT, KATAGO_OFFICIAL_EXTRA_WEIGHTS_PAGE,
    KATAGO_OFFICIAL_RELEASE_BASE, KATAGO_OFFICIAL_WEIGHTS_PAGE, KataGoEnvironment, KataGoLocalInfo,
    KataGoModelDownloadAdapter, KataGoModelInstallError, KataGoModelTier, KataGoReleaseAsset,
    KataGoReleaseInfo, KataGoWeightInfo, MODEL_BALANCED_NAME, MODEL_BALANCED_URL,
    MODEL_LIGHTWEIGHT_NAME, MODEL_LIGHTWEIGHT_URL, MODEL_STRONGEST_NAME, MODEL_STRONGEST_URL,
    build_katago_engine_record, build_katago_human_sl_engine_record, download_katago_weight,
    ensure_katago_environment, fetch_katago_human_sl_weights, fetch_katago_latest_release,
    fetch_katago_official_weights, find_installed_human_sl_model,
    find_installed_normal_katago_model, find_katago_executable,
    find_latest_installed_normal_katago_model, generate_human_sl_gtp_config,
    generate_optimized_gtp_config, human_sl_profiles, inspect_katago_local, install_katago_model,
    install_katago_model_with, install_latest_katago_weight, is_human_sl_weight_name,
    is_valid_human_sl_profile, katago_storage_dir, latest_katago_weight_names,
    merge_katago_weight_catalog, merge_katago_weight_catalog_with_limit, parse_katago_release_json,
    parse_katago_weight_html, prepare_katago_human_sl_engine, repair_katago_engine_record,
    select_katago_binary_asset, set_active_katago_model, update_katago_binary,
    upgrade_katago_via_brew, validate_katago_engine_record,
};
pub use legacy_styles::{LegacyStylesReport, MigratedColorRule, analyze_legacy_styles};
pub use move_grading::{
    GameAnalyticsSummary, MoveEvaluation, MoveQuality, compute_game_move_evaluations,
};
pub use ogs::{
    CurlOgsPublicGameFetch, OGS_GAME_API_ROOT, OgsCompetitionSession, OgsError, OgsGameUpdate,
    OgsMoveSubmission, OgsPublicGameFetch, OgsPublicGameState, OgsServerClock, OgsTransport,
    ogs_game_id_from_public_url, ogs_game_id_from_url, ogs_public_game_api_url,
    parse_ogs_public_game,
};
pub use ogs_auth::OgsAuthState;
pub use ogs_client::{
    LiveOgsClient, OgsChatLine, OgsClientSnapshot, OgsMatchmakingStatus, OgsOnlineGame,
    OgsSocketStatus,
};
pub use ogs_credentials::{
    KeyringOgsCredentialStore, MemoryOgsCredentialStore, OGS_KEYCHAIN_SERVICE, OgsCredentialStore,
    OgsCredentials,
};
pub use ogs_rest::{
    OGS_SERVER_URL, OGS_USER_AGENT, OgsHttpResponse, OgsLoginResult, OgsRestFetch,
    UreqOgsRestFetch, extract_csrf_token, login_via_rest, normalize_cookie_header,
    parse_ogs_login_response,
};
pub use ogs_socket::{
    OGS_SOCKET_URL, OgsIncoming, OgsWebSocketTransport, TungsteniteOgsWebSocketTransport,
    build_authenticate_payload, decode_incoming, encode_event, encode_request,
};
pub use persistence::{HostPersistence, record_recent_file, synchronize_autosave};
pub use plugin_commands::{BuiltinPluginCommand, BuiltinPluginCommandRegistry};
pub use plugin_controller::{PluginController, PluginControllerOutcome};
pub use plugin_supervisor::{
    AUTO_DISABLE_AFTER_CRASHES, DEFAULT_REQUEST_TIMEOUT, PluginProcessInfo, PluginProcessStatus,
    PluginSupervisor, plugin_storage_root,
};
pub use plugin_wasm::{
    WASM_ENTRYPOINT_EXTENSION, WasmWorkflowError, invoke_wasm_command, load_wasm_module,
};
pub use plugin_workflow::{
    PersistedPluginState, PluginPersistence, PluginStore, install_plugin_from_zip_file,
    scan_plugin_installations,
};
pub use position_exporter::{PositionPngOptions, export_position_to_png};
pub use recent_files::{RecentFileDto, RecentFilesStore};
pub use rules_sync::{GameRuleConfig, GoRuleset};
pub use settings::{
    LoadedSettings, SettingKind, SettingValidationError, SettingsPersistence, SettingsStore,
    SettingsValidation, is_legacy_overwrite_marker, load_settings_store, persist_settings_store,
    setting_kind, validate_setting_value, validate_settings,
};
pub use sgf_library::{
    ProcessGitSyncAdapter, RedistributionRights, SgfGitSyncAdapter, SgfLibraryEntry,
    SgfLibraryError, SgfLibrarySource, SgfLibrarySyncOperation, SgfLibrarySyncReport,
    scan_sgf_library, sync_sgf_library,
};
pub use starriver_capture::{
    CurlPublicPageFetch, PublicPageFetch, StarRiverCapture, StarRiverCaptureError,
    capture_public_live_sgf, extract_sgf_collection, validate_public_https_url,
};
pub use territory_estimator::{TerritoryEstimate, estimate_territory};
pub use theme_workflow::{
    ALLOWED_THEME_ASSET_EXTENSIONS, InstalledTheme, MIN_THEME_TOKEN_SCHEMA_VERSION,
    ShellThemeTokens, THEME_SCHEMA_VERSION, THEME_TOKEN_SCHEMA_VERSION, ThemeColor, ThemeError,
    ThemeManifest, ThemeScan, ThemeTokens, install_theme, is_safe_relative_path, is_valid_theme_id,
    parse_hex_color, scan_theme_root, uninstall_theme,
};
pub use whole_game_review::{
    BatchReviewProgress, BlunderEntry, BlunderGrade, LineageMove, ReviewedPosition,
    active_lineage_moves, active_lineage_review_nodes, find_blunders,
};
pub use workspace_tabs::{WorkspaceTab, WorkspaceTabError, WorkspaceTabs};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecodedGameFile {
    pub content: String,
    pub encoding: SourceEncoding,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceEncoding {
    Utf8,
    ShiftJis,
    EucJp,
    Gbk,
    Big5,
}

pub trait GameFileAccess {
    fn read_game_file(&self, path: &std::path::Path) -> Result<DecodedGameFile, HostError>;

    fn write_game_file(
        &mut self,
        path: &std::path::Path,
        content: &str,
        encoding: SourceEncoding,
    ) -> Result<(), HostError>;
}

pub trait HostEventSink {
    fn emit(&mut self, event: HostEvent);
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum HostEvent {
    GameChanged { snapshot: Box<GameSnapshot> },
    AutosaveChanged { info: AutosaveInfo },
    ExternalFileStatusChanged { status: ExternalFileStatusDto },
}

#[derive(Clone, Debug)]
pub struct HostApplication {
    game: GameDocument,
    source_encoding: SourceEncoding,
}

#[derive(Debug, Error)]
pub enum HostError {
    #[error("the current game has no save location")]
    NoSaveLocation,
    #[error("could not read the game file: {0}")]
    FileRead(String),
    #[error("could not write the game file: {0}")]
    FileWrite(String),
    #[error(transparent)]
    Domain(#[from] ryusei_domain_core::DomainError),
}

impl Default for HostApplication {
    fn default() -> Self {
        Self::new(19, 19).expect("the standard board size is valid")
    }
}

impl HostApplication {
    pub fn new(width: usize, height: usize) -> Result<Self, HostError> {
        Ok(Self {
            game: GameDocument::new(width, height)?,
            source_encoding: SourceEncoding::Utf8,
        })
    }

    pub fn snapshot(&self) -> GameSnapshot {
        self.game.snapshot()
    }

    /// Replaces a root SGF property without creating a move-tree node. This is
    /// used for game-level metadata such as rules, time controls, and results.
    pub fn set_root_property(&mut self, property: &str, values: Vec<String>) -> GameSnapshot {
        self.game.set_root_property(property, values);
        self.game.snapshot()
    }

    pub fn source_encoding(&self) -> SourceEncoding {
        self.source_encoding
    }

    pub fn create_new(
        &mut self,
        width: usize,
        height: usize,
        events: &mut impl HostEventSink,
    ) -> Result<GameSnapshot, HostError> {
        self.game = GameDocument::new(width, height)?;
        self.source_encoding = SourceEncoding::Utf8;
        Ok(self.emit_snapshot(events))
    }

    /// Creates a fresh document and applies root properties while it is still
    /// being constructed, so new-game defaults (komi, handicap, setup stones)
    /// are part of the clean initial document rather than dirty edits.
    pub fn create_new_with_properties(
        &mut self,
        width: usize,
        height: usize,
        root_properties: &Properties,
        events: &mut impl HostEventSink,
    ) -> Result<GameSnapshot, HostError> {
        let mut game = GameDocument::new(width, height)?;
        for (property, values) in root_properties {
            game.set_root_property(property, values.clone());
        }
        self.game = game;
        self.source_encoding = SourceEncoding::Utf8;
        Ok(self.emit_snapshot(events))
    }

    pub fn open(
        &mut self,
        path: PathBuf,
        file_access: &impl GameFileAccess,
        events: &mut impl HostEventSink,
    ) -> Result<GameSnapshot, HostError> {
        let decoded_file = file_access.read_game_file(&path)?;
        let (content, encoding) = match ryusei_domain_core::legacy::file_extension(&path) {
            Some(extension) if matches!(extension.as_str(), "ngf" | "gib" | "ugf") => (
                ryusei_domain_core::legacy::import_by_extension(&extension, &decoded_file.content)
                    .map_err(|error| HostError::FileRead(error.to_string()))?,
                // Imported legacy files are normalized to UTF-8 SGF text.
                SourceEncoding::Utf8,
            ),
            _ => (decoded_file.content, decoded_file.encoding),
        };
        self.open_decoded(path, content, encoding, events)
    }

    pub fn open_decoded(
        &mut self,
        path: PathBuf,
        content: String,
        encoding: SourceEncoding,
        events: &mut impl HostEventSink,
    ) -> Result<GameSnapshot, HostError> {
        let mut parsed_game = GameDocument::from_sgf(&content)?;
        parsed_game.set_source_path(Some(path.to_string_lossy().into_owned()));

        self.game = parsed_game;
        self.source_encoding = encoding;
        Ok(self.emit_snapshot(events))
    }

    pub fn save(
        &mut self,
        file_access: &mut impl GameFileAccess,
        events: &mut impl HostEventSink,
    ) -> Result<GameSnapshot, HostError> {
        let source_path = self
            .game
            .snapshot()
            .file_state
            .path
            .ok_or(HostError::NoSaveLocation)?;
        self.save_at(PathBuf::from(source_path), file_access, events)
    }

    pub fn save_at(
        &mut self,
        path: PathBuf,
        file_access: &mut impl GameFileAccess,
        events: &mut impl HostEventSink,
    ) -> Result<GameSnapshot, HostError> {
        let serialized_game = self.game.to_sgf();
        file_access.write_game_file(&path, &serialized_game, self.source_encoding)?;
        self.game
            .set_source_path(Some(path.to_string_lossy().into_owned()));
        Ok(self.emit_snapshot(events))
    }

    pub fn play_move(
        &mut self,
        color: Color,
        vertex: Option<Vertex>,
        events: &mut impl HostEventSink,
    ) -> Result<GameSnapshot, HostError> {
        self.game.play_move(color, vertex)?;
        Ok(self.emit_snapshot(events))
    }

    pub fn apply_transaction(
        &mut self,
        transaction: GameTransaction,
        events: &mut impl HostEventSink,
    ) -> Result<GameSnapshot, HostError> {
        self.game.apply_transaction(transaction)?;
        Ok(self.emit_snapshot(events))
    }

    pub fn undo(&mut self, events: &mut impl HostEventSink) -> Result<GameSnapshot, HostError> {
        self.game.undo();
        Ok(self.emit_snapshot(events))
    }

    pub fn redo(&mut self, events: &mut impl HostEventSink) -> Result<GameSnapshot, HostError> {
        self.game.redo();
        Ok(self.emit_snapshot(events))
    }

    pub fn discard_source_location(
        &mut self,
        events: &mut impl HostEventSink,
    ) -> Result<GameSnapshot, HostError> {
        self.game.set_source_path(None);
        self.game.mark_dirty();
        self.source_encoding = SourceEncoding::Utf8;
        Ok(self.emit_snapshot(events))
    }

    /// Replaces the document from a persisted workspace-tab snapshot without
    /// turning a clean source-backed document into recovery-dirty state.
    pub fn restore_workspace_tab(
        &mut self,
        content: &str,
        source_path: Option<String>,
        source_encoding: SourceEncoding,
        events: &mut impl HostEventSink,
    ) -> Result<GameSnapshot, HostError> {
        self.restore_workspace_tab_with_dirty(content, source_path, source_encoding, false, events)
    }

    /// Restores a persisted workspace snapshot while preserving whether the
    /// snapshot represented unsaved edits. A source path establishes the
    /// external-file baseline; `is_dirty` then re-applies the pending edit
    /// marker without pretending that the snapshot was clean.
    pub fn restore_workspace_tab_with_dirty(
        &mut self,
        content: &str,
        source_path: Option<String>,
        source_encoding: SourceEncoding,
        is_dirty: bool,
        events: &mut impl HostEventSink,
    ) -> Result<GameSnapshot, HostError> {
        self.restore_workspace_tab_with_state(
            content,
            source_path,
            source_encoding,
            is_dirty,
            None,
            events,
        )
    }

    /// Restores a workspace snapshot, including the currently selected node
    /// when that node still exists in the serialized SGF tree.
    pub fn restore_workspace_tab_with_state(
        &mut self,
        content: &str,
        source_path: Option<String>,
        source_encoding: SourceEncoding,
        is_dirty: bool,
        current_node_id: Option<&str>,
        events: &mut impl HostEventSink,
    ) -> Result<GameSnapshot, HostError> {
        let mut restored_game = GameDocument::from_sgf(content)?;
        restored_game.set_source_path(source_path);
        if let Some(node_id) = current_node_id {
            // Node identifiers are deterministic for the current serializer,
            // but older snapshots may use a different numbering scheme. The
            // SGF remains recoverable even when the selection cannot be.
            let _ = restored_game.restore_current_node(node_id);
        }
        if is_dirty {
            restored_game.mark_dirty();
        }
        self.game = restored_game;
        self.source_encoding = source_encoding;
        Ok(self.emit_snapshot(events))
    }

    pub fn restore_from_sgf(
        &mut self,
        content: &str,
        events: &mut impl HostEventSink,
    ) -> Result<GameSnapshot, HostError> {
        let mut restored_game = GameDocument::from_sgf(content)?;
        restored_game.set_source_path(None);
        restored_game.mark_dirty();

        self.game = restored_game;
        self.source_encoding = SourceEncoding::Utf8;
        Ok(self.emit_snapshot(events))
    }

    pub fn to_sgf(&self) -> String {
        self.game.to_sgf()
    }

    fn emit_snapshot(&self, events: &mut impl HostEventSink) -> GameSnapshot {
        let snapshot = self.game.snapshot();
        events.emit(HostEvent::GameChanged {
            snapshot: Box::new(snapshot.clone()),
        });
        snapshot
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DecodedGameFile, GameFileAccess, HostApplication, HostError, HostEvent, HostEventSink,
        SourceEncoding,
    };
    use ryusei_domain_core::{
        CURRENT_TRANSACTION_SCHEMA_VERSION, Color, GameTransaction, GameTransactionType, Vertex,
    };
    use std::{
        collections::BTreeMap,
        path::{Path, PathBuf},
    };

    #[derive(Default)]
    struct MemoryFileAccess {
        files: BTreeMap<PathBuf, DecodedGameFile>,
    }

    impl GameFileAccess for MemoryFileAccess {
        fn read_game_file(&self, path: &Path) -> Result<DecodedGameFile, HostError> {
            self.files
                .get(path)
                .cloned()
                .ok_or_else(|| HostError::FileRead("the file does not exist".to_owned()))
        }

        fn write_game_file(
            &mut self,
            path: &Path,
            content: &str,
            encoding: SourceEncoding,
        ) -> Result<(), HostError> {
            self.files.insert(
                path.to_owned(),
                DecodedGameFile {
                    content: content.to_owned(),
                    encoding,
                },
            );
            Ok(())
        }
    }

    #[derive(Default)]
    struct RecordedEvents {
        events: Vec<HostEvent>,
    }

    impl HostEventSink for RecordedEvents {
        fn emit(&mut self, event: HostEvent) {
            self.events.push(event);
        }
    }

    fn play_move_transaction() -> GameTransaction {
        GameTransaction {
            schema_version: CURRENT_TRANSACTION_SCHEMA_VERSION,
            transaction_type: GameTransactionType::PlayMove,
            color: Some(Color::Black),
            vertex: Some(Vertex { column: 3, row: 3 }),
            node_id: None,
            property: None,
            values: Vec::new(),
            marker: None,
            nodes: Vec::new(),
            score_override: None,
        }
    }

    #[test]
    fn opens_edits_saves_and_reopens_through_ui_independent_ports() {
        let game_path = PathBuf::from("/games/opening.sgf");
        let mut file_access = MemoryFileAccess {
            files: BTreeMap::from([(
                game_path.clone(),
                DecodedGameFile {
                    content: "(;FF[4]CA[Shift_JIS]SZ[19])".to_owned(),
                    encoding: SourceEncoding::ShiftJis,
                },
            )]),
        };
        let mut application = HostApplication::default();
        let mut events = RecordedEvents::default();

        application
            .open(game_path.clone(), &file_access, &mut events)
            .unwrap();
        application
            .apply_transaction(play_move_transaction(), &mut events)
            .unwrap();
        application.save(&mut file_access, &mut events).unwrap();

        let mut reopened_application = HostApplication::default();
        reopened_application
            .open(game_path.clone(), &file_access, &mut events)
            .unwrap();

        assert_eq!(application.source_encoding(), SourceEncoding::ShiftJis);
        assert_eq!(reopened_application.snapshot().moves.len(), 1);
        assert!(events.events.len() >= 4);
        assert!(matches!(
            events.events.last(),
            Some(HostEvent::GameChanged { .. })
        ));
    }

    #[test]
    fn keeps_the_current_game_when_opening_cannot_read_or_parse_a_file() {
        let mut application = HostApplication::default();
        let mut events = RecordedEvents::default();
        application
            .apply_transaction(play_move_transaction(), &mut events)
            .unwrap();
        let snapshot_before_open = application.snapshot();
        let file_access = MemoryFileAccess::default();

        let error = application
            .open(
                PathBuf::from("/games/missing.sgf"),
                &file_access,
                &mut events,
            )
            .expect_err("a missing file must not replace the current document");

        assert!(matches!(error, HostError::FileRead(_)));
        assert_eq!(application.snapshot().moves, snapshot_before_open.moves);
    }

    #[test]
    fn requires_a_save_location_for_normal_save() {
        let mut application = HostApplication::default();
        let mut file_access = MemoryFileAccess::default();
        let mut events = RecordedEvents::default();

        assert!(matches!(
            application.save(&mut file_access, &mut events),
            Err(HostError::NoSaveLocation)
        ));
    }

    #[test]
    fn discarding_the_source_location_keeps_local_changes_without_a_path() {
        let game_path = PathBuf::from("/games/opening.sgf");
        let mut application = HostApplication::default();
        let mut events = RecordedEvents::default();
        application
            .open_decoded(
                game_path.clone(),
                "(;FF[4]SZ[19])".to_owned(),
                SourceEncoding::Utf8,
                &mut events,
            )
            .unwrap();

        let snapshot = application
            .discard_source_location(&mut events)
            .expect("discarding the source location succeeds");

        assert!(snapshot.file_state.is_dirty);
        assert!(snapshot.file_state.path.is_none());
    }

    #[test]
    fn restoring_a_recovery_document_marks_it_dirty_and_pathless() {
        let mut application = HostApplication::default();
        let mut events = RecordedEvents::default();
        application
            .open_decoded(
                PathBuf::from("/games/opening.sgf"),
                "(;FF[4]SZ[19])".to_owned(),
                SourceEncoding::ShiftJis,
                &mut events,
            )
            .unwrap();

        let snapshot = application
            .restore_from_sgf("(;FF[4]SZ[9]C[recovered])", &mut events)
            .expect("restoring the recovery document succeeds");

        assert!(snapshot.file_state.is_dirty);
        assert!(snapshot.file_state.path.is_none());
        assert_eq!(snapshot.board.width, 9);
        assert_eq!(application.source_encoding(), SourceEncoding::Utf8);
    }

    #[test]
    fn restoring_a_workspace_snapshot_preserves_dirty_state() {
        let mut application = HostApplication::default();
        let mut events = RecordedEvents::default();
        let snapshot = application
            .restore_workspace_tab_with_dirty(
                "(;FF[4]SZ[9];B[dd])",
                Some("/games/unfinished.sgf".to_owned()),
                SourceEncoding::Utf8,
                true,
                &mut events,
            )
            .expect("workspace snapshot restores");
        assert!(snapshot.file_state.is_dirty);
        assert_eq!(
            snapshot.file_state.path.as_deref(),
            Some("/games/unfinished.sgf")
        );

        let clean = application
            .restore_workspace_tab_with_dirty(
                "(;FF[4]SZ[9])",
                Some("/games/clean.sgf".to_owned()),
                SourceEncoding::Utf8,
                false,
                &mut events,
            )
            .expect("clean workspace snapshot restores");
        assert!(!clean.file_state.is_dirty);
    }

    #[test]
    fn restoring_a_workspace_snapshot_preserves_selected_node() {
        let mut application = HostApplication::default();
        let mut events = RecordedEvents::default();
        let snapshot = application
            .restore_workspace_tab_with_state(
                "(;FF[4]SZ[9];B[dd];W[ee])",
                None,
                SourceEncoding::Utf8,
                false,
                Some("node-2"),
                &mut events,
            )
            .expect("workspace snapshot restores");
        assert_eq!(snapshot.current_node_id, "node-2");
        assert_eq!(
            snapshot.board.current_vertex,
            Some(Vertex { column: 4, row: 4 })
        );
    }

    #[test]
    fn serializing_the_current_document_round_trips_through_the_domain() {
        let mut application = HostApplication::default();
        let mut events = RecordedEvents::default();
        application
            .apply_transaction(play_move_transaction(), &mut events)
            .unwrap();

        let serialized_game = application.to_sgf();
        let mut reopened_application = HostApplication::default();
        let reopened_snapshot = reopened_application
            .open_decoded(
                PathBuf::from("/games/reopened.sgf"),
                serialized_game,
                SourceEncoding::Utf8,
                &mut events,
            )
            .unwrap();

        assert_eq!(reopened_snapshot.moves.len(), 1);
    }

    #[test]
    fn playing_a_move_through_the_host_updates_the_document() {
        let mut application = HostApplication::default();
        let mut events = RecordedEvents::default();

        let snapshot = application
            .play_move(
                Color::Black,
                Some(Vertex { column: 3, row: 3 }),
                &mut events,
            )
            .expect("playing a move succeeds");

        assert_eq!(snapshot.moves.len(), 1);
        assert!(snapshot.file_state.is_dirty);
        assert_eq!(events.events.len(), 1);
        assert!(matches!(
            events.events.last(),
            Some(HostEvent::GameChanged { .. })
        ));
    }

    #[test]
    fn creating_a_new_game_with_root_defaults_stays_clean() {
        let mut application = HostApplication::default();
        let mut events = RecordedEvents::default();
        let root_properties = BTreeMap::from([
            ("KM".to_owned(), vec!["6.5".to_owned()]),
            ("HA".to_owned(), vec!["2".to_owned()]),
            ("AB".to_owned(), vec!["dp".to_owned(), "pd".to_owned()]),
        ]);

        let snapshot = application
            .create_new_with_properties(19, 19, &root_properties, &mut events)
            .expect("new game with defaults succeeds");

        assert!(!snapshot.file_state.is_dirty);
        assert_eq!(snapshot.root_properties.get("KM").unwrap(), &vec!["6.5"]);
        assert_eq!(snapshot.root_properties.get("HA").unwrap(), &vec!["2"]);
        assert_eq!(
            snapshot.root_properties.get("AB").unwrap(),
            &vec!["dp", "pd"]
        );
        assert_eq!(events.events.len(), 1);
    }
}
