use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

pub const RECENT_FILES_SCHEMA_VERSION: u32 = 1;
pub const MAX_RECENT_FILE_COUNT: usize = 10;

static RECENT_FILE_ID_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedRecentFiles {
    pub schema_version: u32,
    pub entries: Vec<RecentFileEntry>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RecentFileEntry {
    pub id: String,
    pub path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentFileDto {
    pub id: String,
    pub display_name: String,
    pub is_missing: bool,
}

#[derive(Clone, Debug, Default)]
pub struct RecentFilesStore {
    entries: Vec<RecentFileEntry>,
}

impl RecentFilesStore {
    pub fn list(&self) -> Vec<RecentFileDto> {
        self.entries
            .iter()
            .map(|entry| RecentFileDto {
                id: entry.id.clone(),
                display_name: display_name_for_path(&entry.path),
                is_missing: !entry.path.is_file(),
            })
            .collect()
    }

    pub fn resolve_path(&self, identifier: &str) -> Option<PathBuf> {
        self.entries
            .iter()
            .find(|entry| entry.id == identifier)
            .map(|entry| entry.path.clone())
    }

    pub fn record_path(&mut self, path: PathBuf) {
        let normalized_path = normalize_path(path);
        self.entries.retain(|entry| entry.path != normalized_path);
        self.entries.insert(
            0,
            RecentFileEntry {
                id: create_recent_file_identifier(),
                path: normalized_path,
            },
        );
        self.entries.truncate(MAX_RECENT_FILE_COUNT);
    }

    pub fn from_persisted(persisted: PersistedRecentFiles) -> Self {
        let mut entries = Vec::new();
        for entry in persisted.entries {
            if entry.id.trim().is_empty() || entry.path.as_os_str().is_empty() {
                continue;
            }
            if entries
                .iter()
                .any(|existing: &RecentFileEntry| existing.path == entry.path)
            {
                continue;
            }
            entries.push(entry);
            if entries.len() == MAX_RECENT_FILE_COUNT {
                break;
            }
        }
        Self { entries }
    }

    pub fn to_persisted(&self) -> PersistedRecentFiles {
        PersistedRecentFiles {
            schema_version: RECENT_FILES_SCHEMA_VERSION,
            entries: self.entries.clone(),
        }
    }
}

pub fn persisted_recent_files_from_bytes(content: &[u8]) -> Result<PersistedRecentFiles, String> {
    serde_json::from_slice(content).map_err(|error| error.to_string())
}

pub fn persisted_recent_files_to_bytes(
    persisted: &PersistedRecentFiles,
) -> Result<Vec<u8>, String> {
    serde_json::to_vec(persisted).map_err(|error| error.to_string())
}

fn normalize_path(path: PathBuf) -> PathBuf {
    path.canonicalize().unwrap_or(path)
}

fn display_name_for_path(path: &Path) -> String {
    path.file_name()
        .and_then(|file_name| file_name.to_str())
        .filter(|file_name| !file_name.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

fn create_recent_file_identifier() -> String {
    let timestamp_milliseconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    let counter = RECENT_FILE_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("recent-{timestamp_milliseconds}-{counter}")
}

#[cfg(test)]
mod tests {
    use super::{RecentFilesStore, create_recent_file_identifier};
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    static TEST_DIRECTORY_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn create_test_directory() -> PathBuf {
        let counter = TEST_DIRECTORY_COUNTER.fetch_add(1, Ordering::Relaxed);
        let test_directory = std::env::temp_dir().join(format!(
            "sabaki-host-recent-files-{}-{counter}",
            std::process::id()
        ));
        fs::create_dir_all(&test_directory).expect("test directory should be created");
        test_directory
    }

    #[test]
    fn records_deduplicated_recent_files_without_exposing_paths() {
        let test_directory = create_test_directory();
        let first_path = test_directory.join("first.sgf");
        let second_path = test_directory.join("second.sgf");
        fs::write(&first_path, "(;FF[4])").unwrap();
        fs::write(&second_path, "(;FF[4])").unwrap();
        let mut store = RecentFilesStore::default();

        store.record_path(first_path.clone());
        store.record_path(second_path.clone());
        store.record_path(first_path.clone());
        let entries = store.list();

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].display_name, "first.sgf");
        assert!(!entries[0].is_missing);
        assert!(!entries[0].id.contains("first.sgf"));
        let resolved_path = store
            .resolve_path(&entries[0].id)
            .expect("the opaque recent-file identifier should resolve in the host");
        assert_eq!(
            resolved_path
                .file_name()
                .and_then(|file_name| file_name.to_str()),
            Some("first.sgf")
        );
        fs::remove_dir_all(test_directory).unwrap();
    }

    #[test]
    fn reports_missing_files_from_the_host_without_the_path() {
        let test_directory = create_test_directory();
        let existing_path = test_directory.join("existing.sgf");
        fs::write(&existing_path, "(;FF[4])").unwrap();
        let mut store = RecentFilesStore::default();
        store.record_path(existing_path.clone());

        fs::remove_file(&existing_path).unwrap();
        let entries = store.list();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].display_name, "existing.sgf");
        assert!(entries[0].is_missing);
        fs::remove_dir_all(test_directory).unwrap();
    }

    #[test]
    fn caps_the_persisted_history_to_ten_entries() {
        let test_directory = create_test_directory();
        let mut store = RecentFilesStore::default();

        for index in 0..12 {
            store.record_path(test_directory.join(format!("game-{index}.sgf")));
        }

        assert_eq!(store.list().len(), 10);
        assert_eq!(store.list()[0].display_name, "game-11.sgf");
        assert_eq!(store.list()[9].display_name, "game-2.sgf");
        fs::remove_dir_all(test_directory).unwrap();
    }

    #[test]
    fn persists_and_reconstructs_deduplicated_entries() {
        let mut store = RecentFilesStore::default();
        store.record_path(PathBuf::from("/games/one.sgf"));
        store.record_path(PathBuf::from("/games/two.sgf"));

        let reconstructed = RecentFilesStore::from_persisted(store.to_persisted());

        assert_eq!(reconstructed.list().len(), 2);
        assert_eq!(reconstructed.list()[0].display_name, "two.sgf");
    }

    #[test]
    fn generates_unique_identifiers_for_repeated_records() {
        let first = create_recent_file_identifier();
        let second = create_recent_file_identifier();
        assert_ne!(first, second);
    }
}
