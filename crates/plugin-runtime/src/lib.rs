use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs,
    io::{BufReader, Read, Write},
    path::{Component, Path, PathBuf},
    process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{RecvTimeoutError, SyncSender, sync_channel},
    },
    time::Duration,
};

pub mod storage;
pub mod wasm;

pub use wasm::{
    MAX_WASM_FUEL, MAX_WASM_MEMORY_PAGES, MAX_WASM_PAYLOAD_BYTES, WasmCapabilities, WasmError,
    WasmPluginInstance, WasmPluginModule,
};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use thiserror::Error;

pub const PLUGIN_API_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PluginPermission {
    GameRead,
    GameWrite,
    EngineRead,
    EngineControl,
    Storage,
    FileRead,
    FileWrite,
    Network,
    ClipboardRead,
    ClipboardWrite,
    UiPanel,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PluginRuntime {
    Wasm,
    Native,
    Declarative,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifest {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub version: String,
    pub api_version: u32,
    pub runtime: PluginRuntime,
    #[serde(default)]
    pub activation_events: Vec<String>,
    #[serde(default)]
    pub permissions: BTreeSet<PluginPermission>,
    #[serde(default)]
    pub contributes: PluginContributions,
    #[serde(default)]
    pub entrypoint: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginContributions {
    #[serde(default)]
    pub commands: Vec<PluginCommand>,
    #[serde(default)]
    pub settings: Vec<PluginSetting>,
    #[serde(default)]
    pub menus: Vec<PluginMenuContribution>,
    #[serde(default)]
    pub panels: Vec<PluginPanelContribution>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginCommand {
    pub id: String,
    pub title: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginSetting {
    pub key: String,
    #[serde(rename = "type")]
    pub value_type: String,
    pub default: Value,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginMenuContribution {
    pub menu: String,
    pub command: String,
}

pub const PLUGIN_CONTRIBUTION_SCHEMA_VERSION: u32 = 1;

/// A host-validated, closed-set widget for a plugin panel. Plugins may never
/// embed arbitrary Rust/GPUI components or GPU contexts; they contribute data
/// that the host renders from this closed set.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PanelWidget {
    Label {
        text: String,
    },
    Value {
        label: String,
        value: String,
    },
    Button {
        id: String,
        title: String,
    },
    Select {
        id: String,
        options: Vec<String>,
        selected: Option<String>,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginPanelContribution {
    pub schema_version: u32,
    pub plugin_id: String,
    pub panel_title: String,
    pub widgets: Vec<PanelWidget>,
}

impl PluginPanelContribution {
    pub fn parse(json: &str) -> Result<Self, String> {
        let contribution: Self = serde_json::from_str(json)
            .map_err(|error| format!("invalid plugin panel contribution: {error}"))?;
        contribution.validate()?;
        Ok(contribution)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != PLUGIN_CONTRIBUTION_SCHEMA_VERSION {
            return Err(format!(
                "unsupported plugin contribution schema version {}",
                self.schema_version
            ));
        }
        if self.plugin_id.trim().is_empty() || !self.plugin_id.contains('.') {
            return Err("plugin id must be a reverse-domain identifier".to_owned());
        }
        if self.panel_title.trim().is_empty() {
            return Err("panel title must not be empty".to_owned());
        }
        let mut button_ids = std::collections::BTreeSet::new();
        for widget in &self.widgets {
            match widget {
                PanelWidget::Label { text } if text.trim().is_empty() => {
                    return Err("label widget must not be empty".to_owned());
                }
                PanelWidget::Value { label, .. } if label.trim().is_empty() => {
                    return Err("value widget must have a label".to_owned());
                }
                PanelWidget::Button { id, title } => {
                    if title.trim().is_empty() {
                        return Err("button widget must have a title".to_owned());
                    }
                    if !button_ids.insert(id.as_str()) {
                        return Err(format!("duplicate button id {id:?}"));
                    }
                }
                PanelWidget::Select { id, options, .. } if options.is_empty() => {
                    return Err(format!("select widget {id:?} needs at least one option"));
                }
                _ => {}
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginRecord {
    pub manifest: PluginManifest,
    pub install_path: PathBuf,
    pub enabled: bool,
    pub granted_permissions: BTreeSet<PluginPermission>,
    pub native_execution_authorized: bool,
}

#[derive(Debug, Error)]
pub enum PluginError {
    #[error("plugin manifest is invalid: {0}")]
    InvalidManifest(String),
    #[error("plugin API version {actual} is unsupported; expected {expected}")]
    UnsupportedApiVersion { actual: u32, expected: u32 },
    #[error("plugin requests ungranted permissions: {0:?}")]
    PermissionDenied(BTreeSet<PluginPermission>),
    #[error("native plugin execution requires explicit user authorization")]
    NativeExecutionNotAuthorized,
    #[error("plugin runtime is not native")]
    InvalidRuntime,
    #[error("plugin entrypoint is missing")]
    MissingEntrypoint,
    #[error("plugin manifest could not be read: {0}")]
    ManifestRead(#[from] std::io::Error),
    #[error("plugin manifest could not be decoded: {0}")]
    ManifestDecode(#[from] serde_json::Error),
    #[error("plugin process could not be started: {0}")]
    ProcessStart(std::io::Error),
    #[error("plugin process exited with status {status}")]
    ProcessExited { status: String },
    #[error("plugin request {request_id} timed out")]
    RpcTimeout { request_id: u64 },
    #[error("plugin rejected the request: {message} (code {code})")]
    RpcRejected { code: i64, message: String },
    #[error("plugin response carried no result")]
    MissingResult,
    #[error("plugin exceeded the {count} automatic restart limit")]
    RestartLimitReached { count: u32 },
    #[error("plugin process does not expose standard input")]
    MissingStandardInput,
    #[error("plugin RPC message could not be encoded: {0}")]
    RpcEncode(serde_json::Error),
    #[error("plugin RPC stream ended before a full message was received")]
    UnexpectedEndOfStream,
    #[error("plugin RPC frame exceeds the host limit")]
    FrameTooLarge,
    #[error("plugin RPC message could not be decoded: {0}")]
    RpcDecode(serde_json::Error),
}

impl PluginManifest {
    pub fn load(install_path: impl AsRef<Path>) -> Result<Self, PluginError> {
        let manifest_path = install_path.as_ref().join("ryusei-plugin.json");
        let manifest: Self = serde_json::from_slice(&fs::read(manifest_path)?)?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<(), PluginError> {
        if self.schema_version != 1 {
            return Err(PluginError::InvalidManifest(
                "schemaVersion must be 1".to_owned(),
            ));
        }
        if self.api_version != PLUGIN_API_VERSION {
            return Err(PluginError::UnsupportedApiVersion {
                actual: self.api_version,
                expected: PLUGIN_API_VERSION,
            });
        }
        if self.id.trim().is_empty() || !self.id.contains('.') {
            return Err(PluginError::InvalidManifest(
                "id must be a reverse-domain identifier".to_owned(),
            ));
        }
        if self.name.trim().is_empty() || self.version.trim().is_empty() {
            return Err(PluginError::InvalidManifest(
                "name and version are required".to_owned(),
            ));
        }
        if matches!(self.runtime, PluginRuntime::Wasm | PluginRuntime::Native)
            && self.entrypoint.as_deref().is_none_or(str::is_empty)
        {
            return Err(PluginError::MissingEntrypoint);
        }
        if self.contributes.commands.iter().any(|command| {
            command.id.trim().is_empty() || !command.id.starts_with(&format!("{}.", self.id))
        }) {
            return Err(PluginError::InvalidManifest(
                "command identifiers must be namespaced by the plugin id".to_owned(),
            ));
        }
        for panel in &self.contributes.panels {
            panel.validate().map_err(PluginError::InvalidManifest)?;
            if panel.plugin_id != self.id {
                return Err(PluginError::InvalidManifest(
                    "panel contribution plugin id must match the manifest id".to_owned(),
                ));
            }
        }
        Ok(())
    }

    pub fn ungranted_permissions(
        &self,
        granted_permissions: &BTreeSet<PluginPermission>,
    ) -> BTreeSet<PluginPermission> {
        self.permissions
            .difference(granted_permissions)
            .cloned()
            .collect()
    }
}

impl PluginRecord {
    pub fn install(
        install_path: impl Into<PathBuf>,
        granted_permissions: BTreeSet<PluginPermission>,
    ) -> Result<Self, PluginError> {
        let install_path = install_path.into();
        let manifest = PluginManifest::load(&install_path)?;
        Ok(Self {
            manifest,
            install_path,
            enabled: false,
            granted_permissions,
            native_execution_authorized: false,
        })
    }

    pub fn enable(&mut self) -> Result<(), PluginError> {
        let missing_permissions = self
            .manifest
            .ungranted_permissions(&self.granted_permissions);
        if !missing_permissions.is_empty() {
            return Err(PluginError::PermissionDenied(missing_permissions));
        }
        if matches!(self.manifest.runtime, PluginRuntime::Native)
            && !self.native_execution_authorized
        {
            return Err(PluginError::NativeExecutionNotAuthorized);
        }
        self.enabled = true;
        Ok(())
    }

    pub fn authorize_native_execution(&mut self) -> Result<(), PluginError> {
        if !matches!(self.manifest.runtime, PluginRuntime::Native) {
            return Err(PluginError::InvalidRuntime);
        }
        self.native_execution_authorized = true;
        Ok(())
    }

    pub fn resolve_entrypoint(&self) -> Result<PathBuf, PluginError> {
        let entrypoint = self
            .manifest
            .entrypoint
            .as_deref()
            .ok_or(PluginError::MissingEntrypoint)?;
        let entrypoint = Path::new(entrypoint);
        if entrypoint.is_absolute()
            || entrypoint
                .components()
                .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(PluginError::InvalidManifest(
                "entrypoint must remain within the plugin directory".to_owned(),
            ));
        }
        Ok(self.install_path.join(entrypoint))
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JsonRpcRequest<'request> {
    pub jsonrpc: &'static str,
    pub id: u64,
    pub method: &'request str,
    pub params: Value,
}

#[derive(Debug, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: u64,
    #[serde(default)]
    pub result: Option<Value>,
    #[serde(default)]
    pub error: Option<JsonRpcError>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(default)]
    pub data: Option<Value>,
}

pub const MAX_RPC_FRAME_BYTES: usize = 1024 * 1024;

pub fn write_rpc_frame<T: Serialize>(
    writer: &mut impl Write,
    value: &T,
) -> Result<(), PluginError> {
    let serialized = serde_json::to_vec(value).map_err(PluginError::RpcEncode)?;
    if serialized.len() > MAX_RPC_FRAME_BYTES {
        return Err(PluginError::FrameTooLarge);
    }
    writer.write_all(&(serialized.len() as u32).to_le_bytes())?;
    writer.write_all(&serialized)?;
    writer.flush()?;
    Ok(())
}

pub fn read_rpc_frame<T: DeserializeOwned>(reader: &mut impl Read) -> Result<T, PluginError> {
    let mut length_bytes = [0; 4];
    reader
        .read_exact(&mut length_bytes)
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::UnexpectedEof => PluginError::UnexpectedEndOfStream,
            _ => PluginError::ManifestRead(error),
        })?;
    let length = u32::from_le_bytes(length_bytes) as usize;
    if length > MAX_RPC_FRAME_BYTES {
        return Err(PluginError::FrameTooLarge);
    }
    let mut payload = vec![0; length];
    reader
        .read_exact(&mut payload)
        .map_err(|_| PluginError::UnexpectedEndOfStream)?;
    serde_json::from_slice(&payload).map_err(PluginError::RpcDecode)
}

/// How the supervised process should be launched. Kept separate from the
/// running handle so a crash can be restarted with the identical command.
#[derive(Clone, Debug)]
pub struct ProcessSpawnSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub working_dir: PathBuf,
    pub environment: Vec<(String, String)>,
}

impl ProcessSpawnSpec {
    pub fn for_record(record: &PluginRecord) -> Result<Self, PluginError> {
        if !matches!(record.manifest.runtime, PluginRuntime::Native) {
            return Err(PluginError::InvalidRuntime);
        }
        let entrypoint = record.resolve_entrypoint()?;
        let mut environment = vec![
            ("RYUSEI_PLUGIN_ID".to_owned(), record.manifest.id.clone()),
            (
                "RYUSEI_PLUGIN_VERSION".to_owned(),
                record.manifest.version.clone(),
            ),
        ];
        if let Some(api_version) = std::env::var_os("RYUSEI_PLUGIN_API_VERSION") {
            environment.push((
                "RYUSEI_PLUGIN_API_VERSION".to_owned(),
                api_version.to_string_lossy().into_owned(),
            ));
        }
        Ok(Self {
            program: entrypoint,
            args: Vec::new(),
            working_dir: record.install_path.clone(),
            environment,
        })
    }
}

/// Maximum number of stderr log lines kept per process.
pub const MAX_PROCESS_LOG_LINES: usize = 200;

/// Maximum length of a single logged line; longer lines are truncated.
pub const MAX_PROCESS_LOG_LINE_CHARS: usize = 512;

/// Maximum number of automatic restarts before the supervisor gives up.
pub const MAX_PROCESS_RESTARTS: u32 = 3;

/// A supervised native plugin process.
///
/// The process runs length-prefixed JSON-RPC over stdio (see
/// `write_rpc_frame`/`read_rpc_frame`). A background thread owns standard
/// output and routes responses back to the waiting caller by request id; a
/// second thread captures standard error into a bounded ring log. When the
/// process exits, pending callers receive a typed `ProcessExited` error and
/// the supervisor can detect the crash with `try_wait`, restart with
/// `restart`, or give up after `MAX_PROCESS_RESTARTS` and let the host
/// disable the plugin (design §10.4: no infinite restarts).
pub struct SupervisedNativePluginProcess {
    spec: ProcessSpawnSpec,
    child: Child,
    standard_input: ChildStdin,
    next_request_id: u64,
    pending: Arc<Mutex<BTreeMap<u64, SyncSender<JsonRpcResponse>>>>,
    log: Arc<Mutex<VecDeque<String>>>,
    exited: Arc<AtomicBool>,
    restart_count: u32,
}

fn spawn_supervised(spec: &ProcessSpawnSpec) -> Result<(Child, ChildStdin), PluginError> {
    let mut command = Command::new(&spec.program);
    command
        .args(&spec.args)
        .current_dir(&spec.working_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_clear();
    for (key, value) in &spec.environment {
        command.env(key, value);
    }
    let mut child = command.spawn().map_err(PluginError::ProcessStart)?;
    let standard_input = child
        .stdin
        .take()
        .ok_or(PluginError::MissingStandardInput)?;
    Ok((child, standard_input))
}

fn run_response_reader(
    mut child_stdout: ChildStdout,
    pending: Arc<Mutex<BTreeMap<u64, SyncSender<JsonRpcResponse>>>>,
    exited: Arc<AtomicBool>,
) {
    loop {
        match read_rpc_frame::<JsonRpcResponse>(&mut child_stdout) {
            Ok(response) => {
                let sender = pending
                    .lock()
                    .expect("pending map is not poisoned")
                    .remove(&response.id);
                if let Some(sender) = sender {
                    let _ = sender.send(response);
                }
            }
            Err(PluginError::UnexpectedEndOfStream) | Err(PluginError::ManifestRead(_)) => {
                // The child exited or closed its stdout: wake every pending
                // caller with a typed error so no request hangs forever, and
                // record the exit so later callers fail immediately too.
                exited.store(true, Ordering::SeqCst);
                let error = JsonRpcError {
                    code: -32000,
                    message: "plugin process exited".to_owned(),
                    data: None,
                };
                let mut pending = pending.lock().expect("pending map is not poisoned");
                for sender in pending.values() {
                    let _ = sender.send(JsonRpcResponse {
                        jsonrpc: "2.0".to_owned(),
                        id: 0,
                        result: None,
                        error: Some(error.clone()),
                    });
                }
                pending.clear();
                return;
            }
            Err(_) => {
                // Oversized or undecodable frames are dropped; the reader
                // keeps going so one bad message cannot wedge the channel.
                continue;
            }
        }
    }
}

fn run_stderr_logger(mut child_stderr: ChildStderr, log: Arc<Mutex<VecDeque<String>>>) {
    use std::io::BufRead;
    let mut reader = BufReader::new(&mut child_stderr);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {
                let mut line = line.trim_end_matches(['\r', '\n']).to_owned();
                if line.chars().count() > MAX_PROCESS_LOG_LINE_CHARS {
                    line = line.chars().take(MAX_PROCESS_LOG_LINE_CHARS).collect();
                }
                let mut log = log.lock().expect("log is not poisoned");
                if log.len() >= MAX_PROCESS_LOG_LINES {
                    log.pop_front();
                }
                log.push_back(line);
            }
        }
    }
}

impl SupervisedNativePluginProcess {
    pub fn start(spec: ProcessSpawnSpec) -> Result<Self, PluginError> {
        let (child, standard_input) = spawn_supervised(&spec)?;
        let pending = Arc::new(Mutex::new(BTreeMap::new()));
        let log = Arc::new(Mutex::new(VecDeque::new()));
        let exited = Arc::new(AtomicBool::new(false));
        let stdout_reader_pending = Arc::clone(&pending);
        let stdout_reader_exited = Arc::clone(&exited);
        let stderr_logger_log = Arc::clone(&log);
        let mut child_for_io = child;
        if let Some(stdout) = child_for_io.stdout.take() {
            std::thread::spawn(move || {
                run_response_reader(stdout, stdout_reader_pending, stdout_reader_exited)
            });
        }
        if let Some(stderr) = child_for_io.stderr.take() {
            std::thread::spawn(move || run_stderr_logger(stderr, stderr_logger_log));
        }
        Ok(Self {
            spec,
            child: child_for_io,
            standard_input,
            next_request_id: 1,
            pending,
            log,
            exited,
            restart_count: 0,
        })
    }

    /// Sends a request and returns its id; the response is delivered by
    /// `await_response`. The process must still be running.
    pub fn send_request(&mut self, method: &str, params: Value) -> Result<u64, PluginError> {
        if let Some(status) = self.child.try_wait().map_err(PluginError::ProcessStart)? {
            return Err(PluginError::ProcessExited {
                status: status.to_string(),
            });
        }
        let request_id = self.next_request_id;
        self.next_request_id += 1;
        write_rpc_frame(
            &mut self.standard_input,
            &JsonRpcRequest {
                jsonrpc: "2.0",
                id: request_id,
                method,
                params,
            },
        )?;
        Ok(request_id)
    }

    /// Waits for the response with the given id. Returns `RpcTimeout` when
    /// the deadline passes, `ProcessExited` when the child died meanwhile.
    pub fn await_response(
        &mut self,
        request_id: u64,
        timeout: Duration,
    ) -> Result<Value, PluginError> {
        if self.exited.load(Ordering::SeqCst) {
            return Err(PluginError::ProcessExited {
                status: self
                    .child
                    .try_wait()
                    .ok()
                    .flatten()
                    .map(|status| status.to_string())
                    .unwrap_or_else(|| "unknown".to_owned()),
            });
        }
        let (sender, receiver) = sync_channel::<JsonRpcResponse>(1);
        self.pending
            .lock()
            .expect("pending map is not poisoned")
            .insert(request_id, sender);
        match receiver.recv_timeout(timeout) {
            Ok(response) => match response.error {
                Some(error) if error.code == -32000 => Err(PluginError::ProcessExited {
                    status: self
                        .child
                        .try_wait()
                        .ok()
                        .flatten()
                        .map(|status| status.to_string())
                        .unwrap_or_else(|| "unknown".to_owned()),
                }),
                Some(error) => Err(PluginError::RpcRejected {
                    code: error.code,
                    message: error.message,
                }),
                None => response.result.ok_or(PluginError::MissingResult),
            },
            Err(RecvTimeoutError::Timeout) => {
                self.pending
                    .lock()
                    .expect("pending map is not poisoned")
                    .remove(&request_id);
                Err(PluginError::RpcTimeout { request_id })
            }
            Err(RecvTimeoutError::Disconnected) => Err(PluginError::UnexpectedEndOfStream),
        }
    }

    /// Non-blocking crash check. `Ok(None)` means still running; `Ok(Some)`
    /// carries the exit status of a terminated process.
    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>, PluginError> {
        self.child.try_wait().map_err(PluginError::ProcessStart)
    }

    /// Recent standard-error output, oldest first, bounded by
    /// `MAX_PROCESS_LOG_LINES`.
    pub fn logs(&self) -> Vec<String> {
        self.log
            .lock()
            .expect("log is not poisoned")
            .iter()
            .cloned()
            .collect()
    }

    pub fn restart_count(&self) -> u32 {
        self.restart_count
    }

    /// Kills the current process and starts a fresh one with the same spec.
    /// Fails once `MAX_PROCESS_RESTARTS` automatic restarts were consumed;
    /// the host then disables the plugin instead of looping forever.
    pub fn restart(&mut self) -> Result<(), PluginError> {
        if self.restart_count >= MAX_PROCESS_RESTARTS {
            return Err(PluginError::RestartLimitReached {
                count: self.restart_count,
            });
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        let (child, standard_input) = spawn_supervised(&self.spec)?;
        self.child = child;
        self.standard_input = standard_input;
        self.restart_count += 1;
        self.exited.store(false, Ordering::SeqCst);
        let stdout_reader_pending = Arc::clone(&self.pending);
        let stdout_reader_exited = Arc::clone(&self.exited);
        let stderr_logger_log = Arc::clone(&self.log);
        if let Some(stdout) = self.child.stdout.take() {
            std::thread::spawn(move || {
                run_response_reader(stdout, stdout_reader_pending, stdout_reader_exited)
            });
        }
        if let Some(stderr) = self.child.stderr.take() {
            std::thread::spawn(move || run_stderr_logger(stderr, stderr_logger_log));
        }
        Ok(())
    }

    pub fn terminate(&mut self) -> Result<(), std::io::Error> {
        self.child.kill()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_a_plugin_command_outside_its_namespace() {
        let manifest = PluginManifest {
            schema_version: 1,
            id: "org.example.training".to_owned(),
            name: "Training".to_owned(),
            version: "1.0.0".to_owned(),
            api_version: PLUGIN_API_VERSION,
            runtime: PluginRuntime::Declarative,
            activation_events: Vec::new(),
            permissions: BTreeSet::new(),
            contributes: PluginContributions {
                commands: vec![PluginCommand {
                    id: "unrelated.command".to_owned(),
                    title: "Invalid".to_owned(),
                }],
                ..PluginContributions::default()
            },
            entrypoint: None,
        };

        assert!(manifest.validate().is_err());
    }

    #[test]
    fn validates_declarative_panel_contributions_from_manifests() {
        let panel = PluginPanelContribution::parse(
            r#"{
                "schemaVersion": 1,
                "pluginId": "org.example.training",
                "panelTitle": "Training",
                "widgets": [
                    {"type": "label", "text": "Ready"},
                    {"type": "button", "id": "org.example.training.start", "title": "Start"}
                ]
            }"#,
        )
        .unwrap();
        let manifest = PluginManifest {
            schema_version: 1,
            id: "org.example.training".to_owned(),
            name: "Training".to_owned(),
            version: "1.0.0".to_owned(),
            api_version: PLUGIN_API_VERSION,
            runtime: PluginRuntime::Declarative,
            activation_events: Vec::new(),
            permissions: BTreeSet::new(),
            contributes: PluginContributions {
                panels: vec![panel],
                ..PluginContributions::default()
            },
            entrypoint: None,
        };

        assert!(manifest.validate().is_ok());
    }

    #[test]
    fn rejects_panel_contributions_for_another_plugin_id() {
        let panel = PluginPanelContribution {
            schema_version: PLUGIN_CONTRIBUTION_SCHEMA_VERSION,
            plugin_id: "org.example.other".to_owned(),
            panel_title: "Training".to_owned(),
            widgets: Vec::new(),
        };
        let manifest = PluginManifest {
            schema_version: 1,
            id: "org.example.training".to_owned(),
            name: "Training".to_owned(),
            version: "1.0.0".to_owned(),
            api_version: PLUGIN_API_VERSION,
            runtime: PluginRuntime::Declarative,
            activation_events: Vec::new(),
            permissions: BTreeSet::new(),
            contributes: PluginContributions {
                panels: vec![panel],
                ..PluginContributions::default()
            },
            entrypoint: None,
        };

        assert!(manifest.validate().is_err());
    }

    #[test]
    fn frames_json_rpc_messages_with_a_bounded_length_prefix() {
        let request = JsonRpcRequest {
            jsonrpc: "2.0",
            id: 42,
            method: "game.snapshot",
            params: serde_json::json!({"includeMarkup": false}),
        };
        let mut bytes = Vec::new();
        write_rpc_frame(&mut bytes, &request).unwrap();
        let decoded: Value = read_rpc_frame(&mut bytes.as_slice()).unwrap();

        assert_eq!(decoded["id"], 42);
        assert_eq!(decoded["method"], "game.snapshot");
    }

    // ------------------------------------------------------------------
    // Supervised process tests against real python3 subprocesses. Skipped
    // when python3 is unavailable, mirroring the GTP stream tests.
    // ------------------------------------------------------------------

    /// A JSON-RPC echo plugin: reads length-prefixed frames from stdin and
    /// answers every request with `{"result": params}`. Exits cleanly on
    /// stdin EOF. A leading `CRASH_AFTER` environment variable makes the
    /// process die before answering the first frame (crash simulation).
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

    fn echo_spec() -> ProcessSpawnSpec {
        ProcessSpawnSpec {
            program: PathBuf::from("python3"),
            args: vec!["-c".to_owned(), ECHO_PLUGIN.to_owned()],
            working_dir: std::env::temp_dir(),
            environment: Vec::new(),
        }
    }

    #[test]
    fn supervised_process_round_trips_a_request() {
        let Some(()) = python3() else {
            eprintln!("python3 not found; skipping supervised process test");
            return;
        };
        let mut process =
            SupervisedNativePluginProcess::start(echo_spec()).expect("echo plugin starts");
        let id = process
            .send_request("game.snapshot", serde_json::json!({"depth": 2}))
            .expect("request is sent");
        let result = process
            .await_response(id, Duration::from_secs(5))
            .expect("echo plugin answers");
        assert_eq!(result, serde_json::json!({"depth": 2}));
        process.terminate().ok();
    }

    #[test]
    fn supervised_process_times_out_when_the_plugin_never_answers() {
        let Some(()) = python3() else {
            eprintln!("python3 not found; skipping timeout test");
            return;
        };
        let mut spec = echo_spec();
        // A hanging plugin: never answer, but keep the pipe open.
        spec.args = vec![
            "-c".to_owned(),
            "import struct, sys, time; f=sys.stdin.buffer.read(); time.sleep(60)".to_owned(),
        ];
        let mut process =
            SupervisedNativePluginProcess::start(spec).expect("hanging plugin starts");
        let id = process
            .send_request("ping", Value::Null)
            .expect("request is sent");
        assert!(matches!(
            process.await_response(id, Duration::from_millis(300)),
            Err(PluginError::RpcTimeout { .. })
        ));
        process.terminate().ok();
    }

    #[test]
    fn supervised_process_detects_a_crash_and_reports_the_exit_status() {
        let Some(()) = python3() else {
            eprintln!("python3 not found; skipping crash test");
            return;
        };
        let mut spec = echo_spec();
        spec.environment = vec![("CRASH_AFTER".to_owned(), "0".to_owned())];
        let mut process =
            SupervisedNativePluginProcess::start(spec).expect("crashing plugin starts");
        let id = process
            .send_request("ping", Value::Null)
            .expect("request is sent");
        assert!(
            matches!(
                process.await_response(id, Duration::from_secs(5)),
                Err(PluginError::ProcessExited { .. })
            ),
            "a crashed plugin must surface as ProcessExited"
        );
        // On Windows the stdout EOF and the process termination are not
        // atomic: poll briefly until the exit status becomes observable.
        let mut status = None;
        for _ in 0..20 {
            status = process.try_wait().expect("try_wait succeeds");
            if status.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(status.is_some(), "the crashed process must be reaped");
        let logs = process.logs();
        assert!(
            logs.iter().any(|line| line.contains("crashing now")),
            "stderr must be captured: {logs:?}"
        );
    }

    #[test]
    fn supervised_process_restarts_after_a_crash_within_the_limit() {
        let Some(()) = python3() else {
            eprintln!("python3 not found; skipping restart test");
            return;
        };
        // CRASH_AFTER=1: answer the first request, crash on the second.
        let mut spec = echo_spec();
        spec.environment = vec![("CRASH_AFTER".to_owned(), "1".to_owned())];
        let mut process = SupervisedNativePluginProcess::start(spec).expect("plugin starts");

        let first = process
            .send_request("ping", serde_json::json!({"n": 1}))
            .expect("first request is sent");
        assert_eq!(
            process
                .await_response(first, Duration::from_secs(5))
                .unwrap(),
            serde_json::json!({"n": 1})
        );

        let second = process
            .send_request("ping", serde_json::json!({"n": 2}))
            .expect("second request is sent");
        assert!(matches!(
            process.await_response(second, Duration::from_secs(5)),
            Err(PluginError::ProcessExited { .. })
        ));

        process
            .restart()
            .expect("restart within the limit succeeds");
        assert_eq!(process.restart_count(), 1);
        let third = process
            .send_request("ping", serde_json::json!({"n": 3}))
            .expect("request after restart is sent");
        assert_eq!(
            process
                .await_response(third, Duration::from_secs(5))
                .unwrap(),
            serde_json::json!({"n": 3}),
            "the restarted process must answer"
        );
        process.terminate().ok();
    }

    #[test]
    fn supervised_process_never_restarts_beyond_the_limit() {
        let Some(()) = python3() else {
            eprintln!("python3 not found; skipping restart limit test");
            return;
        };
        let mut spec = echo_spec();
        // CRASH_AFTER=0: crash on the very first request, every time.
        spec.environment = vec![("CRASH_AFTER".to_owned(), "0".to_owned())];
        let mut process = SupervisedNativePluginProcess::start(spec).expect("plugin starts");

        let mut restarts = 0;
        loop {
            let id = match process.send_request("ping", Value::Null) {
                Ok(id) => id,
                Err(PluginError::ProcessExited { .. }) => {
                    process
                        .restart()
                        .expect("restart within the limit succeeds");
                    restarts += 1;
                    continue;
                }
                Err(error) => panic!("request could not be sent: {error}"),
            };
            match process.await_response(id, Duration::from_secs(5)) {
                Err(PluginError::ProcessExited { .. })
                    if process.restart_count() < MAX_PROCESS_RESTARTS =>
                {
                    process
                        .restart()
                        .expect("restart within the limit succeeds");
                    restarts += 1;
                }
                Err(PluginError::ProcessExited { .. }) => {
                    // At the limit the next restart must be refused.
                    assert!(matches!(
                        process.restart(),
                        Err(PluginError::RestartLimitReached { .. })
                    ));
                    break;
                }
                other => panic!("unexpected outcome: {other:?}"),
            }
        }
        assert_eq!(restarts, MAX_PROCESS_RESTARTS as usize);
        assert_eq!(process.restart_count(), MAX_PROCESS_RESTARTS);
        process.terminate().ok();
    }
}
