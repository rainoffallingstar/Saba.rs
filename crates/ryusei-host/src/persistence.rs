use std::path::PathBuf;

use crate::autosave::{AutosaveCandidate, AutosaveInfo, AutosaveStore};
use crate::recent_files::RecentFilesStore;
use crate::workspace_tabs::{WorkspaceTabError, WorkspaceTabs};

pub trait HostPersistence {
    fn load_autosave(&self) -> AutosaveStore;

    fn persist_autosave(&self, store: &AutosaveStore) -> Result<(), String>;

    fn clear_autosave(&self) -> Result<(), String>;

    fn load_recent_files(&self) -> Result<RecentFilesStore, String>;

    fn persist_recent_files(&self, store: &RecentFilesStore) -> Result<(), String>;

    /// Restores the complete multi-session workspace when the adapter supports
    /// it. Existing adapters remain valid and simply opt out by default.
    fn load_workspace_tabs(&self) -> Result<Option<WorkspaceTabs>, String> {
        Ok(None)
    }

    /// Persists every workspace session independently from user preferences.
    fn persist_workspace_tabs(&self, _tabs: &WorkspaceTabs) -> Result<(), String> {
        Err("workspace-session persistence is not supported by this adapter".to_owned())
    }

    /// Writes an exported current-position PNG through a task-level host port.
    fn persist_png_export(&self, _path: &std::path::Path, _bytes: &[u8]) -> Result<(), String> {
        Err("PNG export persistence is not supported by this adapter".to_owned())
    }

    /// Writes an exported animated GIF through a task-level host port.
    fn persist_gif_export(&self, _path: &std::path::Path, _bytes: &[u8]) -> Result<(), String> {
        Err("GIF export persistence is not supported by this adapter".to_owned())
    }
}

pub fn synchronize_autosave(
    persistence: &impl HostPersistence,
    store: &mut AutosaveStore,
    candidate: Option<AutosaveCandidate>,
) -> Result<Option<AutosaveInfo>, String> {
    let previous_store = store.clone();
    let persistence_result = match candidate {
        Some(candidate) => {
            store.replace_with(candidate);
            persistence.persist_autosave(store)
        }
        None if store.has_recovery() => {
            persistence.clear_autosave()?;
            store.clear();
            Ok(())
        }
        None => return Ok(None),
    };

    if let Err(error) = persistence_result {
        *store = previous_store;
        return Err(error);
    }

    Ok(Some(store.info()))
}

pub fn record_recent_file(
    persistence: &impl HostPersistence,
    store: &mut RecentFilesStore,
    path: PathBuf,
) -> Result<(), String> {
    let previous_store = store.clone();
    store.record_path(path);
    if let Err(error) = persistence.persist_recent_files(store) {
        *store = previous_store;
        return Err(error);
    }
    Ok(())
}

/// Deserializes and validates a persisted workspace-tabs payload. Returns
/// `Ok(None)` when the payload is absent so adapters can treat "no file" and
/// "invalid file" distinctly, while surfacing corrupt data as an error instead
/// of letting an out-of-range `active_tab_id` reach a panicking accessor.
pub fn deserialize_workspace_tabs(json: &str) -> Result<Option<WorkspaceTabs>, WorkspaceTabError> {
    WorkspaceTabs::deserialize_validated(json).map(Some)
}

#[cfg(test)]
mod tests {
    use super::{
        HostPersistence, deserialize_workspace_tabs, record_recent_file, synchronize_autosave,
    };
    use crate::autosave::{AutosaveCandidate, AutosaveStore};
    use crate::recent_files::RecentFilesStore;
    use crate::workspace_tabs::WorkspaceTabError;
    use std::{cell::RefCell, path::PathBuf};

    #[derive(Default)]
    struct MemoryHostPersistence {
        autosave: RefCell<AutosaveStore>,
        recent_files: RefCell<RecentFilesStore>,
        fail_autosave_write: bool,
        fail_autosave_clear: bool,
        fail_recent_file_write: bool,
    }

    impl HostPersistence for MemoryHostPersistence {
        fn load_autosave(&self) -> AutosaveStore {
            self.autosave.borrow().clone()
        }

        fn persist_autosave(&self, store: &AutosaveStore) -> Result<(), String> {
            if self.fail_autosave_write {
                return Err("autosave storage is unavailable".to_owned());
            }
            *self.autosave.borrow_mut() = store.clone();
            Ok(())
        }

        fn clear_autosave(&self) -> Result<(), String> {
            if self.fail_autosave_clear {
                return Err("autosave storage cannot be cleared".to_owned());
            }
            self.autosave.borrow_mut().clear();
            Ok(())
        }

        fn load_recent_files(&self) -> Result<RecentFilesStore, String> {
            Ok(self.recent_files.borrow().clone())
        }

        fn persist_recent_files(&self, store: &RecentFilesStore) -> Result<(), String> {
            if self.fail_recent_file_write {
                return Err("recent-file storage is unavailable".to_owned());
            }
            *self.recent_files.borrow_mut() = store.clone();
            Ok(())
        }
    }

    #[test]
    fn persists_and_clears_recovery_through_an_injected_boundary() {
        let persistence = MemoryHostPersistence::default();
        let mut store = AutosaveStore::default();
        let candidate = AutosaveCandidate {
            sgf: "(;FF[4]C[recovery])".to_owned(),
            revision: 12,
            source_display_name: Some("opening.sgf".to_owned()),
        };

        let persisted_info = synchronize_autosave(&persistence, &mut store, Some(candidate))
            .expect("recovery must persist")
            .expect("recovery must report an info DTO");

        assert_eq!(persisted_info.revision, Some(12));
        assert_eq!(
            persistence.load_autosave().recovery_sgf(),
            Some("(;FF[4]C[recovery])".to_owned())
        );

        synchronize_autosave(&persistence, &mut store, None)
            .expect("recovery must clear")
            .expect("clear must report an info DTO");

        assert!(!store.has_recovery());
        assert!(!persistence.load_autosave().has_recovery());
    }

    #[test]
    fn restores_the_in_memory_recovery_when_persistence_fails() {
        let persistence = MemoryHostPersistence {
            fail_autosave_write: true,
            ..MemoryHostPersistence::default()
        };
        let mut store = AutosaveStore::default();

        let error = synchronize_autosave(
            &persistence,
            &mut store,
            Some(AutosaveCandidate {
                sgf: "(;FF[4]C[recovery])".to_owned(),
                revision: 3,
                source_display_name: None,
            }),
        )
        .expect_err("failed recovery persistence must surface an error");

        assert_eq!(error, "autosave storage is unavailable");
        assert!(!store.has_recovery());
        assert!(!persistence.load_autosave().has_recovery());
    }

    #[test]
    fn retains_recovery_when_clear_persistence_fails() {
        let persistence = MemoryHostPersistence {
            fail_autosave_clear: true,
            ..MemoryHostPersistence::default()
        };
        let mut store = AutosaveStore::default();
        store.replace_with(AutosaveCandidate {
            sgf: "(;FF[4]C[recovery])".to_owned(),
            revision: 6,
            source_display_name: None,
        });

        let error = synchronize_autosave(&persistence, &mut store, None)
            .expect_err("failed recovery clear must surface an error");

        assert_eq!(error, "autosave storage cannot be cleared");
        assert!(store.has_recovery());
    }

    #[test]
    fn restores_recent_file_history_when_persistence_fails() {
        let persistence = MemoryHostPersistence {
            fail_recent_file_write: true,
            ..MemoryHostPersistence::default()
        };
        let mut store = RecentFilesStore::default();
        let path = PathBuf::from("/games/opening.sgf");

        let error = record_recent_file(&persistence, &mut store, path)
            .expect_err("failed recent-file persistence must surface an error");

        assert_eq!(error, "recent-file storage is unavailable");
        assert!(store.list().is_empty());
        assert!(persistence.load_recent_files().unwrap().list().is_empty());
    }

    #[test]
    fn validated_workspace_tabs_load_rejects_corrupt_payloads() {
        use crate::workspace_tabs::WorkspaceTabs;

        // Build a fully-populated valid payload by serializing a real workspace.
        let tabs = WorkspaceTabs::new("(;SZ[19])".to_owned(), "One");
        let valid_json = serde_json::to_string(&tabs).expect("tabs serialize");
        let loaded = deserialize_workspace_tabs(&valid_json)
            .expect("valid payload must load")
            .expect("payload must be present");
        assert_eq!(loaded.active_tab().id, "session-1");

        // Point the active id at a tab that does not exist.
        let mut value: serde_json::Value =
            serde_json::from_str(&valid_json).expect("serialized output parses");
        value["activeTabId"] = serde_json::json!("session-99");
        let unknown_active = value.to_string();
        assert!(matches!(
            deserialize_workspace_tabs(&unknown_active),
            Err(WorkspaceTabError::UnknownActiveTab(id)) if id == "session-99"
        ));

        // Empty the tab list entirely.
        let mut value: serde_json::Value =
            serde_json::from_str(&valid_json).expect("serialized output parses");
        value["tabs"] = serde_json::json!([]);
        let empty = value.to_string();
        assert!(matches!(
            deserialize_workspace_tabs(&empty),
            Err(WorkspaceTabError::EmptyTabs)
        ));
    }
}
