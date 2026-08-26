//! Plugin-private persistent storage.
//!
//! Each plugin owns a namespace under the host-managed storage root; the
//! host decides where the root lives and the plugin never sees paths. Keys
//! are validated so a plugin can only address its own namespace, and every
//! write is atomic (temporary sibling + rename) so a crash never leaves a
//! torn value. The `Storage` permission gates access at the workflow layer;
//! this module only implements the storage mechanics.

use std::{
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
};

use serde_json::Value;
use thiserror::Error;

/// Maximum serialized size of a single stored value, mirroring the RPC frame
/// limit so a plugin cannot grow the host disk unboundedly.
pub const MAX_STORAGE_ENTRY_BYTES: usize = 1024 * 1024;

/// Maximum number of entries a single plugin may store.
pub const MAX_STORAGE_KEYS: usize = 4096;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("invalid plugin id: {0:?}")]
    InvalidPluginId(String),
    #[error("invalid storage key: {0:?}")]
    InvalidKey(String),
    #[error("storage entry exceeds the {MAX_STORAGE_ENTRY_BYTES}-byte limit")]
    EntryTooLarge,
    #[error("plugin storage holds too many entries")]
    TooManyKeys,
    #[error("could not read storage entry {key:?}: {source}")]
    Read { key: String, source: std::io::Error },
    #[error("could not write storage entry {key:?}: {source}")]
    Write { key: String, source: std::io::Error },
    #[error("stored value for {key:?} is corrupt: {source}")]
    Corrupt {
        key: String,
        source: serde_json::Error,
    },
}

/// Validates a plugin id for use as a path component. The manifest validator
/// already requires a reverse-domain shape; this is the defensive check at
/// the storage boundary so no id can escape its namespace.
pub fn is_valid_plugin_id(plugin_id: &str) -> bool {
    !plugin_id.is_empty()
        && plugin_id.len() <= 128
        && plugin_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
        && !plugin_id.starts_with('.')
        && !plugin_id.ends_with('.')
        && !plugin_id.contains("..")
}

/// Validates a storage key: a short dotted name, never a path.
pub fn is_valid_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 256
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
        && !key.starts_with('.')
        && !key.ends_with('.')
}

fn storage_dir_for(storage_root: &Path, plugin_id: &str) -> Result<PathBuf, StorageError> {
    if !is_valid_plugin_id(plugin_id) {
        return Err(StorageError::InvalidPluginId(plugin_id.to_owned()));
    }
    Ok(storage_root.join(plugin_id))
}

fn entry_path(storage_root: &Path, plugin_id: &str, key: &str) -> Result<PathBuf, StorageError> {
    if !is_valid_key(key) {
        return Err(StorageError::InvalidKey(key.to_owned()));
    }
    Ok(storage_dir_for(storage_root, plugin_id)?.join(format!("{key}.json")))
}

/// Host-managed key-value storage namespaced per plugin. The storage root is
/// owned by the host (typically `<config dir>/plugin-storage`); plugins only
/// ever pass ids and keys.
#[derive(Clone, Debug)]
pub struct PluginPrivateStore {
    storage_root: PathBuf,
}

impl PluginPrivateStore {
    pub fn new(storage_root: impl Into<PathBuf>) -> Self {
        Self {
            storage_root: storage_root.into(),
        }
    }

    pub fn storage_root(&self) -> &Path {
        &self.storage_root
    }

    /// Lists every stored key for the plugin, sorted.
    pub fn list_keys(&self, plugin_id: &str) -> Result<Vec<String>, StorageError> {
        let directory = storage_dir_for(&self.storage_root, plugin_id)?;
        let mut keys = Vec::new();
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(keys),
            Err(error) => {
                return Err(StorageError::Read {
                    key: String::new(),
                    source: error,
                });
            }
        };
        for entry in entries {
            let entry = entry.map_err(|error| StorageError::Read {
                key: String::new(),
                source: error,
            })?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            let Some(stripped) = name.strip_suffix(".json") else {
                continue;
            };
            if is_valid_key(stripped) {
                keys.push(stripped.to_owned());
            }
        }
        keys.sort();
        Ok(keys)
    }

    /// Reads the value for `key`, or `None` when no value was stored yet.
    pub fn read_value(&self, plugin_id: &str, key: &str) -> Result<Option<Value>, StorageError> {
        let path = entry_path(&self.storage_root, plugin_id, key)?;
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(StorageError::Read {
                    key: key.to_owned(),
                    source: error,
                });
            }
        };
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|source| StorageError::Corrupt {
                key: key.to_owned(),
                source,
            })
    }

    /// Stores `value` for `key`, replacing any previous value atomically.
    pub fn write_value(
        &self,
        plugin_id: &str,
        key: &str,
        value: &Value,
    ) -> Result<(), StorageError> {
        let path = entry_path(&self.storage_root, plugin_id, key)?;
        let mut serialized = serde_json::to_vec(value).map_err(|source| StorageError::Corrupt {
            key: key.to_owned(),
            source,
        })?;
        serialized.push(b'\n');
        if serialized.len() > MAX_STORAGE_ENTRY_BYTES {
            return Err(StorageError::EntryTooLarge);
        }
        if self.list_keys(plugin_id)?.len() >= MAX_STORAGE_KEYS && !path.exists() {
            return Err(StorageError::TooManyKeys);
        }

        let directory = path.parent().ok_or_else(|| StorageError::Write {
            key: key.to_owned(),
            source: std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "entry path has no parent",
            ),
        })?;
        fs::create_dir_all(directory).map_err(|source| StorageError::Write {
            key: key.to_owned(),
            source,
        })?;
        let temporary_path = directory.join(format!(".{key}.tmp-{}", std::process::id()));
        let write_result = (|| -> std::io::Result<()> {
            let mut file = fs::File::create(&temporary_path)?;
            file.write_all(&serialized)?;
            file.sync_all()?;
            Ok(())
        })();
        if let Err(source) = write_result {
            let _ = fs::remove_file(&temporary_path);
            return Err(StorageError::Write {
                key: key.to_owned(),
                source,
            });
        }
        fs::rename(&temporary_path, &path).map_err(|source| StorageError::Write {
            key: key.to_owned(),
            source,
        })?;
        Ok(())
    }

    /// Removes the value for `key`; missing keys are a no-op.
    pub fn remove_value(&self, plugin_id: &str, key: &str) -> Result<(), StorageError> {
        let path = entry_path(&self.storage_root, plugin_id, key)?;
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(StorageError::Write {
                key: key.to_owned(),
                source,
            }),
        }
    }
}

/// Ensures every component of a plugin-supplied path stays inside the storage
/// root; kept for callers that deal in raw paths rather than keys.
pub fn path_is_within(root: &Path, candidate: &Path) -> bool {
    let mut remaining = candidate.components().peekable();
    while matches!(remaining.peek(), Some(Component::CurDir)) {
        remaining.next();
    }
    remaining.all(|component| matches!(component, Component::Normal(_)))
        && candidate.starts_with(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_root(test_name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "ryusei-plugin-storage-{test_name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        root
    }

    #[test]
    fn rejects_path_traversal_in_ids_and_keys() {
        assert!(!is_valid_plugin_id("../escape"));
        assert!(!is_valid_plugin_id("org.example/escape"));
        assert!(!is_valid_plugin_id("org.example..escape"));
        assert!(!is_valid_plugin_id(".org.example"));
        assert!(is_valid_plugin_id("org.example.opening-trainer"));

        assert!(!is_valid_key("../escape"));
        assert!(!is_valid_key("a/b"));
        assert!(!is_valid_key(".hidden"));
        assert!(!is_valid_key("trailing."));
        assert!(is_valid_key("game.lastMove"));
    }

    #[test]
    fn writes_reads_lists_and_removes_values() {
        let root = fresh_root("roundtrip");
        let store = PluginPrivateStore::new(&root);

        assert!(store.list_keys("org.example.one").unwrap().is_empty());
        assert_eq!(store.read_value("org.example.one", "prefs").unwrap(), None);

        store
            .write_value(
                "org.example.one",
                "prefs",
                &serde_json::json!({"showCoordinates": true, "depth": 3}),
            )
            .unwrap();
        assert_eq!(
            store.read_value("org.example.one", "prefs").unwrap(),
            Some(serde_json::json!({"showCoordinates": true, "depth": 3}))
        );
        assert_eq!(store.list_keys("org.example.one").unwrap(), vec!["prefs"]);

        store
            .write_value("org.example.one", "prefs", &serde_json::json!({"depth": 5}))
            .unwrap();
        assert_eq!(
            store.read_value("org.example.one", "prefs").unwrap(),
            Some(serde_json::json!({"depth": 5})),
            "a rewrite must replace the previous value"
        );

        store.remove_value("org.example.one", "prefs").unwrap();
        assert_eq!(
            store.read_value("org.example.one", "prefs").unwrap(),
            None,
            "removed keys read back as absent"
        );
        store.remove_value("org.example.one", "prefs").unwrap();
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn namespaces_are_isolated_between_plugins() {
        let root = fresh_root("isolation");
        let store = PluginPrivateStore::new(&root);
        store
            .write_value("org.example.one", "prefs", &serde_json::json!({"x": 1}))
            .unwrap();

        assert_eq!(
            store.read_value("org.example.two", "prefs").unwrap(),
            None,
            "another plugin must never see the value"
        );
        assert!(store.list_keys("org.example.two").unwrap().is_empty());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn rejects_values_over_the_size_limit() {
        let root = fresh_root("oversize");
        let store = PluginPrivateStore::new(&root);
        let big = serde_json::json!({"payload": "x".repeat(MAX_STORAGE_ENTRY_BYTES + 1)});

        assert!(matches!(
            store.write_value("org.example.one", "big", &big),
            Err(StorageError::EntryTooLarge)
        ));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn rejects_entries_over_the_key_count_limit() {
        let root = fresh_root("keycount");
        let store = PluginPrivateStore::new(&root);
        for index in 0..MAX_STORAGE_KEYS {
            store
                .write_value(
                    "org.example.one",
                    &format!("key{index}"),
                    &serde_json::json!(index),
                )
                .unwrap();
        }
        assert!(matches!(
            store.write_value("org.example.one", "overflow", &serde_json::json!(1)),
            Err(StorageError::TooManyKeys)
        ));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn survives_a_corrupt_entry_with_a_typed_error() {
        let root = fresh_root("corrupt");
        let store = PluginPrivateStore::new(&root);
        store
            .write_value("org.example.one", "good", &serde_json::json!(1))
            .unwrap();
        let directory = root.join("org.example.one");
        fs::write(directory.join("broken.json"), b"not json").unwrap();

        assert!(matches!(
            store.read_value("org.example.one", "broken"),
            Err(StorageError::Corrupt { .. })
        ));
        assert_eq!(
            store.read_value("org.example.one", "good").unwrap(),
            Some(serde_json::json!(1)),
            "one corrupt entry must not affect the others"
        );
        fs::remove_dir_all(&root).ok();
    }
}
