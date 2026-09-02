//! Plugin process supervision (design §10.4).
//!
//! The supervisor owns the lifecycle of native plugin processes: start,
//! request/response with timeout, crash detection, bounded restarts, and the
//! auto-disable policy ("每次崩溃记录诊断并自动禁用,避免无限重启"). The
//! supervised process mechanics live in `ryusei-plugin-runtime`; this module
//! adds the host-side policy: when a process keeps crashing, the supervisor
//! records the diagnostics and disables the plugin record.

use std::{path::PathBuf, time::Duration};

use ryusei_plugin_runtime::{
    PluginError, PluginRecord, ProcessSpawnSpec, SupervisedNativePluginProcess,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Default request timeout for a single plugin RPC call.
pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Number of crash restarts tolerated before the plugin is auto-disabled.
pub const AUTO_DISABLE_AFTER_CRASHES: u32 = 3;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PluginProcessStatus {
    Stopped,
    Running,
    Crashed,
    Disabled,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginProcessInfo {
    pub plugin_id: String,
    pub status: PluginProcessStatus,
    /// Recent standard-error output, oldest first.
    pub logs: Vec<String>,
    /// Total number of restarts performed for this plugin.
    pub restart_count: u32,
    /// Total number of crashes observed for this plugin.
    pub crash_count: u32,
    /// Whether automatic restarts are exhausted and the host should disable.
    pub auto_disabled: bool,
}

/// Host-side supervision policy over one native plugin process. Not `Clone`:
/// it owns the live child process.
pub struct PluginSupervisor {
    plugin_id: String,
    process: Option<SupervisedNativePluginProcess>,
    crash_count: u32,
    auto_disabled: bool,
    logs: Vec<String>,
    process_start_error: Option<String>,
}

impl PluginSupervisor {
    pub fn new(plugin_id: impl Into<String>) -> Self {
        Self {
            plugin_id: plugin_id.into(),
            process: None,
            crash_count: 0,
            auto_disabled: false,
            logs: Vec::new(),
            process_start_error: None,
        }
    }

    pub fn plugin_id(&self) -> &str {
        &self.plugin_id
    }

    pub fn is_running(&self) -> bool {
        self.process.is_some()
    }

    /// Starts the plugin process from its record. Fails when the plugin is
    /// disabled, unauthorized, or when the process cannot be spawned.
    pub fn start(&mut self, record: &PluginRecord) -> Result<(), PluginError> {
        if self.auto_disabled {
            return Err(PluginError::RestartLimitReached {
                count: self.crash_count,
            });
        }
        let spec = ProcessSpawnSpec::for_record(record)?;
        let process = SupervisedNativePluginProcess::start(spec)?;
        self.process = Some(process);
        self.process_start_error = None;
        Ok(())
    }

    /// Sends a JSON-RPC request and waits for its response with the default
    /// timeout. Any crash surfaced while waiting is recorded and the process
    /// handle is dropped; callers can restart or inspect `info()`.
    pub fn request(&mut self, method: &str, params: Value) -> Result<Value, PluginError> {
        let Some(process) = self.process.as_mut() else {
            return Err(PluginError::MissingEntrypoint);
        };
        let request_id = process.send_request(method, params)?;
        match process.await_response(request_id, DEFAULT_REQUEST_TIMEOUT) {
            Ok(value) => Ok(value),
            Err(PluginError::ProcessExited { status }) => {
                let diagnostic = format!("process crashed: {status}");
                self.record_crash(diagnostic);
                Err(PluginError::ProcessExited { status })
            }
            Err(error) => Err(error),
        }
    }

    /// Attempts one automatic restart. Returns `Ok(())` when the process was
    /// restarted, or `RestartLimitReached` once the crash budget is spent;
    /// the supervisor then marks itself auto-disabled (host disables the
    /// plugin record to avoid infinite restarts).
    pub fn restart(&mut self) -> Result<(), PluginError> {
        if self.auto_disabled {
            return Err(PluginError::RestartLimitReached {
                count: self.crash_count,
            });
        }
        let Some(process) = self.process.as_mut() else {
            return Err(PluginError::MissingEntrypoint);
        };
        match process.restart() {
            Ok(()) => Ok(()),
            Err(PluginError::RestartLimitReached { count }) => {
                self.auto_disabled = true;
                self.process = None;
                Err(PluginError::RestartLimitReached { count })
            }
            Err(error) => Err(error),
        }
    }

    /// Stops the process cleanly (best-effort kill).
    pub fn stop(&mut self) {
        if let Some(mut process) = self.process.take() {
            process.terminate().ok();
        }
    }

    /// Non-blocking crash check; records the crash and drops the dead handle
    /// so a follow-up restart starts fresh.
    pub fn poll(&mut self) -> bool {
        let Some(process) = self.process.as_mut() else {
            return false;
        };
        match process.try_wait() {
            Ok(Some(status)) => {
                self.record_crash(status.to_string());
                true
            }
            _ => false,
        }
    }

    fn record_crash(&mut self, diagnostic: String) {
        self.crash_count += 1;
        if let Some(mut process) = self.process.take() {
            let logs = process.logs();
            if !logs.is_empty() {
                self.logs = logs;
            }
            process.terminate().ok();
        }
        if self.crash_count >= AUTO_DISABLE_AFTER_CRASHES {
            self.auto_disabled = true;
        }
        self.process_start_error = Some(diagnostic);
    }

    /// Snapshot for UI/telemetry.
    pub fn info(&self) -> PluginProcessInfo {
        let (status, logs) = if self.auto_disabled {
            (PluginProcessStatus::Disabled, self.logs.clone())
        } else if let Some(process) = &self.process {
            let mut logs = self.logs.clone();
            logs.extend(process.logs());
            (PluginProcessStatus::Running, logs)
        } else if self.crash_count > 0 {
            (PluginProcessStatus::Crashed, self.logs.clone())
        } else {
            (PluginProcessStatus::Stopped, self.logs.clone())
        };
        PluginProcessInfo {
            plugin_id: self.plugin_id.clone(),
            status,
            logs,
            restart_count: self
                .process
                .as_ref()
                .map(|process| process.restart_count())
                .unwrap_or(self.crash_count.saturating_sub(1)),
            crash_count: self.crash_count,
            auto_disabled: self.auto_disabled,
        }
    }

    /// The last process start/exit diagnostic, if any.
    pub fn last_diagnostic(&self) -> Option<&str> {
        self.process_start_error.as_deref()
    }
}

/// Path helper for the plugin storage root (host-owned, never plugin-owned).
pub fn plugin_storage_root(config_directory: &std::path::Path) -> PathBuf {
    config_directory.join("plugin-storage")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ryusei_plugin_runtime::PluginError;
    use std::fs;

    /// A python3 JSON-RPC echo plugin, same protocol as the runtime tests.
    const ECHO_PLUGIN: &str = r#"
import os, struct, sys, json

def read_frame():
    header = sys.stdin.buffer.read(4)
    if not header:
        return None
    (length,) = struct.unpack("<I", header)
    return json.loads(sys.stdin.buffer.read(length))

def write_frame(obj):
    payload = json.dumps(obj).encode("utf-8")
    sys.stdout.buffer.write(struct.pack("<I", len(payload)) + payload)
    sys.stdout.buffer.flush()

crash_after = os.environ.get("CRASH_AFTER")
answered = 0
while True:
    request = read_frame()
    if request is None:
        break
    if crash_after is not None and answered >= int(crash_after):
        sys.stderr.write("crashing now\n")
        sys.stderr.flush()
        sys.exit(3)
    answered += 1
    write_frame({"jsonrpc": "2.0", "id": request["id"], "result": request["params"]})
"#;

    fn python3() -> Option<()> {
        std::process::Command::new("python3")
            .arg("--version")
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|_| ())
    }

    fn echo_spec(crash_after: Option<&str>) -> ProcessSpawnSpec {
        let mut environment = Vec::new();
        if let Some(crash_after) = crash_after {
            environment.push(("CRASH_AFTER".to_owned(), crash_after.to_owned()));
        }
        ProcessSpawnSpec {
            program: PathBuf::from("python3"),
            args: vec!["-c".to_owned(), ECHO_PLUGIN.to_owned()],
            working_dir: std::env::temp_dir(),
            environment,
        }
    }

    /// Generous upper bound for the python echo plugin tests. CI runners
    /// (especially Windows) share the test binary with the `go build`
    /// integration test, so an interpreter spawn + first frame can exceed the
    /// production RPC timeout under load.
    const TEST_RPC_TIMEOUT: Duration = Duration::from_secs(60);

    #[test]
    fn supervisor_round_trips_a_request_and_reports_running() {
        let Some(()) = python3() else {
            eprintln!("python3 not found; skipping supervisor test");
            return;
        };
        let mut supervisor = PluginSupervisor::new("org.example.echo");
        supervisor.process =
            Some(SupervisedNativePluginProcess::start(echo_spec(None)).expect("echo starts"));

        let id = supervisor
            .process
            .as_mut()
            .expect("process set")
            .send_request("game.snapshot", serde_json::json!({"depth": 1}))
            .expect("request sent");
        let result = supervisor
            .process
            .as_mut()
            .expect("process set")
            .await_response(id, TEST_RPC_TIMEOUT)
            .expect("request succeeds");
        assert_eq!(result, serde_json::json!({"depth": 1}));
        assert!(supervisor.is_running());
        assert_eq!(supervisor.info().status, PluginProcessStatus::Running);
        supervisor.stop();
        assert!(!supervisor.is_running());
    }

    #[test]
    fn supervisor_records_crashes_and_auto_disables_after_the_budget() {
        let Some(()) = python3() else {
            eprintln!("python3 not found; skipping crash budget test");
            return;
        };
        let mut supervisor = PluginSupervisor::new("org.example.echo");

        // Crash-budget policy: after AUTO_DISABLE_AFTER_CRASHES crashes the
        // supervisor records the diagnostic and marks the plugin disabled.
        for _ in 0..AUTO_DISABLE_AFTER_CRASHES {
            supervisor.process = Some(
                SupervisedNativePluginProcess::start(echo_spec(Some("0"))).expect("plugin starts"),
            );
            let id = supervisor
                .process
                .as_mut()
                .expect("process set")
                .send_request("ping", Value::Null)
                .expect("request sent");
            match supervisor
                .process
                .as_mut()
                .expect("process set")
                .await_response(id, TEST_RPC_TIMEOUT)
            {
                Err(PluginError::ProcessExited { status }) => {
                    supervisor.record_crash(status);
                }
                other => panic!("unexpected outcome: {other:?}"),
            }
        }

        assert_eq!(supervisor.crash_count, AUTO_DISABLE_AFTER_CRASHES);
        assert!(supervisor.auto_disabled);
        assert_eq!(supervisor.info().status, PluginProcessStatus::Disabled);
        assert!(
            supervisor.last_diagnostic().is_some(),
            "the crash diagnostic must be recorded"
        );
        assert!(
            supervisor
                .info()
                .logs
                .iter()
                .any(|line| line.contains("crashing now")),
            "the stderr log must be attached to the crash info"
        );
    }

    /// End-to-end against the real Go example plugin: builds (when `go` is
    /// available) the sgf-exporter binary, starts it through the supervisor
    /// and round-trips a JSON-RPC request. This is the same path the GPUI
    /// command buttons take.
    #[test]
    fn supervisor_calls_the_real_go_example_plugin() {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let repo_root = manifest_dir
            .parent()
            .and_then(|dir| dir.parent())
            .expect("workspace root");
        let plugin_dir = repo_root.join("examples/plugins/sgf-exporter");
        if !plugin_dir.join("main.go").exists() {
            eprintln!("go example plugin missing; skipping");
            return;
        }
        let go_available = std::process::Command::new("go")
            .arg("version")
            .output()
            .ok()
            .filter(|output| output.status.success())
            .is_some();
        if !go_available {
            eprintln!("go not found; skipping go plugin test");
            return;
        }
        let bin_dir = plugin_dir.join("bin");
        let _ = fs::remove_dir_all(&bin_dir);
        let go_cache = std::env::temp_dir().join("saba-go-cache");
        let go_path = std::env::temp_dir().join("saba-go-path");
        let build = std::process::Command::new("go")
            .args(["build", "-o", "bin/ryusei-sgf-exporter", "."])
            .current_dir(&plugin_dir)
            .env("GOCACHE", &go_cache)
            .env("GOPATH", &go_path)
            .output()
            .expect("go build runs");
        assert!(
            build.status.success(),
            "go build failed: {}",
            String::from_utf8_lossy(&build.stderr)
        );

        let manifest = ryusei_plugin_runtime::PluginManifest {
            schema_version: 1,
            id: "org.ryusei.sgf-exporter".to_owned(),
            name: "SGF Exporter".to_owned(),
            version: "0.1.0".to_owned(),
            api_version: 1,
            runtime: ryusei_plugin_runtime::PluginRuntime::Native,
            activation_events: Vec::new(),
            permissions: Default::default(),
            contributes: Default::default(),
            entrypoint: Some("bin/ryusei-sgf-exporter".to_owned()),
        };
        let record = PluginRecord {
            manifest,
            install_path: plugin_dir.clone(),
            enabled: true,
            granted_permissions: Default::default(),
            native_execution_authorized: true,
        };
        let mut supervisor = PluginSupervisor::new("org.ryusei.sgf-exporter");
        supervisor.start(&record).expect("go plugin starts");
        let result = supervisor
            .request(
                "org.ryusei.sgf-exporter.export",
                serde_json::json!({"command": "export"}),
            )
            .expect("go plugin answers");
        assert_eq!(result["status"], "ready");
        supervisor.stop();
        let _ = fs::remove_dir_all(&bin_dir);
    }

    #[test]
    fn supervisor_starts_from_a_plugin_record() {
        // ProcessSpawnSpec::for_record must derive a runnable spec from a
        // native plugin record, including the id/version environment.
        let manifest = ryusei_plugin_runtime::PluginManifest {
            schema_version: 1,
            id: "org.example.fixture".to_owned(),
            name: "Fixture".to_owned(),
            version: "2.1.0".to_owned(),
            api_version: 1,
            runtime: ryusei_plugin_runtime::PluginRuntime::Native,
            activation_events: Vec::new(),
            permissions: Default::default(),
            contributes: Default::default(),
            entrypoint: Some("python3".to_owned()),
        };
        let record = PluginRecord {
            manifest,
            install_path: PathBuf::from("/plugins/fixture"),
            enabled: true,
            granted_permissions: Default::default(),
            native_execution_authorized: true,
        };
        let spec = ProcessSpawnSpec::for_record(&record).expect("record yields a spec");
        assert_eq!(spec.program, PathBuf::from("/plugins/fixture/python3"));
        assert_eq!(spec.working_dir, PathBuf::from("/plugins/fixture"));
        assert!(
            spec.environment.iter().any(|(key, value)| {
                key == "RYUSEI_PLUGIN_ID" && value == "org.example.fixture"
            })
        );
        assert!(
            spec.environment
                .iter()
                .any(|(key, value)| { key == "RYUSEI_PLUGIN_VERSION" && value == "2.1.0" })
        );
    }
}
