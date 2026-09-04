//! Library persistence and ingest workflow.
//!
//! This module owns the *operations* behind the unified library: loading and
//! persisting a `LibraryIndex`, ingesting game records from any source
//! (Local / Git / OGS / Fox / Live), and resolving where a record's content
//! lives. All file-system side effects go through the injectable
//! `LibraryStoreIo` seam so the workflow is testable hermetically and the
//! optional local save root is a property of the adapter, not of the logic.
//!
//! Design points:
//!
//! - **Optional local root.** When the adapter reports a `local_root`, ingested
//!   records are written under `records/<content-hash>.sgf` and the index is
//!   persisted as `index.json`. With no local root, the index is memory-only
//!   and nothing touches disk — matching the pre-existing "Git sync only"
//!   behaviour.
//! - **Atomicity.** Every workflow snapshots the index first and restores it on
//!   failure, mirroring `persistence.rs`.
//! - **Path safety.** The records file name is derived from a SHA-256 of the
//!   record id, so even a hostile id can never escape the managed root.

use std::path::{Path, PathBuf};

#[cfg(test)]
use std::cell::RefCell;

use ryusei_domain_core::{
    GameRecord, InsertOutcome, LibraryIndex, RecordId, RecordMetadata, RecordNumber, RecordSource,
};

use crate::external_file::fingerprint_content;

pub const LIBRARY_INDEX_FILE_NAME: &str = "index.json";
pub const LIBRARY_RECORDS_DIR: &str = "records";
pub const LIBRARY_REVISIONS_DIR: &str = "revisions";
/// Default number of revisions kept per record (newest first).
pub const DEFAULT_REVISION_LIMIT: usize = 50;

/// The persistence seam: everything a library workflow needs from storage.
/// Production adapters write atomically under the configured local root; test
/// adapters keep everything in memory.
pub trait LibraryStoreIo {
    /// Loads a previously persisted index. `Ok(None)` means "no index exists
    /// yet" (fresh library or no local root configured).
    fn load_index(&self) -> Result<Option<LibraryIndex>, LibraryStoreError>;

    /// Persists the full index. Called after every structural change.
    fn save_index(&self, index: &LibraryIndex) -> Result<(), LibraryStoreError>;

    /// The managed local data root, when local persistence is enabled.
    fn local_root(&self) -> Option<&Path>;

    /// Writes a record's SGF content under the managed root. Only called when
    /// `local_root()` is `Some`.
    fn write_record(&self, id: &RecordId, content: &str) -> Result<(), LibraryStoreError>;

    /// Reads a record's SGF content back from the managed root.
    fn read_record(&self, id: &RecordId) -> Result<String, LibraryStoreError>;

    /// Deletes a record's content from the managed root.
    fn delete_record(&self, id: &RecordId) -> Result<(), LibraryStoreError>;

    /// Reads an arbitrary SGF source file (a Git checkout path or a local path)
    /// for metadata extraction during ingest. Content that already lives on
    /// disk (Local / Git sources) is never copied into the managed records
    /// directory.
    fn read_source_file(&self, path: &Path) -> Result<String, LibraryStoreError> {
        std::fs::read_to_string(path).map_err(|error| {
            LibraryStoreError::RecordRead(format!("could not read {}: {error}", path.display()))
        })
    }

    /// Snapshots one revision's content under the managed root. Adapters
    /// without a local root may no-op. Revision *references* still live in the
    /// index regardless of whether content is persisted.
    fn write_revision(
        &self,
        id: &RecordId,
        revision: u64,
        content: &str,
    ) -> Result<(), LibraryStoreError> {
        let _ = (id, revision, content);
        Ok(())
    }

    /// Reads a revision's content back from the managed root.
    fn read_revision(&self, id: &RecordId, revision: u64) -> Result<String, LibraryStoreError> {
        Err(LibraryStoreError::RecordRead(format!(
            "revision {revision} of {} is not persisted",
            id.as_str()
        )))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum LibraryStoreError {
    #[error("library index could not be read: {0}")]
    IndexRead(String),
    #[error("library index could not be written: {0}")]
    IndexWrite(String),
    #[error("library record could not be written: {0}")]
    RecordWrite(String),
    #[error("library record could not be read: {0}")]
    RecordRead(String),
    #[error("library record could not be deleted: {0}")]
    RecordDelete(String),
    #[error("local library persistence is disabled (no local root configured)")]
    LocalPersistenceDisabled,
    #[error("atomic file write failed: {0}")]
    FileWrite(String),
}

/// Outcome of ingesting one record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IngestOutcome {
    pub record: GameRecord,
    pub number: RecordNumber,
    pub outcome: InsertOutcome,
}

/// Loads the current index, defaulting to an empty one when none exists.
pub fn load_library(io: &impl LibraryStoreIo) -> Result<LibraryIndex, LibraryStoreError> {
    Ok(io.load_index()?.unwrap_or_default())
}

/// Persists the current index through the adapter.
pub fn persist_library(
    io: &impl LibraryStoreIo,
    index: &LibraryIndex,
) -> Result<(), LibraryStoreError> {
    io.save_index(index)
}

/// Ingests one SGF record from a fetched source (OGS / Fox / Live / pasted
/// content). The record is inserted into the index (deduplicating on source
/// identity and preserving stable numbers), its content is written under the
/// managed local root when one is configured, and the index is persisted. On
/// any failure the in-memory index is restored to its prior state.
pub fn ingest_library_record(
    io: &impl LibraryStoreIo,
    index: &mut LibraryIndex,
    source: RecordSource,
    content: &str,
    tags: Vec<String>,
) -> Result<IngestOutcome, LibraryStoreError> {
    let previous = index.clone();
    let outcome = ingest_record_no_save(index, source, content, tags);
    let result = (|| {
        if io.local_root().is_some() {
            io.write_record(&outcome.record.id, content)?;
        }
        io.save_index(index)?;
        Ok(outcome)
    })();
    if result.is_err() {
        *index = previous;
    }
    result
}

/// Ingests a Git-synchronized scan entry. Content already lives in the
/// synchronized checkout, so it is read only for metadata and is never copied
/// into the managed records directory.
pub fn ingest_git_entry(
    io: &impl LibraryStoreIo,
    index: &mut LibraryIndex,
    entry: &crate::sgf_library::SgfLibraryEntry,
) -> Result<IngestOutcome, LibraryStoreError> {
    let content = io.read_source_file(&entry.path)?;
    let previous = index.clone();
    let source = RecordSource::Git {
        source_id: entry.source_id.clone(),
        relative_path: entry.relative_path.clone(),
    };
    let outcome = ingest_record_no_save(index, source, &content, Vec::new());
    let result = io.save_index(index).map(|_| outcome);
    if result.is_err() {
        *index = previous;
    }
    result
}

/// Batch-ingests every Git scan entry and persists the index once, so opening
/// a large library performs a single atomic index write instead of one per
/// file. On failure the whole batch is rolled back.
pub fn ingest_git_entries(
    io: &impl LibraryStoreIo,
    index: &mut LibraryIndex,
    entries: &[crate::sgf_library::SgfLibraryEntry],
) -> Result<Vec<IngestOutcome>, LibraryStoreError> {
    let previous = index.clone();
    let mut outcomes = Vec::with_capacity(entries.len());
    for entry in entries {
        let content = io.read_source_file(&entry.path)?;
        let source = RecordSource::Git {
            source_id: entry.source_id.clone(),
            relative_path: entry.relative_path.clone(),
        };
        outcomes.push(ingest_record_no_save(index, source, &content, Vec::new()));
    }
    let result = io.save_index(index).map(|_| outcomes);
    if result.is_err() {
        *index = previous;
    }
    result
}

/// Ingests a local SGF file by path. The canonical path becomes the record's
/// provenance; content is read only for metadata and never copied into the
/// managed root (the file already lives where the user put it).
pub fn ingest_local_path(
    io: &impl LibraryStoreIo,
    index: &mut LibraryIndex,
    path: &Path,
) -> Result<IngestOutcome, LibraryStoreError> {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let content = io.read_source_file(&canonical)?;
    let previous = index.clone();
    let source = RecordSource::Local {
        path: canonical.clone(),
    };
    let outcome = ingest_record_no_save(index, source, &content, Vec::new());
    let result = io.save_index(index).map(|_| outcome);
    if result.is_err() {
        *index = previous;
    }
    result
}

/// Shared pure ingest body: normalizes metadata from the SGF root, inserts the
/// record into the index (dedupe + stable numbering) and returns the outcome.
/// Does no I/O; callers persist through the adapter and own rollback.
fn ingest_record_no_save(
    index: &mut LibraryIndex,
    source: RecordSource,
    content: &str,
    tags: Vec<String>,
) -> IngestOutcome {
    let now = unix_milliseconds();
    let id = RecordId::for_source(&source);
    let properties = ryusei_domain_core::extract_root_properties(content);
    let metadata = RecordMetadata::from_root_properties(&properties);
    let title = metadata.display_name(source.kind_label());
    let fingerprint = Some(fingerprint_content(content));
    let existing = index.get(&id);
    let created_at = existing.map(|e| e.created_at).unwrap_or(now);
    let updated_at = match existing {
        Some(e) if e.content_fingerprint == fingerprint => e.updated_at,
        _ => now,
    };

    let record = GameRecord {
        id,
        number: RecordNumber(0),
        title,
        source,
        metadata,
        tags,
        content_fingerprint: fingerprint,
        revisions: Vec::new(),
        created_at,
        updated_at,
    };
    let (number, outcome) = index.insert(record.clone());
    IngestOutcome {
        record,
        number,
        outcome,
    }
}

/// Records a versioned revision of an existing library record. The revision
/// reference (with content fingerprint) is appended to the record in the index,
/// bounded to `limit` newest entries; when a local root is configured the
/// revision's content is also snapshotted under the managed root. Returns the
/// revision reference, or `None` when no record with `id` exists. This is the
/// versioned-history seam: single-slot crash recovery (`autosave`) remains a
/// separate concern and is not mixed into the permanent library.
pub fn append_library_revision(
    io: &impl LibraryStoreIo,
    index: &mut LibraryIndex,
    id: &RecordId,
    content: &str,
    trigger: ryusei_domain_core::RevisionTrigger,
    limit: usize,
) -> Result<Option<ryusei_domain_core::RecordRevisionRef>, LibraryStoreError> {
    let previous = index.clone();
    let now = unix_milliseconds();
    let revision_ref = ryusei_domain_core::RecordRevisionRef {
        revision: 0,
        saved_at_unix_milliseconds: now,
        trigger,
        content_fingerprint: Some(fingerprint_content(content)),
    };
    let result = (|| {
        let sequence = index
            .push_revision(id, revision_ref.clone(), limit)
            .ok_or_else(|| {
                LibraryStoreError::RecordRead(format!("no record with id `{}`", id.as_str()))
            })?;
        if io.local_root().is_some() {
            io.write_revision(id, sequence, content)?;
        }
        io.save_index(index)?;
        Ok(Some(ryusei_domain_core::RecordRevisionRef {
            revision: sequence,
            ..revision_ref
        }))
    })();
    if result.is_err() {
        *index = previous;
    }
    result
}

/// Deletes a record from the index and from the managed root (when enabled).
/// Returns `false` when no such record existed.
pub fn remove_library_record(
    io: &impl LibraryStoreIo,
    index: &mut LibraryIndex,
    id: &RecordId,
) -> Result<bool, LibraryStoreError> {
    let previous = index.clone();
    let existed = index.remove(id);
    if !existed {
        return Ok(false);
    }
    let result = (|| {
        if io.local_root().is_some() {
            io.delete_record(id)?;
        }
        io.save_index(index)?;
        Ok(true)
    })();
    if result.is_err() {
        *index = previous;
    }
    result
}

/// Loads a record's SGF content. Content lives under the managed local root for
/// records persisted there; the caller decides what to do for sources whose
/// content is already on disk (Local / Git paths).
pub fn read_library_record(
    io: &impl LibraryStoreIo,
    id: &RecordId,
) -> Result<String, LibraryStoreError> {
    io.read_record(id)
}

fn unix_milliseconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

/// Filesystem-safe name for a record's content under the managed root: a hex
/// digest of the record id, so ids containing path separators or `..` can never
/// escape the records directory.
pub fn record_file_name(id: &RecordId) -> String {
    format!("{}.sgf", &fingerprint_content(id.as_str())[..16])
}

/// Resolves the managed content path for a record id. Returns
/// `LocalPersistenceDisabled` when the adapter has no local root.
pub fn managed_record_path(
    io: &impl LibraryStoreIo,
    id: &RecordId,
) -> Result<PathBuf, LibraryStoreError> {
    let root = io
        .local_root()
        .ok_or(LibraryStoreError::LocalPersistenceDisabled)?;
    Ok(root.join(LIBRARY_RECORDS_DIR).join(record_file_name(id)))
}

/// A filesystem adapter rooted at `local_root`. All writes are atomic
/// (temporary sibling + rename) so a crash never leaves a half-written index or
/// record.
pub struct FsLibraryStore {
    local_root: PathBuf,
}

impl FsLibraryStore {
    pub fn new(local_root: PathBuf) -> Self {
        Self { local_root }
    }

    fn index_path(&self) -> PathBuf {
        self.local_root.join(LIBRARY_INDEX_FILE_NAME)
    }

    fn records_dir(&self) -> PathBuf {
        self.local_root.join(LIBRARY_RECORDS_DIR)
    }
}

impl LibraryStoreIo for FsLibraryStore {
    fn load_index(&self) -> Result<Option<LibraryIndex>, LibraryStoreError> {
        let path = self.index_path();
        if !path.is_file() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&path).map_err(|error| {
            LibraryStoreError::IndexRead(format!("{}: {error}", path.display()))
        })?;
        LibraryIndex::from_json(&content)
            .map(Some)
            .map_err(LibraryStoreError::IndexRead)
    }

    fn save_index(&self, index: &LibraryIndex) -> Result<(), LibraryStoreError> {
        let json = index.to_json().map_err(LibraryStoreError::IndexWrite)?;
        atomic_write_text(&self.index_path(), &json)
            .map_err(|e| LibraryStoreError::IndexWrite(e.to_string()))
    }

    fn local_root(&self) -> Option<&Path> {
        Some(&self.local_root)
    }

    fn write_record(&self, id: &RecordId, content: &str) -> Result<(), LibraryStoreError> {
        let path = self.records_dir().join(record_file_name(id));
        atomic_write_text(&path, content).map_err(|e| LibraryStoreError::RecordWrite(e.to_string()))
    }

    fn read_record(&self, id: &RecordId) -> Result<String, LibraryStoreError> {
        let path = self.records_dir().join(record_file_name(id));
        std::fs::read_to_string(&path)
            .map_err(|error| LibraryStoreError::RecordRead(format!("{}: {error}", path.display())))
    }

    fn delete_record(&self, id: &RecordId) -> Result<(), LibraryStoreError> {
        let path = self.records_dir().join(record_file_name(id));
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(LibraryStoreError::RecordDelete(format!(
                "{}: {error}",
                path.display()
            ))),
        }
    }

    fn write_revision(
        &self,
        id: &RecordId,
        revision: u64,
        content: &str,
    ) -> Result<(), LibraryStoreError> {
        let path = self
            .local_root
            .join(LIBRARY_REVISIONS_DIR)
            .join(record_file_name(id))
            .join(format!("{revision}.sgf"));
        atomic_write_text(&path, content).map_err(|e| LibraryStoreError::RecordWrite(e.to_string()))
    }

    fn read_revision(&self, id: &RecordId, revision: u64) -> Result<String, LibraryStoreError> {
        let path = self
            .local_root
            .join(LIBRARY_REVISIONS_DIR)
            .join(record_file_name(id))
            .join(format!("{revision}.sgf"));
        std::fs::read_to_string(&path)
            .map_err(|error| LibraryStoreError::RecordRead(format!("{}: {error}", path.display())))
    }
}

/// Writes `content` to `path` atomically: parent directories are created and
/// the final write is a temporary-sibling + rename.
pub fn atomic_write_text(path: &Path, content: &str) -> Result<(), LibraryStoreError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            LibraryStoreError::FileWrite(format!("could not create {}: {error}", parent.display()))
        })?;
    }
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, content).map_err(|error| {
        LibraryStoreError::FileWrite(format!("could not write {}: {error}", temporary.display()))
    })?;
    std::fs::rename(&temporary, path).map_err(|error| {
        LibraryStoreError::FileWrite(format!(
            "could not move {} into place: {error}",
            path.display()
        ))
    })
}

/// Scans every synchronized Git checkout under `base/<source-id>` and merges
/// the results into the persistent library index (via `FsLibraryStore` at
/// `base`), so each entry carries a stable record number. Returns the entries
/// ordered by record number plus the entry-id → number map. Content that already
/// lives in the checkouts is never copied into the managed records directory.
pub fn index_library_sources(
    sources: &[crate::sgf_library::SgfLibrarySource],
    base: &Path,
) -> Result<
    (
        Vec<crate::sgf_library::SgfLibraryEntry>,
        std::collections::HashMap<String, u64>,
    ),
    LibraryStoreError,
> {
    let mut entries = Vec::new();
    for source in sources {
        entries.extend(
            crate::sgf_library::scan_sgf_library(&source.id, &base.join(&source.id))
                .map_err(|error| LibraryStoreError::IndexRead(error.to_string()))?,
        );
    }
    let store = FsLibraryStore::new(base.to_path_buf());
    let mut index = load_library(&store)?;
    let outcomes = ingest_git_entries(&store, &mut index, &entries)?;
    let numbers = entries
        .iter()
        .zip(outcomes.iter())
        .map(|(entry, outcome)| (entry.entry_id(), outcome.number.0))
        .collect::<std::collections::HashMap<String, u64>>();
    entries.sort_by_key(|entry| numbers.get(&entry.entry_id()).copied().unwrap_or(u64::MAX));
    Ok((entries, numbers))
}

/// In-memory adapter for hermetic tests. Honors the optional local root flag
/// for behaviour assertions but keeps everything in memory behind `RefCell`.
#[cfg(test)]
#[derive(Default)]
struct MemoryLibraryStoreIo {
    index: RefCell<Option<LibraryIndex>>,
    records: RefCell<std::collections::HashMap<RecordId, String>>,
    revisions: RefCell<std::collections::HashMap<(RecordId, u64), String>>,
    local_root: Option<PathBuf>,
}

#[cfg(test)]
impl LibraryStoreIo for MemoryLibraryStoreIo {
    fn load_index(&self) -> Result<Option<LibraryIndex>, LibraryStoreError> {
        Ok(self.index.borrow().clone())
    }

    fn save_index(&self, index: &LibraryIndex) -> Result<(), LibraryStoreError> {
        *self.index.borrow_mut() = Some(index.clone());
        Ok(())
    }

    fn local_root(&self) -> Option<&Path> {
        self.local_root.as_deref()
    }

    fn write_record(&self, id: &RecordId, content: &str) -> Result<(), LibraryStoreError> {
        self.records
            .borrow_mut()
            .insert(id.clone(), content.to_owned());
        Ok(())
    }

    fn read_record(&self, id: &RecordId) -> Result<String, LibraryStoreError> {
        self.records
            .borrow()
            .get(id)
            .cloned()
            .ok_or_else(|| LibraryStoreError::RecordRead("missing in memory".to_owned()))
    }

    fn delete_record(&self, id: &RecordId) -> Result<(), LibraryStoreError> {
        self.records.borrow_mut().remove(id);
        Ok(())
    }

    fn write_revision(
        &self,
        id: &RecordId,
        revision: u64,
        content: &str,
    ) -> Result<(), LibraryStoreError> {
        self.revisions
            .borrow_mut()
            .insert((id.clone(), revision), content.to_owned());
        Ok(())
    }

    fn read_revision(&self, id: &RecordId, revision: u64) -> Result<String, LibraryStoreError> {
        self.revisions
            .borrow()
            .get(&(id.clone(), revision))
            .cloned()
            .ok_or_else(|| LibraryStoreError::RecordRead("revision missing in memory".to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fox_source(chess_id: &str) -> RecordSource {
        RecordSource::Fox {
            chess_id: chess_id.to_owned(),
        }
    }

    fn fox_sgf(black: &str, white: &str) -> String {
        format!("(;GM[1]SZ[19]PB[{black}]PW[{white}]RE[B+R])")
    }

    #[test]
    fn ingest_assigns_stable_numbers_and_dedupes_by_source() {
        let io = MemoryLibraryStoreIo::default();
        let mut index = load_library(&io).expect("fresh index");

        let first = ingest_library_record(
            &io,
            &mut index,
            fox_source("g1"),
            &fox_sgf("甲", "乙"),
            vec![],
        )
        .expect("ingest one");
        assert_eq!(first.number, RecordNumber(1));
        assert_eq!(first.outcome, InsertOutcome::Added);
        assert_eq!(first.record.metadata.black.as_deref(), Some("甲"));
        assert!(first.record.content_fingerprint.as_deref().is_some());

        let second = ingest_library_record(
            &io,
            &mut index,
            fox_source("g2"),
            &fox_sgf("丙", "丁"),
            vec![],
        )
        .expect("ingest two");
        assert_eq!(second.number, RecordNumber(2));

        // Same fox game re-ingested updates in place, keeping number 1.
        let duplicate = ingest_library_record(
            &io,
            &mut index,
            fox_source("g1"),
            &fox_sgf("甲", "乙"),
            vec![],
        )
        .expect("re-ingest duplicate");
        assert_eq!(duplicate.outcome, InsertOutcome::Updated);
        assert_eq!(duplicate.number, RecordNumber(1));
        assert_eq!(index.len(), 2);
    }

    #[test]
    fn reingesting_unchanged_content_preserves_updated_at() {
        let io = MemoryLibraryStoreIo::default();
        let mut index = load_library(&io).expect("fresh index");
        let initial = ingest_library_record(
            &io,
            &mut index,
            fox_source("g1"),
            &fox_sgf("甲", "乙"),
            vec![],
        )
        .expect("initial");
        let first_updated_at = initial.record.updated_at;

        std::thread::sleep(std::time::Duration::from_millis(5));

        // Re-ingest with identical content: updated_at must NOT change!
        let again = ingest_library_record(
            &io,
            &mut index,
            fox_source("g1"),
            &fox_sgf("甲", "乙"),
            vec![],
        )
        .expect("again");
        assert_eq!(again.record.updated_at, first_updated_at);
    }

    #[test]
    fn ingest_without_local_root_persists_index_to_adapter_but_not_content() {
        let io = MemoryLibraryStoreIo::default();
        let mut index = load_library(&io).expect("fresh index");
        let ingested = ingest_library_record(
            &io,
            &mut index,
            fox_source("g1"),
            &fox_sgf("甲", "乙"),
            vec![],
        )
        .expect("ingest");
        assert_eq!(ingested.number, RecordNumber(1));
        // No local root: content is not stored by the workflow.
        assert!(io.local_root().is_none());
    }

    #[test]
    fn ingest_failure_restores_the_previous_index() {
        // An adapter whose save_index always fails must leave the index intact.
        struct FailingIo {
            inner: MemoryLibraryStoreIo,
        }
        impl LibraryStoreIo for FailingIo {
            fn load_index(&self) -> Result<Option<LibraryIndex>, LibraryStoreError> {
                self.inner.load_index()
            }
            fn save_index(&self, _: &LibraryIndex) -> Result<(), LibraryStoreError> {
                Err(LibraryStoreError::IndexWrite("disk full".to_owned()))
            }
            fn local_root(&self) -> Option<&Path> {
                self.inner.local_root()
            }
            fn write_record(&self, id: &RecordId, content: &str) -> Result<(), LibraryStoreError> {
                self.inner.write_record(id, content)
            }
            fn read_record(&self, id: &RecordId) -> Result<String, LibraryStoreError> {
                self.inner.read_record(id)
            }
            fn delete_record(&self, id: &RecordId) -> Result<(), LibraryStoreError> {
                self.inner.delete_record(id)
            }
        }

        let io = FailingIo {
            inner: MemoryLibraryStoreIo::default(),
        };
        let mut index = load_library(&io).expect("fresh index");
        assert!(
            ingest_library_record(
                &io,
                &mut index,
                fox_source("g1"),
                &fox_sgf("甲", "乙"),
                vec![]
            )
            .is_err()
        );
        assert!(index.is_empty(), "failed ingest must roll the index back");
    }

    #[test]
    fn fs_adapter_persists_and_reloads_index_and_records() {
        let root = std::env::temp_dir().join("ryusei-host-library-store-test");
        let _ = std::fs::remove_dir_all(&root);
        let io = FsLibraryStore::new(root.clone());
        let mut index = load_library(&io).expect("fresh fs index");

        let fox = ingest_library_record(
            &io,
            &mut index,
            fox_source("g1"),
            &fox_sgf("柯洁", "申真谞"),
            vec!["大赛".to_owned()],
        )
        .expect("ingest fox into fs store");
        assert_eq!(fox.number, RecordNumber(1));

        // The record file and index were written under the local root.
        let id = &fox.record.id;
        assert!(root.join("records").join(record_file_name(id)).is_file());
        assert!(root.join(LIBRARY_INDEX_FILE_NAME).is_file());
        assert!(io.read_record(id).expect("read back").contains("柯洁"));

        // A second store over the same root reloads index + numbers.
        let io2 = FsLibraryStore::new(root.clone());
        let mut reloaded = load_library(&io2).expect("reload index");
        assert_eq!(reloaded.len(), 1);
        let next = ingest_library_record(
            &io2,
            &mut reloaded,
            fox_source("g2"),
            &fox_sgf("甲", "乙"),
            vec![],
        )
        .expect("ingest into reloaded");
        // New store began numbering where the reloaded index left off.
        assert_eq!(next.number, RecordNumber(2));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn remove_library_record_deletes_content_and_index_entry() {
        let root = std::env::temp_dir().join("ryusei-host-library-remove-test");
        let _ = std::fs::remove_dir_all(&root);
        let io = FsLibraryStore::new(root.clone());
        let mut index = load_library(&io).expect("fresh");
        let ingested = ingest_library_record(
            &io,
            &mut index,
            fox_source("g1"),
            &fox_sgf("甲", "乙"),
            vec![],
        )
        .expect("ingest");
        assert_eq!(index.len(), 1);

        let removed = remove_library_record(&io, &mut index, &ingested.record.id).expect("remove");
        assert!(removed);
        assert!(index.is_empty());
        assert!(
            !root
                .join("records")
                .join(record_file_name(&ingested.record.id))
                .exists()
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn record_file_name_is_safe_for_hostile_ids() {
        // Even an id trying to escape the root maps to a hex filename inside
        // records/, never to a path with separators or parent segments.
        let hostile = RecordId::for_source(&RecordSource::Fox {
            chess_id: "../../evil".to_owned(),
        });
        let name = record_file_name(&hostile);
        assert!(!name.contains('/'));
        assert!(!name.contains(".."));
        assert!(name.ends_with(".sgf"));
        // Local paths embed slashes; the digest must still collapse them.
        let local = RecordId::for_source(&RecordSource::Local {
            path: PathBuf::from("/etc/passwd"),
        });
        let local_name = record_file_name(&local);
        assert!(!local_name.contains('/'));
        assert_eq!(local_name.len(), 20);
    }

    #[test]
    fn ingest_git_entry_reads_content_but_never_copies_it_into_the_managed_root() {
        let root = std::env::temp_dir().join("ryusei-host-library-git-ingest-test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("checkout")).expect("checkout dir");
        let sgf = root.join("checkout").join("a.sgf");
        std::fs::write(&sgf, "(;GM[1]PB[黑]PW[白]RE[W+R])").expect("write sgf");

        let entries =
            crate::sgf_library::scan_sgf_library("pro", &root.join("checkout")).expect("scan");
        assert_eq!(entries.len(), 1);

        let io = FsLibraryStore::new(root.join("managed"));
        let mut index = load_library(&io).expect("fresh index");
        let ingested = ingest_git_entry(&io, &mut index, &entries[0]).expect("ingest git entry");
        assert_eq!(ingested.number, RecordNumber(1));
        assert!(matches!(
            &ingested.record.source,
            RecordSource::Git {
                source_id,
                relative_path
            } if source_id == "pro" && relative_path == "a.sgf"
        ));
        assert_eq!(ingested.record.metadata.black.as_deref(), Some("黑"));

        // Content already lives in the checkout; the managed records dir holds
        // no duplicate of it.
        assert!(!root.join("managed").join("records").exists());
        assert!(root.join("managed").join(LIBRARY_INDEX_FILE_NAME).is_file());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn ingest_local_path_uses_canonical_source_and_metadata() {
        let root = std::env::temp_dir().join("ryusei-host-library-local-ingest-test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("dir");
        let sgf = root.join("local.sgf");
        std::fs::write(&sgf, "(;SZ[9]PB[甲]PW[乙]GN[本地对局])").expect("write sgf");

        let io = FsLibraryStore::new(root.join("managed"));
        let mut index = load_library(&io).expect("fresh index");
        let ingested = ingest_local_path(&io, &mut index, &sgf).expect("ingest local path");
        assert_eq!(ingested.number, RecordNumber(1));
        assert!(matches!(&ingested.record.source, RecordSource::Local { path } if path.is_file()));
        assert_eq!(
            ingested.record.metadata.game_name.as_deref(),
            Some("本地对局")
        );
        // No duplicate content in the managed root.
        assert!(!root.join("managed").join("records").exists());

        // Ingesting the same canonical file again is an update, not a duplicate.
        let again = ingest_local_path(&io, &mut index, &sgf).expect("re-ingest local path");
        assert_eq!(again.outcome, InsertOutcome::Updated);
        assert_eq!(again.number, RecordNumber(1));
        assert_eq!(index.len(), 1);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn append_library_revision_records_content_and_bounds_history() {
        use ryusei_domain_core::RevisionTrigger;

        let root = std::env::temp_dir().join("ryusei-host-library-revision-test");
        let _ = std::fs::remove_dir_all(&root);
        let io = FsLibraryStore::new(root.clone());
        let mut index = load_library(&io).expect("fresh index");
        let ingested = ingest_library_record(
            &io,
            &mut index,
            fox_source("g1"),
            &fox_sgf("甲", "乙"),
            vec![],
        )
        .expect("ingest");
        let id = ingested.record.id.clone();

        for revision in 1..=5u64 {
            let content = format!("(;GM[1]PB[甲]PW[乙]C[revision {revision}])");
            let recorded = append_library_revision(
                &io,
                &mut index,
                &id,
                &content,
                RevisionTrigger::ManualSave,
                3,
            )
            .expect("append revision")
            .expect("record exists");
            assert_eq!(recorded.revision, revision);
        }

        // History is bounded to the newest 3.
        let record = index.get(&id).expect("record exists");
        let seqs: Vec<u64> = record.revisions.iter().map(|r| r.revision).collect();
        assert_eq!(seqs, vec![5, 4, 3]);

        // Revision content was snapshotted under the managed root.
        let name = record_file_name(&id);
        let revision_dir = root.join(LIBRARY_REVISIONS_DIR).join(&name);
        assert!(revision_dir.join("1.sgf").is_file());
        assert!(revision_dir.join("5.sgf").is_file());
        let read_back = io.read_revision(&id, 5).expect("read persisted revision");
        assert!(read_back.contains("revision 5"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn append_library_revision_without_local_root_records_reference_only() {
        use ryusei_domain_core::RevisionTrigger;

        let io = MemoryLibraryStoreIo::default();
        let mut index = load_library(&io).expect("fresh index");
        let ingested = ingest_library_record(
            &io,
            &mut index,
            fox_source("g1"),
            &fox_sgf("甲", "乙"),
            vec![],
        )
        .expect("ingest");
        let id = ingested.record.id.clone();

        let recorded = append_library_revision(
            &io,
            &mut index,
            &id,
            "(;GM[1]C[note])",
            RevisionTrigger::Autosave,
            DEFAULT_REVISION_LIMIT,
        )
        .expect("append")
        .expect("record exists");
        assert_eq!(recorded.revision, 1);
        assert_eq!(recorded.trigger, RevisionTrigger::Autosave);
        // No local root: reference exists but persisted content is unavailable.
        assert!(io.read_revision(&id, 1).is_err());
        // Appending for a missing record is a no-op that keeps the index intact.
        let ghost = RecordId::for_source(&RecordSource::Ogs { game_id: 999 });
        assert!(
            append_library_revision(
                &io,
                &mut index,
                &ghost,
                "(;C[x])",
                RevisionTrigger::Import,
                3
            )
            .is_err()
        );
        assert_eq!(index.len(), 1);
    }

    #[test]
    fn ingest_git_entries_batch_writes_once_and_numbers_survive_reopen() {
        let root = std::env::temp_dir().join("ryusei-host-library-git-batch-test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("checkout")).expect("checkout dir");
        for name in ["a.sgf", "b.sgf", "c.sgf"] {
            std::fs::write(
                root.join("checkout").join(name),
                format!("(;GM[1]PB[黑]PW[白]GN[{name}])"),
            )
            .expect("write fixture");
        }

        let managed = root.join("managed");
        let io = FsLibraryStore::new(managed.clone());
        let mut index = load_library(&io).expect("fresh index");
        let entries =
            crate::sgf_library::scan_sgf_library("pro", &root.join("checkout")).expect("scan");
        let outcomes = ingest_git_entries(&io, &mut index, &entries).expect("batch ingest");
        assert_eq!(outcomes.len(), 3);
        assert_eq!(index.len(), 3);

        // A reopened store sees the same records with the same numbers, so a
        // freshly added file continues numbering (never reusing old numbers).
        let io2 = FsLibraryStore::new(managed);
        let mut reopened = load_library(&io2).expect("reopen");
        assert_eq!(reopened.len(), 3);
        std::fs::write(root.join("checkout").join("d.sgf"), "(;GM[1]PB[甲]PW[乙])")
            .expect("write d");
        let more =
            crate::sgf_library::scan_sgf_library("pro", &root.join("checkout")).expect("scan d");
        let d = more
            .iter()
            .find(|e| e.relative_path == "d.sgf")
            .expect("d present");
        let added = ingest_git_entry(&io2, &mut reopened, d).expect("ingest d");
        assert_eq!(
            added.number,
            RecordNumber(4),
            "no number reuse after reopen"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn index_library_sources_orchestrates_scanning_batch_ingest_and_ordering() {
        let root = std::env::temp_dir().join("ryusei-host-index-sources-test");
        let _ = std::fs::remove_dir_all(&root);
        let src1_dir = root.join("src1");
        let src2_dir = root.join("src2");
        std::fs::create_dir_all(&src1_dir).expect("src1");
        std::fs::create_dir_all(&src2_dir).expect("src2");
        std::fs::write(src1_dir.join("a.sgf"), "(;GM[1]PB[A1]PW[W1])").expect("a.sgf");
        std::fs::write(src2_dir.join("b.sgf"), "(;GM[1]PB[A2]PW[W2])").expect("b.sgf");

        let sources = vec![
            crate::sgf_library::SgfLibrarySource {
                id: "src1".to_owned(),
                name: "Source 1".to_owned(),
                github_url: "https://github.com/test/src1".to_owned(),
                reference: "main".to_owned(),
                rights: crate::sgf_library::RedistributionRights::Permitted,
                license_name: Some("MIT".to_owned()),
                license_url: Some("https://example.com".to_owned()),
            },
            crate::sgf_library::SgfLibrarySource {
                id: "src2".to_owned(),
                name: "Source 2".to_owned(),
                github_url: "https://github.com/test/src2".to_owned(),
                reference: "main".to_owned(),
                rights: crate::sgf_library::RedistributionRights::Permitted,
                license_name: Some("MIT".to_owned()),
                license_url: Some("https://example.com".to_owned()),
            },
        ];

        let (entries, numbers) = index_library_sources(&sources, &root).expect("index sources");
        assert_eq!(entries.len(), 2);
        assert_eq!(numbers.len(), 2);
        assert_eq!(numbers.get("src1-a.sgf"), Some(&1));
        assert_eq!(numbers.get("src2-b.sgf"), Some(&2));
        assert_eq!(entries[0].relative_path, "a.sgf");
        assert_eq!(entries[1].relative_path, "b.sgf");

        let _ = std::fs::remove_dir_all(&root);
    }
}
