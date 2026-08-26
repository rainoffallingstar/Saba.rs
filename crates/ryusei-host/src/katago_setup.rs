//! KataGo environment and neural-network weights manager (ported from LizzieYZY).
//!
//! Detects an existing KataGo executable, downloads official network weights,
//! generates versioned GTP configuration, and builds Sabaki engine records.
//! Binary installation is intentionally not promised on platforms without a
//! verified pinned release asset.

use std::{
    path::{Path, PathBuf},
    process::Command,
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::engine_workflow::EngineRecord;

pub const KATAGO_OFFICIAL_RELEASE_BASE: &str =
    "https://github.com/lightvector/KataGo/releases/download/v1.17.1/";
const MANAGED_CONFIG_MARKER: &str = "# Sabaki managed KataGo config";
const MANAGED_CONFIG_VERSION: u32 = 2;

pub const MODEL_LIGHTWEIGHT_NAME: &str = "b10c384h6nbttflrs.bin.gz";
pub const MODEL_LIGHTWEIGHT_URL: &str =
    "https://github.com/lightvector/KataGo/releases/download/v1.17.1/b10c384h6nbttflrs.bin.gz";

pub const MODEL_BALANCED_NAME: &str = "b10c512h8nbt3tflrs-fson-silu-rsnh.bin.gz";
pub const MODEL_BALANCED_URL: &str = "https://github.com/lightvector/KataGo/releases/download/v1.17.1/b10c512h8nbt3tflrs-fson-silu-rsnh.bin.gz";

pub const MODEL_STRONGEST_NAME: &str = "b11c768h12nbt3tflrs-fson-silu.bin.gz";
pub const MODEL_STRONGEST_URL: &str = "https://github.com/lightvector/KataGo/releases/download/v1.17.1/b11c768h12nbt3tflrs-fson-silu.bin.gz";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum HardwareBackend {
    AppleSiliconMetal,
    MacOsOpenCL,
    NvidiaCuda,
    OpenCLGeneric,
    CpuOnly,
}

impl HardwareBackend {
    pub fn detect_current_platform() -> Self {
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        {
            HardwareBackend::AppleSiliconMetal
        }
        #[cfg(all(target_os = "macos", not(target_arch = "aarch64")))]
        {
            HardwareBackend::MacOsOpenCL
        }
        #[cfg(target_os = "windows")]
        {
            HardwareBackend::NvidiaCuda
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            HardwareBackend::OpenCLGeneric
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            HardwareBackend::AppleSiliconMetal => "Apple Silicon (Metal GPU)",
            HardwareBackend::MacOsOpenCL => "macOS Intel (OpenCL)",
            HardwareBackend::NvidiaCuda => "NVIDIA GPU (CUDA)",
            HardwareBackend::OpenCLGeneric => "Generic GPU (OpenCL)",
            HardwareBackend::CpuOnly => "CPU (Eigen)",
        }
    }

    /// Returns only a release filename verified to exist in the pinned
    /// v1.17.1 asset manifest. KataGo does not publish the previously
    /// advertised macOS archives, and CUDA assets require a deliberate
    /// CUDA/cuDNN compatibility choice, so those platforms must use an
    /// existing local/Homebrew binary until a supported installer is added.
    pub fn download_archive_name(self) -> Option<&'static str> {
        match self {
            HardwareBackend::OpenCLGeneric => Some("katago-v1.17.1-opencl-linux-x64.zip"),
            HardwareBackend::CpuOnly => Some("katago-v1.17.1-eigen-linux-x64.zip"),
            HardwareBackend::AppleSiliconMetal
            | HardwareBackend::MacOsOpenCL
            | HardwareBackend::NvidiaCuda => None,
        }
    }

    pub fn download_url(self) -> Option<String> {
        self.download_archive_name()
            .map(|asset| format!("{KATAGO_OFFICIAL_RELEASE_BASE}{asset}"))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum KataGoModelTier {
    Lightweight,
    Balanced,
    Strongest,
}

impl KataGoModelTier {
    pub fn label(self) -> &'static str {
        match self {
            KataGoModelTier::Lightweight => "Fast Analysis (38 MB)",
            KataGoModelTier::Balanced => "Transformer 10B Balanced (94 MB)",
            KataGoModelTier::Strongest => "Strongest 11B (240 MB)",
        }
    }

    pub fn file_name(self) -> &'static str {
        match self {
            KataGoModelTier::Lightweight => MODEL_LIGHTWEIGHT_NAME,
            KataGoModelTier::Balanced => MODEL_BALANCED_NAME,
            KataGoModelTier::Strongest => MODEL_STRONGEST_NAME,
        }
    }

    pub fn download_url(self) -> &'static str {
        match self {
            KataGoModelTier::Lightweight => MODEL_LIGHTWEIGHT_URL,
            KataGoModelTier::Balanced => MODEL_BALANCED_URL,
            KataGoModelTier::Strongest => MODEL_STRONGEST_URL,
        }
    }
}

/// Generates an optimized GTP configuration for KataGo.
pub fn generate_optimized_gtp_config(threads: usize, max_batch_size: usize) -> String {
    format!(
        r#"# Sabaki managed KataGo config
# schemaVersion = 2
rules = chinese
logDir = katago_logs
logAllGTPCommunication = false
logSearchInfo = false
logSearchInfoForChosenMove = false
logToStderr = false
numSearchThreads = {threads}
nnMaxBatchSize = {max_batch_size}
maxVisits = 500
analysisPVLen = 15
reportAnalysisWinratesAs = BLACK
ponderingEnabled = false
lagBuffer = 0.0
"#
    )
}

/// The only network-and-process seam used to retrieve an official model.
/// Task callers select a tier; this adapter owns transfer mechanics.
pub trait KataGoModelDownloadAdapter {
    fn download_to(&self, url: &str, destination: &Path) -> Result<(), String>;
}

/// Production adapter backed by `curl`. It writes only to the temporary path
/// supplied by the task module, never directly to the live model location.
pub struct CurlKataGoModelDownloadAdapter;

impl KataGoModelDownloadAdapter for CurlKataGoModelDownloadAdapter {
    fn download_to(&self, url: &str, destination: &Path) -> Result<(), String> {
        let status = Command::new("curl")
            .arg("--fail")
            .arg("--silent")
            .arg("--show-error")
            .arg("--location")
            .arg("--max-time")
            .arg("300")
            .arg("--output")
            .arg(destination)
            .arg(url)
            .status()
            .map_err(|error| format!("could not start curl: {error}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("curl exited with status {status}"))
        }
    }
}

#[derive(Debug, Error)]
pub enum KataGoModelInstallError {
    #[error("could not create the KataGo model directory: {0}")]
    CreateDirectory(#[source] std::io::Error),
    #[error("model download failed: {0}")]
    Download(String),
    #[error("downloaded model is empty")]
    EmptyDownload,
    #[error("could not install the downloaded model: {0}")]
    Finalize(#[source] std::io::Error),
}

/// Downloads one official model through the supplied adapter and atomically
/// makes it live only after a successful, non-empty transfer. A failed download
/// leaves an existing model untouched and removes its temporary file.
pub fn install_katago_model_with<A>(
    base: &Path,
    tier: KataGoModelTier,
    downloader: &A,
) -> Result<PathBuf, KataGoModelInstallError>
where
    A: KataGoModelDownloadAdapter,
{
    let models_dir = katago_storage_dir(base).join("models");
    std::fs::create_dir_all(&models_dir).map_err(KataGoModelInstallError::CreateDirectory)?;
    let destination = models_dir.join(tier.file_name());
    let temporary = destination.with_extension("download");
    std::fs::remove_file(&temporary).ok();

    if let Err(error) = downloader.download_to(tier.download_url(), &temporary) {
        std::fs::remove_file(&temporary).ok();
        return Err(KataGoModelInstallError::Download(error));
    }
    let is_empty = std::fs::metadata(&temporary)
        .map_err(KataGoModelInstallError::Finalize)?
        .len()
        == 0;
    if is_empty {
        std::fs::remove_file(&temporary).ok();
        return Err(KataGoModelInstallError::EmptyDownload);
    }

    // Source and destination share a directory, so Unix replacement is atomic.
    #[cfg(not(target_os = "windows"))]
    std::fs::rename(&temporary, &destination).map_err(KataGoModelInstallError::Finalize)?;

    // Windows cannot reliably rename over an existing path. Keep a rollback
    // copy while replacing it so a finalization failure does not lose a model.
    #[cfg(target_os = "windows")]
    {
        let backup = destination.with_extension("previous");
        std::fs::remove_file(&backup).ok();
        if destination.exists() {
            std::fs::rename(&destination, &backup).map_err(KataGoModelInstallError::Finalize)?;
        }
        if let Err(error) = std::fs::rename(&temporary, &destination) {
            if backup.exists() {
                std::fs::rename(&backup, &destination).ok();
            }
            return Err(KataGoModelInstallError::Finalize(error));
        }
        std::fs::remove_file(&backup).ok();
    }
    Ok(destination)
}

/// Downloads one official KataGo model with the production transfer adapter.
pub fn install_katago_model(
    base: &Path,
    tier: KataGoModelTier,
) -> Result<PathBuf, KataGoModelInstallError> {
    install_katago_model_with(base, tier, &CurlKataGoModelDownloadAdapter)
}

/// Resolves the local engine storage root for KataGo assets.
pub fn katago_storage_dir(base: &Path) -> PathBuf {
    base.join("engines").join("katago")
}

/// Discovers an existing `katago` executable in standard system locations or PATH.
pub fn find_katago_executable(storage_dir: Option<&Path>) -> Option<PathBuf> {
    // 1. Check PATH
    if let Ok(output) = std::process::Command::new("which").arg("katago").output()
        && output.status.success()
    {
        let path_str = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if !path_str.is_empty() {
            let path = PathBuf::from(path_str);
            if path.exists() {
                return Some(path);
            }
        }
    }

    // 2. Check macOS Homebrew & standard Unix locations
    let candidates = [
        "/opt/homebrew/bin/katago",
        "/usr/local/bin/katago",
        "/usr/bin/katago",
        "/usr/local/games/katago",
    ];
    for candidate in candidates {
        let path = PathBuf::from(candidate);
        if path.exists() {
            return Some(path);
        }
    }

    // 3. Check local storage directory
    if let Some(storage) = storage_dir {
        let local_bin = if cfg!(target_os = "windows") {
            storage.join("katago.exe")
        } else {
            storage.join("katago")
        };
        if local_bin.exists() {
            return Some(local_bin);
        }
    }

    None
}

/// Result of probing the local KataGo environment.
#[derive(Clone, Debug)]
pub struct KataGoEnvironment {
    pub executable: PathBuf,
    pub executable_exists: bool,
    pub config: PathBuf,
    pub model: PathBuf,
    pub model_exists: bool,
    pub backend: HardwareBackend,
    pub engine_record: EngineRecord,
}

/// Prepares the directories and configuration for KataGo, building an engine record.
pub fn ensure_katago_environment(
    base: &Path,
    tier: KataGoModelTier,
    custom_model_path: Option<&Path>,
) -> Result<KataGoEnvironment, std::io::Error> {
    let engine_dir = katago_storage_dir(base);
    let models_dir = engine_dir.join("models");
    let configs_dir = engine_dir.join("configs");

    std::fs::create_dir_all(&engine_dir)?;
    std::fs::create_dir_all(&models_dir)?;
    std::fs::create_dir_all(&configs_dir)?;

    let config_path = configs_dir.join("default_gtp.cfg");
    let default_config = generate_optimized_gtp_config(8, 16);
    let should_write_config = match std::fs::read_to_string(&config_path) {
        Ok(existing) => {
            existing.starts_with(MANAGED_CONFIG_MARKER)
                && !existing.contains(&format!("# schemaVersion = {MANAGED_CONFIG_VERSION}"))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
        Err(error) => return Err(error),
    };
    if should_write_config {
        std::fs::write(&config_path, default_config)?;
    }

    let backend = HardwareBackend::detect_current_platform();

    let (executable, executable_exists) = match find_katago_executable(Some(&engine_dir)) {
        Some(path) => (path, true),
        None => {
            let default_exe = if cfg!(target_os = "windows") {
                engine_dir.join("katago.exe")
            } else {
                engine_dir.join("katago")
            };
            let exists = default_exe.exists();
            (default_exe, exists)
        }
    };

    let (model, model_exists) = if let Some(custom) = custom_model_path {
        (custom.to_path_buf(), custom.exists())
    } else {
        // A requested tier is an explicit user choice and therefore always
        // wins over arbitrary directory iteration order.
        let tier_model = models_dir.join(tier.file_name());
        if tier_model.is_file() {
            (tier_model, true)
        } else {
            // Preserve compatibility with imported non-tier models, but make
            // the fallback deterministic.
            let mut candidates = std::fs::read_dir(&models_dir)
                .into_iter()
                .flatten()
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| {
                    let name = path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("");
                    path.is_file()
                        && (name.ends_with(".bin.gz")
                            || name.ends_with(".bin")
                            || name.ends_with(".onnx"))
                })
                .collect::<Vec<_>>();
            candidates.sort();
            match candidates.into_iter().next() {
                Some(path) => (path, true),
                None => (tier_model, false),
            }
        }
    };

    let engine_record = build_katago_engine_record(&executable, &model, &config_path, backend);

    Ok(KataGoEnvironment {
        executable,
        executable_exists,
        config: config_path,
        model,
        model_exists,
        backend,
        engine_record,
    })
}

/// Builds an `EngineRecord` ready for registration in Sabaki's `EngineStore`.
pub fn build_katago_engine_record(
    executable_path: &Path,
    model_path: &Path,
    config_path: &Path,
    backend: HardwareBackend,
) -> EngineRecord {
    let name = format!("KataGo ({})", backend.label());
    let args = format!(
        "gtp -model \"{}\" -config \"{}\"",
        model_path.display(),
        config_path.display()
    );
    EngineRecord {
        name,
        path: executable_path.display().to_string(),
        args,
        commands: None,
    }
}

/// Extracts the value following a `-flag "..."` / `-flag '...'` / `-flag ...`
/// argument from an engine command line.
fn extract_arg_value(args: &str, flag: &str) -> Option<String> {
    let tokens = crate::parse_engine_arguments(args);
    let position = tokens.iter().position(|token| token == flag)?;
    tokens.get(position + 1).cloned()
}

/// Scans candidate directories for a KataGo network model, preferring the
/// directory of the stale path first so a re-downloaded / renamed model in the
/// same folder is found.
fn discover_katago_model(stale_path: &Path) -> Option<PathBuf> {
    let mut roots = Vec::new();
    if let Some(parent) = stale_path.parent() {
        roots.push(parent.to_path_buf());
    }
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        roots.push(home.join(".config/ryusei-gpui/plugins/engines/katago/models"));
        roots.push(home.join(".config/ryusei/plugins/engines/katago/models"));
        roots.push(home.join(".config/ryusei-gpui/engines/katago/models"));
        roots.push(home.join(".katago/models"));
    }

    for root in roots {
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let is_model = name.ends_with(".bin.gz")
                || name.ends_with(".txt.gz")
                || name.ends_with(".bin")
                || name.ends_with(".onnx");
            if is_model && path.is_file() {
                return Some(path);
            }
        }
    }
    None
}

/// Repairs a KataGo engine record whose `-model` path no longer exists by
/// discovering a replacement model (e.g. after a re-download changed the
/// filename). Returns a repaired record, or the original when the record is
/// healthy or not a KataGo record.
pub fn repair_katago_engine_record(record: &EngineRecord) -> EngineRecord {
    let is_katago = record.path.to_lowercase().contains("katago")
        || record.name.to_lowercase().contains("katago")
        || record.args.to_lowercase().contains("katago");
    if !is_katago {
        return record.clone();
    }

    let Some(model) = extract_arg_value(&record.args, "-model") else {
        return record.clone();
    };
    let model_path = PathBuf::from(&model);
    if model_path.is_file() {
        return record.clone();
    }

    let Some(replacement) = discover_katago_model(&model_path) else {
        return record.clone();
    };

    let mut repaired = record.clone();
    let config = extract_arg_value(&record.args, "-config");
    repaired.args = match config {
        Some(config) => format!(
            "gtp -model \"{}\" -config \"{}\"",
            replacement.display(),
            config
        ),
        None => format!("gtp -model \"{}\"", replacement.display()),
    };
    repaired
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FixtureDownloader {
        content: Result<Vec<u8>, String>,
    }

    impl KataGoModelDownloadAdapter for FixtureDownloader {
        fn download_to(&self, _url: &str, destination: &Path) -> Result<(), String> {
            let content = self.content.as_ref().map_err(Clone::clone)?;
            std::fs::write(destination, content).map_err(|error| error.to_string())
        }
    }

    #[test]
    fn model_install_is_atomic_from_the_callers_perspective() {
        let base = std::env::temp_dir().join(format!("ryusei-katago-model-{}", std::process::id()));
        std::fs::remove_dir_all(&base).ok();
        let models_dir = katago_storage_dir(&base).join("models");
        std::fs::create_dir_all(&models_dir).expect("model directory exists");
        let destination = models_dir.join(MODEL_LIGHTWEIGHT_NAME);
        std::fs::write(&destination, b"existing-model").expect("existing model writes");

        let failed = install_katago_model_with(
            &base,
            KataGoModelTier::Lightweight,
            &FixtureDownloader {
                content: Err("network unavailable".to_owned()),
            },
        );
        assert!(matches!(failed, Err(KataGoModelInstallError::Download(_))));
        assert_eq!(
            std::fs::read(&destination).expect("old model remains"),
            b"existing-model"
        );

        let installed = install_katago_model_with(
            &base,
            KataGoModelTier::Lightweight,
            &FixtureDownloader {
                content: Ok(b"new-model".to_vec()),
            },
        )
        .expect("successful model download installs");
        assert_eq!(installed, destination);
        assert_eq!(
            std::fs::read(&installed).expect("installed model reads"),
            b"new-model"
        );
        assert!(!installed.with_extension("download").exists());
        std::fs::remove_dir_all(base).ok();
    }

    #[test]
    fn empty_model_download_does_not_replace_an_existing_model() {
        let base = std::env::temp_dir().join(format!("ryusei-katago-empty-{}", std::process::id()));
        std::fs::remove_dir_all(&base).ok();
        let models_dir = katago_storage_dir(&base).join("models");
        std::fs::create_dir_all(&models_dir).expect("model directory exists");
        let destination = models_dir.join(MODEL_BALANCED_NAME);
        std::fs::write(&destination, b"existing-model").expect("existing model writes");

        let result = install_katago_model_with(
            &base,
            KataGoModelTier::Balanced,
            &FixtureDownloader {
                content: Ok(Vec::new()),
            },
        );
        assert!(matches!(
            result,
            Err(KataGoModelInstallError::EmptyDownload)
        ));
        assert_eq!(
            std::fs::read(&destination).expect("old model remains"),
            b"existing-model"
        );
        std::fs::remove_dir_all(base).ok();
    }

    #[test]
    fn release_mapping_only_advertises_verified_platform_assets() {
        assert_eq!(
            HardwareBackend::CpuOnly.download_archive_name(),
            Some("katago-v1.17.1-eigen-linux-x64.zip")
        );
        assert_eq!(
            HardwareBackend::OpenCLGeneric.download_archive_name(),
            Some("katago-v1.17.1-opencl-linux-x64.zip")
        );
        assert_eq!(
            HardwareBackend::AppleSiliconMetal.download_url(),
            None,
            "v1.17.1 has no official macOS Metal release asset"
        );
        assert_eq!(HardwareBackend::MacOsOpenCL.download_url(), None);
    }

    #[test]
    fn requested_model_tier_wins_over_other_models_in_storage() {
        let base = std::env::temp_dir().join(format!(
            "ryusei-katago-tier-selection-{}",
            std::process::id()
        ));
        std::fs::remove_dir_all(&base).ok();
        let models_dir = katago_storage_dir(&base).join("models");
        std::fs::create_dir_all(&models_dir).expect("models directory exists");
        std::fs::write(models_dir.join(MODEL_LIGHTWEIGHT_NAME), b"light")
            .expect("lightweight model writes");
        std::fs::write(models_dir.join(MODEL_STRONGEST_NAME), b"strong")
            .expect("strongest model writes");

        let environment = ensure_katago_environment(&base, KataGoModelTier::Strongest, None)
            .expect("environment prepares");
        assert_eq!(environment.model, models_dir.join(MODEL_STRONGEST_NAME));
        std::fs::remove_dir_all(base).ok();
    }

    #[test]
    fn managed_config_is_upgraded_but_user_config_is_preserved() {
        let base = std::env::temp_dir().join(format!(
            "ryusei-katago-config-upgrade-{}",
            std::process::id()
        ));
        std::fs::remove_dir_all(&base).ok();
        let config_dir = katago_storage_dir(&base).join("configs");
        std::fs::create_dir_all(&config_dir).expect("config directory exists");
        let config = config_dir.join("default_gtp.cfg");
        std::fs::write(
            &config,
            "# Sabaki managed KataGo config\n# schemaVersion = 1\n",
        )
        .expect("old managed config writes");
        ensure_katago_environment(&base, KataGoModelTier::Balanced, None)
            .expect("environment upgrades managed config");
        assert!(
            std::fs::read_to_string(&config)
                .expect("managed config reads")
                .contains("# schemaVersion = 2")
        );

        std::fs::write(&config, "# user config\nnumSearchThreads = 3\n")
            .expect("user config writes");
        ensure_katago_environment(&base, KataGoModelTier::Balanced, None)
            .expect("environment preserves user config");
        assert_eq!(
            std::fs::read_to_string(&config).expect("user config reads"),
            "# user config\nnumSearchThreads = 3\n"
        );
        std::fs::remove_dir_all(base).ok();
    }

    #[test]
    fn generates_valid_gtp_config() {
        let cfg = generate_optimized_gtp_config(8, 16);
        assert!(cfg.contains("rules = chinese"));
        assert!(cfg.contains("logAllGTPCommunication = false"));
        assert!(cfg.contains("logSearchInfo = false"));
        assert!(cfg.contains("numSearchThreads = 8"));
        assert!(cfg.contains("nnMaxBatchSize = 16"));
        assert!(!cfg.contains("useOwnership"));
        assert!(!cfg.contains("useScoreLead"));
    }

    #[test]
    fn builds_complete_engine_record() {
        let exe = PathBuf::from("/usr/local/bin/katago");
        let model = PathBuf::from("/models/b10.bin.gz");
        let cfg = PathBuf::from("/configs/gtp.cfg");
        let record =
            build_katago_engine_record(&exe, &model, &cfg, HardwareBackend::AppleSiliconMetal);
        assert_eq!(record.name, "KataGo (Apple Silicon (Metal GPU))");
        assert_eq!(record.path, "/usr/local/bin/katago");
        assert!(record.args.contains("-model \"/models/b10.bin.gz\""));
        assert!(record.args.contains("-config \"/configs/gtp.cfg\""));
    }

    #[test]
    fn repairs_a_stale_katago_model_path() {
        let base =
            std::env::temp_dir().join(format!("ryusei-katago-repair-{}", std::process::id()));
        std::fs::remove_dir_all(&base).ok();
        let models_dir = base.join("models");
        std::fs::create_dir_all(&models_dir).expect("models dir exists");
        let replacement = models_dir.join("b10c384h6nbttflrs.bin.gz");
        std::fs::write(&replacement, b"model").expect("replacement model writes");

        let stale_model = models_dir.join("b10c512h8nbt3tflrs.bin.gz");
        let config = base.join("default_gtp.cfg");
        let stale_record = EngineRecord::new(
            "KataGo (Apple Silicon (Metal GPU))",
            "/opt/homebrew/bin/katago",
            format!(
                "gtp -model \"{}\" -config \"{}\"",
                stale_model.display(),
                config.display()
            ),
        );

        let repaired = repair_katago_engine_record(&stale_record);
        assert!(repaired.args.contains(&replacement.display().to_string()));
        assert!(!repaired.args.contains("b10c512h8"));
        assert!(repaired.args.contains(&config.display().to_string()));

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn preserves_healthy_quoted_paths_with_spaces() {
        let base = std::env::temp_dir().join(format!(
            "ryusei-katago-path-with-spaces-{}",
            std::process::id()
        ));
        std::fs::remove_dir_all(&base).ok();
        let model_dir = base.join("model directory");
        let config_dir = base.join("config directory");
        std::fs::create_dir_all(&model_dir).expect("model dir exists");
        std::fs::create_dir_all(&config_dir).expect("config dir exists");
        let model = model_dir.join("network.bin.gz");
        let config = config_dir.join("default.cfg");
        std::fs::write(&model, b"model").expect("model writes");
        std::fs::write(&config, b"config").expect("config writes");
        let record = EngineRecord::new(
            "KataGo",
            "/opt/homebrew/bin/katago",
            format!(
                "gtp -model \"{}\" -config \"{}\"",
                model.display(),
                config.display()
            ),
        );
        assert_eq!(repair_katago_engine_record(&record), record);
        std::fs::remove_dir_all(base).ok();
    }

    #[test]
    fn leaves_healthy_and_non_katago_records_unchanged() {
        // Healthy model path is untouched.
        let base =
            std::env::temp_dir().join(format!("ryusei-katago-healthy-{}", std::process::id()));
        std::fs::remove_dir_all(&base).ok();
        let models_dir = base.join("models");
        std::fs::create_dir_all(&models_dir).expect("models dir exists");
        let model = models_dir.join("b10.bin.gz");
        std::fs::write(&model, b"model").expect("model writes");
        let config = base.join("gtp.cfg");
        let healthy = EngineRecord::new(
            "KataGo",
            "/opt/homebrew/bin/katago",
            format!(
                "gtp -model \"{}\" -config \"{}\"",
                model.display(),
                config.display()
            ),
        );
        assert_eq!(repair_katago_engine_record(&healthy), healthy);

        // Non-KataGo engines are never rewritten.
        let leela = EngineRecord::new(
            "Leela Zero",
            "/usr/bin/leelaz",
            "-g -w /weights/lz.gz".to_owned(),
        );
        assert_eq!(repair_katago_engine_record(&leela), leela);

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn ensures_katago_environment_creates_directories_and_config() {
        let temp_dir =
            std::env::temp_dir().join(format!("ryusei_katago_test_{}", std::process::id()));
        let env = ensure_katago_environment(&temp_dir, KataGoModelTier::Balanced, None)
            .expect("environment creates successfully");
        assert!(env.config.exists(), "config file must be generated");
        assert!(
            env.engine_record.args.contains("gtp -model"),
            "engine record must format valid gtp args"
        );
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
