//! Unified library domain vocabulary.
//!
//! This module is the shared model behind the game-record library: a single
//! `GameRecord` with a stable `RecordId`, a stable `RecordNumber`, a typed
//! `RecordSource` provenance (Local / Git / OGS / Fox / Live), normalized
//! `RecordMetadata`, and a `LibraryIndex` that owns numbering, deduplication,
//! and querying. UI layers render summaries of these records; IO layers
//! (scanning, saving) live behind adapters in `ryusei-host`.
//!
//! Nothing in this module touches the file system or the network: every type
//! is a pure value with serde round-tripping, which keeps the vocabulary
//! testable at a hermetic seam.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::Properties;

pub const LIBRARY_SCHEMA_VERSION: u32 = 1;

/// Opaque stable identity of one library record. Identity is derived from the
/// record's provenance (the same external game always maps to the same id), so
/// re-ingesting a Git/OGS/Fox/Live record updates it instead of duplicating it.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RecordId(String);

impl RecordId {
    pub fn for_source(source: &RecordSource) -> Self {
        let key = match source {
            RecordSource::Local { path } => format!("local:{}", path.display()),
            RecordSource::Git {
                source_id,
                relative_path,
            } => format!("git:{source_id}:{relative_path}"),
            RecordSource::Ogs { game_id } => format!("ogs:{game_id}"),
            RecordSource::Fox { chess_id } => format!("fox:{chess_id}"),
            RecordSource::Live { page_url } => format!("live:{page_url}"),
        };
        Self(key)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Stable, human-facing ordinal of a record inside the library. Allocated once
/// by `LibraryIndex::insert`; it never changes when the record is updated,
/// re-sorted, or filtered, so list views can display it as "game number".
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RecordNumber(pub u64);

/// Provenance of a record. The internal `kind` tag keeps serde JSON backwards
/// compatible: old `SgfLibraryEntry` JSON maps onto `Git`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RecordSource {
    /// A file on the local machine (canonical path).
    Local { path: PathBuf },
    /// A file inside a license-gated Git synchronization source.
    Git {
        source_id: String,
        relative_path: String,
    },
    /// A game on the Online Go Server.
    Ogs { game_id: u64 },
    /// A game on the Fox Go Server (野狐).
    Fox { chess_id: String },
    /// A read-only live broadcast (OGS public game or live page).
    Live { page_url: String },
}

impl RecordSource {
    pub fn kind_label(&self) -> &'static str {
        match self {
            Self::Local { .. } => "本地",
            Self::Git { .. } => "Git",
            Self::Ogs { .. } => "OGS",
            Self::Fox { .. } => "野狐",
            Self::Live { .. } => "直播",
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Self::Local { .. } => "local",
            Self::Git { .. } => "git",
            Self::Ogs { .. } => "ogs",
            Self::Fox { .. } => "fox",
            Self::Live { .. } => "live",
        }
    }
}

/// Header metadata normalized from SGF root properties. Komi is kept as a
/// string (the SGF representation is decimal text) so the type can derive
/// `Eq`/`PartialEq` without floating-point surprises.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordMetadata {
    pub black: Option<String>,
    pub white: Option<String>,
    pub result: Option<String>,
    pub game_name: Option<String>,
    pub date: Option<String>,
    pub event: Option<String>,
    pub round: Option<String>,
    pub komi: Option<String>,
    pub rules: Option<String>,
    pub handicap: Option<u8>,
    pub board_size: Option<u8>,
}

impl RecordMetadata {
    /// Normalizes a raw SGF root-property map into header metadata. Missing or
    /// empty values become `None`; malformed numeric fields are ignored rather
    /// than failing the whole record.
    pub fn from_root_properties(properties: &Properties) -> Self {
        let first = |key: &str| {
            properties
                .get(key)
                .and_then(|values| values.first())
                .filter(|value| !value.is_empty())
                .cloned()
        };
        let first_parsed = |key: &str, parse: fn(&str) -> Option<u8>| -> Option<u8> {
            first(key).and_then(|value| parse(&value))
        };
        Self {
            black: first("PB"),
            white: first("PW"),
            result: first("RE"),
            game_name: first("GN"),
            date: first("DT"),
            event: first("EV"),
            round: first("RO"),
            komi: first("KM"),
            rules: first("RU"),
            handicap: first_parsed("HA", parse_small_u8),
            board_size: first_parsed("SZ", parse_small_u8),
        }
    }

    /// Human-facing game title: the game name (`GN`) when present, otherwise a
    /// `black vs white` pairing, otherwise the supplied fallback.
    pub fn display_name(&self, fallback: &str) -> String {
        match self.game_name() {
            Some(name) => name,
            None => match (self.black.as_deref(), self.white.as_deref()) {
                (Some(black), Some(white)) if !black.is_empty() && !white.is_empty() => {
                    format!("{black} vs {white}")
                }
                _ => fallback.to_owned(),
            },
        }
    }

    pub fn game_name(&self) -> Option<String> {
        self.game_name
            .as_deref()
            .filter(|name| !name.is_empty())
            .map(ToOwned::to_owned)
    }
}

/// Parses a small unsigned integer from the SGF representation. `SZ` may be a
/// single number or a `width:height` pair, in which case the width is used.
fn parse_small_u8(value: &str) -> Option<u8> {
    let width = value.split(':').next().unwrap_or(value);
    width.trim().parse::<u8>().ok()
}

/// Why a revision was recorded. Autosave is *not* a parallel record source: it
/// is one trigger among others for a revision of an existing library record,
/// keeping crash recovery separate from the permanent library.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RevisionTrigger {
    ManualSave,
    Autosave,
    Import,
}

/// A point-in-time snapshot reference of a record. Revision *content* lives in
/// host persistence; the index only tracks the metadata so it stays small.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordRevisionRef {
    /// Monotonic per-record revision number, starting at 1.
    pub revision: u64,
    pub saved_at_unix_milliseconds: u64,
    pub trigger: RevisionTrigger,
    /// Content fingerprint at the moment this revision was taken.
    #[serde(default)]
    pub content_fingerprint: Option<String>,
}

/// A full library record. The SGF content itself is not held here: it lives on
/// disk and is resolved by the host through the record's `source`. This keeps
/// the index small and the type a pure value.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameRecord {
    pub id: RecordId,
    /// Stable ordinal; `0` before the record has been inserted into an index.
    pub number: RecordNumber,
    pub title: String,
    pub source: RecordSource,
    pub metadata: RecordMetadata,
    #[serde(default)]
    pub tags: Vec<String>,
    /// SHA-256 of the SGF content, when known, used for thumbnail invalidation
    /// and change detection.
    #[serde(default)]
    pub content_fingerprint: Option<String>,
    /// Most recent revisions of this record, newest first, bounded by the host
    /// when pushing. `#[serde(default)]` keeps old index files readable.
    #[serde(default)]
    pub revisions: Vec<RecordRevisionRef>,
    pub created_at: u64,
    pub updated_at: u64,
}

/// The persisted, queryable library. Owns stable numbering and deduplication.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryIndex {
    #[serde(default = "default_library_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    next_record_number: u64,
    records: BTreeMap<RecordId, GameRecord>,
}

impl Default for LibraryIndex {
    fn default() -> Self {
        Self::new()
    }
}

fn default_library_schema_version() -> u32 {
    LIBRARY_SCHEMA_VERSION
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InsertOutcome {
    /// A brand-new record; a fresh number was allocated.
    Added,
    /// An existing record with the same identity was replaced; the original
    /// number was preserved.
    Updated,
}

impl LibraryIndex {
    pub fn new() -> Self {
        Self {
            schema_version: LIBRARY_SCHEMA_VERSION,
            next_record_number: 1,
            records: BTreeMap::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn get(&self, id: &RecordId) -> Option<&GameRecord> {
        self.records.get(id)
    }

    /// Appends a revision to a stored record. Revisions are kept newest-first
    /// and bounded to `cap` entries (oldest dropped). Returns the revision's
    /// sequence number, or `None` when no record with `id` exists.
    pub fn push_revision(
        &mut self,
        id: &RecordId,
        revision: RecordRevisionRef,
        cap: usize,
    ) -> Option<u64> {
        let record = self.records.get_mut(id)?;
        let next = record
            .revisions
            .first()
            .map(|latest| latest.revision.saturating_add(1))
            .unwrap_or(1);
        let revision = RecordRevisionRef {
            revision: next,
            ..revision
        };
        record.revisions.insert(0, revision.clone());
        if record.revisions.len() > cap.max(1) {
            record.revisions.truncate(cap.max(1));
        }
        Some(revision.revision)
    }

    /// Inserts or updates a record. New records receive the next stable
    /// `RecordNumber`; updated records keep their original number. Returns the
    /// stored record's number and whether it was added or updated.
    pub fn insert(&mut self, mut record: GameRecord) -> (RecordNumber, InsertOutcome) {
        let (number, outcome) = match self.records.get(&record.id) {
            Some(existing) => {
                let number = existing.number;
                record.number = number;
                (number, InsertOutcome::Updated)
            }
            None => {
                let number = RecordNumber(self.next_record_number);
                self.next_record_number += 1;
                record.number = number;
                (number, InsertOutcome::Added)
            }
        };
        self.records.insert(record.id.clone(), record);
        (number, outcome)
    }

    pub fn remove(&mut self, id: &RecordId) -> bool {
        self.records.remove(id).is_some()
    }

    pub fn records(&self) -> impl Iterator<Item = &GameRecord> {
        self.records.values()
    }

    /// Queries the index. Filters are ANDed; text searches across title, black,
    /// white, event and round. Results are sorted per `LibraryQuery::sort`.
    pub fn query(&self, query: &LibraryQuery) -> Vec<&GameRecord> {
        let mut results: Vec<&GameRecord> = self
            .records
            .values()
            .filter(|record| matches_query(record, query))
            .collect();
        match query.sort {
            LibrarySort::NumberAscending => {
                results.sort_by_key(|record| record.number);
            }
            LibrarySort::UpdatedDescending => {
                results.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
            }
            LibrarySort::TitleAscending => {
                results.sort_by(|left, right| left.title.cmp(&right.title));
            }
        }
        if let Some(limit) = query.limit {
            results.truncate(limit);
        }
        results
    }

    pub fn to_json(&self) -> Result<String, String> {
        serde_json::to_string(self).map_err(|error| error.to_string())
    }

    pub fn from_json(json: &str) -> Result<Self, String> {
        let index: Self = serde_json::from_str(json)
            .map_err(|error| format!("invalid library index: {error}"))?;
        if index.schema_version > LIBRARY_SCHEMA_VERSION {
            return Err(format!(
                "library index schema version {} is newer than supported {LIBRARY_SCHEMA_VERSION}",
                index.schema_version
            ));
        }
        Ok(index)
    }
}

fn matches_query(record: &GameRecord, query: &LibraryQuery) -> bool {
    if let Some(text) = query
        .text
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        let needle = text.to_lowercase();
        let haystacks = [
            record.title.to_lowercase(),
            record
                .metadata
                .black
                .clone()
                .unwrap_or_default()
                .to_lowercase(),
            record
                .metadata
                .white
                .clone()
                .unwrap_or_default()
                .to_lowercase(),
            record
                .metadata
                .event
                .clone()
                .unwrap_or_default()
                .to_lowercase(),
            record
                .metadata
                .round
                .clone()
                .unwrap_or_default()
                .to_lowercase(),
        ];
        if !haystacks.iter().any(|haystack| haystack.contains(&needle)) {
            return false;
        }
    }
    if let Some(player) = query
        .player
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
    {
        let needle = player.to_lowercase();
        let black = record
            .metadata
            .black
            .as_deref()
            .unwrap_or_default()
            .to_lowercase();
        let white = record
            .metadata
            .white
            .as_deref()
            .unwrap_or_default()
            .to_lowercase();
        if !black.contains(&needle) && !white.contains(&needle) {
            return false;
        }
    }
    if let Some(result) = query
        .result
        .as_deref()
        .map(str::trim)
        .filter(|r| !r.is_empty())
    {
        if record
            .metadata
            .result
            .as_deref()
            .is_none_or(|value| value != result)
        {
            return false;
        }
    }
    if let Some(kind) = query.source_kind.as_deref() {
        if record.source.kind() != kind {
            return false;
        }
    }
    if let Some(tag) = query
        .tag
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        if !record.tags.iter().any(|candidate| candidate == tag) {
            return false;
        }
    }
    if let Some(from) = query
        .date_from
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty())
    {
        let date = record.metadata.date.as_deref().unwrap_or_default();
        if date < from {
            return false;
        }
    }
    if let Some(to) = query
        .date_to
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty())
    {
        let date = record.metadata.date.as_deref().unwrap_or_default();
        if date.is_empty() || date > to {
            return false;
        }
    }
    true
}

/// Filters and ordering for a library query. The result of a query is a list of
/// `&GameRecord`; stable numbers come from each record's `number`, never from
/// the list position.
#[derive(Clone, Debug, Default)]
pub struct LibraryQuery {
    /// Free text across title / players / event / round.
    pub text: Option<String>,
    pub player: Option<String>,
    pub result: Option<String>,
    /// One of `RecordSource::kind()` values, e.g. `"fox"`, `"ogs"`, `"git"`.
    pub source_kind: Option<String>,
    pub tag: Option<String>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    /// Maximum number of records to return after sorting (for pagination).
    pub limit: Option<usize>,
    pub sort: LibrarySort,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LibrarySort {
    /// Most recently updated first (default, matches the library history view).
    #[default]
    UpdatedDescending,
    /// Number ascending: the library's stable insertion order.
    NumberAscending,
    /// Alphabetical by title.
    TitleAscending,
}
