use std::{
    collections::BTreeSet,
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
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
        let manifest_path = install_path.as_ref().join("sabaki-plugin.json");
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

#[derive(Debug, Deserialize)]
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

pub struct NativePluginProcess {
    child: Child,
    standard_input: ChildStdin,
    next_request_id: u64,
}

impl NativePluginProcess {
    pub fn start(record: &PluginRecord) -> Result<Self, PluginError> {
        if !matches!(record.manifest.runtime, PluginRuntime::Native) {
            return Err(PluginError::InvalidRuntime);
        }
        if !record.enabled || !record.native_execution_authorized {
            return Err(PluginError::NativeExecutionNotAuthorized);
        }

        let entrypoint = record.resolve_entrypoint()?;
        let mut child = Command::new(entrypoint)
            .current_dir(&record.install_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env_clear()
            .env("SABAKI_PLUGIN_ID", &record.manifest.id)
            .spawn()
            .map_err(PluginError::ProcessStart)?;
        let standard_input = child
            .stdin
            .take()
            .ok_or(PluginError::MissingStandardInput)?;
        Ok(Self {
            child,
            standard_input,
            next_request_id: 1,
        })
    }

    pub fn send_request(&mut self, method: &str, params: Value) -> Result<u64, PluginError> {
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
}
