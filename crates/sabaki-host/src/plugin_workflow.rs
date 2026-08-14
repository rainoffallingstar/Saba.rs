use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use sabaki_plugin_runtime::{PluginError, PluginPermission, PluginRecord};
use serde::{Deserialize, Serialize};

/// The persisted plugin metadata that accompanies each installation. The
/// manifest itself is always re-read from the install directory on the next
/// scan, so a moved or upgraded plugin is re-derived instead of going stale.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedPluginState {
    pub install_path: PathBuf,
    pub enabled: bool,
    #[serde(default)]
    pub granted_permissions: Vec<PluginPermission>,
    #[serde(default)]
    pub native_execution_authorized: bool,
}

impl PersistedPluginState {
    fn for_record(record: &PluginRecord) -> Self {
        Self {
            install_path: record.install_path.clone(),
            enabled: record.enabled,
            granted_permissions: record.granted_permissions.iter().cloned().collect(),
            native_execution_authorized: record.native_execution_authorized,
        }
    }
}

/// Persistence boundary for the plugin registry. Implementations own the
/// storage location; the host workflows keep the record format stable.
pub trait PluginPersistence {
    fn load_plugin_states(&self) -> Result<Vec<PersistedPluginState>, String>;

    fn persist_plugin_states(&self, states: &[PersistedPluginState]) -> Result<(), String>;
}

/// UI-independent registry of installed plugins. Records are derived from the
/// install directory and overlaid with the persisted enabled/granted state.
#[derive(Clone, Debug, Default)]
pub struct PluginStore {
    records: Vec<PluginRecord>,
}

impl PluginStore {
    /// Builds a store from already-scanned records, for shells that resolve
    /// their install root themselves or inject fixtures.
    pub fn from_records(records: Vec<PluginRecord>) -> Self {
        Self { records }
    }

    /// Scans the install root and re-applies the persisted metadata. Plugins
    /// whose directories disappeared are dropped; newly added directories are
    /// kept disabled until explicitly enabled and authorized.
    pub fn restore(
        persistence: &impl PluginPersistence,
        install_root: &Path,
    ) -> Result<Self, String> {
        let states = persistence.load_plugin_states()?;
        let states_by_path: BTreeMap<PathBuf, PersistedPluginState> = states
            .into_iter()
            .map(|state| (state.install_path.clone(), state))
            .collect();
        let mut records = scan_plugin_installations(install_root)?;
        for record in &mut records {
            let Some(state) = states_by_path.get(&record.install_path) else {
                continue;
            };
            record.enabled = state.enabled;
            record.granted_permissions = state.granted_permissions.iter().cloned().collect();
            record.native_execution_authorized = state.native_execution_authorized;
        }
        Ok(Self { records })
    }

    /// Restores an explicitly installed registry from the persisted states:
    /// each state carries its install path, and the manifest is re-read from
    /// that directory. Plugins whose directories disappeared are dropped.
    pub fn restore_installed(persistence: &impl PluginPersistence) -> Result<Self, String> {
        let states = persistence.load_plugin_states()?;
        let mut records = Vec::new();
        for state in states {
            let install_path = state.install_path.clone();
            let mut record = match PluginRecord::install(
                install_path.clone(),
                state.granted_permissions.iter().cloned().collect(),
            ) {
                Ok(record) => record,
                Err(PluginError::ManifestRead(error))
                    if error.kind() == std::io::ErrorKind::NotFound =>
                {
                    continue;
                }
                Err(error) => {
                    return Err(format!(
                        "could not restore plugin at {}: {error}",
                        install_path.display()
                    ));
                }
            };
            record.enabled = state.enabled;
            record.native_execution_authorized = state.native_execution_authorized;
            records.push(record);
        }
        Ok(Self { records })
    }

    pub fn list(&self) -> &[PluginRecord] {
        &self.records
    }

    fn record_mut(&mut self, plugin_id: &str) -> Result<&mut PluginRecord, PluginError> {
        self.records
            .iter_mut()
            .find(|record| record.manifest.id == plugin_id)
            .ok_or_else(|| PluginError::InvalidManifest(format!("no plugin {plugin_id} installed")))
    }

    /// Merges the requested permissions into the granted set, keeping the
    /// record disabled unless it already was enabled.
    pub fn grant_permissions(
        &mut self,
        plugin_id: &str,
        permissions: impl IntoIterator<Item = PluginPermission>,
    ) -> Result<(), PluginError> {
        let record = self.record_mut(plugin_id)?;
        record.granted_permissions.extend(permissions.into_iter());
        Ok(())
    }

    /// Enables a plugin; permission checks run against the granted set.
    pub fn enable(&mut self, plugin_id: &str) -> Result<(), PluginError> {
        self.record_mut(plugin_id)?.enable()
    }

    pub fn disable(&mut self, plugin_id: &str) -> Result<(), PluginError> {
        self.record_mut(plugin_id)?.enabled = false;
        Ok(())
    }

    /// Authorizes native execution for a plugin whose runtime requires it.
    pub fn authorize_native(&mut self, plugin_id: &str) -> Result<(), PluginError> {
        self.record_mut(plugin_id)?.authorize_native_execution()
    }

    /// Persists the current registry state through the injected boundary.
    pub fn persist(&self, persistence: &impl PluginPersistence) -> Result<(), String> {
        let states: Vec<PersistedPluginState> = self
            .records
            .iter()
            .map(PersistedPluginState::for_record)
            .collect();
        persistence.persist_plugin_states(&states)
    }
}

/// Scans each direct child of the install root for a plugin manifest and
/// builds a disabled record with no granted permissions. Non-plugin
/// directories are skipped.
pub fn scan_plugin_installations(install_root: &Path) -> Result<Vec<PluginRecord>, String> {
    let mut install_paths = Vec::new();
    for entry in std::fs::read_dir(install_root).map_err(|error| {
        format!(
            "could not read plugin directory {}: {error}",
            install_root.display()
        )
    })? {
        let entry = entry.map_err(|error| format!("could not list plugin directory: {error}"))?;
        if entry
            .file_type()
            .map_err(|error| format!("could not inspect plugin: {error}"))?
            .is_dir()
        {
            install_paths.push(entry.path());
        }
    }
    install_paths.sort();

    let mut records = Vec::new();
    for install_path in install_paths {
        match PluginRecord::install(install_path.clone(), Default::default()) {
            Ok(record) => records.push(record),
            Err(PluginError::ManifestRead(error))
                if error.kind() == std::io::ErrorKind::NotFound =>
            {
                continue;
            }
            Err(error) => {
                return Err(format!(
                    "could not load plugin at {}: {error}",
                    install_path.display()
                ));
            }
        }
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::{PersistedPluginState, PluginPersistence, PluginStore, scan_plugin_installations};
    use sabaki_plugin_runtime::{PluginError, PluginManifest, PluginPermission, PluginRecord};
    use std::{
        cell::RefCell,
        collections::BTreeSet,
        path::{Path, PathBuf},
    };

    #[derive(Default)]
    struct MemoryPluginPersistence {
        states: RefCell<Vec<PersistedPluginState>>,
    }

    impl PluginPersistence for MemoryPluginPersistence {
        fn load_plugin_states(&self) -> Result<Vec<PersistedPluginState>, String> {
            Ok(self.states.borrow().clone())
        }

        fn persist_plugin_states(&self, states: &[PersistedPluginState]) -> Result<(), String> {
            *self.states.borrow_mut() = states.to_vec();
            Ok(())
        }
    }

    fn sample_manifest(id: &str, name: &str) -> PluginManifest {
        PluginManifest {
            schema_version: 1,
            id: id.to_owned(),
            name: name.to_owned(),
            version: "1.0.0".to_owned(),
            api_version: 1,
            runtime: sabaki_plugin_runtime::PluginRuntime::Declarative,
            activation_events: Vec::new(),
            permissions: BTreeSet::from([PluginPermission::GameRead]),
            contributes: Default::default(),
            entrypoint: None,
        }
    }

    fn write_plugin(temp_dir: &Path, id: &str, name: &str) -> PathBuf {
        let install_path = temp_dir.join(id.replace('.', "-"));
        std::fs::create_dir_all(&install_path).expect("temp plugin dir is created");
        let manifest = sample_manifest(id, name);
        std::fs::write(
            install_path.join("sabaki-plugin.json"),
            serde_json::to_vec(&manifest).expect("manifest serializes"),
        )
        .expect("manifest is written");
        install_path
    }

    fn fresh_plugin_root(test_name: &str) -> PathBuf {
        let temp_dir =
            std::env::temp_dir().join(format!("sabaki-host-{test_name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).expect("temp root is created");
        temp_dir
    }

    #[test]
    fn scanning_collects_installed_plugins_and_skips_non_plugins() {
        let temp_dir = fresh_plugin_root("scanning");
        write_plugin(&temp_dir, "org.example.one", "One");
        write_plugin(&temp_dir, "org.example.two", "Two");
        std::fs::create_dir_all(temp_dir.join("not-a-plugin")).expect("decoy dir is created");

        let records = scan_plugin_installations(&temp_dir).expect("scan succeeds");

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].manifest.id, "org.example.one");
        assert_eq!(records[1].manifest.id, "org.example.two");
        assert!(!records[0].enabled);
        assert!(records[0].granted_permissions.is_empty());
        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn store_overlays_persisted_state_onto_scanned_records() {
        let temp_dir = fresh_plugin_root("store-overlay");
        let install_path = write_plugin(&temp_dir, "org.example.one", "One");
        let persistence = MemoryPluginPersistence {
            states: RefCell::new(vec![PersistedPluginState {
                install_path: install_path.clone(),
                enabled: true,
                granted_permissions: vec![PluginPermission::GameRead],
                native_execution_authorized: false,
            }]),
        };

        let store = PluginStore::restore(&persistence, &temp_dir).expect("store restores");

        assert_eq!(store.list().len(), 1);
        assert!(store.list()[0].enabled);
        assert_eq!(
            store.list()[0].granted_permissions,
            BTreeSet::from([PluginPermission::GameRead])
        );
        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn restore_installed_rebuilds_records_from_explicit_state_paths() {
        let temp_dir = fresh_plugin_root("restore-installed");
        let install_path = write_plugin(&temp_dir, "org.example.one", "One");
        let persistence = MemoryPluginPersistence {
            states: RefCell::new(vec![PersistedPluginState {
                install_path: install_path.clone(),
                enabled: true,
                granted_permissions: vec![PluginPermission::GameRead],
                native_execution_authorized: false,
            }]),
        };

        let store = PluginStore::restore_installed(&persistence).expect("store restores");

        assert_eq!(store.list().len(), 1);
        assert_eq!(store.list()[0].manifest.id, "org.example.one");
        assert!(store.list()[0].enabled);
        assert_eq!(
            store.list()[0].granted_permissions,
            BTreeSet::from([PluginPermission::GameRead])
        );
        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn restore_installed_drops_plugins_whose_directories_disappeared() {
        let persistence = MemoryPluginPersistence {
            states: RefCell::new(vec![PersistedPluginState {
                install_path: PathBuf::from("/nowhere/plugin"),
                enabled: true,
                granted_permissions: Vec::new(),
                native_execution_authorized: false,
            }]),
        };

        let store = PluginStore::restore_installed(&persistence).expect("store restores");

        assert!(store.list().is_empty());
    }

    #[test]
    fn enable_requires_granted_permissions() {
        let mut store = PluginStore {
            records: vec![PluginRecord {
                manifest: sample_manifest("org.example.one", "One"),
                install_path: PathBuf::from("/plugins/one"),
                enabled: false,
                granted_permissions: Default::default(),
                native_execution_authorized: false,
            }],
        };

        assert!(matches!(
            store.enable("org.example.one"),
            Err(PluginError::PermissionDenied(_))
        ));
        store
            .grant_permissions("org.example.one", [PluginPermission::GameRead])
            .expect("grant succeeds");
        store.enable("org.example.one").expect("enable succeeds");
        assert!(store.list()[0].enabled);

        store.disable("org.example.one").expect("disable succeeds");
        assert!(!store.list()[0].enabled);
    }

    #[test]
    fn persist_and_restore_round_trip_enabled_state() {
        let persistence = MemoryPluginPersistence::default();
        let store = PluginStore {
            records: vec![PluginRecord {
                manifest: sample_manifest("org.example.one", "One"),
                install_path: PathBuf::from("/plugins/one"),
                enabled: true,
                granted_permissions: BTreeSet::from([PluginPermission::GameRead]),
                native_execution_authorized: true,
            }],
        };

        store.persist(&persistence).expect("persist succeeds");
        let restored_states = persistence.load_plugin_states().expect("states load");

        assert_eq!(restored_states.len(), 1);
        assert_eq!(
            restored_states[0].install_path,
            PathBuf::from("/plugins/one")
        );
        assert!(restored_states[0].enabled);
        assert_eq!(
            restored_states[0].granted_permissions,
            vec![PluginPermission::GameRead]
        );
        assert!(restored_states[0].native_execution_authorized);
    }
}
