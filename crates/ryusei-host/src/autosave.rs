use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

pub const AUTOSAVE_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedAutosave {
    pub schema_version: u32,
    pub sgf: String,
    pub revision: u64,
    pub saved_at_unix_milliseconds: u128,
    pub source_display_name: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct AutosaveStore {
    recovery: Option<PersistedAutosave>,
    is_recovery_pending: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutosaveInfo {
    pub is_available: bool,
    pub revision: Option<u64>,
    pub saved_at_unix_milliseconds: Option<u128>,
    pub source_display_name: Option<String>,
}

#[derive(Clone, Debug)]
pub struct AutosaveCandidate {
    pub sgf: String,
    pub revision: u64,
    pub source_display_name: Option<String>,
}

impl AutosaveStore {
    pub fn from_persisted(recovery: PersistedAutosave) -> Self {
        Self {
            recovery: Some(recovery),
            is_recovery_pending: true,
        }
    }

    pub fn persisted(&self) -> Option<&PersistedAutosave> {
        self.recovery.as_ref()
    }

    pub fn info(&self) -> AutosaveInfo {
        AutosaveInfo {
            is_available: self.recovery.is_some() && self.is_recovery_pending,
            revision: self.recovery.as_ref().map(|recovery| recovery.revision),
            saved_at_unix_milliseconds: self
                .recovery
                .as_ref()
                .map(|recovery| recovery.saved_at_unix_milliseconds),
            source_display_name: self
                .recovery
                .as_ref()
                .and_then(|recovery| recovery.source_display_name.clone()),
        }
    }

    pub fn has_recovery(&self) -> bool {
        self.recovery.is_some()
    }

    pub fn recovery_sgf(&self) -> Option<String> {
        self.recovery.as_ref().map(|recovery| recovery.sgf.clone())
    }

    pub fn resolve_restore(&mut self) -> Option<AutosaveCandidate> {
        let recovery = self.recovery.as_ref()?;
        self.is_recovery_pending = false;
        Some(AutosaveCandidate {
            sgf: recovery.sgf.clone(),
            revision: recovery.revision,
            source_display_name: recovery.source_display_name.clone(),
        })
    }

    pub fn resolve_discard(&mut self) {
        self.clear();
    }

    pub fn set_recovery_pending(&mut self, is_recovery_pending: bool) {
        self.is_recovery_pending = is_recovery_pending;
    }

    pub fn is_recovery_pending(&self) -> bool {
        self.is_recovery_pending
    }

    pub fn replace_with(&mut self, candidate: AutosaveCandidate) {
        self.recovery = Some(PersistedAutosave {
            schema_version: AUTOSAVE_SCHEMA_VERSION,
            sgf: candidate.sgf,
            revision: candidate.revision,
            saved_at_unix_milliseconds: current_unix_milliseconds(),
            source_display_name: candidate.source_display_name,
        });
    }

    pub fn clear(&mut self) {
        self.recovery = None;
        self.is_recovery_pending = false;
    }
}

fn current_unix_milliseconds() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{AutosaveCandidate, AutosaveStore};

    #[test]
    fn restoring_a_recovery_releases_the_gate_and_preserves_the_candidate() {
        let mut store = AutosaveStore::default();
        store.replace_with(AutosaveCandidate {
            sgf: "(;C[recovery])".to_owned(),
            revision: 9,
            source_display_name: Some("board.sgf".to_owned()),
        });
        store.set_recovery_pending(true);

        let candidate = store.resolve_restore().expect("recovery must exist");

        assert!(!store.is_recovery_pending());
        assert!(!store.info().is_available);
        assert_eq!(candidate.sgf, "(;C[recovery])");
        assert_eq!(candidate.revision, 9);
        assert_eq!(candidate.source_display_name, Some("board.sgf".to_owned()));
        assert!(store.has_recovery());
    }

    #[test]
    fn discarding_a_recovery_removes_it_and_releases_the_gate() {
        let mut store = AutosaveStore::default();
        store.replace_with(AutosaveCandidate {
            sgf: "(;C[recovery])".to_owned(),
            revision: 3,
            source_display_name: None,
        });
        store.set_recovery_pending(true);

        store.resolve_discard();

        assert!(!store.has_recovery());
        assert!(!store.is_recovery_pending());
        assert!(!store.info().is_available);
        assert_eq!(store.recovery_sgf(), None);
    }

    #[test]
    fn preserves_recovery_content_after_the_prompt_is_resolved() {
        let mut store = AutosaveStore::default();
        store.replace_with(AutosaveCandidate {
            sgf: "(;C[recovery])".to_owned(),
            revision: 4,
            source_display_name: None,
        });
        store.set_recovery_pending(false);

        assert!(!store.info().is_available);
        assert_eq!(store.recovery_sgf(), Some("(;C[recovery])".to_owned()));
    }

    #[test]
    fn reconstructs_a_store_from_persisted_recovery() {
        let persisted = super::PersistedAutosave {
            schema_version: super::AUTOSAVE_SCHEMA_VERSION,
            sgf: "(;C[recovered])".to_owned(),
            revision: 11,
            saved_at_unix_milliseconds: 12345,
            source_display_name: Some("archive.sgf".to_owned()),
        };

        let store = AutosaveStore::from_persisted(persisted.clone());

        assert!(store.is_recovery_pending());
        assert!(store.has_recovery());
        assert_eq!(store.recovery_sgf(), Some("(;C[recovered])".to_owned()));
        assert_eq!(store.info().revision, Some(11));
        assert_eq!(
            store.info().source_display_name,
            Some("archive.sgf".to_owned())
        );
        assert_eq!(store.persisted(), Some(&persisted));
    }
}
