//! License-gated Git synchronization for professional SGF collections.
//!
//! A repository being public does not imply that its game records may be
//! redistributed. Sources must carry an explicit compatible license before any
//! clone, fetch, or packaging workflow is allowed to use them.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RedistributionRights {
    /// The source explicitly permits redistribution under `license_name`.
    Permitted,
    /// The source has no verified right statement and cannot be synchronized.
    Unknown,
    /// The publisher explicitly forbids redistribution.
    Prohibited,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SgfLibrarySource {
    pub id: String,
    pub name: String,
    pub github_url: String,
    pub reference: String,
    pub rights: RedistributionRights,
    pub license_name: Option<String>,
    pub license_url: Option<String>,
}

impl SgfLibrarySource {
    pub fn validate_for_sync(&self) -> Result<(), SgfLibraryError> {
        if self.id.trim().is_empty() {
            return Err(SgfLibraryError::InvalidSource(
                "source id is empty".to_owned(),
            ));
        }
        if !self
            .id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(SgfLibraryError::InvalidSource(
                "source id may contain only ASCII letters, numbers, '-' and '_'".to_owned(),
            ));
        }
        if self.name.trim().is_empty() {
            return Err(SgfLibraryError::InvalidSource(
                "source name is empty".to_owned(),
            ));
        }
        validate_github_url(&self.github_url)?;
        validate_reference(&self.reference)?;
        if self.rights != RedistributionRights::Permitted {
            return Err(SgfLibraryError::RedistributionNotPermitted {
                source_id: self.id.clone(),
            });
        }
        if self.license_name.as_deref().is_none_or(str::is_empty)
            || self.license_url.as_deref().is_none_or(str::is_empty)
        {
            return Err(SgfLibraryError::MissingLicenseEvidence {
                source_id: self.id.clone(),
            });
        }
        Ok(())
    }
}

/// Strictly validates that `raw` is an HTTPS GitHub repository URL suitable
/// for an unauthenticated clone or fetch: scheme `https`, host `github.com`,
/// no userinfo, and no explicit non-default port. Anything else is rejected so
/// that a misconfigured source can never point the importer at an unintended
/// host or trigger an interactive credential prompt.
fn validate_github_url(raw: &str) -> Result<(), SgfLibraryError> {
    let url = Url::parse(raw)
        .map_err(|_| SgfLibraryError::InvalidSource("GitHub URL is not a valid URL".to_owned()))?;
    if url.scheme() != "https" {
        return Err(SgfLibraryError::InvalidSource(
            "only an HTTPS GitHub repository URL is accepted".to_owned(),
        ));
    }
    if url.host_str() != Some("github.com") {
        return Err(SgfLibraryError::InvalidSource(
            "only the github.com host is accepted".to_owned(),
        ));
    }
    if url.username() != "" || url.password().is_some() {
        return Err(SgfLibraryError::InvalidSource(
            "GitHub URL must not carry userinfo".to_owned(),
        ));
    }
    if url.port().is_some() {
        return Err(SgfLibraryError::InvalidSource(
            "GitHub URL must not specify a port".to_owned(),
        ));
    }
    Ok(())
}

/// Rejects empty, control-character-laden, or obviously invalid Git references.
/// A reference is passed straight to `git clone --branch` / `git fetch`, so it
/// must be a well-formed ref name rather than arbitrary shell-ish text. There
/// is no shell here (git arguments are handed to `Command` as a vector), but
/// rejecting obvious junk keeps invalid refs from reaching git at all.
fn validate_reference(reference: &str) -> Result<(), SgfLibraryError> {
    if reference.trim().is_empty() {
        return Err(SgfLibraryError::InvalidSource(
            "Git reference is empty".to_owned(),
        ));
    }
    if reference.chars().any(char::is_control) {
        return Err(SgfLibraryError::InvalidSource(
            "Git reference contains control characters".to_owned(),
        ));
    }
    if reference != reference.trim() {
        return Err(SgfLibraryError::InvalidSource(
            "Git reference must not have surrounding whitespace".to_owned(),
        ));
    }
    if reference.starts_with('-')
        || reference.contains("..")
        || reference.contains("@{")
        || reference.contains(char::is_whitespace)
    {
        return Err(SgfLibraryError::InvalidSource(
            "Git reference is not a valid ref name".to_owned(),
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SgfLibrarySyncReport {
    pub source_id: String,
    pub destination: String,
    pub operation: SgfLibrarySyncOperation,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SgfLibrarySyncOperation {
    Cloned,
    Fetched,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SgfLibraryEntry {
    pub source_id: String,
    pub relative_path: String,
    pub path: PathBuf,
}

/// Finds regular SGF files without following symlinks out of the synchronized
/// repository. Results are stable and capped so a malformed source cannot make
/// the desktop library allocate without bound.
pub fn scan_sgf_library(
    source_id: &str,
    root: &Path,
) -> Result<Vec<SgfLibraryEntry>, SgfLibraryError> {
    const MAX_LIBRARY_FILES: usize = 10_000;
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut pending = vec![root.to_path_buf()];
    let mut entries = Vec::new();
    while let Some(directory) = pending.pop() {
        let children = std::fs::read_dir(&directory).map_err(|error| {
            SgfLibraryError::Scan(format!("could not read {}: {error}", directory.display()))
        })?;
        for child in children {
            let child = child.map_err(|error| SgfLibraryError::Scan(error.to_string()))?;
            let path = child.path();
            let metadata = std::fs::symlink_metadata(&path)
                .map_err(|error| SgfLibraryError::Scan(error.to_string()))?;
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                if child.file_name() != ".git" {
                    pending.push(path);
                }
                continue;
            }
            let is_sgf = path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("sgf"));
            if !metadata.is_file() || !is_sgf {
                continue;
            }
            let relative_path = path
                .strip_prefix(root)
                .map_err(|error| SgfLibraryError::Scan(error.to_string()))?
                .to_string_lossy()
                .replace('\\', "/");
            entries.push(SgfLibraryEntry {
                source_id: source_id.to_owned(),
                relative_path,
                path,
            });
            if entries.len() >= MAX_LIBRARY_FILES {
                return Err(SgfLibraryError::Scan(format!(
                    "source `{source_id}` exceeds the {MAX_LIBRARY_FILES} SGF file limit"
                )));
            }
        }
    }
    entries.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(entries)
}

/// Real Git adapter. It deliberately disables terminal prompts: the library
/// importer only supports public HTTPS sources and never obtains credentials.
#[derive(Clone, Copy, Debug, Default)]
pub struct ProcessGitSyncAdapter;

impl ProcessGitSyncAdapter {
    fn run(&self, command: &mut std::process::Command) -> Result<(), String> {
        let output = command
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .map_err(|error| format!("could not start git: {error}"))?;
        if output.status.success() {
            return Ok(());
        }
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        Err(if detail.is_empty() {
            format!("git exited with {}", output.status)
        } else {
            detail
        })
    }
}

impl ProcessGitSyncAdapter {
    fn run_with_output(&self, command: &mut std::process::Command) -> Result<String, String> {
        let output = command
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .map_err(|error| format!("could not start git: {error}"))?;
        if !output.status.success() {
            let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            return Err(if detail.is_empty() {
                format!("git exited with {}", output.status)
            } else {
                detail
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

impl SgfGitSyncAdapter for ProcessGitSyncAdapter {
    fn clone_repository(
        &mut self,
        repository_url: &str,
        reference: &str,
        destination: &Path,
    ) -> Result<(), String> {
        let mut command = std::process::Command::new("git");
        command.args([
            "clone",
            "--depth",
            "1",
            "--branch",
            reference,
            repository_url,
        ]);
        command.arg(destination);
        self.run(&mut command)
    }

    fn remote_url(&mut self, destination: &Path) -> Result<Option<String>, String> {
        let mut command = std::process::Command::new("git");
        command
            .current_dir(destination)
            .args(["remote", "get-url", "origin"]);
        let output = self.run_with_output(&mut command)?;
        let trimmed = output.trim();
        Ok(if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_owned())
        })
    }

    fn fetch_repository(&mut self, reference: &str, destination: &Path) -> Result<(), String> {
        let mut fetch = std::process::Command::new("git");
        fetch
            .current_dir(destination)
            .args(["fetch", "--depth", "1", "origin", reference]);
        self.run(&mut fetch)?;
        let mut reset = std::process::Command::new("git");
        reset
            .current_dir(destination)
            .args(["checkout", "--detach", "FETCH_HEAD"]);
        self.run(&mut reset)
    }
}

pub trait SgfGitSyncAdapter {
    fn clone_repository(
        &mut self,
        repository_url: &str,
        reference: &str,
        destination: &Path,
    ) -> Result<(), String>;

    /// Returns the configured `origin` URL of the existing repository at
    /// `destination`, if any. Used to verify that an already-initialized
    /// repository really points at the declared source before fetching from it.
    fn remote_url(&mut self, destination: &Path) -> Result<Option<String>, String>;

    fn fetch_repository(&mut self, reference: &str, destination: &Path) -> Result<(), String>;
}

pub fn sync_sgf_library(
    source: &SgfLibrarySource,
    destination: &Path,
    adapter: &mut impl SgfGitSyncAdapter,
) -> Result<SgfLibrarySyncReport, SgfLibraryError> {
    source.validate_for_sync()?;
    if destination.join(".git").is_dir() {
        let origin = adapter
            .remote_url(destination)
            .map_err(SgfLibraryError::Git)?;
        let origin = origin.ok_or_else(|| {
            SgfLibraryError::Git("existing repository has no origin remote".to_owned())
        })?;
        if !normalize_remote_url(&origin)
            .map_err(SgfLibraryError::Git)?
            .eq_ignore_ascii_case(
                &normalize_remote_url(&source.github_url).map_err(SgfLibraryError::Git)?,
            )
        {
            return Err(SgfLibraryError::OriginMismatch {
                source_id: source.id.clone(),
                origin,
                declared: source.github_url.clone(),
            });
        }
        adapter
            .fetch_repository(&source.reference, destination)
            .map_err(SgfLibraryError::Git)?;
        Ok(SgfLibrarySyncReport {
            source_id: source.id.clone(),
            destination: destination.display().to_string(),
            operation: SgfLibrarySyncOperation::Fetched,
        })
    } else {
        adapter
            .clone_repository(&source.github_url, &source.reference, destination)
            .map_err(SgfLibraryError::Git)?;
        Ok(SgfLibrarySyncReport {
            source_id: source.id.clone(),
            destination: destination.display().to_string(),
            operation: SgfLibrarySyncOperation::Cloned,
        })
    }
}

/// Normalizes a remote URL for comparison: strips a trailing `.git` suffix and
/// trailing slashes so that `https://github.com/o/r.git` and
/// `https://github.com/o/r/` compare equal to `https://github.com/o/r`.
fn normalize_remote_url(raw: &str) -> Result<String, String> {
    let url = Url::parse(raw.trim())
        .map_err(|_| format!("repository origin is not a valid URL: {raw}"))?;
    if url.scheme() != "https" || url.host_str() != Some("github.com") {
        return Err(format!(
            "repository origin is not an HTTPS github.com URL: {raw}"
        ));
    }
    let mut normalized = raw.trim().trim_end_matches('/').to_owned();
    if normalized.ends_with(".git") {
        normalized.truncate(normalized.len() - ".git".len());
    }
    Ok(normalized)
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SgfLibraryError {
    #[error("invalid SGF library source: {0}")]
    InvalidSource(String),
    #[error("SGF library source `{source_id}` does not permit redistribution")]
    RedistributionNotPermitted { source_id: String },
    #[error("SGF library source `{source_id}` needs a verifiable license name and URL")]
    MissingLicenseEvidence { source_id: String },
    #[error("Git synchronization failed: {0}")]
    Git(String),
    #[error("SGF library scan failed: {0}")]
    Scan(String),
    #[error(
        "SGF library source `{source_id}` origin `{origin}` does not match declared `{declared}`"
    )]
    OriginMismatch {
        source_id: String,
        origin: String,
        declared: String,
    },
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;

    struct FakeGit {
        clone_calls: Vec<(String, String, PathBuf)>,
        fetch_calls: Vec<(String, PathBuf)>,
        origin: Option<String>,
    }

    impl FakeGit {
        fn with_origin(origin: &str) -> Self {
            FakeGit {
                clone_calls: Vec::new(),
                fetch_calls: Vec::new(),
                origin: Some(origin.to_owned()),
            }
        }
    }

    impl Default for FakeGit {
        fn default() -> Self {
            FakeGit {
                clone_calls: Vec::new(),
                fetch_calls: Vec::new(),
                origin: Some("https://github.com/example/pro-games".to_owned()),
            }
        }
    }

    impl SgfGitSyncAdapter for FakeGit {
        fn clone_repository(
            &mut self,
            repository_url: &str,
            reference: &str,
            destination: &Path,
        ) -> Result<(), String> {
            self.clone_calls.push((
                repository_url.to_owned(),
                reference.to_owned(),
                destination.to_owned(),
            ));
            Ok(())
        }

        fn remote_url(&mut self, _destination: &Path) -> Result<Option<String>, String> {
            Ok(self.origin.clone())
        }

        fn fetch_repository(&mut self, reference: &str, destination: &Path) -> Result<(), String> {
            self.fetch_calls
                .push((reference.to_owned(), destination.to_owned()));
            Ok(())
        }
    }

    fn licensed_source() -> SgfLibrarySource {
        SgfLibrarySource {
            id: "example-pro-games".to_owned(),
            name: "Example Professional Games".to_owned(),
            github_url: "https://github.com/example/pro-games.git".to_owned(),
            reference: "main".to_owned(),
            rights: RedistributionRights::Permitted,
            license_name: Some("CC BY 4.0".to_owned()),
            license_url: Some("https://creativecommons.org/licenses/by/4.0/".to_owned()),
        }
    }

    #[test]
    fn rejects_public_repositories_without_explicit_redistribution_rights() {
        let mut source = licensed_source();
        source.rights = RedistributionRights::Unknown;
        assert!(matches!(
            source.validate_for_sync(),
            Err(SgfLibraryError::RedistributionNotPermitted { .. })
        ));
    }

    #[test]
    fn rejects_source_ids_that_could_escape_the_managed_directory() {
        let mut source = licensed_source();
        source.id = "../outside".to_owned();
        assert!(matches!(
            source.validate_for_sync(),
            Err(SgfLibraryError::InvalidSource(message)) if message.contains("ASCII")
        ));
    }

    #[test]
    fn clone_is_allowed_only_for_licensed_source() {
        let destination = std::env::temp_dir().join("ryusei-sgf-library-unit-test");
        let mut git = FakeGit::default();
        let report = sync_sgf_library(&licensed_source(), &destination, &mut git)
            .expect("licensed source synchronizes");
        assert_eq!(report.operation, SgfLibrarySyncOperation::Cloned);
        assert_eq!(git.clone_calls.len(), 1);
    }

    #[test]
    fn rejects_non_https_github_url() {
        let mut source = licensed_source();
        source.github_url = "http://github.com/example/pro-games".to_owned();
        assert!(matches!(
            source.validate_for_sync(),
            Err(SgfLibraryError::InvalidSource(message)) if message.contains("HTTPS")
        ));
    }

    #[test]
    fn rejects_non_github_host() {
        let mut source = licensed_source();
        source.github_url = "https://gitlab.com/example/pro-games".to_owned();
        assert!(matches!(
            source.validate_for_sync(),
            Err(SgfLibraryError::InvalidSource(message)) if message.contains("github.com")
        ));
    }

    #[test]
    fn rejects_userinfo_in_url() {
        let mut source = licensed_source();
        source.github_url = "https://user:token@github.com/example/pro-games".to_owned();
        assert!(matches!(
            source.validate_for_sync(),
            Err(SgfLibraryError::InvalidSource(message)) if message.contains("userinfo")
        ));
    }

    #[test]
    fn rejects_non_default_port_in_url() {
        let mut source = licensed_source();
        source.github_url = "https://github.com:8080/example/pro-games".to_owned();
        assert!(matches!(
            source.validate_for_sync(),
            Err(SgfLibraryError::InvalidSource(message)) if message.contains("port")
        ));
    }

    #[test]
    fn rejects_control_characters_in_reference() {
        let mut source = licensed_source();
        source.reference = "main\n".to_owned();
        assert!(matches!(
            source.validate_for_sync(),
            Err(SgfLibraryError::InvalidSource(message)) if message.contains("control")
        ));
    }

    #[test]
    fn rejects_surrounding_whitespace_in_reference() {
        let mut source = licensed_source();
        source.reference = "  main  ".to_owned();
        assert!(matches!(
            source.validate_for_sync(),
            Err(SgfLibraryError::InvalidSource(message)) if message.contains("whitespace")
        ));
    }

    #[test]
    fn rejects_obviously_invalid_reference() {
        let mut source = licensed_source();
        source.reference = "feature..branch".to_owned();
        assert!(matches!(
            source.validate_for_sync(),
            Err(SgfLibraryError::InvalidSource(message)) if message.contains("ref")
        ));
    }

    #[test]
    fn fetch_is_allowed_when_origin_matches_declared_url() {
        let destination = std::env::temp_dir().join("ryusei-sgf-library-unit-test-match");
        let _ = std::fs::create_dir_all(destination.join(".git"));
        let mut git = FakeGit::default();
        let report = sync_sgf_library(&licensed_source(), &destination, &mut git)
            .expect("matching origin fetches");
        assert_eq!(report.operation, SgfLibrarySyncOperation::Fetched);
        assert_eq!(git.fetch_calls.len(), 1);
        assert!(git.clone_calls.is_empty());
        let _ = std::fs::remove_dir_all(&destination);
    }

    #[test]
    fn fetch_is_allowed_when_origin_has_trailing_git_suffix() {
        let destination = std::env::temp_dir().join("ryusei-sgf-library-unit-test-git-suffix");
        let _ = std::fs::create_dir_all(destination.join(".git"));
        let mut git = FakeGit::with_origin("https://github.com/example/pro-games.git/");
        let report = sync_sgf_library(&licensed_source(), &destination, &mut git)
            .expect("matching origin with .git suffix fetches");
        assert_eq!(report.operation, SgfLibrarySyncOperation::Fetched);
        let _ = std::fs::remove_dir_all(&destination);
    }

    #[test]
    fn does_not_fetch_when_origin_mismatches_declared_url() {
        let destination = std::env::temp_dir().join("ryusei-sgf-library-unit-test-mismatch");
        let _ = std::fs::create_dir_all(destination.join(".git"));
        let mut git = FakeGit::with_origin("https://github.com/someone/else");
        let result = sync_sgf_library(&licensed_source(), &destination, &mut git);
        assert!(matches!(
            result,
            Err(SgfLibraryError::OriginMismatch { .. })
        ));
        assert!(
            git.fetch_calls.is_empty(),
            "must not fetch when the origin differs from the declared URL"
        );
        assert!(git.clone_calls.is_empty());
        let _ = std::fs::remove_dir_all(&destination);
    }

    #[test]
    fn scans_sgf_files_without_following_git_or_symlink_directories() {
        let root = std::env::temp_dir().join("ryusei-sgf-library-scan-test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("nested")).expect("scan fixture directories");
        std::fs::write(root.join("a.sgf"), "(;GM[1])").expect("scan fixture sgf");
        std::fs::write(root.join("nested/b.SGF"), "(;GM[1])").expect("scan fixture sgf");
        std::fs::create_dir_all(root.join(".git")).expect("scan fixture git dir");
        std::fs::write(root.join("notes.txt"), "not an sgf").expect("scan fixture text");

        let entries = scan_sgf_library("scan-test", &root).expect("scan succeeds");
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.relative_path.as_str())
                .collect::<Vec<_>>(),
            vec!["a.sgf", "nested/b.SGF"]
        );
        assert!(entries.iter().all(|entry| entry.source_id == "scan-test"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn rejects_existing_repository_without_origin() {
        let destination = std::env::temp_dir().join("ryusei-sgf-library-unit-test-no-origin");
        let _ = std::fs::create_dir_all(destination.join(".git"));
        let mut git = FakeGit {
            origin: None,
            ..FakeGit::default()
        };
        let result = sync_sgf_library(&licensed_source(), &destination, &mut git);
        assert!(result.is_err());
        assert!(git.fetch_calls.is_empty());
        let _ = std::fs::remove_dir_all(&destination);
    }
}
