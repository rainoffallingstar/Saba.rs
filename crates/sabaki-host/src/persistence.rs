use std::path::PathBuf;

use crate::autosave::{AutosaveCandidate, AutosaveInfo, AutosaveStore};
use crate::recent_files::RecentFilesStore;

pub trait HostPersistence {
    fn load_autosave(&self) -> AutosaveStore;

    fn persist_autosave(&self, store: &AutosaveStore) -> Result<(), String>;

    fn clear_autosave(&self) -> Result<(), String>;

    fn load_recent_files(&self) -> Result<RecentFilesStore, String>;

    fn persist_recent_files(&self, store: &RecentFilesStore) -> Result<(), String>;
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

#[cfg(test)]
mod tests {
    use super::{HostPersistence, record_recent_file, synchronize_autosave};
    use crate::autosave::{AutosaveCandidate, AutosaveStore};
    use crate::recent_files::RecentFilesStore;
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
}
