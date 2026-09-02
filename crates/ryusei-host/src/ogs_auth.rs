//! Safe OGS account-login boundary.
//!
//! The native client does not collect an OGS password and cannot reuse the
//! browser's cookie jar. Until an OGS OAuth client registration and an OS
//! credential-store adapter are configured, this module only opens the
//! official login page and reports that the app itself is not authenticated.

use std::process::Command;

pub const OGS_LOGIN_URL: &str = "https://online-go.com/sign-in";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OgsAuthState {
    SignedOut,
    BrowserLoginOnly,
    Authenticated,
}

impl OgsAuthState {
    pub const fn label(self) -> &'static str {
        match self {
            Self::SignedOut => "未登录",
            Self::BrowserLoginOnly => "已打开浏览器；Ryusei 未登录",
            Self::Authenticated => "已登录",
        }
    }

    pub const fn can_submit_moves(self) -> bool {
        matches!(self, Self::Authenticated)
    }
}

/// Opens the official OGS login page without importing or persisting browser
/// cookies. Success means only that the system browser was launched.
pub fn open_ogs_login_page() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let result = Command::new("open").arg(OGS_LOGIN_URL).status();
    #[cfg(target_os = "windows")]
    let result = Command::new("cmd")
        .args(["/C", "start", "", OGS_LOGIN_URL])
        .status();
    #[cfg(all(unix, not(target_os = "macos")))]
    let result = Command::new("xdg-open").arg(OGS_LOGIN_URL).status();
    #[cfg(not(any(target_os = "macos", target_os = "windows", unix)))]
    let result: Result<std::process::ExitStatus, std::io::Error> =
        Err(std::io::Error::other("unsupported desktop platform"));

    match result {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("system browser exited with status {status}")),
        Err(error) => Err(format!("could not open system browser: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_login_does_not_claim_an_app_session() {
        assert!(!OgsAuthState::SignedOut.can_submit_moves());
        assert!(!OgsAuthState::BrowserLoginOnly.can_submit_moves());
        assert!(OgsAuthState::Authenticated.can_submit_moves());
        assert_eq!(OGS_LOGIN_URL, "https://online-go.com/sign-in");
    }
}
