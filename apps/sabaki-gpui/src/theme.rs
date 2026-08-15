//! Theme token types and the derived shell UI palette.
//!
//! The canonical theme definitions live in `sabaki-host::theme_workflow`
//! (design §8.2): the host validates theme packages and tokens before any
//! render layer applies them. This module re-exports those types and derives
//! readable light/dark shell colors from the validated background color.

pub use sabaki_host::{ThemeColor, ThemeTokens};

/// Shell colors derived from the active theme background. Theme packages only
/// provide board colors today, so the surrounding UI picks a light or dark
/// palette from the background luminance instead of requiring token schema v2.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UiPalette {
    pub text: u32,
    pub muted: u32,
    pub subtle: u32,
    pub panel: u32,
    pub input: u32,
    pub border: u32,
    pub button: u32,
    pub button_active: u32,
    pub accent: u32,
    pub danger: u32,
    pub danger_text: u32,
    pub success: u32,
    pub track: u32,
}

pub fn ui_palette(theme: &ThemeTokens) -> UiPalette {
    let background = theme.background_color();
    let is_dark = relative_luminance(background) < 0.45;
    if is_dark {
        UiPalette {
            text: 0xf0ebe2,
            muted: 0xbfb8ac,
            subtle: 0x8f887e,
            panel: 0x26231f,
            input: 0x1e1c19,
            border: 0x4a443b,
            button: 0x3d362c,
            button_active: 0x4f463a,
            accent: 0xb39a6b,
            danger: 0x4a2626,
            danger_text: 0xff9b8f,
            success: 0x7ecb89,
            track: 0x4a4a4a,
        }
    } else {
        UiPalette {
            text: 0x222222,
            muted: 0x444444,
            subtle: 0x999999,
            panel: 0xffffff,
            input: 0xffffff,
            border: 0xd8cfc0,
            button: 0xf7ecd8,
            button_active: 0xe8e0d4,
            accent: 0x8a6d3b,
            danger: 0xf5d6d6,
            danger_text: 0xc0392b,
            success: 0x2e6b34,
            track: 0xdddddd,
        }
    }
}

fn relative_luminance(color: ThemeColor) -> f32 {
    let channel = |value: u8| {
        let normalized = f32::from(value) / 255.0;
        if normalized <= 0.04045 {
            normalized / 12.92
        } else {
            ((normalized + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(color.red) + 0.7152 * channel(color.green) + 0.0722 * channel(color.blue)
}

#[cfg(test)]
mod tests {
    use super::{ThemeTokens, ui_palette};

    #[test]
    fn classic_theme_uses_a_light_shell_palette() {
        let palette = ui_palette(&ThemeTokens::default());
        assert_eq!(palette.text, 0x222222);
        assert_eq!(palette.panel, 0xffffff);
    }

    #[test]
    fn dark_backgrounds_derive_a_dark_shell_palette() {
        let theme = ThemeTokens::parse(
            r##"{"schemaVersion":1,"boardWood":"#2e2a24","boardLine":"#8a7a5a","starPoint":"#a89264","stoneBlack":"#0f0f0f","stoneWhite":"#ececec","background":"#1b1b1b"}"##,
        )
        .unwrap();
        let palette = ui_palette(&theme);
        assert_eq!(palette.text, 0xf0ebe2);
        assert_eq!(palette.panel, 0x26231f);
    }
}
