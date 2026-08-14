//! WASM plugin workflow (design §10.3).
//!
//! The default plugin layer runs sandboxed WebAssembly with no host imports
//! unless a capability is explicitly granted. This module wires the runtime
//! into the host workflows: loading a plugin's `.wasm` entrypoint, enforcing
//! the same enable/permission gate as native plugins, and invoking commands
//! with bounded fuel/memory/payload.

use std::{fs, path::PathBuf};

use sabaki_plugin_runtime::{
    PluginError, PluginPermission, PluginRecord, PluginRuntime, WasmPluginInstance,
    WasmPluginModule,
};
use serde_json::Value;
use thiserror::Error;

/// Entrypoint extension for WASM plugins; anything else is rejected at load
/// time so a misconfigured manifest fails loudly instead of running the
/// wrong bytes.
pub const WASM_ENTRYPOINT_EXTENSION: &str = "wasm";

#[derive(Debug, Error)]
pub enum WasmWorkflowError {
    #[error("plugin {0} is not a wasm plugin")]
    NotWasm(String),
    #[error("plugin is not enabled")]
    NotEnabled,
    #[error("plugin entrypoint is not a .wasm file: {0}")]
    BadEntrypoint(PathBuf),
    #[error("plugin entrypoint could not be read: {0}")]
    Read(#[from] std::io::Error),
    #[error(transparent)]
    Runtime(#[from] sabaki_plugin_runtime::WasmError),
    #[error(transparent)]
    Plugin(#[from] PluginError),
}

impl WasmWorkflowError {
    /// Maps the workflow error onto the shared host error vocabulary so
    /// callers only ever handle one error type.
    pub fn into_plugin_error(self) -> PluginError {
        match self {
            WasmWorkflowError::NotWasm(_) => PluginError::InvalidRuntime,
            WasmWorkflowError::NotEnabled => PluginError::InvalidManifest(
                "the plugin must be enabled before invoking a command".to_owned(),
            ),
            WasmWorkflowError::BadEntrypoint(path) => PluginError::InvalidManifest(format!(
                "wasm entrypoint must end in .wasm, got {}",
                path.display()
            )),
            WasmWorkflowError::Read(error) => PluginError::ManifestRead(error),
            WasmWorkflowError::Runtime(error) => {
                PluginError::InvalidManifest(format!("wasm execution failed: {error}"))
            }
            WasmWorkflowError::Plugin(error) => error,
        }
    }
}

/// Loads and compiles the WASM module for a plugin record, validating the
/// runtime kind, enabled state and entrypoint shape.
pub fn load_wasm_module(record: &PluginRecord) -> Result<WasmPluginModule, WasmWorkflowError> {
    if !matches!(record.manifest.runtime, PluginRuntime::Wasm) {
        return Err(WasmWorkflowError::NotWasm(record.manifest.id.clone()));
    }
    if !record.enabled {
        return Err(WasmWorkflowError::NotEnabled);
    }
    let entrypoint = record.resolve_entrypoint()?;
    if entrypoint.extension().and_then(|ext| ext.to_str()) != Some(WASM_ENTRYPOINT_EXTENSION) {
        return Err(WasmWorkflowError::BadEntrypoint(entrypoint));
    }
    let bytes = fs::read(&entrypoint)?;
    WasmPluginModule::compile(&bytes).map_err(WasmWorkflowError::Runtime)
}

/// Builds the capability set for a plugin record from its granted
/// permissions (design §10.3: the host provides the minimal capability
/// imports matching the granted permissions). `game_snapshot` may be
/// supplied by the caller; it is only wired in when the plugin was granted
/// `GameRead`.
pub fn wasm_capabilities_for(
    record: &PluginRecord,
    game_snapshot: Option<&str>,
) -> sabaki_plugin_runtime::WasmCapabilities {
    let has_game_read = record
        .granted_permissions
        .contains(&PluginPermission::GameRead);
    sabaki_plugin_runtime::WasmCapabilities {
        game_snapshot: if has_game_read {
            game_snapshot.map(str::to_owned)
        } else {
            None
        },
    }
}

/// Invokes a plugin command through the sandboxed WASM instance. The request
/// DTO mirrors the native JSON-RPC shape (`method` + `params`) so plugins
/// implement one protocol regardless of layer. `game_snapshot` is exposed to
/// the plugin only when `GameRead` was granted.
pub fn invoke_wasm_command(
    record: &PluginRecord,
    module: &WasmPluginModule,
    method: &str,
    params: Value,
    game_snapshot: Option<&str>,
) -> Result<Value, WasmWorkflowError> {
    if !matches!(record.manifest.runtime, PluginRuntime::Wasm) {
        return Err(WasmWorkflowError::NotWasm(record.manifest.id.clone()));
    }
    if !record.enabled {
        return Err(WasmWorkflowError::NotEnabled);
    }
    let capabilities = wasm_capabilities_for(record, game_snapshot);
    let mut instance = WasmPluginInstance::instantiate_with_capabilities(module, &capabilities)
        .map_err(WasmWorkflowError::Runtime)?;
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
    });
    instance
        .invoke(&request)
        .map_err(WasmWorkflowError::Runtime)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sabaki_plugin_runtime::{PluginManifest, PluginPermission};
    use std::collections::BTreeSet;

    const ECHO_WAT: &str = r#"
(module
  (memory (export "memory") 1)
  (func (export "invoke") (param $ptr i32) (param $len i32) (result i32)
    local.get $len))
"#;

    fn wasm_record(install_path: PathBuf, enabled: bool) -> PluginRecord {
        PluginRecord {
            manifest: PluginManifest {
                schema_version: 1,
                id: "org.example.wasm-echo".to_owned(),
                name: "Wasm Echo".to_owned(),
                version: "1.0.0".to_owned(),
                api_version: 1,
                runtime: PluginRuntime::Wasm,
                activation_events: Vec::new(),
                permissions: BTreeSet::from([PluginPermission::GameRead]),
                contributes: Default::default(),
                entrypoint: Some("echo.wasm".to_owned()),
            },
            install_path,
            enabled,
            granted_permissions: BTreeSet::from([PluginPermission::GameRead]),
            native_execution_authorized: false,
        }
    }

    fn fresh_plugin_dir(test_name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sabaki-host-wasm-{test_name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir is created");
        dir
    }

    fn write_echo_wasm(install_path: &std::path::Path) {
        let bytes = wat::parse_str(ECHO_WAT).expect("WAT parses");
        fs::write(install_path.join("echo.wasm"), bytes).expect("wasm is written");
    }

    #[test]
    fn loads_and_invokes_a_wasm_plugin() {
        let install_path = fresh_plugin_dir("invoke");
        write_echo_wasm(&install_path);
        let record = wasm_record(install_path.clone(), true);

        let module = load_wasm_module(&record).expect("module loads");
        let result = invoke_wasm_command(
            &record,
            &module,
            "game.snapshot",
            serde_json::json!({"depth": 1}),
            None,
        )
        .expect("invocation succeeds");

        assert_eq!(result["method"], "game.snapshot");
        assert_eq!(result["params"], serde_json::json!({"depth": 1}));
        fs::remove_dir_all(&install_path).ok();
    }

    #[test]
    fn rejects_disabled_or_non_wasm_plugins() {
        let install_path = fresh_plugin_dir("disabled");
        write_echo_wasm(&install_path);

        let disabled = wasm_record(install_path.clone(), false);
        assert!(matches!(
            load_wasm_module(&disabled),
            Err(WasmWorkflowError::NotEnabled)
        ));

        let mut native = wasm_record(install_path.clone(), true);
        native.manifest.runtime = PluginRuntime::Native;
        assert!(matches!(
            load_wasm_module(&native),
            Err(WasmWorkflowError::NotWasm(_))
        ));
        fs::remove_dir_all(&install_path).ok();
    }

    #[test]
    fn rejects_entrypoints_that_are_not_wasm_files() {
        let install_path = fresh_plugin_dir("bad-entrypoint");
        let mut record = wasm_record(install_path.clone(), true);
        record.manifest.entrypoint = Some("plugin.js".to_owned());

        assert!(matches!(
            load_wasm_module(&record),
            Err(WasmWorkflowError::BadEntrypoint(_))
        ));
        fs::remove_dir_all(&install_path).ok();
    }

    #[test]
    fn capabilities_follow_granted_permissions() {
        use sabaki_plugin_runtime::WasmCapabilities;

        let install_path = fresh_plugin_dir("capabilities");
        write_echo_wasm(&install_path);
        let mut record = wasm_record(install_path.clone(), true);
        record.granted_permissions = BTreeSet::from([PluginPermission::GameRead]);

        let with_read = super::wasm_capabilities_for(&record, Some("{\"moves\":3}"));
        assert_eq!(
            with_read.game_snapshot.as_deref(),
            Some("{\"moves\":3}"),
            "GameRead must expose the game snapshot capability"
        );

        record.granted_permissions = BTreeSet::new();
        let without_read = super::wasm_capabilities_for(&record, Some("{\"moves\":3}"));
        assert_eq!(
            without_read.game_snapshot, None,
            "without GameRead the snapshot capability must stay absent"
        );
        let _ = &WasmCapabilities::default();
        fs::remove_dir_all(&install_path).ok();
    }

    #[test]
    fn error_maps_onto_the_shared_plugin_error_vocabulary() {
        let install_path = fresh_plugin_dir("error-map");
        write_echo_wasm(&install_path);
        let disabled = wasm_record(install_path, false);
        assert!(matches!(
            load_wasm_module(&disabled).map_err(WasmWorkflowError::into_plugin_error),
            Err(PluginError::InvalidManifest(_))
        ));
    }
}
