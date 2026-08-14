//! Theme token types shared with the render layer.
//!
//! The canonical definitions live in `sabaki-host::theme_workflow` (design
//! §8.2): the host validates theme packages and tokens before any render
//! layer applies them. This module re-exports those types so the GPUI
//! render code keeps importing from `crate::theme`.

pub use sabaki_host::ThemeTokens;
