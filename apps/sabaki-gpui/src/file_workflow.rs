use std::{cell::RefCell, path::PathBuf};

use sabaki_host::{
    AutosaveCandidate, AutosaveInfo, AutosaveStore, HostApplication, HostPersistence,
    PersistedPluginState, PluginPersistence, RecentFilesStore, SettingsPersistence,
    record_recent_file, synchronize_autosave,
};

/// An in-memory `HostPersistence` so the shell can exercise the autosave and
/// recent-files workflows without touching a real app config directory. The
/// production adapter already lives in the Tauri side; the GPUI shell will
/// reuse the same trait when it gains a config directory.
#[derive(Clone, Debug, Default)]
#[allow(dead_code)]
pub struct MemoryHostPersistence {
    autosave: RefCell<AutosaveStore>,
    recent_files: RefCell<RecentFilesStore>,
}

impl MemoryHostPersistence {
    #[allow(dead_code)]
    pub fn recent_display_names(&self) -> Vec<String> {
        self.recent_files
            .borrow()
            .list()
            .into_iter()
            .map(|entry| entry.display_name)
            .collect()
    }
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

/// Records the just-opened/saved path in the recent-files store. On persistence
/// failure the in-memory store is rolled back by the host helper.
pub fn record_opened_file(
    persistence: &impl HostPersistence,
    store: &mut RecentFilesStore,
    path: PathBuf,
) -> Result<(), String> {
    record_recent_file(persistence, store, path)
}

/// Snapshots the current document as a crash-recovery candidate when it is
/// dirty. Returns the current autosave info so the shell can surface it.
pub fn capture_autosave(
    persistence: &impl HostPersistence,
    store: &mut AutosaveStore,
    host: &HostApplication,
    source_display_name: Option<&str>,
) -> Result<AutosaveInfo, String> {
    let snapshot = host.snapshot();
    let candidate = AutosaveCandidate {
        sgf: host.to_sgf(),
        revision: snapshot.revision,
        source_display_name: source_display_name.map(ToOwned::to_owned),
    };
    store.set_recovery_pending(true);
    synchronize_autosave(persistence, store, Some(candidate))
        .map(|info| info.expect("a candidate always reports info"))
}

/// Clears any crash recovery after an explicit clean save.
pub fn clear_autosave(
    persistence: &impl HostPersistence,
    store: &mut AutosaveStore,
) -> Result<(), String> {
    synchronize_autosave(persistence, store, None).map(|_| ())
}

/// An in-memory `SettingsPersistence` so tests can round-trip settings without
/// touching a config directory.
#[derive(Clone, Debug, Default)]
#[allow(dead_code)]
pub struct MemorySettingsPersistence {
    settings_json: RefCell<Option<String>>,
    styles_css: RefCell<String>,
}

impl SettingsPersistence for MemorySettingsPersistence {
    fn load_settings(&self) -> Result<Option<String>, String> {
        Ok(self.settings_json.borrow().clone())
    }

    fn load_styles(&self) -> String {
        self.styles_css.borrow().clone()
    }

    fn persist_settings(&mut self, settings_json: &str, styles_css: &str) -> Result<(), String> {
        *self.settings_json.borrow_mut() = Some(settings_json.to_owned());
        *self.styles_css.borrow_mut() = styles_css.to_owned();
        Ok(())
    }
}

/// An in-memory `PluginPersistence` so the plugin panel can round-trip the
/// registry through the host workflows without a config directory.
#[derive(Clone, Debug, Default)]
pub struct MemoryPluginPersistence {
    states: RefCell<Vec<PersistedPluginState>>,
}

impl MemoryPluginPersistence {
    /// Exposes the persisted states for tests and diagnostics.
    #[allow(dead_code)]
    pub fn stored_states(&self) -> Vec<PersistedPluginState> {
        self.states.borrow().clone()
    }
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

fn select_config_directory(config_root: PathBuf) -> PathBuf {
    let primary = config_root.join("saba-rs");
    let legacy = config_root.join("sabaki-gpui");
    if !primary.exists() && legacy.exists() {
        legacy
    } else {
        primary
    }
}

/// Resolves the config directory from `SABAKI_CONFIG_DIR` when set. New
/// installations use `$HOME/.config/saba-rs`; an existing `sabaki-gpui`
/// directory remains authoritative so users do not silently lose settings.
pub fn current_user_config_directory() -> Result<PathBuf, String> {
    let config_directory = match std::env::var("SABAKI_CONFIG_DIR") {
        Ok(path) => PathBuf::from(path),
        Err(_) => {
            let home = std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))
                .map_err(|_| "neither SABAKI_CONFIG_DIR, HOME nor USERPROFILE is set".to_owned())?;
            select_config_directory(PathBuf::from(home).join(".config"))
        }
    };
    std::fs::create_dir_all(&config_directory)
        .map_err(|error| format!("could not create config directory: {error}"))?;
    Ok(config_directory)
}

/// Seeds built-in official plugins into the plugins directory on first launch.
pub fn seed_builtin_plugins(install_root: &std::path::Path) {
    let katago_hub_dir = install_root.join("katago-setup-hub");
    let _ = std::fs::create_dir_all(&katago_hub_dir);
    let _ = std::fs::write(
        katago_hub_dir.join("sabaki-plugin.json"),
        include_str!("../../../examples/plugins/katago-setup-hub/sabaki-plugin.json"),
    );

    let fox_kifu_dir = install_root.join("fox-kifu-sync");
    let _ = std::fs::create_dir_all(&fox_kifu_dir);
    let _ = std::fs::write(
        fox_kifu_dir.join("sabaki-plugin.json"),
        include_str!("../../../examples/plugins/fox-kifu-sync/sabaki-plugin.json"),
    );

    let pos_checker_dir = install_root.join("position-checker");
    let _ = std::fs::create_dir_all(&pos_checker_dir);
    let _ = std::fs::write(
        pos_checker_dir.join("sabaki-plugin.json"),
        include_str!("../../../examples/plugins/position-checker/sabaki-plugin.json"),
    );

    let sgf_exp_dir = install_root.join("sgf-exporter");
    let _ = std::fs::create_dir_all(&sgf_exp_dir);
    let _ = std::fs::write(
        sgf_exp_dir.join("sabaki-plugin.json"),
        include_str!("../../../examples/plugins/sgf-exporter/sabaki-plugin.json"),
    );
}

/// Resolves the plugin install root (`<config directory>/plugins`), creating
/// it on demand so the host scan always sees a readable directory.
pub fn plugin_install_root() -> Result<PathBuf, String> {
    let install_root = current_user_config_directory()?.join("plugins");
    std::fs::create_dir_all(&install_root)
        .map_err(|error| format!("could not create plugin directory: {error}"))?;
    seed_builtin_plugins(&install_root);
    Ok(install_root)
}

/// Resolves the theme root (`<config directory>/themes`), creating it on
/// demand. Themes live next to plugins so both are host-owned and never
/// writable by plugin code.
pub fn theme_root() -> Result<PathBuf, String> {
    let theme_root = current_user_config_directory()?.join("themes");
    std::fs::create_dir_all(&theme_root)
        .map_err(|error| format!("could not create theme directory: {error}"))?;
    Ok(theme_root)
}

/// Writes `content` to `path` through a temporary sibling file + rename so a
/// crash never leaves a half-written config file.
pub(crate) fn write_file_atomically(path: &std::path::Path, content: &str) -> Result<(), String> {
    write_bytes_atomically(path, content.as_bytes())
}

/// Writes raw bytes to `path` atomically: the parent directory is created on
/// demand, the bytes land in a unique temporary sibling file and are renamed
/// over the destination, and a failed write never leaves a partial file.
pub(crate) fn write_bytes_atomically(path: &std::path::Path, content: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "config path has no parent directory".to_owned())?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("could not create config directory: {error}"))?;
    let temporary_path = parent.join(format!(
        ".{}.tmp-{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("file"),
        std::process::id()
    ));
    std::fs::write(&temporary_path, content)
        .map_err(|error| format!("could not write config file: {error}"))?;
    std::fs::rename(&temporary_path, path)
        .map_err(|error| format!("could not replace config file: {error}"))?;
    Ok(())
}

/// File-system settings boundary for the real client: reads and writes
/// `settings.json` plus `styles.css` inside the owning config directory.
#[derive(Clone, Debug)]
pub struct NativeSettingsPersistence {
    config_directory: PathBuf,
}
impl NativeSettingsPersistence {
    pub fn new(config_directory: PathBuf) -> Self {
        Self { config_directory }
    }

    /// Resolves the config directory for the current user and returns a
    /// settings boundary backed by it.
    pub fn for_current_user() -> Result<Self, String> {
        Ok(Self::new(current_user_config_directory()?))
    }

    fn settings_path(&self) -> PathBuf {
        self.config_directory.join("settings.json")
    }

    fn styles_path(&self) -> PathBuf {
        self.config_directory.join("styles.css")
    }
}

impl SettingsPersistence for NativeSettingsPersistence {
    fn load_settings(&self) -> Result<Option<String>, String> {
        let settings_path = self.settings_path();
        if !settings_path.exists() {
            return Ok(None);
        }
        std::fs::read_to_string(&settings_path)
            .map(Some)
            .map_err(|error| format!("could not read persisted settings: {error}"))
    }

    fn load_styles(&self) -> String {
        std::fs::read_to_string(self.styles_path()).unwrap_or_default()
    }

    fn persist_settings(&mut self, settings_json: &str, styles_css: &str) -> Result<(), String> {
        write_file_atomically(&self.settings_path(), settings_json)?;
        write_file_atomically(&self.styles_path(), styles_css)?;
        Ok(())
    }
}

/// File-system `HostPersistence` for the real client: writes `recovery.json`
/// and `recent-files.json` inside the owning config directory, mirroring the
/// shared layout so a config directory can move between the reference and this
/// client.
#[derive(Clone, Debug)]
pub struct NativeHostPersistence {
    config_directory: PathBuf,
}

impl NativeHostPersistence {
    pub fn new(config_directory: PathBuf) -> Self {
        Self { config_directory }
    }

    /// Resolves the config directory for the current user and returns a host
    /// persistence boundary backed by it.
    pub fn for_current_user() -> Result<Self, String> {
        Ok(Self::new(current_user_config_directory()?))
    }

    fn recovery_path(&self) -> PathBuf {
        self.config_directory.join("recovery.json")
    }

    fn recent_files_path(&self) -> PathBuf {
        self.config_directory.join("recent-files.json")
    }
}

impl HostPersistence for NativeHostPersistence {
    fn load_autosave(&self) -> AutosaveStore {
        let Ok(content) = std::fs::read(self.recovery_path()) else {
            return AutosaveStore::default();
        };
        let Ok(recovery) =
            serde_json::from_slice::<sabaki_host::autosave::PersistedAutosave>(&content)
        else {
            return AutosaveStore::default();
        };
        if recovery.schema_version != sabaki_host::autosave::AUTOSAVE_SCHEMA_VERSION
            || recovery.sgf.trim().is_empty()
        {
            return AutosaveStore::default();
        }
        AutosaveStore::from_persisted(recovery)
    }

    fn persist_autosave(&self, store: &AutosaveStore) -> Result<(), String> {
        let recovery = store
            .persisted()
            .ok_or_else(|| "there is no autosave recovery to persist".to_owned())?;
        let content = serde_json::to_vec_pretty(recovery)
            .map_err(|error| format!("could not serialize autosave recovery: {error}"))?;
        write_file_atomically(&self.recovery_path(), &String::from_utf8_lossy(&content))
    }

    fn clear_autosave(&self) -> Result<(), String> {
        match std::fs::remove_file(self.recovery_path()) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("could not clear autosave recovery: {error}")),
        }
    }

    fn load_recent_files(&self) -> Result<RecentFilesStore, String> {
        let recent_files_path = self.recent_files_path();
        if !recent_files_path.exists() {
            return Ok(RecentFilesStore::default());
        }
        let content = std::fs::read(&recent_files_path)
            .map_err(|error| format!("could not read recent files: {error}"))?;
        let persisted = sabaki_host::recent_files::persisted_recent_files_from_bytes(&content)
            .map_err(|error| format!("could not parse recent files: {error}"))?;
        if persisted.schema_version != sabaki_host::recent_files::RECENT_FILES_SCHEMA_VERSION {
            return Err(format!(
                "unsupported recent-files schema version {}",
                persisted.schema_version
            ));
        }
        Ok(RecentFilesStore::from_persisted(persisted))
    }

    fn persist_recent_files(&self, store: &RecentFilesStore) -> Result<(), String> {
        let content =
            sabaki_host::recent_files::persisted_recent_files_to_bytes(&store.to_persisted())
                .map_err(|error| format!("could not serialize recent files: {error}"))?;
        write_file_atomically(
            &self.recent_files_path(),
            &String::from_utf8_lossy(&content),
        )
    }
}

/// File-system `PluginPersistence` for the real client: writes `plugins.json`
/// inside the owning config directory, mirroring the shared layout so the
/// registry can move between the reference and this client.
#[derive(Clone, Debug)]
pub struct NativePluginPersistence {
    config_directory: PathBuf,
}

impl NativePluginPersistence {
    pub fn new(config_directory: PathBuf) -> Self {
        Self { config_directory }
    }

    /// Resolves the config directory for the current user and returns a plugin
    /// persistence boundary backed by it.
    pub fn for_current_user() -> Result<Self, String> {
        Ok(Self::new(current_user_config_directory()?))
    }

    fn plugin_states_path(&self) -> PathBuf {
        self.config_directory.join("plugins.json")
    }
}

impl PluginPersistence for NativePluginPersistence {
    fn load_plugin_states(&self) -> Result<Vec<PersistedPluginState>, String> {
        let states_path = self.plugin_states_path();
        if !states_path.exists() {
            return Ok(Vec::new());
        }
        let content = std::fs::read(&states_path)
            .map_err(|error| format!("could not read plugin states: {error}"))?;
        serde_json::from_slice(&content)
            .map_err(|error| format!("could not parse plugin states: {error}"))
    }

    fn persist_plugin_states(&self, states: &[PersistedPluginState]) -> Result<(), String> {
        let content = serde_json::to_vec_pretty(states)
            .map_err(|error| format!("could not serialize plugin states: {error}"))?;
        write_file_atomically(
            &self.plugin_states_path(),
            &String::from_utf8_lossy(&content),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MemoryHostPersistence, MemoryPluginPersistence, MemorySettingsPersistence,
        NativeHostPersistence, NativePluginPersistence, NativeSettingsPersistence,
        capture_autosave, clear_autosave, record_opened_file, select_config_directory,
    };
    use sabaki_host::{
        AutosaveStore, HostApplication, HostEventSink, HostPersistence, PluginPersistence,
        PluginStore, RecentFilesStore, SettingsStore, load_settings_store, persist_settings_store,
    };
    use sabaki_plugin_runtime::PluginPermission;
    use std::path::PathBuf;

    fn fresh_config_directory(test_name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!("sabaki-gpui-{test_name}"));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("test config directory is created");
        directory
    }

    #[test]
    fn opening_a_file_records_it_in_recent_files() {
        let persistence = MemoryHostPersistence::default();
        let mut store = RecentFilesStore::default();

        record_opened_file(
            &persistence,
            &mut store,
            PathBuf::from("/games/opening.sgf"),
        )
        .expect("recording a recent file succeeds");

        let names = persistence.recent_display_names();
        assert_eq!(names, vec!["opening.sgf".to_owned()]);
    }

    #[test]
    fn dirty_document_captures_a_recovery_and_clean_save_clears_it() {
        let persistence = MemoryHostPersistence::default();
        let mut autosave = AutosaveStore::default();
        let mut host = HostApplication::default();
        let mut events = TestEventSink;
        host.play_move(
            sabaki_domain_core::Color::Black,
            Some(sabaki_domain_core::Vertex { column: 3, row: 3 }),
            &mut events,
        )
        .unwrap();

        let info = capture_autosave(&persistence, &mut autosave, &host, Some("opening.sgf"))
            .expect("a dirty document must capture recovery");

        assert!(info.is_available);
        assert_eq!(info.source_display_name.as_deref(), Some("opening.sgf"));
        assert!(persistence.load_autosave().has_recovery());

        clear_autosave(&persistence, &mut autosave).expect("clean save clears recovery");
        assert!(!persistence.load_autosave().has_recovery());
    }

    #[test]
    fn theme_persists_through_the_settings_boundary() {
        let mut store = SettingsStore::default();
        let mut persistence = MemorySettingsPersistence::default();

        store
            .set("theme.current", serde_json::json!("dark"))
            .expect("theme writes are validated");
        persist_settings_store(&store, &mut persistence).expect("theme persists");

        let reloaded = load_settings_store(&persistence).expect("reload succeeds");
        assert_eq!(reloaded.store.get_str("theme.current"), Some("dark"));
    }

    #[test]
    fn plugin_registry_persists_through_the_boundary() {
        let persistence = MemoryPluginPersistence::default();
        let mut store = PluginStore::from_records(vec![sabaki_plugin_runtime::PluginRecord {
            manifest: crate::plugin_panel::parse_manifest(
                r#"{
                    "schemaVersion": 1,
                    "id": "org.example.opening-trainer",
                    "name": "Opening Trainer",
                    "version": "1.2.0",
                    "apiVersion": 1,
                    "runtime": "declarative",
                    "permissions": ["gameRead", "uiPanel"]
                }"#,
            )
            .expect("sample manifest is valid"),
            install_path: PathBuf::from("/plugins/opening-trainer"),
            enabled: false,
            granted_permissions: Default::default(),
            native_execution_authorized: false,
        }]);

        store
            .grant_permissions(
                "org.example.opening-trainer",
                [PluginPermission::GameRead, PluginPermission::UiPanel],
            )
            .expect("permissions are granted");
        store
            .enable("org.example.opening-trainer")
            .expect("plugin enables");
        store.persist(&persistence).expect("registry persists");

        let stored = persistence.stored_states();
        assert_eq!(stored.len(), 1);
        assert!(stored[0].enabled);
        assert_eq!(stored[0].granted_permissions.len(), 2);
    }

    #[test]
    fn config_directory_selection_preserves_legacy_data_until_migrated() {
        let root = fresh_config_directory("config-migration");
        assert_eq!(select_config_directory(root.clone()), root.join("saba-rs"));

        std::fs::create_dir_all(root.join("sabaki-gpui")).expect("legacy config exists");
        assert_eq!(
            select_config_directory(root.clone()),
            root.join("sabaki-gpui"),
            "an existing legacy directory is used until a new primary directory exists"
        );

        std::fs::create_dir_all(root.join("saba-rs")).expect("new config exists");
        assert_eq!(select_config_directory(root.clone()), root.join("saba-rs"));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn native_settings_persist_and_reload_through_the_filesystem() {
        let directory = fresh_config_directory("native-settings");
        let mut persistence = NativeSettingsPersistence::new(directory.clone());
        let mut store = SettingsStore::default();
        store
            .set("theme.current", serde_json::json!("mist"))
            .expect("theme writes are validated");
        store.set_user_styles("body { color: red; }".to_owned());

        persist_settings_store(&store, &mut persistence).expect("settings persist");

        assert!(directory.join("settings.json").exists());
        assert!(directory.join("styles.css").exists());
        assert!(
            std::fs::read_dir(&directory)
                .expect("config directory is readable")
                .filter_map(Result::ok)
                .all(|entry| !entry.file_name().to_string_lossy().contains(".tmp-"))
        );

        let reloaded = NativeSettingsPersistence::new(directory.clone());
        let loaded = load_settings_store(&reloaded).expect("settings reload");
        assert_eq!(loaded.store.get_str("theme.current"), Some("mist"));
        assert_eq!(loaded.store.user_styles(), "body { color: red; }");

        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn native_settings_absent_files_load_empty() {
        let directory = fresh_config_directory("native-settings-absent");
        let persistence = NativeSettingsPersistence::new(directory.clone());

        let loaded = load_settings_store(&persistence).expect("absence is not an error");
        assert!(loaded.store.is_empty());
        assert_eq!(loaded.store.user_styles(), "");

        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn native_host_persistence_round_trips_recovery_and_recent_files() {
        let directory = fresh_config_directory("native-host-persistence");
        let persistence = NativeHostPersistence::new(directory.clone());
        let mut host = HostApplication::default();
        let mut events = TestEventSink;
        host.play_move(
            sabaki_domain_core::Color::Black,
            Some(sabaki_domain_core::Vertex { column: 3, row: 3 }),
            &mut events,
        )
        .expect("a setup move is legal");

        let mut autosave = AutosaveStore::default();
        capture_autosave(&persistence, &mut autosave, &host, Some("opening.sgf"))
            .expect("recovery captures");
        assert!(directory.join("recovery.json").exists());

        let mut recent_files = RecentFilesStore::default();
        record_opened_file(
            &persistence,
            &mut recent_files,
            directory.join("opening.sgf"),
        )
        .expect("recent file records");
        assert!(directory.join("recent-files.json").exists());

        let reloaded_persistence = NativeHostPersistence::new(directory.clone());
        assert!(reloaded_persistence.load_autosave().info().is_available);
        assert_eq!(
            reloaded_persistence
                .load_recent_files()
                .expect("recent files reload")
                .list()
                .len(),
            1
        );

        clear_autosave(&reloaded_persistence, &mut autosave).expect("recovery clears");
        assert!(!reloaded_persistence.load_autosave().info().is_available);

        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn native_host_persistence_absent_files_load_empty() {
        let directory = fresh_config_directory("native-host-absent");
        let persistence = NativeHostPersistence::new(directory.clone());

        assert!(!persistence.load_autosave().info().is_available);
        assert!(
            persistence
                .load_recent_files()
                .expect("absence is not an error")
                .list()
                .is_empty()
        );

        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn native_plugin_persistence_round_trips_registry_states() {
        let directory = fresh_config_directory("native-plugin-persistence");
        let persistence = NativePluginPersistence::new(directory.clone());

        assert!(
            persistence
                .load_plugin_states()
                .expect("a missing state file loads empty")
                .is_empty()
        );

        let install_path = directory.join("plugins").join("opening-trainer");
        std::fs::create_dir_all(&install_path).expect("plugin dir is created");
        std::fs::write(
            install_path.join("sabaki-plugin.json"),
            r#"{
                "schemaVersion": 1,
                "id": "org.example.opening-trainer",
                "name": "Opening Trainer",
                "version": "1.2.0",
                "apiVersion": 1,
                "runtime": "declarative",
                "permissions": ["gameRead", "uiPanel"]
            }"#,
        )
        .expect("manifest is written");

        let mut store = PluginStore::from_records(vec![sabaki_plugin_runtime::PluginRecord {
            manifest: crate::plugin_panel::parse_manifest(
                r#"{
                    "schemaVersion": 1,
                    "id": "org.example.opening-trainer",
                    "name": "Opening Trainer",
                    "version": "1.2.0",
                    "apiVersion": 1,
                    "runtime": "declarative",
                    "permissions": ["gameRead", "uiPanel"]
                }"#,
            )
            .expect("sample manifest is valid"),
            install_path: install_path.clone(),
            enabled: false,
            granted_permissions: Default::default(),
            native_execution_authorized: false,
        }]);
        store
            .grant_permissions(
                "org.example.opening-trainer",
                [PluginPermission::GameRead, PluginPermission::UiPanel],
            )
            .expect("permissions are granted");
        store
            .enable("org.example.opening-trainer")
            .expect("plugin enables");
        store.persist(&persistence).expect("registry persists");

        assert!(directory.join("plugins.json").exists());

        let reloaded_persistence = NativePluginPersistence::new(directory.clone());
        let reloaded = PluginStore::restore_installed(&reloaded_persistence)
            .expect("registry restores from disk");
        assert_eq!(reloaded.list().len(), 1);
        assert!(reloaded.list()[0].enabled);
        assert_eq!(reloaded.list()[0].granted_permissions.len(), 2);

        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn scan_grant_enable_persist_restore_round_trips_the_full_panel_flow() {
        let directory = fresh_config_directory("plugin-panel-flow");
        let install_root = directory.join("plugins");
        let install_path = install_root.join("opening-trainer");
        std::fs::create_dir_all(&install_path).expect("plugin dir is created");
        std::fs::write(
            install_path.join("sabaki-plugin.json"),
            r#"{
                "schemaVersion": 1,
                "id": "org.example.opening-trainer",
                "name": "Opening Trainer",
                "version": "1.2.0",
                "apiVersion": 1,
                "runtime": "declarative",
                "permissions": ["gameRead", "uiPanel"],
                "contributes": {
                    "commands": [
                        {"id": "org.example.opening-trainer.start", "title": "Start Training"}
                    ]
                }
            }"#,
        )
        .expect("manifest is written");
        let persistence = NativePluginPersistence::new(directory.clone());

        // Scan: the plugin is discovered but disabled with no permissions.
        let mut store = PluginStore::restore(&persistence, &install_root).expect("scan succeeds");
        assert_eq!(store.list().len(), 1);
        assert!(!store.list()[0].enabled);
        assert!(store.list()[0].granted_permissions.is_empty());
        assert!(
            !store.list()[0]
                .manifest
                .ungranted_permissions(&store.list()[0].granted_permissions)
                .is_empty()
        );

        // Grant + enable + persist, as the panel's "grant & enable" button does.
        store
            .grant_permissions(
                "org.example.opening-trainer",
                [PluginPermission::GameRead, PluginPermission::UiPanel],
            )
            .expect("permissions are granted");
        store
            .enable("org.example.opening-trainer")
            .expect("plugin enables");
        store.persist(&persistence).expect("registry persists");

        // Restore: a fresh scan over the same directory + state file keeps the
        // plugin enabled with its granted permissions (startup path).
        let restarted =
            PluginStore::restore(&persistence, &install_root).expect("restore succeeds");
        assert_eq!(restarted.list().len(), 1);
        assert!(restarted.list()[0].enabled);
        assert_eq!(restarted.list()[0].granted_permissions.len(), 2);
        assert!(
            restarted.list()[0]
                .manifest
                .ungranted_permissions(&restarted.list()[0].granted_permissions)
                .is_empty()
        );

        std::fs::remove_dir_all(&directory).ok();
    }

    #[derive(Default)]
    struct TestEventSink;

    impl HostEventSink for TestEventSink {
        fn emit(&mut self, _event: sabaki_host::HostEvent) {}
    }
}
