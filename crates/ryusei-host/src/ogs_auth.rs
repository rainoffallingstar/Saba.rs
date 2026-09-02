//! OGS account-authentication state.
//!
//! The native client authenticates through its own REST login flow. Browser
//! login is intentionally not part of the native account workflow.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OgsAuthState {
    SignedOut,
    Authenticated,
}

impl OgsAuthState {
    pub const fn label(self) -> &'static str {
        match self {
            Self::SignedOut => "未登录",
            Self::Authenticated => "已登录",
        }
    }

    pub const fn can_submit_moves(self) -> bool {
        matches!(self, Self::Authenticated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_authenticated_state_can_submit_moves() {
        assert!(!OgsAuthState::SignedOut.can_submit_moves());
        assert!(OgsAuthState::Authenticated.can_submit_moves());
    }
}
