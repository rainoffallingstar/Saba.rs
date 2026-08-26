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

use serde_json::Value;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::engine_workflow::EngineRecord;

pub const KATAGO_OFFICIAL_RELEASE_BASE: &str =
    "https://github.com/lightvector/KataGo/releases/download/v1.17.1/";
const MANAGED_CONFIG_MARKER: &str = "# Sabaki managed KataGo config";
const MANAGED_CONFIG_VERSION: u32 = 2;
pub const KATAGO_RELEASES_LATEST_API: &str =
    "https://api.github.com/repos/lightvector/KataGo/releases/latest";
/// Official continuously-updated KataGo self-play network catalog.
pub const KATAGO_OFFICIAL_WEIGHTS_PAGE: &str = "https://katagotraining.org/networks/";
/// Official supplemental catalog, used only for special-purpose networks such
/// as HumanSL that are intentionally not part of the main self-play run.
pub const KATAGO_OFFICIAL_EXTRA_WEIGHTS_PAGE: &str = "https://katagotraining.org/extra_networks/";
pub const KATAGO_UNIFIED_MODEL_NAME: &str = "active-model.bin.gz";
pub const KATAGO_HUMAN_SL_CONFIG_NAME: &str = "human_sl_gtp.cfg";

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

/// Returns whether a HumanSL profile is in KataGo's documented 20K–9D range.
/// Both the modern `rank_` and historical `preaz_` families are supported.
pub fn is_valid_human_sl_profile(profile: &str) -> bool {
    let normalized = profile.to_ascii_lowercase();
    let Some((family, value)) = normalized.split_once('_') else {
        return false;
    };
    if family != "rank" && family != "preaz" {
        return false;
    }
    if let Some(rank) = value.strip_suffix('k') {
        return rank
            .parse::<u8>()
            .is_ok_and(|rank| (1..=20).contains(&rank));
    }
    if let Some(rank) = value.strip_suffix('d') {
        return rank.parse::<u8>().is_ok_and(|rank| (1..=9).contains(&rank));
    }
    false
}

/// Produces all selectable HumanSL profiles, from 20K through 9D, for a
/// profile picker without hard-coding a partial strength ladder in the UI.
pub fn human_sl_profiles() -> Vec<String> {
    let mut profiles = Vec::with_capacity(58);
    for family in ["rank", "preaz"] {
        for rank in (1..=20).rev() {
            profiles.push(format!("{family}_{rank}k"));
        }
        for rank in 1..=9 {
            profiles.push(format!("{family}_{rank}d"));
        }
    }
    profiles
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum KataGoModelTier {
    Lightweight,
    Balanced,
    Strongest,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct KataGoReleaseAsset {
    pub name: String,
    pub download_url: String,
    pub size: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct KataGoReleaseInfo {
    pub version: String,
    pub url: String,
    pub assets: Vec<KataGoReleaseAsset>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct KataGoWeightInfo {
    pub name: String,
    pub path: PathBuf,
    pub download_url: Option<String>,
    pub installed: bool,
    pub active: bool,
    pub size: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct KataGoLocalInfo {
    pub executable: PathBuf,
    pub executable_exists: bool,
    pub version: Option<String>,
    pub config: PathBuf,
    pub unified_model: PathBuf,
    pub active_model: Option<PathBuf>,
    pub weights: Vec<KataGoWeightInfo>,
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

/// Generates a dedicated HumanSL configuration. A HumanSL model is supplied as
/// `-human-model` alongside a normal KataGo strength model and must not share
/// the standard analysis configuration.
pub fn generate_human_sl_gtp_config(profile: &str) -> String {
    let profile = if is_valid_human_sl_profile(profile) {
        profile.to_ascii_lowercase()
    } else {
        "rank_5k".to_owned()
    };
    format!(
        r#"# Sabaki managed KataGo HumanSL config
# schemaVersion = 1
rules = chinese
logDir = katago_logs
logAllGTPCommunication = false
logSearchInfo = false
logSearchInfoForChosenMove = false
logToStderr = false
numSearchThreads = 1
nnMaxBatchSize = 16
maxVisits = 40
analysisPVLen = 15
reportAnalysisWinratesAs = BLACK
ponderingEnabled = false
lagBuffer = 1.0
allowResignation = true
resignThreshold = -0.99
resignConsecTurns = 20
humanSLProfile = {profile}
humanSLChosenMoveProp = 1.0
humanSLChosenMoveIgnorePass = true
humanSLChosenMovePiklLambda = 100000000
humanSLRootExploreProbWeightless = 0.0
humanSLRootExploreProbWeightful = 0.0
humanSLPlaExploreProbWeightless = 0.0
humanSLPlaExploreProbWeightful = 0.0
humanSLOppExploreProbWeightless = 0.0
humanSLOppExploreProbWeightful = 0.0
humanSLCpuctExploration = 0.50
humanSLCpuctPermanent = 0.2
chosenMoveTemperatureEarly = 0.85
chosenMoveTemperature = 0.70
chosenMoveTemperatureHalflife = 80
chosenMoveTemperatureOnlyBelowProb = 0.01
ignorePreRootHistory = false
analysisIgnorePreRootHistory = false
rootNumSymmetriesToSample = 2
useLcbForSelection = false
winLossUtilityFactor = 1.0
staticScoreUtilityFactor = 0.30
dynamicScoreUtilityFactor = 0.00
useUncertainty = false
subtreeValueBiasFactor = 0.0
useNoisePruning = false
"#
    )
}

/// Fetches the official latest KataGo release metadata from GitHub.
pub fn fetch_katago_latest_release() -> Result<KataGoReleaseInfo, String> {
    let output = Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--max-time",
            "30",
            "--header",
            "Accept: application/vnd.github+json",
            "--user-agent",
            "Ryusei-KataGo-Panel",
            KATAGO_RELEASES_LATEST_API,
        ])
        .output()
        .map_err(|error| format!("could not start curl: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "latest KataGo release request failed: {}",
            output.status
        ));
    }
    parse_katago_release_json(&String::from_utf8_lossy(&output.stdout))
}

pub fn parse_katago_release_json(json: &str) -> Result<KataGoReleaseInfo, String> {
    let value: Value = serde_json::from_str(json).map_err(|error| error.to_string())?;
    let version = value
        .get("tag_name")
        .and_then(Value::as_str)
        .ok_or_else(|| "KataGo release response has no tag_name".to_owned())?
        .to_owned();
    let url = value
        .get("html_url")
        .and_then(Value::as_str)
        .unwrap_or("https://github.com/lightvector/KataGo/releases")
        .to_owned();
    let assets = value
        .get("assets")
        .and_then(Value::as_array)
        .ok_or_else(|| "KataGo release response has no assets".to_owned())?
        .iter()
        .filter_map(|asset| {
            Some(KataGoReleaseAsset {
                name: asset.get("name")?.as_str()?.to_owned(),
                download_url: asset.get("browser_download_url")?.as_str()?.to_owned(),
                size: asset
                    .get("size")
                    .and_then(Value::as_u64)
                    .unwrap_or_default(),
            })
        })
        .collect();
    Ok(KataGoReleaseInfo {
        version,
        url,
        assets,
    })
}

/// Fetches the official, continuously-updated KataGo self-play network page.
/// GitHub releases remain authoritative for engine binaries and versions.
pub fn fetch_katago_official_weights() -> Result<Vec<KataGoReleaseAsset>, String> {
    fetch_katago_weight_page(KATAGO_OFFICIAL_WEIGHTS_PAGE)
}

/// Fetches only HumanSL entries from KataGo's supplemental official catalog.
/// These models need `-human-model` and a HumanSL configuration; callers must
/// not place them behind the normal `active-model.bin.gz` link.
pub fn fetch_katago_human_sl_weights() -> Result<Vec<KataGoReleaseAsset>, String> {
    Ok(
        fetch_katago_weight_page(KATAGO_OFFICIAL_EXTRA_WEIGHTS_PAGE)?
            .into_iter()
            .filter(|asset| is_human_sl_weight_name(&asset.name))
            .collect(),
    )
}

fn fetch_katago_weight_page(page: &str) -> Result<Vec<KataGoReleaseAsset>, String> {
    let output = Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--max-time",
            "30",
            "--user-agent",
            "Ryusei-KataGo-Panel",
            page,
        ])
        .output()
        .map_err(|error| format!("could not start curl: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "official KataGo weight page request failed: {}",
            output.status
        ));
    }
    Ok(parse_katago_weight_html(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

/// Extracts downloadable network files from KataGo Training's server-rendered
/// catalog markup. Kept pure because catalog page layout changes should be
/// caught with fixtures rather than discovered during a download.
pub fn parse_katago_weight_html(html: &str) -> Vec<KataGoReleaseAsset> {
    let mut assets = Vec::new();
    for quote in ['\"', '\''] {
        let marker = format!("href={quote}");
        let mut remainder = html;
        while let Some(start) = remainder.find(&marker) {
            remainder = &remainder[start + marker.len()..];
            let Some(end) = remainder.find(quote) else {
                break;
            };
            let href = &remainder[..end];
            remainder = &remainder[end + 1..];
            if !(href.ends_with(".bin.gz") || href.ends_with(".txt.gz") || href.ends_with(".onnx"))
            {
                continue;
            }
            let url = if href.starts_with("http://") || href.starts_with("https://") {
                href.to_owned()
            } else {
                format!("https://katagotraining.org{href}")
            };
            let name = url.rsplit('/').next().unwrap_or_default().to_owned();
            if !name.is_empty()
                && !assets
                    .iter()
                    .any(|asset: &KataGoReleaseAsset| asset.name == name)
            {
                assets.push(KataGoReleaseAsset {
                    name,
                    download_url: url,
                    size: 0,
                });
            }
        }
    }
    assets.sort_by(|left, right| left.name.cmp(&right.name));
    assets
}

/// HumanSL file naming is part of the published distribution contract. Keep
/// the check conservative so ordinary models never receive HumanSL startup
/// flags by accident.
pub fn is_human_sl_weight_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.contains("humanv") || name.contains("human-sl") || name.contains("humansl")
}

/// Reports local KataGo, installed weights, and the active unified model link.
pub fn inspect_katago_local(base: &Path) -> Result<KataGoLocalInfo, std::io::Error> {
    let engine_dir = katago_storage_dir(base);
    let models_dir = engine_dir.join("models");
    let configs_dir = engine_dir.join("configs");
    std::fs::create_dir_all(&models_dir)?;
    std::fs::create_dir_all(&configs_dir)?;
    let executable = find_katago_executable(Some(&engine_dir)).unwrap_or_else(|| {
        if cfg!(target_os = "windows") {
            engine_dir.join("katago.exe")
        } else {
            engine_dir.join("katago")
        }
    });
    let version = executable
        .is_file()
        .then(|| probe_katago_version(&executable));
    let unified_model = models_dir.join(KATAGO_UNIFIED_MODEL_NAME);
    let active_model = std::fs::read_link(&unified_model).ok().map(|target| {
        if target.is_absolute() {
            target
        } else {
            models_dir.join(target)
        }
    });
    let mut weights = std::fs::read_dir(&models_dir)?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        (name.ends_with(".bin.gz")
                            || name.ends_with(".txt.gz")
                            || name.ends_with(".onnx"))
                            && name != KATAGO_UNIFIED_MODEL_NAME
                    })
        })
        .map(|path| {
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let size = std::fs::metadata(&path).ok().map(|metadata| metadata.len());
            KataGoWeightInfo {
                active: active_model.as_ref().is_some_and(|active| active == &path),
                installed: true,
                name,
                path,
                download_url: None,
                size,
            }
        })
        .collect::<Vec<_>>();
    weights.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(KataGoLocalInfo {
        executable_exists: executable.is_file(),
        executable,
        version: version.and_then(|result| result.ok()),
        config: configs_dir.join("default_gtp.cfg"),
        unified_model,
        active_model,
        weights,
    })
}

fn probe_katago_version(executable: &Path) -> Result<String, String> {
    let output = Command::new(executable)
        .arg("version")
        .output()
        .map_err(|error| error.to_string())?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    text.lines()
        .find(|line| line.to_ascii_lowercase().contains("katago v"))
        .map(str::trim)
        .map(ToOwned::to_owned)
        .ok_or_else(|| "KataGo version output was not recognized".to_owned())
}

pub fn merge_katago_weight_catalog(
    local: &KataGoLocalInfo,
    release: &KataGoReleaseInfo,
) -> Vec<KataGoWeightInfo> {
    let mut weights = local.weights.clone();
    for asset in &release.assets {
        if !(asset.name.ends_with(".bin.gz")
            || asset.name.ends_with(".txt.gz")
            || asset.name.ends_with(".onnx"))
        {
            continue;
        }
        if let Some(existing) = weights.iter_mut().find(|weight| weight.name == asset.name) {
            existing.download_url = Some(asset.download_url.clone());
            existing.size = Some(asset.size);
        } else {
            weights.push(KataGoWeightInfo {
                name: asset.name.clone(),
                path: local
                    .weights
                    .iter()
                    .find(|weight| weight.name == asset.name)
                    .map(|weight| weight.path.clone())
                    .unwrap_or_else(|| local.unified_model.with_file_name(&asset.name)),
                download_url: Some(asset.download_url.clone()),
                installed: false,
                active: false,
                size: Some(asset.size),
            });
        }
    }
    weights.sort_by(|left, right| left.name.cmp(&right.name));
    weights
}

/// Makes the stable model path point to one downloaded weight file.
pub fn set_active_katago_model(base: &Path, model: &Path) -> Result<PathBuf, String> {
    let models_dir = katago_storage_dir(base).join("models");
    std::fs::create_dir_all(&models_dir).map_err(|error| error.to_string())?;
    let model = if model.is_absolute() {
        model.to_path_buf()
    } else {
        models_dir.join(model)
    };
    if !model.is_file() {
        return Err(format!(
            "KataGo weight file does not exist: {}",
            model.display()
        ));
    }
    let link = models_dir.join(KATAGO_UNIFIED_MODEL_NAME);
    let _ = std::fs::remove_file(&link);
    #[cfg(unix)]
    std::os::unix::fs::symlink(model.file_name().unwrap_or_default(), &link)
        .map_err(|error| format!("could not create KataGo model symlink: {error}"))?;
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(model.file_name().unwrap_or_default(), &link)
        .map_err(|error| format!("could not create KataGo model symlink (enable Developer Mode or run elevated): {error}"))?;
    Ok(link)
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

pub fn download_katago_weight(base: &Path, asset: &KataGoReleaseAsset) -> Result<PathBuf, String> {
    let models_dir = katago_storage_dir(base).join("models");
    std::fs::create_dir_all(&models_dir).map_err(|error| error.to_string())?;
    let destination = models_dir.join(&asset.name);
    let temporary = destination.with_extension("download");
    CurlKataGoModelDownloadAdapter.download_to(&asset.download_url, &temporary)?;
    if !temporary.is_file()
        || std::fs::metadata(&temporary)
            .map_err(|error| error.to_string())?
            .len()
            == 0
    {
        let _ = std::fs::remove_file(&temporary);
        return Err(format!("downloaded KataGo weight is empty: {}", asset.name));
    }
    #[cfg(target_os = "windows")]
    std::fs::remove_file(&destination).ok();
    std::fs::rename(&temporary, &destination).map_err(|error| error.to_string())?;
    Ok(destination)
}

/// Selects a release archive suitable for the current supported update target.
pub fn select_katago_binary_asset(release: &KataGoReleaseInfo) -> Option<KataGoReleaseAsset> {
    let target = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        return None;
    };
    let mut candidates = release
        .assets
        .iter()
        .filter(|asset| {
            let name = asset.name.to_ascii_lowercase();
            name.contains(target) && (name.ends_with(".zip") || name.ends_with(".tar.gz"))
        })
        .cloned()
        .collect::<Vec<_>>();
    candidates.sort_by_key(|asset| {
        let name = asset.name.to_ascii_lowercase();
        (!name.contains("x64"), !name.contains("eigen"), name)
    });
    candidates.into_iter().next()
}

/// Downloads and installs the latest supported Windows/Linux KataGo archive.
/// macOS deliberately returns `None` from `select_katago_binary_asset` because
/// official releases do not publish a stable macOS archive for this workflow.
pub fn update_katago_binary(base: &Path, release: &KataGoReleaseInfo) -> Result<PathBuf, String> {
    let asset = select_katago_binary_asset(release).ok_or_else(|| {
        "official KataGo binary updates are supported on Windows/Linux only".to_owned()
    })?;
    let engine_dir = katago_storage_dir(base);
    let staging = engine_dir.join(".binary-update-staging");
    std::fs::remove_dir_all(&staging).ok();
    std::fs::create_dir_all(&staging).map_err(|error| error.to_string())?;
    let archive = staging.join(&asset.name);
    CurlKataGoModelDownloadAdapter.download_to(&asset.download_url, &archive)?;
    let status = if asset.name.ends_with(".zip") {
        if cfg!(target_os = "windows") {
            let command = format!(
                "Expand-Archive -LiteralPath '{}' -DestinationPath '{}' -Force",
                archive.display(),
                staging.display()
            );
            Command::new("powershell")
                .args(["-NoProfile", "-Command"])
                .arg(command)
                .status()
        } else {
            Command::new("unzip")
                .args(["-q", "-o"])
                .arg(&archive)
                .arg(&staging)
                .status()
        }
    } else {
        Command::new("tar")
            .args(["-xzf"])
            .arg(&archive)
            .arg("-C")
            .arg(&staging)
            .status()
    }
    .map_err(|error| format!("could not extract KataGo archive: {error}"))?;
    if !status.success() {
        return Err(format!("could not extract KataGo archive: {status}"));
    }
    let executable_name = if cfg!(target_os = "windows") {
        "katago.exe"
    } else {
        "katago"
    };
    let executable = find_file_recursive(&staging, executable_name)
        .ok_or_else(|| format!("KataGo archive did not contain {executable_name}"))?;
    let destination = engine_dir.join(executable_name);
    std::fs::copy(&executable, &destination).map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&destination)
            .map_err(|error| error.to_string())?
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&destination, permissions).map_err(|error| error.to_string())?;
    }
    std::fs::remove_dir_all(staging).ok();
    Ok(destination)
}

fn find_file_recursive(root: &Path, file_name: &str) -> Option<PathBuf> {
    let entries = std::fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.file_name().and_then(|name| name.to_str()) == Some(file_name) && path.is_file() {
            return Some(path);
        }
        if path.is_dir()
            && let Some(found) = find_file_recursive(&path, file_name)
        {
            return Some(found);
        }
    }
    None
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
    } else if models_dir.join(KATAGO_UNIFIED_MODEL_NAME).is_file() {
        // The stable link is the single model path used by engine records.
        // Switching weights only changes this link, never the engine config.
        (models_dir.join(KATAGO_UNIFIED_MODEL_NAME), true)
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

/// Builds the dedicated GTP record required by the official HumanSL startup
/// mode. The normal model supplies KataGo's evaluation/search; the HumanSL
/// model supplies human-move prediction via `-human-model`.
pub fn build_katago_human_sl_engine_record(
    executable_path: &Path,
    normal_model_path: &Path,
    human_model_path: &Path,
    config_path: &Path,
    backend: HardwareBackend,
) -> EngineRecord {
    EngineRecord {
        name: format!("KataGo HumanSL ({})", backend.label()),
        path: executable_path.display().to_string(),
        args: format!(
            "gtp -model \"{}\" -human-model \"{}\" -config \"{}\"",
            normal_model_path.display(),
            human_model_path.display(),
            config_path.display(),
        ),
        commands: None,
    }
}

/// Creates a HumanSL engine record and its managed profile configuration.
/// `normal_model_path` must be a standard KataGo network; using the HumanSL
/// file for both flags is rejected early with an actionable error.
/// Finds an installed regular KataGo model suitable as the `-model` half of
/// HumanSL. The active symlink is deliberately skipped because an older Ryusei
/// version may have pointed it at the HumanSL file itself.
pub fn find_installed_normal_katago_model(base: &Path) -> Option<PathBuf> {
    let models_dir = katago_storage_dir(base).join("models");
    let mut candidates = std::fs::read_dir(models_dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            path.is_file()
                && name != KATAGO_UNIFIED_MODEL_NAME
                && (name.ends_with(".bin.gz")
                    || name.ends_with(".txt.gz")
                    || name.ends_with(".onnx"))
                && !is_human_sl_weight_name(name)
        })
        .collect::<Vec<_>>();
    candidates.sort();
    candidates.into_iter().next()
}

pub fn prepare_katago_human_sl_engine(
    base: &Path,
    normal_model_path: &Path,
    human_model_path: &Path,
    profile: &str,
) -> Result<EngineRecord, String> {
    if !is_valid_human_sl_profile(profile) {
        return Err(format!(
            "unsupported HumanSL profile: {profile}; choose rank_20k..rank_1k or rank_1d..rank_9d"
        ));
    }
    if is_human_sl_weight_name(
        normal_model_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default(),
    ) {
        return Err("choose a normal KataGo network as the HumanSL base model; the HumanSL file belongs only after -human-model".to_owned());
    }
    if !normal_model_path.is_file() {
        return Err(format!(
            "normal KataGo model was not found: {}",
            normal_model_path.display()
        ));
    }
    if !human_model_path.is_file() {
        return Err(format!(
            "HumanSL model was not found: {}",
            human_model_path.display()
        ));
    }
    if !is_human_sl_weight_name(
        human_model_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default(),
    ) {
        return Err(
            "the selected human model is not recognized as an official HumanSL weight".to_owned(),
        );
    }
    let environment =
        ensure_katago_environment(base, KataGoModelTier::Balanced, Some(normal_model_path))
            .map_err(|error| error.to_string())?;
    let config_path = katago_storage_dir(base)
        .join("configs")
        .join(KATAGO_HUMAN_SL_CONFIG_NAME);
    std::fs::write(&config_path, generate_human_sl_gtp_config(profile))
        .map_err(|error| format!("could not write HumanSL config: {error}"))?;
    Ok(build_katago_human_sl_engine_record(
        &environment.executable,
        normal_model_path,
        human_model_path,
        &config_path,
        environment.backend,
    ))
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
        // Keep both the immediately previous Ryusei layout and the older
        // Sabaki layout discoverable. The project rename must not strand a
        // valid downloaded model behind a stale engine record.
        roots.push(home.join(".config/ryusei-gpui/plugins/engines/katago/models"));
        roots.push(home.join(".config/ryusei/plugins/engines/katago/models"));
        roots.push(home.join(".config/ryusei-gpui/engines/katago/models"));
        roots.push(home.join(".config/sabaki-gpui/plugins/engines/katago/models"));
        roots.push(home.join(".config/sabaki-gpui/engines/katago/models"));
        roots.push(home.join(".config/saba-rs/plugins/engines/katago/models"));
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

/// Validates the local resources needed before starting a KataGo GTP process.
///
/// A missing model or config makes KataGo abort before it can emit the first
/// GTP response. Checking these paths before spawning avoids reporting that
/// process failure as an opaque `UnexpectedEndOfStream` handshake error.
pub fn validate_katago_engine_record(record: &EngineRecord) -> Result<(), String> {
    let is_katago = record.path.to_lowercase().contains("katago")
        || record.name.to_lowercase().contains("katago")
        || record.args.to_lowercase().contains("katago");
    if !is_katago {
        return Ok(());
    }

    let executable = Path::new(&record.path);
    if executable.components().count() > 1 && !executable.is_file() {
        return Err(format!(
            "KataGo executable was not found: {}",
            executable.display()
        ));
    }

    let model = extract_arg_value(&record.args, "-model")
        .ok_or_else(|| "KataGo engine arguments are missing `-model <path>`".to_owned())?;
    let model_path = PathBuf::from(&model);
    if !model_path.is_file() {
        return Err(format!(
            "KataGo model was not found: {}. Run KataGo setup/download or repair the engine record.",
            model_path.display()
        ));
    }

    if let Some(human_model) = extract_arg_value(&record.args, "-human-model") {
        let human_model_path = PathBuf::from(&human_model);
        if !human_model_path.is_file() {
            return Err(format!(
                "KataGo HumanSL model was not found: {}. Download the HumanSL weight again.",
                human_model_path.display()
            ));
        }
    }

    if let Some(config) = extract_arg_value(&record.args, "-config") {
        let config_path = PathBuf::from(&config);
        if !config_path.is_file() {
            return Err(format!(
                "KataGo config was not found: {}",
                config_path.display()
            ));
        }
    }

    Ok(())
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
    fn parses_main_catalog_and_identifies_human_sl_weights() {
        let assets = parse_katago_weight_html(
            r#"<a href="https://media.katagotraining.org/latest.bin.gz">main</a>
                <a href='/uploaded/networks/models_extra/b18c384nbt-humanv0.bin.gz'>human</a>
                <a href='skip.zip'>checkpoint</a>"#,
        );
        assert_eq!(assets.len(), 2);
        assert!(assets.iter().any(|asset| asset.name == "latest.bin.gz"));
        assert!(
            assets
                .iter()
                .any(|asset| is_human_sl_weight_name(&asset.name))
        );
    }

    #[test]
    fn validates_all_documented_human_sl_rank_ranges() {
        assert!(is_valid_human_sl_profile("rank_20k"));
        assert!(is_valid_human_sl_profile("rank_1k"));
        assert!(is_valid_human_sl_profile("rank_9d"));
        assert!(is_valid_human_sl_profile("preaz_1d"));
        assert_eq!(human_sl_profiles().len(), 58);
        assert!(!is_valid_human_sl_profile("rank_21k"));
        assert!(!is_valid_human_sl_profile("rank_10d"));
        assert!(!is_valid_human_sl_profile("rank_5k_extra"));
    }

    #[test]
    fn human_sl_record_uses_normal_and_human_model_flags() {
        let base = std::env::temp_dir().join(format!("ryusei-human-sl-{}", std::process::id()));
        std::fs::remove_dir_all(&base).ok();
        let models = katago_storage_dir(&base).join("models");
        std::fs::create_dir_all(&models).expect("models directory exists");
        let normal = models.join("normal.bin.gz");
        let human = models.join("b18c384nbt-humanv0.bin.gz");
        std::fs::write(&normal, b"normal").expect("normal model writes");
        std::fs::write(&human, b"human").expect("human model writes");

        let record = prepare_katago_human_sl_engine(&base, &normal, &human, "rank_5k")
            .expect("HumanSL record builds");
        assert!(
            record
                .args
                .contains(&format!("-model \"{}\"", normal.display()))
        );
        assert!(
            record
                .args
                .contains(&format!("-human-model \"{}\"", human.display()))
        );
        let config = katago_storage_dir(&base)
            .join("configs")
            .join(KATAGO_HUMAN_SL_CONFIG_NAME);
        let config_contents = std::fs::read_to_string(config).expect("HumanSL config writes");
        assert!(config_contents.contains("humanSLProfile = rank_5k"));
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn parses_official_release_metadata_and_filters_weight_assets() {
        let release = parse_katago_release_json(
            r#"{
                "tag_name":"v1.18.0",
                "html_url":"https://github.com/lightvector/KataGo/releases/tag/v1.18.0",
                "assets":[
                    {"name":"b10.bin.gz","browser_download_url":"https://example.invalid/b10.bin.gz","size":42},
                    {"name":"katago-v1.18.0-linux-x64-eigen.zip","browser_download_url":"https://example.invalid/katago.zip","size":99},
                    {"name":"checksums.txt","browser_download_url":"https://example.invalid/checksums.txt","size":1}
                ]
            }"#,
        )
        .expect("release metadata parses");
        assert_eq!(release.version, "v1.18.0");
        assert_eq!(release.assets.len(), 3);
        let local = KataGoLocalInfo {
            executable: PathBuf::from("katago"),
            executable_exists: false,
            version: None,
            config: PathBuf::from("config.cfg"),
            unified_model: PathBuf::from("active-model.bin.gz"),
            active_model: None,
            weights: Vec::new(),
        };
        let weights = merge_katago_weight_catalog(&local, &release);
        assert_eq!(weights.len(), 1);
        assert_eq!(weights[0].name, "b10.bin.gz");
    }

    #[test]
    fn selects_a_supported_windows_or_linux_binary_asset() {
        let release = KataGoReleaseInfo {
            version: "v1.18.1".to_owned(),
            url: String::new(),
            assets: vec![
                KataGoReleaseAsset {
                    name: "katago-v1.18.1-eigen-linux-x64.zip".to_owned(),
                    download_url: String::new(),
                    size: 1,
                },
                KataGoReleaseAsset {
                    name: "katago-v1.18.1-eigen-windows-x64.zip".to_owned(),
                    download_url: String::new(),
                    size: 1,
                },
            ],
        };
        let selected = select_katago_binary_asset(&release);
        if cfg!(target_os = "windows") {
            assert_eq!(
                selected.map(|asset| asset.name),
                Some("katago-v1.18.1-eigen-windows-x64.zip".to_owned())
            );
        } else if cfg!(target_os = "linux") {
            assert_eq!(
                selected.map(|asset| asset.name),
                Some("katago-v1.18.1-eigen-linux-x64.zip".to_owned())
            );
        } else {
            assert!(selected.is_none());
        }
    }

    #[test]
    fn creates_a_stable_active_model_link() {
        let base = std::env::temp_dir().join(format!("ryusei-katago-link-{}", std::process::id()));
        std::fs::remove_dir_all(&base).ok();
        let models = katago_storage_dir(&base).join("models");
        std::fs::create_dir_all(&models).expect("models directory exists");
        let selected = models.join("selected.bin.gz");
        std::fs::write(&selected, b"weight").expect("weight exists");
        let link = set_active_katago_model(&base, &selected).expect("active link creates");
        assert_eq!(
            std::fs::read_link(link).expect("active link target"),
            PathBuf::from("selected.bin.gz")
        );
        std::fs::remove_dir_all(base).ok();
    }

    #[test]
    fn rejects_katago_records_with_missing_startup_resources() {
        let base =
            std::env::temp_dir().join(format!("ryusei-katago-preflight-{}", std::process::id()));
        std::fs::remove_dir_all(&base).ok();
        std::fs::create_dir_all(&base).expect("preflight directory exists");
        let executable = std::env::current_exe().expect("test executable exists");
        let record = EngineRecord::new(
            "KataGo",
            executable.display().to_string(),
            format!(
                "gtp -model \"{}\" -config \"{}\"",
                base.join("missing-model.bin.gz").display(),
                base.join("missing.cfg").display()
            ),
        );

        let error = validate_katago_engine_record(&record).expect_err("missing model is rejected");
        assert!(error.contains("KataGo model was not found"));
        assert!(error.contains("missing-model.bin.gz"));
        std::fs::remove_dir_all(base).ok();
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
