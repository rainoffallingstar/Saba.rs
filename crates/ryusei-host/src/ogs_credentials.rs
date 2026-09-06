//! Encrypted OGS session-credential persistence.
//!
//! Only the OGS session JWT and the app's own REST session cookie are stored,
//! and only through an OS credential store (macOS Keychain, Windows Credential
//! Manager, or the Linux secret service). The OGS password is never persisted
//! and never reaches this module.

use serde_json::Value;

/// The keychain service name used for every OGS credential entry.
pub const OGS_KEYCHAIN_SERVICE: &str = "net.ryusei.ogs";

/// A persisted OGS session. The password is absent by construction.
///
/// Serialized as a single JSON blob under one keychain entry so a startup
/// restore touches exactly one credential instead of three.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct OgsCredentials {
    pub server_url: String,
    pub jwt_token: String,
    pub cookie_header: Option<String>,
    pub user: Option<Value>,
    pub created_at: Option<i64>,
}

/// Storage boundary for OGS session credentials. Implementations must refuse
/// to persist when an encrypted OS credential store is unavailable.
pub trait OgsCredentialStore: Send + Sync {
    fn is_available(&self) -> bool;
    fn load(&self) -> Option<OgsCredentials>;
    fn save(&self, credentials: &OgsCredentials) -> Result<(), String>;
    fn clear(&self);
}

/// The keychain account name holding the whole OGS session as one JSON blob.
/// Keeping everything under a single account means macOS shows at most one
/// Keychain authorization prompt on startup, instead of one per legacy
/// `jwt` / `cookie` / `meta` entry (three prompts).
const SESSION_ACCOUNT: &str = "session";
/// Accounts used by the pre-consolidation layout, kept only for one-time
/// migration so existing logins are not dropped.
const LEGACY_ACCOUNTS: &[&str] = &["jwt", "cookie", "meta"];

/// Production store backed by the `keyring` crate. The JWT, REST cookie and
/// non-secret metadata live in a single generic-password entry.
#[derive(Clone, Debug, Default)]
pub struct KeyringOgsCredentialStore;

impl KeyringOgsCredentialStore {
    pub fn new() -> Self {
        Self
    }

    fn entry(&self, account: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(OGS_KEYCHAIN_SERVICE, account).map_err(|error| error.to_string())
    }

    fn read_entry(&self, account: &str) -> Option<String> {
        self.entry(account)
            .ok()?
            .get_password()
            .ok()
            .filter(|value| !value.is_empty())
    }

    fn delete_entry(&self, account: &str) {
        if let Ok(entry) = self.entry(account) {
            let _ = entry.delete_credential();
        }
    }

    /// Loads the legacy three-entry layout, preserving the full session.
    fn load_legacy(&self) -> Option<OgsCredentials> {
        let jwt_token = self.read_entry("jwt")?;
        let cookie_header = self.read_entry("cookie");
        let meta = self
            .read_entry("meta")
            .and_then(|text| serde_json::from_str::<Value>(&text).ok());
        let server_url = meta
            .as_ref()
            .and_then(|value| value.get("server_url"))
            .and_then(Value::as_str)
            .unwrap_or("https://online-go.com")
            .to_owned();
        let user = meta.as_ref().and_then(|value| value.get("user")).cloned();
        let created_at = meta
            .as_ref()
            .and_then(|value| value.get("created_at"))
            .and_then(Value::as_i64);
        Some(OgsCredentials {
            server_url,
            jwt_token,
            cookie_header,
            user,
            created_at,
        })
    }
}

impl OgsCredentialStore for KeyringOgsCredentialStore {
    fn is_available(&self) -> bool {
        self.entry(SESSION_ACCOUNT).is_ok()
    }

    fn load(&self) -> Option<OgsCredentials> {
        // Prefer the consolidated single entry (one keychain prompt).
        if let Some(text) = self.read_entry(SESSION_ACCOUNT)
            && let Some(credentials) = serde_json::from_str::<OgsCredentials>(&text).ok()
        {
            return Some(credentials);
        }

        // One-time migration from the legacy three-entry layout. This is the
        // last time three prompts can appear; afterwards only one entry exists.
        let legacy = self.load_legacy()?;
        let _ = self.save(&legacy);
        for account in LEGACY_ACCOUNTS {
            self.delete_entry(account);
        }
        Some(legacy)
    }

    fn save(&self, credentials: &OgsCredentials) -> Result<(), String> {
        let text = serde_json::to_string(credentials).map_err(|error| error.to_string())?;
        self.entry(SESSION_ACCOUNT)?
            .set_password(&text)
            .map_err(|error| error.to_string())?;
        // Remove any stale legacy entries so they cannot prompt again later.
        for account in LEGACY_ACCOUNTS {
            self.delete_entry(account);
        }
        Ok(())
    }

    fn clear(&self) {
        self.delete_entry(SESSION_ACCOUNT);
        for account in LEGACY_ACCOUNTS {
            self.delete_entry(account);
        }
    }
}

/// In-memory store for hermetic tests; `available` lets tests exercise the
/// "no secure store → stay signed out" path.
#[derive(Debug, Default)]
pub struct MemoryOgsCredentialStore {
    inner: std::sync::Mutex<Option<OgsCredentials>>,
    pub available: bool,
}

impl MemoryOgsCredentialStore {
    pub fn available() -> Self {
        Self {
            inner: std::sync::Mutex::new(None),
            available: true,
        }
    }

    pub fn unavailable() -> Self {
        Self {
            inner: std::sync::Mutex::new(None),
            available: false,
        }
    }
}

impl OgsCredentialStore for MemoryOgsCredentialStore {
    fn is_available(&self) -> bool {
        self.available
    }

    fn load(&self) -> Option<OgsCredentials> {
        self.inner.lock().ok()?.clone()
    }

    fn save(&self, credentials: &OgsCredentials) -> Result<(), String> {
        if !self.available {
            return Err("secure credential storage is unavailable".to_owned());
        }
        *self.inner.lock().map_err(|error| error.to_string())? = Some(credentials.clone());
        Ok(())
    }

    fn clear(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            *inner = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_store_round_trips_and_clears() {
        let store = MemoryOgsCredentialStore::available();
        assert!(store.is_available());
        assert!(store.load().is_none());
        store
            .save(&OgsCredentials {
                server_url: "https://online-go.com".to_owned(),
                jwt_token: "jwt-secret".to_owned(),
                cookie_header: Some("sessionid=abc; csrftoken=def".to_owned()),
                user: Some(serde_json::json!({"id": 7, "username": "player"})),
                created_at: Some(1_700_000_000),
            })
            .expect("save succeeds");
        let loaded = store.load().expect("load succeeds");
        assert_eq!(loaded.jwt_token, "jwt-secret");
        assert!(
            loaded
                .cookie_header
                .as_deref()
                .unwrap()
                .contains("sessionid=abc")
        );
        store.clear();
        assert!(store.load().is_none());
    }

    #[test]
    fn unavailable_store_refuses_to_persist() {
        let store = MemoryOgsCredentialStore::unavailable();
        assert!(!store.is_available());
        let error = store
            .save(&OgsCredentials {
                server_url: "https://online-go.com".to_owned(),
                jwt_token: "jwt-secret".to_owned(),
                cookie_header: None,
                user: None,
                created_at: None,
            })
            .expect_err("unavailable store must refuse");
        assert!(error.contains("unavailable"));
        assert!(store.load().is_none());
    }

    #[test]
    fn credentials_json_round_trips_every_field() {
        let credentials = OgsCredentials {
            server_url: "https://online-go.com".to_owned(),
            jwt_token: "jwt-secret".to_owned(),
            cookie_header: Some("sessionid=abc; csrftoken=def".to_owned()),
            user: Some(serde_json::json!({"id": 7, "username": "player"})),
            created_at: Some(1_700_000_000),
        };
        let text = serde_json::to_string(&credentials).expect("serialize");
        let decoded: OgsCredentials = serde_json::from_str(&text).expect("deserialize");
        assert_eq!(decoded.server_url, credentials.server_url);
        assert_eq!(decoded.jwt_token, credentials.jwt_token);
        assert_eq!(decoded.cookie_header, credentials.cookie_header);
        assert_eq!(decoded.user, credentials.user);
        assert_eq!(decoded.created_at, credentials.created_at);
    }

    #[test]
    fn credentials_json_handles_absent_optionals() {
        let text = r#"{"server_url":"https://online-go.com","jwt_token":"t","cookie_header":null,"user":null,"created_at":null}"#;
        let decoded: OgsCredentials = serde_json::from_str(text).expect("deserialize");
        assert_eq!(decoded.jwt_token, "t");
        assert!(decoded.cookie_header.is_none());
        assert!(decoded.user.is_none());
        assert!(decoded.created_at.is_none());
    }
}
