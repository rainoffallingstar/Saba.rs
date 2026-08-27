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
#[derive(Clone, Debug)]
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

/// Production store backed by the `keyring` crate. JWT and cookie are separate
/// entries; non-secret metadata (server URL, user summary) is a third entry.
#[derive(Clone, Debug, Default)]
pub struct KeyringOgsCredentialStore;

impl KeyringOgsCredentialStore {
    pub fn new() -> Self {
        Self
    }

    fn entry(&self, account: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(OGS_KEYCHAIN_SERVICE, account).map_err(|error| error.to_string())
    }
}

impl OgsCredentialStore for KeyringOgsCredentialStore {
    fn is_available(&self) -> bool {
        self.entry("jwt").is_ok()
    }

    fn load(&self) -> Option<OgsCredentials> {
        let jwt_token = self.entry("jwt").ok()?.get_password().ok()?;
        if jwt_token.is_empty() {
            return None;
        }
        let cookie_header = self
            .entry("cookie")
            .ok()
            .and_then(|entry| entry.get_password().ok())
            .filter(|value| !value.is_empty());
        let meta = self
            .entry("meta")
            .ok()
            .and_then(|entry| entry.get_password().ok())
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

    fn save(&self, credentials: &OgsCredentials) -> Result<(), String> {
        self.entry("jwt")?
            .set_password(&credentials.jwt_token)
            .map_err(|error| error.to_string())?;
        match &credentials.cookie_header {
            Some(cookie) if !cookie.is_empty() => {
                self.entry("cookie")?
                    .set_password(cookie)
                    .map_err(|error| error.to_string())?;
            }
            _ => {
                if let Ok(entry) = self.entry("cookie") {
                    let _ = entry.delete_credential();
                }
            }
        }
        let meta = serde_json::json!({
            "server_url": credentials.server_url,
            "user": credentials.user,
            "created_at": credentials.created_at,
        });
        self.entry("meta")?
            .set_password(&meta.to_string())
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    fn clear(&self) {
        for account in ["jwt", "cookie", "meta"] {
            if let Ok(entry) = self.entry(account) {
                let _ = entry.delete_credential();
            }
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
}
