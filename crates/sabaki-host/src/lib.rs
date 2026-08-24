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
pub mod katago_setup;
pub mod legacy_styles;
pub mod persistence;
pub mod plugin_commands;
pub mod plugin_controller;
pub mod plugin_supervisor;
pub mod plugin_wasm;
pub mod plugin_workflow;
pub mod recent_files;
pub mod settings;
pub mod territory_estimator;
pub mod theme_workflow;
pub mod whole_game_review;

use std::path::PathBuf;

use sabaki_domain_core::{Color, GameDocument, GameSnapshot, GameTransaction, Properties, Vertex};
use serde::Serialize;
use thiserror::Error;

pub use analysis::{
    AnalysisCommandSink, AnalysisEntry, parse_analysis_response, parse_kata_analysis_line,
    parse_lz_analysis_line, replay_position_stream,
};
pub use analysis_controller::{AnalysisRunController, AnalysisRunTicket};
pub use autosave::{AutosaveCandidate, AutosaveInfo, AutosaveStore};
pub use close_flow::{CloseRequestAction, decide_close_request};
pub use engine_controller::{EngineController, EngineControllerError};
pub use engine_session::{
    EngineSession, EngineSessionError, EngineSessionState, GtpTransport, ProcessGtpTransport,
};
pub use engine_workflow::{
    EngineRecord, EngineStore, engine_list_from_value, engine_list_to_value,
    validate_engine_list_value, validate_engine_record,
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
pub use katago_setup::{
    CurlKataGoModelDownloadAdapter, HardwareBackend, KATAGO_OFFICIAL_RELEASE_BASE,
    KataGoEnvironment, KataGoModelDownloadAdapter, KataGoModelInstallError, KataGoModelTier,
    MODEL_BALANCED_NAME, MODEL_BALANCED_URL, MODEL_LIGHTWEIGHT_NAME, MODEL_LIGHTWEIGHT_URL,
    MODEL_STRONGEST_NAME, MODEL_STRONGEST_URL, build_katago_engine_record,
    ensure_katago_environment, find_katago_executable, generate_optimized_gtp_config,
    install_katago_model, install_katago_model_with, katago_storage_dir,
};
pub use legacy_styles::{LegacyStylesReport, MigratedColorRule, analyze_legacy_styles};
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
pub use recent_files::{RecentFileDto, RecentFilesStore};
pub use settings::{
    LoadedSettings, SettingKind, SettingValidationError, SettingsPersistence, SettingsStore,
    SettingsValidation, is_legacy_overwrite_marker, load_settings_store, persist_settings_store,
    setting_kind, validate_setting_value, validate_settings,
};
pub use territory_estimator::{TerritoryEstimate, estimate_territory};
pub use theme_workflow::{
    ALLOWED_THEME_ASSET_EXTENSIONS, InstalledTheme, MIN_THEME_TOKEN_SCHEMA_VERSION,
    ShellThemeTokens, THEME_SCHEMA_VERSION, THEME_TOKEN_SCHEMA_VERSION, ThemeColor, ThemeError,
    ThemeManifest, ThemeScan, ThemeTokens, install_theme, is_safe_relative_path, is_valid_theme_id,
    parse_hex_color, scan_theme_root, uninstall_theme,
};
pub use whole_game_review::{BlunderEntry, BlunderGrade, ReviewedPosition, find_blunders};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecodedGameFile {
    pub content: String,
    pub encoding: SourceEncoding,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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
    Domain(#[from] sabaki_domain_core::DomainError),
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
        let (content, encoding) = match sabaki_domain_core::legacy::file_extension(&path) {
            Some(extension) if matches!(extension.as_str(), "ngf" | "gib" | "ugf") => (
                sabaki_domain_core::legacy::import_by_extension(&extension, &decoded_file.content)
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
    use sabaki_domain_core::{
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
