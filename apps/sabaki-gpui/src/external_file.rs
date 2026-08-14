//! External SGF change detection for the GPUI client.
//!
//! The detection state machine lives in `sabaki_host::ExternalFileStore`; this
//! module only provides the file-system reader (decoded content fingerprints,
//! mirroring the reference adapter) and the shell-facing check workflow.

use std::path::Path;

use sabaki_host::{
    ExternalFileDecision, ExternalFileReadError, ExternalFileReader, ExternalFileStatus,
    ExternalFileStore, SourceEncoding, decode_sgf_bytes,
};

/// Reads a tracked game file and returns its decoded content. Fingerprints are
/// computed on the decoded text, so a Shift_JIS file re-saved with identical
/// content is not reported as changed.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeExternalFileReader;

impl ExternalFileReader for NativeExternalFileReader {
    fn read_game_file(&self, path: &Path) -> Result<String, ExternalFileReadError> {
        let bytes = std::fs::read(path).map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => ExternalFileReadError::Missing,
            _ => ExternalFileReadError::Unreadable,
        })?;
        decode_sgf_bytes(&bytes)
            .map(|decoded| decoded.content)
            .map_err(|_| ExternalFileReadError::Unreadable)
    }
}

/// The outcome of a periodic external-file check: either the store status
/// should be kept as-is (already applied by the caller) or the document was
/// replaced by the external content.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExternalCheckOutcome {
    Status(ExternalFileStatus),
    Reloaded,
    Failed(String),
}

/// Runs one external-file check against the current document and applies the
/// decision. Clean documents changed on disk are reloaded through the host
/// (decoding with the declared encoding and rebasing the fingerprint); dirty
/// documents only update the store status so the UI can surface the conflict.
///
/// Returns the applied outcome so the shell can update its status bar.
pub fn check_external_file(
    external_file: &mut ExternalFileStore,
    host: &mut sabaki_host::HostApplication,
    is_document_dirty: bool,
) -> ExternalCheckOutcome {
    let decision =
        external_file.decide_current_file_change(is_document_dirty, &NativeExternalFileReader);
    match decision {
        ExternalFileDecision::KeepStatus(status) => {
            external_file.set_status(status);
            ExternalCheckOutcome::Status(status)
        }
        ExternalFileDecision::ReloadCleanDocument { content } => {
            let Some(path) = external_file.tracked_path() else {
                external_file.set_status(ExternalFileStatus::Untracked);
                return ExternalCheckOutcome::Status(ExternalFileStatus::Untracked);
            };
            let encoding = detect_encoding_or_default(&content);
            let mut events = RecordingEventSink::default();
            match host.open_decoded(path.clone(), content.clone(), encoding, &mut events) {
                Ok(_) => {
                    external_file.track_file(path, &content);
                    ExternalCheckOutcome::Reloaded
                }
                Err(error) => {
                    external_file.set_status(ExternalFileStatus::Unreadable);
                    ExternalCheckOutcome::Failed(error.to_string())
                }
            }
        }
    }
}

/// Rebases the external-file baseline after a successful open or save, so the
/// next check compares against the content that is actually on disk.
pub fn track_after_file_operation(
    external_file: &mut ExternalFileStore,
    path: &Path,
) -> Result<(), String> {
    let decoded = NativeExternalFileReader
        .read_game_file(path)
        .map_err(|error| format!("could not re-read the game file: {error}"))?;
    external_file.track_file(path.to_owned(), &decoded);
    Ok(())
}

/// Detects the encoding the reloaded content declares, defaulting to UTF-8 so
/// a reload never fails on a missing declaration.
fn detect_encoding_or_default(content: &str) -> SourceEncoding {
    sabaki_host::detect_sgf_encoding(content.as_bytes())
        .ok()
        .flatten()
        .unwrap_or(SourceEncoding::Utf8)
}

#[derive(Default)]
struct RecordingEventSink;

impl sabaki_host::HostEventSink for RecordingEventSink {
    fn emit(&mut self, _event: sabaki_host::HostEvent) {}
}

#[cfg(test)]
mod tests {
    use super::{
        ExternalCheckOutcome, NativeExternalFileReader, check_external_file,
        track_after_file_operation,
    };
    use sabaki_host::{
        ExternalFileReadError, ExternalFileReader, ExternalFileStatus, ExternalFileStore,
        GameFileAccess, HostApplication, HostEventSink, SourceEncoding,
    };
    use std::{
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    static TEST_DIRECTORY_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn fresh_test_directory(test_name: &str) -> PathBuf {
        let counter = TEST_DIRECTORY_COUNTER.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "sabaki-gpui-external-{test_name}-{}-{counter}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("test directory is created");
        directory
    }

    #[derive(Default)]
    struct RecordingEventSink;

    impl HostEventSink for RecordingEventSink {
        fn emit(&mut self, _event: sabaki_host::HostEvent) {}
    }

    fn open_game(path: &Path, content: &str, encoding: SourceEncoding) -> HostApplication {
        let mut host = HostApplication::default();
        let mut events = RecordingEventSink::default();
        host.open_decoded(path.to_owned(), content.to_owned(), encoding, &mut events)
            .expect("opening the fixture game succeeds");
        host
    }

    #[test]
    fn reader_returns_decoded_content_for_legacy_encodings() {
        let directory = fresh_test_directory("shiftjis-fingerprint");
        let path = directory.join("japanese.sgf");
        let source_sgf = "(;FF[4]CA[Shift_JIS]C[日本語])";
        let bytes = sabaki_host::encode_sgf(source_sgf, SourceEncoding::ShiftJis)
            .expect("fixture is representable as Shift_JIS");
        std::fs::write(&path, bytes).expect("fixture is written");

        let decoded = NativeExternalFileReader
            .read_game_file(&path)
            .expect("the file must be readable");

        assert_eq!(decoded, source_sgf);
        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn reader_distinguishes_missing_and_unreadable_files() {
        assert!(matches!(
            NativeExternalFileReader.read_game_file(Path::new("/nowhere/game.sgf")),
            Err(ExternalFileReadError::Missing)
        ));

        let directory = fresh_test_directory("unreadable");
        let path = directory.join("broken.sgf");
        std::fs::write(&path, b"(;FF[4]C[\xff])").expect("fixture is written");
        assert!(matches!(
            NativeExternalFileReader.read_game_file(&path),
            Err(ExternalFileReadError::Unreadable)
        ));
        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn clean_external_changes_are_reloaded_and_rebased() {
        let directory = fresh_test_directory("clean-reload");
        let path = directory.join("game.sgf");
        let original = "(;FF[4]SZ[19];B[pd])";
        let external = "(;FF[4]SZ[19];B[pd];W[dp])";
        std::fs::write(&path, original).expect("original is written");

        let mut host = open_game(&path, original, SourceEncoding::Utf8);
        let mut store = ExternalFileStore::default();
        store.track_file(path.clone(), original);

        assert_eq!(store.status().status, ExternalFileStatus::Unchanged);
        std::fs::write(&path, external).expect("external edit is written");

        let outcome = check_external_file(&mut store, &mut host, false);

        assert_eq!(outcome, ExternalCheckOutcome::Reloaded);
        assert_eq!(store.status().status, ExternalFileStatus::Unchanged);
        assert_eq!(host.snapshot().moves.len(), 2);

        let second_check = check_external_file(&mut store, &mut host, false);
        assert_eq!(
            second_check,
            ExternalCheckOutcome::Status(ExternalFileStatus::Unchanged)
        );
        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn dirty_documents_keep_the_change_status_without_reloading() {
        let directory = fresh_test_directory("dirty-conflict");
        let path = directory.join("game.sgf");
        let original = "(;FF[4]SZ[19];B[pd])";
        let external = "(;FF[4]SZ[19];B[pd];W[dp])";
        std::fs::write(&path, original).expect("original is written");

        let mut host = open_game(&path, original, SourceEncoding::Utf8);
        let mut events = RecordingEventSink::default();
        host.play_move(
            sabaki_domain_core::Color::White,
            Some(sabaki_domain_core::Vertex { column: 3, row: 15 }),
            &mut events,
        )
        .expect("a local move makes the document dirty");
        let mut store = ExternalFileStore::default();
        store.track_file(path.clone(), original);
        std::fs::write(&path, external).expect("external edit is written");

        let outcome = check_external_file(&mut store, &mut host, true);

        assert_eq!(
            outcome,
            ExternalCheckOutcome::Status(ExternalFileStatus::Changed)
        );
        assert_eq!(host.snapshot().moves.len(), 2, "no reload for dirty docs");
        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn tracking_after_open_rebases_the_fingerprint_on_disk_content() {
        let directory = fresh_test_directory("rebase");
        let path = directory.join("game.sgf");
        std::fs::write(&path, "(;FF[4]SZ[19])").expect("game is written");

        let mut store = ExternalFileStore::default();
        track_after_file_operation(&mut store, &path).expect("tracking succeeds");

        assert_eq!(
            store.status().status,
            ExternalFileStatus::Unchanged,
            "tracking establishes an unchanged baseline"
        );

        let mut file_access = crate::dialog_service::NativeGameFileAccess::default();
        file_access
            .write_game_file(&path, "(;FF[4]SZ[19])", SourceEncoding::Utf8)
            .expect("rewriting identical decoded content succeeds");

        let mut host = open_game(&path, "(;FF[4]SZ[19])", SourceEncoding::Utf8);
        let outcome = check_external_file(&mut store, &mut host, false);
        assert_eq!(
            outcome,
            ExternalCheckOutcome::Status(ExternalFileStatus::Unchanged)
        );
        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn shift_jis_files_are_tracked_by_their_decoded_fingerprint() {
        let directory = fresh_test_directory("shiftjis-rebase");
        let path = directory.join("japanese.sgf");
        let source_sgf = "(;FF[4]CA[Shift_JIS]C[日本語])";
        let bytes = sabaki_host::encode_sgf(source_sgf, SourceEncoding::ShiftJis)
            .expect("fixture is representable as Shift_JIS");
        std::fs::write(&path, bytes).expect("fixture is written");

        let mut store = ExternalFileStore::default();
        store.track_file(path.clone(), source_sgf);

        let mut host = open_game(&path, source_sgf, SourceEncoding::ShiftJis);
        let outcome = check_external_file(&mut store, &mut host, false);
        assert_eq!(
            outcome,
            ExternalCheckOutcome::Status(ExternalFileStatus::Unchanged)
        );
        std::fs::remove_dir_all(&directory).ok();
    }
}
