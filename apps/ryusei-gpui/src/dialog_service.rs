use std::path::{Path, PathBuf};

use ryusei_host::{DecodedGameFile, GameFileAccess, HostError, SourceEncoding};

/// File dialog port for the GPUI client.
///
/// `gpui 0.2.2` does not expose a native file dialog, so the shell owns this
/// narrow port instead of reaching for a platform API. The future macOS /
/// Windows / Linux native dialog adapters implement the same trait, and the
/// host file workflow (`HostApplication::open` / `save_at`) stays unchanged.
pub trait DialogService {
    /// Ask the user to pick an existing game file to open.
    /// Returning `None` models a cancelled dialog.
    fn pick_open_path(&self) -> Option<PathBuf>;

    /// Ask the user to pick a plugin `.zip` archive to install.
    fn pick_open_zip_path(&self) -> Option<PathBuf>;

    /// Ask the user where to save the game. `suggested_name` is the default
    /// file name; returning `None` models a cancelled dialog.
    fn pick_save_path(&self, suggested_name: &str) -> Option<PathBuf>;

    /// Ask the user where to save an exported GIF animated file.
    fn pick_save_gif_path(&self, suggested_name: &str) -> Option<PathBuf>;
}

/// Deterministic dialog for the GPUI client.
///
/// The open path can be seeded from a command-line argument, which also
/// exercises the "open a file at launch" path. A `None` open path models a
/// cancelled dialog, so the shell can be run without touching real files.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct MockDialogService {
    pub open_path: Option<PathBuf>,
    pub save_path: Option<PathBuf>,
    pub save_gif_path: Option<PathBuf>,
}

impl Default for MockDialogService {
    fn default() -> Self {
        Self {
            open_path: None,
            save_path: Some(PathBuf::from("untitled.sgf")),
            save_gif_path: Some(PathBuf::from("game.gif")),
        }
    }
}

impl DialogService for MockDialogService {
    fn pick_open_path(&self) -> Option<PathBuf> {
        self.open_path.clone()
    }

    fn pick_open_zip_path(&self) -> Option<PathBuf> {
        self.open_path.clone()
    }

    fn pick_save_path(&self, suggested_name: &str) -> Option<PathBuf> {
        Some(
            self.save_path
                .clone()
                .unwrap_or_else(|| PathBuf::from(suggested_name)),
        )
    }

    fn pick_save_gif_path(&self, suggested_name: &str) -> Option<PathBuf> {
        Some(
            self.save_gif_path
                .clone()
                .unwrap_or_else(|| PathBuf::from(suggested_name)),
        )
    }
}

/// Production dialog backed by `rfd`, the cross-platform native file dialog
/// crate. The host file workflow stays behind the `DialogService` port, so the
/// dialog implementation can be swapped per platform without touching the
/// workflow.
#[derive(Clone, Copy, Debug, Default)]
pub struct RfdDialogService;

impl DialogService for RfdDialogService {
    fn pick_open_path(&self) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title("Open SGF")
            .add_filter(
                "Smart Game Format (*.sgf, *.ngf, *.gib, *.ugf)",
                &["sgf", "ngf", "gib", "ugf"],
            )
            .pick_file()
    }

    fn pick_open_zip_path(&self) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title("Install Plugin from ZIP")
            .add_filter("Plugin Archive (*.zip)", &["zip"])
            .pick_file()
    }

    fn pick_save_path(&self, suggested_name: &str) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title("Save SGF")
            .set_file_name(suggested_name)
            .add_filter("Smart Game Format", &["sgf"])
            .save_file()
            .map(ensure_sgf_extension)
    }

    fn pick_save_gif_path(&self, suggested_name: &str) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title("Export Animated GIF")
            .set_file_name(suggested_name)
            .add_filter("GIF Animation (*.gif)", &["gif"])
            .save_file()
            .map(ensure_gif_extension)
    }
}

/// Appends the `.gif` extension when the chosen save path has none.
pub fn ensure_gif_extension(path: PathBuf) -> PathBuf {
    if path.extension().is_none() {
        path.with_extension("gif")
    } else {
        path
    }
}

/// Appends the `.sgf` extension when the chosen save path has none, so a user
/// typing a bare file name still produces a game file with the right type.
pub fn ensure_sgf_extension(path: PathBuf) -> PathBuf {
    if path.extension().is_none() {
        path.with_extension("sgf")
    } else {
        path
    }
}

/// Reads and writes SGF files through the standard library, using the shared
/// `ryusei-host` codec so every supported `CA` encoding round-trips exactly as
/// in the reference adapter. Writes are atomic (temporary sibling + rename).
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeGameFileAccess;

impl GameFileAccess for NativeGameFileAccess {
    fn read_game_file(&self, path: &Path) -> Result<DecodedGameFile, HostError> {
        let bytes = std::fs::read(path).map_err(|error| HostError::FileRead(error.to_string()))?;
        match ryusei_domain_core::legacy::file_extension(path) {
            Some(extension) if matches!(extension.as_str(), "ngf" | "gib" | "ugf") => {
                let content = ryusei_host::decode_legacy_bytes(&bytes)
                    .map_err(|error| HostError::FileRead(error.to_string()))?;
                Ok(DecodedGameFile {
                    content,
                    encoding: SourceEncoding::Utf8,
                })
            }
            _ => ryusei_host::decode_sgf_bytes(&bytes)
                .map_err(|error| HostError::FileRead(error.to_string())),
        }
    }

    fn write_game_file(
        &mut self,
        path: &Path,
        content: &str,
        encoding: SourceEncoding,
    ) -> Result<(), HostError> {
        let encoded = ryusei_host::encode_sgf(content, encoding)
            .map_err(|error| HostError::FileWrite(error.to_string()))?;
        crate::file_workflow::write_bytes_atomically(path, &encoded).map_err(HostError::FileWrite)
    }
}

#[cfg(test)]
mod tests {
    use super::{DialogService, MockDialogService, NativeGameFileAccess, ensure_sgf_extension};
    use ryusei_host::{GameFileAccess, HostError, SourceEncoding};
    use std::path::PathBuf;

    #[test]
    fn mock_dialog_models_cancelled_and_confirmed_open() {
        let cancelled = MockDialogService::default();
        assert_eq!(cancelled.pick_open_path(), None);

        let confirmed = MockDialogService {
            open_path: Some(PathBuf::from("/games/opening.sgf")),
            save_path: None,
            save_gif_path: None,
        };
        assert_eq!(
            confirmed.pick_open_path(),
            Some(PathBuf::from("/games/opening.sgf"))
        );
    }

    #[test]
    fn mock_dialog_returns_the_configured_save_path_or_falls_back() {
        let default_dialog = MockDialogService::default();
        assert_eq!(
            default_dialog.pick_save_path("kifu.sgf"),
            Some(PathBuf::from("untitled.sgf"))
        );
        assert_eq!(
            default_dialog.pick_save_gif_path("game.gif"),
            Some(PathBuf::from("game.gif"))
        );

        let unset_dialog = MockDialogService {
            open_path: None,
            save_path: None,
            save_gif_path: None,
        };
        assert_eq!(
            unset_dialog.pick_save_path("kifu.sgf"),
            Some(PathBuf::from("kifu.sgf"))
        );
        assert_eq!(
            unset_dialog.pick_save_gif_path("game.gif"),
            Some(PathBuf::from("game.gif"))
        );
    }

    #[test]
    fn native_file_access_reads_and_writes_utf8_sgf() {
        let path =
            std::env::temp_dir().join(format!("ryusei-shell-roundtrip-{}.sgf", std::process::id()));
        let mut file_access = NativeGameFileAccess;

        file_access
            .write_game_file(&path, "(;FF[4]CA[UTF-8]SZ[19])", SourceEncoding::Utf8)
            .expect("writing a UTF-8 SGF succeeds");
        let decoded = file_access
            .read_game_file(&path)
            .expect("reading back the written SGF succeeds");

        assert_eq!(decoded.encoding, SourceEncoding::Utf8);
        assert!(decoded.content.contains("SZ[19]"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn native_file_access_round_trips_shift_jis_sgf() {
        let path =
            std::env::temp_dir().join(format!("ryusei-shell-shiftjis-{}.sgf", std::process::id()));
        let mut file_access = NativeGameFileAccess;
        let source_sgf = "(;FF[4]CA[Shift_JIS]C[日本語])";

        file_access
            .write_game_file(&path, source_sgf, SourceEncoding::ShiftJis)
            .expect("writing a Shift_JIS SGF succeeds");
        let decoded = file_access
            .read_game_file(&path)
            .expect("reading back the written Shift_JIS SGF succeeds");

        assert_eq!(decoded.encoding, SourceEncoding::ShiftJis);
        assert_eq!(decoded.content, source_sgf);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn native_file_access_rejects_lossy_non_utf8_writes() {
        let mut file_access = NativeGameFileAccess;
        let error = file_access
            .write_game_file(
                &PathBuf::from("/tmp/never-written.sgf"),
                "(;FF[4]CA[Shift_JIS]C[日本語 😀])",
                SourceEncoding::ShiftJis,
            )
            .expect_err("content not representable in Shift_JIS must be rejected");
        assert!(matches!(error, HostError::FileWrite(_)));
        assert!(!std::path::Path::new("/tmp/never-written.sgf").exists());
    }

    #[test]
    fn native_file_access_writes_atomically_without_temporary_files() {
        let directory =
            std::env::temp_dir().join(format!("ryusei-shell-atomic-{}", std::process::id()));
        let path = directory.join("game.sgf");
        let mut file_access = NativeGameFileAccess;

        file_access
            .write_game_file(&path, "(;FF[4]SZ[19])", SourceEncoding::Utf8)
            .expect("atomic write succeeds");

        assert!(path.exists());
        assert!(
            std::fs::read_dir(&directory)
                .expect("directory is readable")
                .filter_map(Result::ok)
                .all(|entry| !entry.file_name().to_string_lossy().contains(".tmp-"))
        );
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn appends_the_sgf_extension_to_bare_save_paths() {
        assert_eq!(
            ensure_sgf_extension(PathBuf::from("/games/fuseki")),
            PathBuf::from("/games/fuseki.sgf")
        );
        assert_eq!(
            ensure_sgf_extension(PathBuf::from("/games/fuseki.sgf")),
            PathBuf::from("/games/fuseki.sgf")
        );
        assert_eq!(
            ensure_sgf_extension(PathBuf::from("/games/fuseki.txt")),
            PathBuf::from("/games/fuseki.txt")
        );
    }
}
