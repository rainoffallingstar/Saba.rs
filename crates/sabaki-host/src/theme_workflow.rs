//! Theme package workflow (design §8.2).
//!
//! A theme package is a directory (or restricted archive) with
//! `theme.json`, `tokens.json` and allowed asset files:
//!
//! ```text
//! theme-id/
//!   theme.json
//!   tokens.json
//!   board.png
//!   black-stone.png
//!   white-stone.png
//! ```
//!
//! The host validates everything before a theme can be applied: manifest
//! fields, token schema version and hex colors, asset paths staying inside
//! the theme directory, allowed asset types and size limits. `theme.css` is
//! never loaded; legacy `.asar` themes are detected and reported with a
//! migration notice, never executed or unpacked.

use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const THEME_SCHEMA_VERSION: u32 = 1;
pub const THEME_TOKEN_SCHEMA_VERSION: u32 = 1;

/// Maximum size of a single theme asset (10 MiB).
pub const MAX_THEME_ASSET_BYTES: u64 = 10 * 1024 * 1024;

/// Allowed asset extensions (lowercase, without the dot).
pub const ALLOWED_THEME_ASSET_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "svg", "woff", "woff2", "ttf", "otf",
];

/// Maximum size of the tokens document (256 KiB).
pub const MAX_THEME_TOKENS_BYTES: u64 = 256 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ThemeColor {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl ThemeColor {
    pub fn rgb_u32(self) -> u32 {
        ((self.red as u32) << 16) | ((self.green as u32) << 8) | (self.blue as u32)
    }
}

pub fn parse_hex_color(input: &str) -> Result<ThemeColor, String> {
    let hex = input.strip_prefix('#').unwrap_or(input);
    if hex.len() != 6 || !hex.chars().all(|character| character.is_ascii_hexdigit()) {
        return Err(format!("{input:?} is not a #RRGGBB color"));
    }
    let value = u32::from_str_radix(hex, 16)
        .map_err(|error| format!("{input:?} is not a valid hex color: {error}"))?;
    Ok(ThemeColor {
        red: ((value >> 16) & 0xff) as u8,
        green: ((value >> 8) & 0xff) as u8,
        blue: (value & 0xff) as u8,
    })
}

/// The versioned theme-token set applied by the render layer. The host
/// validates the schema version and every color at load time, so the render
/// layer can parse colors without error handling.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeTokens {
    pub schema_version: u32,
    pub board_wood: String,
    pub board_line: String,
    pub star_point: String,
    pub stone_black: String,
    pub stone_white: String,
    pub background: String,
}

impl ThemeTokens {
    pub fn parse(json: &str) -> Result<Self, String> {
        let tokens: Self =
            serde_json::from_str(json).map_err(|error| format!("invalid theme tokens: {error}"))?;
        if tokens.schema_version != THEME_TOKEN_SCHEMA_VERSION {
            return Err(format!(
                "unsupported theme-token schema version {}",
                tokens.schema_version
            ));
        }
        for color in [
            &tokens.board_wood,
            &tokens.board_line,
            &tokens.star_point,
            &tokens.stone_black,
            &tokens.stone_white,
            &tokens.background,
        ] {
            parse_hex_color(color)?;
        }
        Ok(tokens)
    }

    pub fn board_wood_color(&self) -> ThemeColor {
        parse_hex_color(&self.board_wood).expect("theme tokens were validated on parse")
    }

    pub fn board_line_color(&self) -> ThemeColor {
        parse_hex_color(&self.board_line).expect("theme tokens were validated on parse")
    }

    pub fn star_point_color(&self) -> ThemeColor {
        parse_hex_color(&self.star_point).expect("theme tokens were validated on parse")
    }

    pub fn stone_black_color(&self) -> ThemeColor {
        parse_hex_color(&self.stone_black).expect("theme tokens were validated on parse")
    }

    pub fn stone_white_color(&self) -> ThemeColor {
        parse_hex_color(&self.stone_white).expect("theme tokens were validated on parse")
    }

    pub fn background_color(&self) -> ThemeColor {
        parse_hex_color(&self.background).expect("theme tokens were validated on parse")
    }
}

impl Default for ThemeTokens {
    fn default() -> Self {
        Self {
            schema_version: THEME_TOKEN_SCHEMA_VERSION,
            board_wood: "#d9a866".to_owned(),
            board_line: "#4a2f12".to_owned(),
            star_point: "#3a2410".to_owned(),
            stone_black: "#1a1a1a".to_owned(),
            stone_white: "#ffffff".to_owned(),
            background: "#f5f0e8".to_owned(),
        }
    }
}

/// `theme.json`: schema version, id, name, version and the allowed asset
/// list. Assets are relative paths that must stay inside the theme directory.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeManifest {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub assets: Vec<String>,
}

impl ThemeManifest {
    pub fn load(install_path: impl AsRef<Path>) -> Result<Self, ThemeError> {
        let manifest_path = install_path.as_ref().join("theme.json");
        let manifest: Self = serde_json::from_slice(&fs::read(manifest_path)?)?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<(), ThemeError> {
        if self.schema_version != THEME_SCHEMA_VERSION {
            return Err(ThemeError::InvalidManifest(format!(
                "schemaVersion must be {THEME_SCHEMA_VERSION}"
            )));
        }
        if !is_valid_theme_id(&self.id) {
            return Err(ThemeError::InvalidManifest(format!(
                "id {:?} is not a valid theme id",
                self.id
            )));
        }
        if self.name.trim().is_empty() || self.version.trim().is_empty() {
            return Err(ThemeError::InvalidManifest(
                "name and version are required".to_owned(),
            ));
        }
        for asset in &self.assets {
            if !is_safe_relative_path(asset) {
                return Err(ThemeError::InvalidManifest(format!(
                    "asset path {asset:?} must stay inside the theme directory"
                )));
            }
            if let Some(extension) = Path::new(asset).extension().and_then(|ext| ext.to_str()) {
                if !ALLOWED_THEME_ASSET_EXTENSIONS
                    .contains(&extension.to_ascii_lowercase().as_str())
                {
                    return Err(ThemeError::InvalidManifest(format!(
                        "asset {asset:?} has a disallowed extension"
                    )));
                }
            } else {
                return Err(ThemeError::InvalidManifest(format!(
                    "asset {asset:?} has no extension"
                )));
            }
        }
        Ok(())
    }
}

/// A validated, loadable theme: manifest plus tokens, with all declared
/// assets present and within size limits.
#[derive(Clone, Debug)]
pub struct InstalledTheme {
    pub manifest: ThemeManifest,
    pub tokens: ThemeTokens,
    pub install_path: PathBuf,
}

impl InstalledTheme {
    /// Loads and validates a theme package from its directory. Fails when
    /// the manifest or tokens are invalid, when an asset is missing or
    /// oversized, or when the layout is unsafe.
    pub fn load(install_path: impl AsRef<Path>) -> Result<Self, ThemeError> {
        let install_path = install_path.as_ref();
        let manifest = ThemeManifest::load(install_path)?;
        let tokens_path = install_path.join("tokens.json");
        let tokens_bytes = fs::read(&tokens_path)?;
        if tokens_bytes.len() as u64 > MAX_THEME_TOKENS_BYTES {
            return Err(ThemeError::InvalidManifest(
                "tokens.json exceeds the size limit".to_owned(),
            ));
        }
        let tokens_json = String::from_utf8(tokens_bytes)
            .map_err(|_| ThemeError::InvalidManifest("tokens.json is not UTF-8".to_owned()))?;
        let tokens =
            ThemeTokens::parse(&tokens_json).map_err(|error| ThemeError::InvalidTokens(error))?;
        for asset in &manifest.assets {
            let asset_path = install_path.join(asset);
            let metadata = fs::metadata(&asset_path).map_err(|error| ThemeError::MissingAsset {
                asset: asset.clone(),
                source: error,
            })?;
            if metadata.len() > MAX_THEME_ASSET_BYTES {
                return Err(ThemeError::AssetTooLarge {
                    asset: asset.clone(),
                    size: metadata.len(),
                });
            }
        }
        Ok(Self {
            manifest,
            tokens,
            install_path: install_path.to_owned(),
        })
    }
}

/// The result of scanning a themes root: valid packages plus any legacy
/// `.asar` themes that are detected but never executed.
#[derive(Clone, Debug, Default)]
pub struct ThemeScan {
    pub themes: Vec<InstalledTheme>,
    pub legacy_asar: Vec<PathBuf>,
}

/// Scans every direct child of the themes root. Directories carrying a valid
/// `theme.json` become installed themes; `.asar` files are reported as
/// legacy entries with a migration notice; anything else is skipped.
pub fn scan_theme_root(themes_root: &Path) -> Result<ThemeScan, ThemeError> {
    let mut scan = ThemeScan::default();
    let mut entries = match fs::read_dir(themes_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(scan),
        Err(error) => return Err(ThemeError::Read(error)),
    };
    let mut paths = Vec::new();
    while let Some(entry) = entries.next() {
        paths.push(entry.map_err(ThemeError::Read)?.path());
    }
    paths.sort();
    for path in paths {
        if path.extension().and_then(|ext| ext.to_str()) == Some("asar") {
            scan.legacy_asar.push(path);
            continue;
        }
        if !path.is_dir() {
            continue;
        }
        match InstalledTheme::load(&path) {
            Ok(theme) => scan.themes.push(theme),
            Err(ThemeError::ManifestRead(error))
                if error.kind() == std::io::ErrorKind::NotFound =>
            {
                continue;
            }
            Err(ThemeError::ManifestDecode(_)) => continue,
            Err(_) => continue,
        }
    }
    Ok(scan)
}

/// Installs a theme package by copying a validated source directory into the
/// themes root under the manifest id. The source must contain a valid
/// `theme.json` and `tokens.json`; a theme with the same id is replaced.
pub fn install_theme(source: &Path, themes_root: &Path) -> Result<InstalledTheme, ThemeError> {
    let theme = InstalledTheme::load(source)?;
    let destination = themes_root.join(&theme.manifest.id);
    if destination.exists() {
        fs::remove_dir_all(&destination)?;
    }
    copy_dir_all(source, &destination)?;
    InstalledTheme::load(&destination)
}

/// Removes an installed theme by id; unknown ids are a no-op.
pub fn uninstall_theme(themes_root: &Path, theme_id: &str) -> Result<(), ThemeError> {
    if !is_valid_theme_id(theme_id) {
        return Err(ThemeError::InvalidManifest(format!(
            "id {theme_id:?} is not a valid theme id"
        )));
    }
    let destination = themes_root.join(theme_id);
    match fs::remove_dir_all(&destination) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(ThemeError::Read(error)),
    }
}

/// Theme ids follow the same safe-component rules as plugin ids.
pub fn is_valid_theme_id(theme_id: &str) -> bool {
    !theme_id.is_empty()
        && theme_id.len() <= 128
        && theme_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
        && !theme_id.starts_with('.')
        && !theme_id.ends_with('.')
        && !theme_id.contains("..")
}

/// An asset path must be relative, use normal components only, and never
/// escape the theme directory.
pub fn is_safe_relative_path(asset: &str) -> bool {
    !asset.is_empty()
        && !Path::new(asset).is_absolute()
        && Path::new(asset)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn copy_dir_all(source: &Path, destination: &Path) -> Result<(), ThemeError> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let from = entry.path();
        let to = destination.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_all(&from, &to)?;
        } else if file_type.is_file() {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum ThemeError {
    #[error("theme manifest is invalid: {0}")]
    InvalidManifest(String),
    #[error("theme tokens are invalid: {0}")]
    InvalidTokens(String),
    #[error("theme asset {asset:?} is missing: {source}")]
    MissingAsset {
        asset: String,
        source: std::io::Error,
    },
    #[error("theme asset {asset:?} exceeds the {MAX_THEME_ASSET_BYTES}-byte limit")]
    AssetTooLarge { asset: String, size: u64 },
    #[error("theme manifest could not be read: {0}")]
    ManifestRead(#[from] std::io::Error),
    #[error("theme manifest could not be decoded: {0}")]
    ManifestDecode(#[from] serde_json::Error),
    #[error("themes directory could not be read: {0}")]
    Read(std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_root(test_name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "sabaki-host-theme-{test_name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("temp root is created");
        root
    }

    fn write_theme(install_path: &Path, id: &str, name: &str, assets: &[&str]) {
        fs::create_dir_all(install_path).expect("theme dir is created");
        let manifest = serde_json::json!({
            "schemaVersion": THEME_SCHEMA_VERSION,
            "id": id,
            "name": name,
            "version": "1.0.0",
            "assets": assets,
        });
        fs::write(
            install_path.join("theme.json"),
            serde_json::to_vec(&manifest).expect("manifest serializes"),
        )
        .expect("theme.json is written");
        fs::write(
            install_path.join("tokens.json"),
            r##"{"schemaVersion":1,"boardWood":"#d9a866","boardLine":"#4a2f12","starPoint":"#3a2410","stoneBlack":"#1a1a1a","stoneWhite":"#ffffff","background":"#f5f0e8"}"##,
        )
        .expect("tokens.json is written");
        for asset in assets {
            fs::write(install_path.join(asset), b"asset bytes").expect("asset is written");
        }
    }

    #[test]
    fn loads_a_valid_theme_package() {
        let root = fresh_root("load");
        let theme_path = root.join("org.example.wood");
        write_theme(&theme_path, "org.example.wood", "Wood", &["board.png"]);

        let theme = InstalledTheme::load(&theme_path).expect("theme loads");
        assert_eq!(theme.manifest.id, "org.example.wood");
        assert_eq!(theme.tokens.board_wood_color().rgb_u32(), 0xd9a866);
        assert_eq!(theme.manifest.assets, vec!["board.png".to_owned()]);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn rejects_missing_assets() {
        let root = fresh_root("missing-asset");
        let theme_path = root.join("org.example.wood");
        write_theme(&theme_path, "org.example.wood", "Wood", &["board.png"]);
        fs::remove_file(theme_path.join("board.png")).expect("asset removed");

        assert!(matches!(
            InstalledTheme::load(&theme_path),
            Err(ThemeError::MissingAsset { .. })
        ));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn rejects_assets_that_escape_the_theme_directory() {
        let root = fresh_root("escape");
        let theme_path = root.join("org.example.wood");
        fs::create_dir_all(&theme_path).expect("theme dir is created");
        let manifest = serde_json::json!({
            "schemaVersion": THEME_SCHEMA_VERSION,
            "id": "org.example.wood",
            "name": "Wood",
            "version": "1.0.0",
            "assets": ["../secret.png"],
        });
        fs::write(
            theme_path.join("theme.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .expect("manifest written");

        assert!(matches!(
            ThemeManifest::load(&theme_path),
            Err(ThemeError::InvalidManifest(_))
        ));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn rejects_assets_with_disallowed_extensions() {
        let root = fresh_root("extension");
        let theme_path = root.join("org.example.wood");
        write_theme(&theme_path, "org.example.wood", "Wood", &["board.exe"]);

        assert!(matches!(
            ThemeManifest::load(&theme_path),
            Err(ThemeError::InvalidManifest(_))
        ));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn rejects_invalid_tokens() {
        let root = fresh_root("tokens");
        let theme_path = root.join("org.example.wood");
        write_theme(&theme_path, "org.example.wood", "Wood", &["board.png"]);
        fs::write(
            theme_path.join("tokens.json"),
            r##"{"schemaVersion":1,"boardWood":"not-a-color","boardLine":"#4a2f12","starPoint":"#3a2410","stoneBlack":"#1a1a1a","stoneWhite":"#ffffff","background":"#f5f0e8"}"##,
        )
        .expect("bad tokens written");

        assert!(matches!(
            InstalledTheme::load(&theme_path),
            Err(ThemeError::InvalidTokens(_))
        ));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn scan_reports_themes_and_legacy_asar_files() {
        let root = fresh_root("scan");
        write_theme(
            &root.join("org.example.wood"),
            "org.example.wood",
            "Wood",
            &[],
        );
        write_theme(
            &root.join("org.example.mist"),
            "org.example.mist",
            "Mist",
            &[],
        );
        fs::write(root.join("legacy-theme.asar"), b"not an archive")
            .expect("asar decoy is written");
        fs::create_dir_all(root.join("not-a-theme")).expect("decoy dir is created");

        let scan = scan_theme_root(&root).expect("scan succeeds");

        assert_eq!(scan.themes.len(), 2);
        assert_eq!(scan.themes[0].manifest.id, "org.example.mist");
        assert_eq!(scan.themes[1].manifest.id, "org.example.wood");
        assert_eq!(scan.legacy_asar.len(), 1);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn install_copies_and_uninstall_removes() {
        let root = fresh_root("install");
        let source = root.join("source");
        write_theme(&source, "org.example.wood", "Wood", &["board.png"]);

        let themes_root = root.join("themes");
        let installed = install_theme(&source, &themes_root).expect("theme installs");
        assert_eq!(installed.manifest.id, "org.example.wood");
        assert!(
            themes_root
                .join("org.example.wood")
                .join("board.png")
                .exists()
        );

        let scan = scan_theme_root(&themes_root).expect("scan succeeds");
        assert_eq!(scan.themes.len(), 1);
        assert_eq!(scan.themes[0].tokens.board_wood_color().rgb_u32(), 0xd9a866);

        uninstall_theme(&themes_root, "org.example.wood").expect("theme uninstalls");
        assert!(
            scan_theme_root(&themes_root)
                .expect("scan")
                .themes
                .is_empty()
        );
        uninstall_theme(&themes_root, "org.example.wood").expect("missing theme is a no-op");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn scan_on_a_missing_root_is_empty() {
        let scan = scan_theme_root(&PathBuf::from("/nowhere/themes")).expect("scan succeeds");
        assert!(scan.themes.is_empty());
        assert!(scan.legacy_asar.is_empty());
    }
}
