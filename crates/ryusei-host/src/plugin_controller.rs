//! Deep plugin lifecycle module.
//!
//! `PluginController` is the host-owned Module for the installed registry,
//! persistence adapter, and native-process supervision. Shells use its small
//! Interface to perform user actions and render snapshots; they never decide
//! when to create, restart, or stop a supervisor.

use std::{collections::BTreeMap, path::Path};

use ryusei_plugin_runtime::{PluginPermission, PluginRecord, PluginRuntime};

use crate::{PluginPersistence, PluginProcessInfo, PluginSupervisor, install_plugin_from_zip_file};

/// Result of a completed registry or process operation. The message is suitable
/// for a shell status line; callers do not need to recreate lifecycle detail.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PluginControllerOutcome {
    pub plugin_id: String,
    pub message: String,
}

/// Host-owned lifecycle Module for installed plugins.
///
/// Its Interface keeps the persistence and process lifecycle together:
/// mutations are persisted before success is returned, native supervisors are
/// started/stopped alongside enablement, and a native request gets at most one
/// automatic restart attempt. `P` is an Adapter at the persistence seam.
pub struct PluginController<P> {
    store: crate::PluginStore,
    persistence: P,
    supervisors: BTreeMap<String, PluginSupervisor>,
}

impl<P: PluginPersistence> PluginController<P> {
    /// Builds a controller from an already-prepared registry. This keeps a
    /// shell operational when filesystem scanning fails while retaining the
    /// same persistence and process-lifecycle Module.
    pub fn from_store(store: crate::PluginStore, persistence: P) -> Self {
        Self {
            store,
            persistence,
            supervisors: BTreeMap::new(),
        }
    }

    /// Restores the registry from the install root and persists the normalized
    /// state, matching the former shell startup behavior.
    pub fn restore(persistence: P, install_root: &Path) -> Result<Self, String> {
        let store = crate::PluginStore::restore(&persistence, install_root)?;
        store.persist(&persistence)?;
        Ok(Self {
            store,
            persistence,
            supervisors: BTreeMap::new(),
        })
    }

    /// Returns the installed plugin records for rendering or command routing.
    pub fn records(&self) -> &[PluginRecord] {
        self.store.list()
    }

    /// Returns one installed plugin record, if present.
    pub fn record(&self, plugin_id: &str) -> Option<&PluginRecord> {
        self.records()
            .iter()
            .find(|record| record.manifest.id == plugin_id)
    }

    /// Installs an archive, re-scans the registry, and removes old supervisors
    /// so they cannot outlive an upgraded installation.
    pub fn install_zip(
        &mut self,
        zip_path: &Path,
        install_root: &Path,
    ) -> Result<PluginControllerOutcome, String> {
        let destination = install_plugin_from_zip_file(zip_path, install_root)?;
        self.stop_all();
        self.store = crate::PluginStore::restore(&self.persistence, install_root)?;
        self.persist()?;
        Ok(PluginControllerOutcome {
            plugin_id: String::new(),
            message: format!("plugin installed into {}", destination.display()),
        })
    }

    /// Enables a disabled plugin or disables an enabled one. Native lifecycle
    /// follows the registry state and is hidden from the caller.
    pub fn toggle(&mut self, plugin_id: &str) -> Result<PluginControllerOutcome, String> {
        let enabled = self
            .record(plugin_id)
            .map(|record| record.enabled)
            .unwrap_or(false);
        if enabled {
            self.store
                .disable(plugin_id)
                .map_err(|error| error.to_string())?;
            self.stop(plugin_id);
        } else {
            self.store
                .enable(plugin_id)
                .map_err(|error| error.to_string())?;
            self.start_native(plugin_id)?;
        }
        self.persist()?;
        Ok(PluginControllerOutcome {
            plugin_id: plugin_id.to_owned(),
            message: format!(
                "plugin {plugin_id} {}",
                if enabled { "disabled" } else { "enabled" }
            ),
        })
    }

    /// Grants every manifest-requested permission and enables the plugin.
    pub fn grant_and_enable(&mut self, plugin_id: &str) -> Result<PluginControllerOutcome, String> {
        let permissions: Vec<PluginPermission> = self
            .record(plugin_id)
            .ok_or_else(|| format!("plugin {plugin_id} is not installed"))?
            .manifest
            .permissions
            .iter()
            .cloned()
            .collect();
        self.store
            .grant_permissions(plugin_id, permissions)
            .map_err(|error| error.to_string())?;
        self.store
            .enable(plugin_id)
            .map_err(|error| error.to_string())?;
        self.start_native(plugin_id)?;
        self.persist()?;
        Ok(PluginControllerOutcome {
            plugin_id: plugin_id.to_owned(),
            message: format!("plugin {plugin_id} granted and enabled"),
        })
    }

    /// Records explicit native-code consent, grants requested permissions, and
    /// enables the plugin in one persisted lifecycle operation.
    pub fn authorize_and_enable(
        &mut self,
        plugin_id: &str,
    ) -> Result<PluginControllerOutcome, String> {
        self.store
            .authorize_native(plugin_id)
            .map_err(|error| error.to_string())?;
        self.grant_and_enable(plugin_id)
            .map(|_| PluginControllerOutcome {
                plugin_id: plugin_id.to_owned(),
                message: format!("plugin {plugin_id} authorized and enabled"),
            })
    }

    /// Dispatches one native JSON-RPC command, starting a supervisor lazily and
    /// trying exactly one restart after a process exit.
    pub fn dispatch_native(
        &mut self,
        plugin_id: &str,
        command_id: &str,
    ) -> Result<PluginControllerOutcome, String> {
        if !self.supervisors.contains_key(plugin_id) {
            self.start_native(plugin_id)?;
        }
        let request = serde_json::json!({"command": command_id});
        let supervisor = self
            .supervisors
            .get_mut(plugin_id)
            .ok_or_else(|| format!("plugin {plugin_id} process unavailable"))?;
        match supervisor.request(command_id, request.clone()) {
            Ok(result) => Ok(PluginControllerOutcome {
                plugin_id: plugin_id.to_owned(),
                message: format!("plugin {plugin_id} command {command_id} → {result}"),
            }),
            Err(ryusei_plugin_runtime::PluginError::ProcessExited { .. }) => {
                supervisor.restart().map_err(|error| error.to_string())?;
                let result = supervisor
                    .request(command_id, request)
                    .map_err(|error| error.to_string())?;
                Ok(PluginControllerOutcome {
                    plugin_id: plugin_id.to_owned(),
                    message: format!(
                        "plugin {plugin_id} restarted; command {command_id} → {result}"
                    ),
                })
            }
            Err(error) => Err(error.to_string()),
        }
    }

    /// Polls native processes and returns their current snapshots for rendering.
    pub fn process_infos(&mut self) -> Vec<PluginProcessInfo> {
        for supervisor in self.supervisors.values_mut() {
            supervisor.poll();
        }
        self.supervisors
            .values()
            .map(PluginSupervisor::info)
            .collect()
    }

    /// Stops all live native processes. Intended for shell shutdown and plugin
    /// rescan; process state is runtime-only and is not persisted.
    pub fn stop_all(&mut self) {
        for supervisor in self.supervisors.values_mut() {
            supervisor.stop();
        }
        self.supervisors.clear();
    }

    fn persist(&self) -> Result<(), String> {
        self.store.persist(&self.persistence)
    }

    fn stop(&mut self, plugin_id: &str) {
        if let Some(mut supervisor) = self.supervisors.remove(plugin_id) {
            supervisor.stop();
        }
    }

    fn start_native(&mut self, plugin_id: &str) -> Result<(), String> {
        let record = self
            .record(plugin_id)
            .ok_or_else(|| format!("plugin {plugin_id} is not installed"))?;
        if !matches!(record.manifest.runtime, PluginRuntime::Native) {
            return Ok(());
        }
        let mut supervisor = PluginSupervisor::new(plugin_id);
        supervisor
            .start(record)
            .map_err(|error| error.to_string())?;
        self.supervisors.insert(plugin_id.to_owned(), supervisor);
        Ok(())
    }
}

impl<P> Drop for PluginController<P> {
    fn drop(&mut self) {
        for supervisor in self.supervisors.values_mut() {
            supervisor.stop();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, collections::BTreeSet, path::PathBuf};

    use ryusei_plugin_runtime::{PluginManifest, PluginPermission, PluginRecord, PluginRuntime};

    use super::PluginController;
    use crate::{PersistedPluginState, PluginPersistence, PluginStore};

    #[derive(Default)]
    struct MemoryPersistence {
        states: RefCell<Vec<PersistedPluginState>>,
    }

    impl PluginPersistence for MemoryPersistence {
        fn load_plugin_states(&self) -> Result<Vec<PersistedPluginState>, String> {
            Ok(self.states.borrow().clone())
        }

        fn persist_plugin_states(&self, states: &[PersistedPluginState]) -> Result<(), String> {
            *self.states.borrow_mut() = states.to_vec();
            Ok(())
        }
    }

    fn declarative_record() -> PluginRecord {
        PluginRecord {
            manifest: PluginManifest {
                schema_version: 1,
                id: "org.example.fixture".to_owned(),
                name: "Fixture".to_owned(),
                version: "1.0.0".to_owned(),
                api_version: 1,
                runtime: PluginRuntime::Declarative,
                activation_events: Vec::new(),
                permissions: BTreeSet::from([PluginPermission::GameRead]),
                contributes: Default::default(),
                entrypoint: None,
            },
            install_path: PathBuf::from("/fixture"),
            enabled: false,
            granted_permissions: BTreeSet::new(),
            native_execution_authorized: false,
        }
    }

    #[test]
    fn controller_persists_toggle_and_permission_lifecycle() {
        let persistence = MemoryPersistence::default();
        let mut controller = PluginController {
            store: PluginStore::from_records(vec![declarative_record()]),
            persistence,
            supervisors: Default::default(),
        };

        let denied = controller.toggle("org.example.fixture");
        assert!(denied.is_err());
        let granted = controller
            .grant_and_enable("org.example.fixture")
            .expect("permissions enable plugin");
        assert_eq!(
            granted.message,
            "plugin org.example.fixture granted and enabled"
        );
        assert!(
            controller
                .record("org.example.fixture")
                .expect("record")
                .enabled
        );

        let disabled = controller
            .toggle("org.example.fixture")
            .expect("disables plugin");
        assert_eq!(disabled.message, "plugin org.example.fixture disabled");
        assert!(
            !controller
                .record("org.example.fixture")
                .expect("record")
                .enabled
        );
        assert_eq!(controller.persistence.states.borrow().len(), 1);
    }
}
