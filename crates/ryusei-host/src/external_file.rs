use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Clone, Debug, Eq, PartialEq)]
struct TrackedFile {
    path: PathBuf,
    fingerprint: String,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ExternalFileStatus {
    #[default]
    Untracked,
    Unchanged,
    Changed,
    Missing,
    Unreadable,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalFileStatusDto {
    pub status: ExternalFileStatus,
    pub display_name: Option<String>,
}

#[derive(Clone, Debug)]
pub enum ExternalFileObservation {
    Untracked,
    Unchanged,
    Changed { content: String },
    Missing,
    Unreadable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExternalFileDecision {
    KeepStatus(ExternalFileStatus),
    ReloadCleanDocument { content: String },
}

#[derive(Debug, Error)]
pub enum ExternalFileReadError {
    #[error("the tracked game file no longer exists")]
    Missing,
    #[error("the tracked game file cannot be read")]
    Unreadable,
}

pub trait ExternalFileReader {
    fn read_game_file(&self, path: &Path) -> Result<String, ExternalFileReadError>;
}

#[derive(Clone, Debug, Default)]
pub struct ExternalFileStore {
    tracked_file: Option<TrackedFile>,
    status: ExternalFileStatus,
}

impl ExternalFileStore {
    pub fn status(&self) -> ExternalFileStatusDto {
        ExternalFileStatusDto {
            status: self.status,
            display_name: self
                .tracked_file
                .as_ref()
                .and_then(|tracked_file| tracked_file.path.file_name())
                .and_then(|file_name| file_name.to_str())
                .map(ToOwned::to_owned),
        }
    }

    pub fn track_file(&mut self, path: PathBuf, content: &str) {
        self.tracked_file = Some(TrackedFile {
            path,
            fingerprint: fingerprint_content(content),
        });
        self.status = ExternalFileStatus::Unchanged;
    }

    pub fn detach_file(&mut self) {
        self.tracked_file = None;
        self.status = ExternalFileStatus::Untracked;
    }

    pub fn inspect_file(&self, reader: &impl ExternalFileReader) -> ExternalFileObservation {
        let Some(tracked_file) = &self.tracked_file else {
            return ExternalFileObservation::Untracked;
        };
        let content = match reader.read_game_file(&tracked_file.path) {
            Ok(content) => content,
            Err(ExternalFileReadError::Missing) => return ExternalFileObservation::Missing,
            Err(ExternalFileReadError::Unreadable) => return ExternalFileObservation::Unreadable,
        };
        if fingerprint_content(&content) == tracked_file.fingerprint {
            ExternalFileObservation::Unchanged
        } else {
            ExternalFileObservation::Changed { content }
        }
    }

    pub fn decide_current_file_change(
        &self,
        is_document_dirty: bool,
        reader: &impl ExternalFileReader,
    ) -> ExternalFileDecision {
        decide_external_file_change(self.inspect_file(reader), is_document_dirty)
    }

    pub fn set_status(&mut self, status: ExternalFileStatus) {
        self.status = status;
    }

    pub fn tracked_path(&self) -> Option<PathBuf> {
        self.tracked_file
            .as_ref()
            .map(|tracked_file| tracked_file.path.clone())
    }

    /// Returns the fingerprint of the last accepted on-disk baseline. This is
    /// distinct from the active document content when the document is dirty.
    pub fn tracked_fingerprint(&self) -> Option<String> {
        self.tracked_file
            .as_ref()
            .map(|tracked_file| tracked_file.fingerprint.clone())
    }

    /// Restores a previously captured external-file baseline without treating
    /// the current document snapshot as the file's on-disk contents.
    pub fn track_file_with_fingerprint(&mut self, path: PathBuf, fingerprint: String) {
        self.tracked_file = Some(TrackedFile { path, fingerprint });
        self.status = ExternalFileStatus::Unchanged;
    }
}

pub fn decide_external_file_change(
    observation: ExternalFileObservation,
    is_document_dirty: bool,
) -> ExternalFileDecision {
    match observation {
        ExternalFileObservation::Changed { content } if !is_document_dirty => {
            ExternalFileDecision::ReloadCleanDocument { content }
        }
        ExternalFileObservation::Changed { .. } => {
            ExternalFileDecision::KeepStatus(ExternalFileStatus::Changed)
        }
        ExternalFileObservation::Untracked => {
            ExternalFileDecision::KeepStatus(ExternalFileStatus::Untracked)
        }
        ExternalFileObservation::Unchanged => {
            ExternalFileDecision::KeepStatus(ExternalFileStatus::Unchanged)
        }
        ExternalFileObservation::Missing => {
            ExternalFileDecision::KeepStatus(ExternalFileStatus::Missing)
        }
        ExternalFileObservation::Unreadable => {
            ExternalFileDecision::KeepStatus(ExternalFileStatus::Unreadable)
        }
    }
}

pub fn fingerprint_content(content: &str) -> String {
    Sha256::digest(content.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        ExternalFileDecision, ExternalFileObservation, ExternalFileReadError, ExternalFileReader,
        ExternalFileStatus, ExternalFileStore, decide_external_file_change,
    };
    use std::{collections::BTreeMap, path::Path};

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

    #[test]
    fn detects_changed_content_even_when_the_path_is_unchanged() {
        let game_path = "/games/game.sgf";
        let mut store = ExternalFileStore::default();
        store.track_file(std::path::PathBuf::from(game_path), "(;C[original])");
        let reader = MemoryExternalFileReader {
            files: BTreeMap::from([(game_path.to_owned(), "(;C[external])".to_owned())]),
        };

        assert!(matches!(
            store.inspect_file(&reader),
            ExternalFileObservation::Changed { content } if content == "(;C[external])"
        ));
    }

    #[test]
    fn reports_missing_files_without_exposing_the_host_path() {
        let game_path = "/games/private-game.sgf";
        let mut store = ExternalFileStore::default();
        store.track_file(std::path::PathBuf::from(game_path), "(;)");
        let reader = MemoryExternalFileReader::default();

        assert!(matches!(
            store.inspect_file(&reader),
            ExternalFileObservation::Missing
        ));
        store.set_status(ExternalFileStatus::Missing);
        let status = store.status();
        assert_eq!(status.status, ExternalFileStatus::Missing);
        assert_eq!(status.display_name, Some("private-game.sgf".to_owned()));
    }

    #[test]
    fn automatically_reloads_only_clean_documents_after_an_external_change() {
        assert_eq!(
            decide_external_file_change(
                ExternalFileObservation::Changed {
                    content: "(;C[external])".to_owned(),
                },
                false,
            ),
            ExternalFileDecision::ReloadCleanDocument {
                content: "(;C[external])".to_owned(),
            }
        );
        assert_eq!(
            decide_external_file_change(
                ExternalFileObservation::Changed {
                    content: "(;C[external])".to_owned(),
                },
                true,
            ),
            ExternalFileDecision::KeepStatus(ExternalFileStatus::Changed)
        );
    }

    #[test]
    fn never_returns_external_content_for_non_reloadable_states() {
        for observation in [
            ExternalFileObservation::Untracked,
            ExternalFileObservation::Unchanged,
            ExternalFileObservation::Missing,
            ExternalFileObservation::Unreadable,
        ] {
            assert!(matches!(
                decide_external_file_change(observation, false),
                ExternalFileDecision::KeepStatus(_)
            ));
        }
    }

    #[test]
    fn clean_reload_rebases_the_external_file_and_keeps_future_checks_unchanged() {
        let game_path = "/games/game.sgf";
        let mut store = ExternalFileStore::default();
        store.track_file(std::path::PathBuf::from(game_path), "(;C[original])");
        let mut reader = MemoryExternalFileReader {
            files: BTreeMap::from([(game_path.to_owned(), "(;C[external])".to_owned())]),
        };

        let decision = store.decide_current_file_change(false, &reader);
        assert_eq!(
            decision,
            ExternalFileDecision::ReloadCleanDocument {
                content: "(;C[external])".to_owned(),
            }
        );

        store.track_file(std::path::PathBuf::from(game_path), "(;C[external])");
        assert_eq!(
            store.decide_current_file_change(false, &reader),
            ExternalFileDecision::KeepStatus(ExternalFileStatus::Unchanged)
        );
        reader
            .files
            .insert(game_path.to_owned(), "(;C[original])".to_owned());
        assert_eq!(
            store.decide_current_file_change(false, &reader),
            ExternalFileDecision::ReloadCleanDocument {
                content: "(;C[original])".to_owned(),
            }
        );
    }

    #[test]
    fn detaching_after_a_dirty_conflict_removes_the_source_identity() {
        let game_path = "/games/private-game.sgf";
        let mut store = ExternalFileStore::default();
        store.track_file(std::path::PathBuf::from(game_path), "(;C[original])");
        let reader = MemoryExternalFileReader {
            files: BTreeMap::from([(game_path.to_owned(), "(;C[external])".to_owned())]),
        };

        assert_eq!(
            store.decide_current_file_change(true, &reader),
            ExternalFileDecision::KeepStatus(ExternalFileStatus::Changed)
        );
        store.detach_file();

        assert_eq!(store.status().status, ExternalFileStatus::Untracked);
        assert_eq!(store.status().display_name, None);
        assert_eq!(store.tracked_path(), None);
    }

    #[test]
    fn accepting_a_new_baseline_clears_a_change_status() {
        let mut store = ExternalFileStore::default();
        store.track_file(std::path::PathBuf::from("/games/game.sgf"), "(;C[before])");

        store.track_file(std::path::PathBuf::from("/games/game.sgf"), "(;C[after])");

        assert_eq!(store.status().status, ExternalFileStatus::Unchanged);
    }

    #[test]
    fn persisted_fingerprint_restores_the_disk_baseline_for_a_dirty_tab() {
        let game_path = "/games/game.sgf";
        let original = "(;C[original])";
        let fingerprint = super::fingerprint_content(original);
        let mut store = ExternalFileStore::default();
        store.track_file_with_fingerprint(std::path::PathBuf::from(game_path), fingerprint.clone());
        let reader = MemoryExternalFileReader {
            files: BTreeMap::from([(game_path.to_owned(), original.to_owned())]),
        };

        assert_eq!(
            store.tracked_fingerprint().as_deref(),
            Some(fingerprint.as_str())
        );
        assert_eq!(
            store.decide_current_file_change(true, &reader),
            ExternalFileDecision::KeepStatus(ExternalFileStatus::Unchanged)
        );
    }

    #[test]
    fn fingerprints_differ_for_different_decoded_content() {
        assert_ne!(
            super::fingerprint_content("(;C[a])"),
            super::fingerprint_content("(;C[b])")
        );
    }
}
