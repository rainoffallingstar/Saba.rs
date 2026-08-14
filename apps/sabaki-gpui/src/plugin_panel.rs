use sabaki_plugin_runtime::{PluginManifest, PluginRecord};

/// A rendered summary row for an installed plugin panel. The panel renders a
/// closed set of host widgets derived from the manifest; plugins never drive
/// the UI directly.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PluginPanelEntry {
    pub plugin_id: String,
    pub name: String,
    pub version: String,
    pub enabled: bool,
    pub permissions: Vec<String>,
    pub commands: Vec<String>,
    pub command_ids: Vec<String>,
    pub menu_contributions: Vec<String>,
    /// Manifest permissions not yet granted to the plugin.
    pub missing_permissions: Vec<String>,
    /// Whether the plugin runs as a native process.
    pub native_runtime: bool,
    /// Whether native execution has been explicitly authorized by the user.
    pub native_authorized: bool,
    /// Human-readable process status for native plugins (running/crashed/
    /// disabled), absent for declarative/wasm plugins or before first start.
    pub process_status: Option<String>,
    /// Recent standard-error output of the native process, if supervised.
    pub process_logs: Vec<String>,
}

/// Extracts the UI-relevant summary from a manifest. Pure function so the
/// panel can be unit-tested without a view.
pub fn entry_from_manifest(manifest: &PluginManifest) -> PluginPanelEntry {
    PluginPanelEntry {
        plugin_id: manifest.id.clone(),
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        enabled: true,
        permissions: manifest
            .permissions
            .iter()
            .map(|permission| format!("{permission:?}"))
            .collect(),
        commands: manifest
            .contributes
            .commands
            .iter()
            .map(|command| command.title.clone())
            .collect(),
        command_ids: manifest
            .contributes
            .commands
            .iter()
            .map(|command| command.id.clone())
            .collect(),
        menu_contributions: manifest
            .contributes
            .menus
            .iter()
            .map(|menu| format!("{} → {}", menu.menu, menu.command))
            .collect(),
        missing_permissions: manifest
            .ungranted_permissions(&Default::default())
            .into_iter()
            .map(|permission| format!("{permission:?}"))
            .collect(),
        native_runtime: matches!(
            manifest.runtime,
            sabaki_plugin_runtime::PluginRuntime::Native
        ),
        native_authorized: false,
        process_status: None,
        process_logs: Vec::new(),
    }
}

/// Extracts the summary for a persisted plugin record, reflecting its enabled
/// state, granted permissions and native authorization rather than the raw
/// manifest request.
pub fn entry_from_record(record: &PluginRecord) -> PluginPanelEntry {
    let mut entry = entry_from_manifest(&record.manifest);
    entry.enabled = record.enabled;
    entry.permissions = record
        .granted_permissions
        .iter()
        .map(|permission| format!("{permission:?}"))
        .collect();
    entry.missing_permissions = record
        .manifest
        .ungranted_permissions(&record.granted_permissions)
        .into_iter()
        .map(|permission| format!("{permission:?}"))
        .collect();
    entry.native_authorized = record.native_execution_authorized;
    entry
}

/// Overlays the supervisor's process state onto the panel entry, so the
/// plugin list reflects the live native process (running/crashed/disabled)
/// with its captured stderr log.
pub fn apply_process_info(entry: &mut PluginPanelEntry, info: &sabaki_host::PluginProcessInfo) {
    if !entry.native_runtime {
        return;
    }
    let status = match info.status {
        sabaki_host::PluginProcessStatus::Running => {
            format!("running ({} restarts)", info.restart_count)
        }
        sabaki_host::PluginProcessStatus::Crashed => {
            format!("crashed ({} crashes)", info.crash_count)
        }
        sabaki_host::PluginProcessStatus::Disabled => {
            format!("auto-disabled after {} crashes", info.crash_count)
        }
        sabaki_host::PluginProcessStatus::Stopped => "stopped".to_owned(),
    };
    entry.process_status = Some(status);
    entry.process_logs = info.logs.clone();
}

/// Parses a manifest JSON document and validates it against the host rules.
#[allow(dead_code)]
pub fn parse_manifest(json: &str) -> Result<PluginManifest, String> {
    let manifest: PluginManifest =
        serde_json::from_str(json).map_err(|error| format!("invalid plugin manifest: {error}"))?;
    manifest
        .validate()
        .map_err(|error| format!("invalid plugin manifest: {error}"))?;
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::{entry_from_manifest, parse_manifest};
    use sabaki_plugin_runtime::{PluginManifest, PluginRecord};
    use std::{collections::BTreeSet, path::PathBuf};

    fn sample_manifest() -> PluginManifest {
        parse_manifest(
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
                        {"id": "org.example.opening-trainer.start", "title": "Start Training"},
                        {"id": "org.example.opening-trainer.stop", "title": "Stop Training"}
                    ],
                    "menus": [
                        {"menu": "game", "command": "org.example.opening-trainer.start"}
                    ]
                }
            }"#,
        )
        .expect("sample manifest is valid")
    }

    #[test]
    fn extracts_panel_summary_from_a_manifest() {
        let entry = entry_from_manifest(&sample_manifest());

        assert_eq!(entry.plugin_id, "org.example.opening-trainer");
        assert_eq!(entry.name, "Opening Trainer");
        assert_eq!(entry.version, "1.2.0");
        assert!(entry.enabled);
        assert_eq!(
            entry.permissions,
            vec!["GameRead".to_owned(), "UiPanel".to_owned()]
        );
        assert_eq!(
            entry.commands,
            vec!["Start Training".to_owned(), "Stop Training".to_owned()]
        );
        assert_eq!(entry.menu_contributions.len(), 1);
    }

    #[test]
    fn record_entry_reflects_enabled_state_and_granted_permissions() {
        let manifest = sample_manifest();
        let record = PluginRecord {
            manifest: manifest.clone(),
            install_path: PathBuf::from("/plugins/opening-trainer"),
            enabled: false,
            granted_permissions: BTreeSet::from([
                sabaki_plugin_runtime::PluginPermission::GameRead,
            ]),
            native_execution_authorized: false,
        };

        let entry = super::entry_from_record(&record);
        assert!(!entry.enabled);
        assert_eq!(entry.permissions, vec!["GameRead".to_owned()]);
        assert_eq!(entry.missing_permissions, vec!["UiPanel".to_owned()]);
        assert!(!entry.native_runtime);
    }

    #[test]
    fn native_plugins_report_runtime_and_authorization_state() {
        let mut manifest = sample_manifest();
        manifest.runtime = sabaki_plugin_runtime::PluginRuntime::Native;
        manifest.entrypoint = Some("bin/plugin".to_owned());
        let record = PluginRecord {
            manifest,
            install_path: PathBuf::from("/plugins/opening-trainer"),
            enabled: false,
            granted_permissions: BTreeSet::from([
                sabaki_plugin_runtime::PluginPermission::GameRead,
                sabaki_plugin_runtime::PluginPermission::UiPanel,
            ]),
            native_execution_authorized: false,
        };

        let entry = super::entry_from_record(&record);
        assert!(entry.native_runtime);
        assert!(!entry.native_authorized);
        assert!(entry.missing_permissions.is_empty());

        let mut authorized = record;
        authorized.native_execution_authorized = true;
        let entry = super::entry_from_record(&authorized);
        assert!(entry.native_authorized);
    }

    #[test]
    fn process_info_overlays_running_and_crashed_state() {
        use sabaki_host::{PluginProcessInfo, PluginProcessStatus};
        let mut manifest = sample_manifest();
        manifest.runtime = sabaki_plugin_runtime::PluginRuntime::Native;
        manifest.entrypoint = Some("bin/plugin".to_owned());
        let record = PluginRecord {
            manifest,
            install_path: PathBuf::from("/plugins/opening-trainer"),
            enabled: true,
            granted_permissions: BTreeSet::from([
                sabaki_plugin_runtime::PluginPermission::GameRead,
            ]),
            native_execution_authorized: true,
        };
        let mut entry = super::entry_from_record(&record);
        assert!(entry.process_status.is_none());

        super::apply_process_info(
            &mut entry,
            &PluginProcessInfo {
                plugin_id: "org.example.opening-trainer".to_owned(),
                status: PluginProcessStatus::Running,
                logs: vec!["ready".to_owned()],
                restart_count: 1,
                crash_count: 0,
                auto_disabled: false,
            },
        );
        assert_eq!(
            entry.process_status.as_deref(),
            Some("running (1 restarts)")
        );
        assert_eq!(entry.process_logs, vec!["ready".to_owned()]);

        super::apply_process_info(
            &mut entry,
            &PluginProcessInfo {
                plugin_id: "org.example.opening-trainer".to_owned(),
                status: PluginProcessStatus::Disabled,
                logs: Vec::new(),
                restart_count: 3,
                crash_count: 3,
                auto_disabled: true,
            },
        );
        assert_eq!(
            entry.process_status.as_deref(),
            Some("auto-disabled after 3 crashes")
        );
    }

    #[test]
    fn rejects_invalid_manifests() {
        assert!(parse_manifest(r#"{"schemaVersion":1,"id":"bad"}"#).is_err());
        assert!(parse_manifest("not json").is_err());
    }
}
