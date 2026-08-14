use sabaki_domain_core::{Color, GameSnapshot, GameTransaction, Vertex};
use sabaki_host::{
    AutosaveCandidate, AutosaveStore, CloseRequestAction, ExternalFileDecision,
    ExternalFileReadError, ExternalFileReader, ExternalFileStatus, ExternalFileStore,
    GameFileAccess, HostApplication, HostEvent, HostEventSink, HostPersistence, RecentFilesStore,
    SourceEncoding, decide_close_request, record_recent_file, synchronize_autosave,
};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    path::{Path, PathBuf},
};

struct MemoryFileAccess {
    files: BTreeMap<PathBuf, String>,
    encoding: SourceEncoding,
}

impl GameFileAccess for MemoryFileAccess {
    fn read_game_file(
        &self,
        path: &Path,
    ) -> Result<sabaki_host::DecodedGameFile, sabaki_host::HostError> {
        self.files
            .get(path)
            .cloned()
            .map(|content| sabaki_host::DecodedGameFile {
                content,
                encoding: self.encoding,
            })
            .ok_or_else(|| sabaki_host::HostError::FileRead("the file does not exist".to_owned()))
    }

    fn write_game_file(
        &mut self,
        path: &Path,
        content: &str,
        encoding: SourceEncoding,
    ) -> Result<(), sabaki_host::HostError> {
        self.files.insert(path.to_owned(), content.to_owned());
        self.encoding = encoding;
        Ok(())
    }
}

#[derive(Default)]
struct MemoryExternalFileReader {
    files: BTreeMap<String, String>,
}

impl ExternalFileReader for MemoryExternalFileReader {
    fn read_game_file(&self, path: &Path) -> Result<String, ExternalFileReadError> {
        self.files
            .get(&path.to_string_lossy().into_owned())
            .cloned()
            .ok_or(ExternalFileReadError::Missing)
    }
}

#[derive(Default)]
struct MemoryHostPersistence {
    autosave: RefCell<AutosaveStore>,
    recent_files: RefCell<RecentFilesStore>,
}

impl HostPersistence for MemoryHostPersistence {
    fn load_autosave(&self) -> AutosaveStore {
        self.autosave.borrow().clone()
    }

    fn persist_autosave(&self, store: &AutosaveStore) -> Result<(), String> {
        *self.autosave.borrow_mut() = store.clone();
        Ok(())
    }

    fn clear_autosave(&self) -> Result<(), String> {
        self.autosave.borrow_mut().clear();
        Ok(())
    }

    fn load_recent_files(&self) -> Result<RecentFilesStore, String> {
        Ok(self.recent_files.borrow().clone())
    }

    fn persist_recent_files(&self, store: &RecentFilesStore) -> Result<(), String> {
        *self.recent_files.borrow_mut() = store.clone();
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
        schema_version: sabaki_domain_core::CURRENT_TRANSACTION_SCHEMA_VERSION,
        transaction_type: sabaki_domain_core::GameTransactionType::PlayMove,
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

fn autosave_candidate(sgf: String, snapshot: &GameSnapshot) -> AutosaveCandidate {
    AutosaveCandidate {
        sgf,
        revision: snapshot.revision,
        source_display_name: snapshot
            .file_state
            .path
            .as_deref()
            .and_then(|path| Path::new(path).file_name())
            .and_then(|name| name.to_str())
            .map(ToOwned::to_owned),
    }
}

#[test]
fn restores_a_crashed_dirty_document_and_only_allows_save_as() {
    let game_path = PathBuf::from("/games/opening.sgf");
    let mut file_access = MemoryFileAccess {
        files: BTreeMap::from([(game_path.clone(), "(;FF[4]SZ[19])".to_owned())]),
        encoding: SourceEncoding::Utf8,
    };
    let mut host = HostApplication::default();
    let mut events = RecordedEvents::default();
    let persistence = MemoryHostPersistence::default();
    let mut autosave = AutosaveStore::default();

    host.open(game_path.clone(), &file_access, &mut events)
        .unwrap();
    let snapshot = host
        .apply_transaction(play_move_transaction(), &mut events)
        .unwrap();
    assert!(snapshot.file_state.is_dirty);

    synchronize_autosave(
        &persistence,
        &mut autosave,
        Some(autosave_candidate(host.to_sgf(), &snapshot)),
    )
    .unwrap();

    let recovery_sgf = autosave.recovery_sgf().expect("recovery must be persisted");

    let mut restarted_host = HostApplication::default();
    let restored_snapshot = restarted_host
        .restore_from_sgf(&recovery_sgf, &mut events)
        .expect("recovered document must restore");

    assert!(restored_snapshot.file_state.is_dirty);
    assert!(restored_snapshot.file_state.path.is_none());
    assert_eq!(
        restarted_host.snapshot().moves.len(),
        1,
        "the recovered document must keep the played move"
    );

    let save_as_path = PathBuf::from("/games/recovered.sgf");
    let saved_snapshot = restarted_host
        .save_at(save_as_path.clone(), &mut file_access, &mut events)
        .expect("a pathless recovered document must support Save As");

    assert!(!saved_snapshot.file_state.is_dirty);
    assert_eq!(
        saved_snapshot.file_state.path.as_deref(),
        Some("/games/recovered.sgf")
    );
    assert_eq!(file_access.files.get(&save_as_path).is_some(), true);
    assert_eq!(
        file_access.files.get(&game_path).cloned(),
        Some("(;FF[4]SZ[19])".to_owned()),
        "Save As must never overwrite the original source"
    );
}

#[test]
fn clean_external_change_reloads_the_document_and_rebases_the_baseline() {
    let game_path = PathBuf::from("/games/opening.sgf");
    let file_access = MemoryFileAccess {
        files: BTreeMap::from([(game_path.clone(), "(;FF[4]SZ[19];B[pd])".to_owned())]),
        encoding: SourceEncoding::Utf8,
    };
    let mut host = HostApplication::default();
    let mut events = RecordedEvents::default();
    let mut external_file = ExternalFileStore::default();

    let snapshot = host
        .open(game_path.clone(), &file_access, &mut events)
        .unwrap();
    external_file.track_file(game_path.clone(), &file_access.files[&game_path]);

    let mut external_reader = MemoryExternalFileReader::default();
    external_reader.files.insert(
        "/games/opening.sgf".to_owned(),
        "(;FF[4]SZ[19];B[pd];B[dd])".to_owned(),
    );

    let decision =
        external_file.decide_current_file_change(snapshot.file_state.is_dirty, &external_reader);
    assert!(matches!(
        decision,
        ExternalFileDecision::ReloadCleanDocument { .. }
    ));
    let ExternalFileDecision::ReloadCleanDocument { content } = decision else {
        unreachable!()
    };

    host.open_decoded(
        game_path.clone(),
        content.clone(),
        SourceEncoding::Utf8,
        &mut events,
    )
    .expect("the clean external reload must parse");

    external_file.track_file(game_path.clone(), &content);
    assert_eq!(
        external_file.decide_current_file_change(false, &external_reader),
        ExternalFileDecision::KeepStatus(ExternalFileStatus::Unchanged),
        "reloading must rebase the external baseline so the next check is unchanged"
    );
}

#[test]
fn dirty_external_conflict_keeps_local_changes_and_forces_save_as() {
    let game_path = PathBuf::from("/games/opening.sgf");
    let mut file_access = MemoryFileAccess {
        files: BTreeMap::from([(game_path.clone(), "(;FF[4]SZ[19])".to_owned())]),
        encoding: SourceEncoding::Utf8,
    };
    let mut host = HostApplication::default();
    let mut events = RecordedEvents::default();
    let mut external_file = ExternalFileStore::default();

    host.open(game_path.clone(), &file_access, &mut events)
        .unwrap();
    external_file.track_file(game_path.clone(), &file_access.files[&game_path]);

    let snapshot = host
        .apply_transaction(play_move_transaction(), &mut events)
        .unwrap();
    assert!(snapshot.file_state.is_dirty);

    let mut external_reader = MemoryExternalFileReader::default();
    external_reader.files.insert(
        "/games/opening.sgf".to_owned(),
        "(;FF[4]SZ[19];W[pp])".to_owned(),
    );

    assert_eq!(
        external_file.decide_current_file_change(true, &external_reader),
        ExternalFileDecision::KeepStatus(ExternalFileStatus::Changed),
        "a dirty document must never be silently replaced by an external change"
    );

    let kept_snapshot = host
        .discard_source_location(&mut events)
        .expect("keep local changes must detach from the source");

    assert!(kept_snapshot.file_state.is_dirty);
    assert!(kept_snapshot.file_state.path.is_none());

    let save_as_path = PathBuf::from("/games/kept.sgf");
    host.save_at(save_as_path.clone(), &mut file_access, &mut events)
        .expect("kept local changes must be saveable as a new file");

    assert_eq!(
        file_access.files.get(&game_path).cloned(),
        Some("(;FF[4]SZ[19])".to_owned()),
        "keeping local changes must not write back to the conflicted source"
    );
    assert_eq!(
        file_access.files.get(&save_as_path).is_some(),
        true,
        "the kept local changes must be written to the Save As path"
    );
}

#[test]
fn recovery_and_external_change_state_survive_the_close_decision_gate() {
    let game_path = PathBuf::from("/games/opening.sgf");
    let file_access = MemoryFileAccess {
        files: BTreeMap::from([(game_path.clone(), "(;FF[4]SZ[19])".to_owned())]),
        encoding: SourceEncoding::Utf8,
    };
    let mut host = HostApplication::default();
    let mut events = RecordedEvents::default();
    let persistence = MemoryHostPersistence::default();
    let mut autosave = AutosaveStore::default();

    host.open(game_path.clone(), &file_access, &mut events)
        .unwrap();
    let snapshot = host
        .apply_transaction(play_move_transaction(), &mut events)
        .unwrap();
    synchronize_autosave(
        &persistence,
        &mut autosave,
        Some(autosave_candidate(host.to_sgf(), &snapshot)),
    )
    .unwrap();

    assert_eq!(
        decide_close_request(snapshot.file_state.is_dirty, false),
        CloseRequestAction::ConfirmDiscard
    );
    assert_eq!(
        decide_close_request(snapshot.file_state.is_dirty, true),
        CloseRequestAction::Prevent
    );

    let recovery_sgf = autosave.recovery_sgf().expect("recovery must be persisted");
    let mut restarted_host = HostApplication::default();
    let restored = restarted_host
        .restore_from_sgf(&recovery_sgf, &mut events)
        .unwrap();

    assert_eq!(
        decide_close_request(restored.file_state.is_dirty, false),
        CloseRequestAction::ConfirmDiscard,
        "a restored dirty document must still be gated on close"
    );

    synchronize_autosave(&persistence, &mut autosave, None).unwrap();
    assert!(!autosave.has_recovery());
    assert!(!persistence.load_autosave().has_recovery());
}

#[test]
fn recent_files_are_recorded_and_resolved_through_the_persistence_port() {
    let persistence = MemoryHostPersistence::default();
    let mut recent_files = RecentFilesStore::default();
    let first_path = PathBuf::from("/games/opening.sgf");
    let second_path = PathBuf::from("/games/kept.sgf");

    record_recent_file(&persistence, &mut recent_files, first_path.clone()).unwrap();
    record_recent_file(&persistence, &mut recent_files, second_path.clone()).unwrap();
    record_recent_file(&persistence, &mut recent_files, first_path.clone()).unwrap();

    let reloaded = persistence.load_recent_files().unwrap();
    assert_eq!(reloaded.list().len(), 2);
    let first_id = &reloaded.list()[0].id;
    assert_eq!(
        reloaded.resolve_path(first_id).as_deref(),
        Some(first_path.as_path())
    );
}
