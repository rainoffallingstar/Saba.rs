//! Theme token types and the derived shell UI palette.
//!
//! The canonical theme definitions live in `sabaki-host::theme_workflow`
//! (design §8.2): the host validates theme packages and tokens before any
//! render layer applies them. This module re-exports those types and derives
//! readable light/dark shell colors from the validated background color.

use sabaki_host::parse_hex_color;
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
    if let Some(shell) = &theme.shell {
        // Host validation makes these infallible before any render work starts.
        let color = |value: &str| {
            parse_hex_color(value)
                .expect("validated shell color")
                .rgb_u32()
        };
        return UiPalette {
            text: color(&shell.text),
            muted: color(&shell.muted),
            subtle: color(&shell.subtle),
            panel: color(&shell.panel),
            input: color(&shell.input),
            border: color(&shell.border),
            button: color(&shell.button),
            button_active: color(&shell.button_active),
            accent: color(&shell.accent),
            danger: color(&shell.danger),
            danger_text: color(&shell.danger_text),
            success: color(&shell.success),
            track: color(&shell.track),
        };
    }

    // Schema v1 packages only define board tokens. Preserve their compatibility
    // by deriving the same readable shell palette from the background.
    let background = theme.background_color();
    let is_dark = relative_luminance(background) < 0.45;
    if is_dark {
        UiPalette {
            text: 0xf5f5f7,
            muted: 0x98989d,
            subtle: 0x636366,
            panel: 0x252528,
            input: 0x1e1e20,
            border: 0x38383a,
            button: 0x2c2c2e,
            button_active: 0x3a3a3c,
            accent: 0x0a84ff,
            danger: 0x3d1c1c,
            danger_text: 0xff453a,
            success: 0x30d158,
            track: 0x38383a,
        }
    } else {
        UiPalette {
            text: 0x1d1d1f,
            muted: 0x6e6e73,
            subtle: 0x86868b,
            panel: 0xffffff,
            input: 0xf0f0f3,
            border: 0xe5e5ea,
            button: 0xebebef,
            button_active: 0xdedee4,
            accent: 0x007aff,
            danger: 0xfee2e2,
            danger_text: 0xff3b30,
            success: 0x34c759,
            track: 0xe5e5ea,
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
        assert_eq!(palette.text, 0x1d1d1f);
        assert_eq!(palette.panel, 0xffffff);
    }

    #[test]
    fn schema_v2_shell_tokens_override_derived_palette() {
        let theme = ThemeTokens::parse(
            r##"{"schemaVersion":2,"boardWood":"#d9a866","boardLine":"#4a2f12","starPoint":"#3a2410","stoneBlack":"#1a1a1a","stoneWhite":"#ffffff","background":"#f5f0e8","shell":{"text":"#112233","muted":"#223344","subtle":"#334455","panel":"#445566","input":"#556677","border":"#667788","button":"#778899","buttonActive":"#8899aa","accent":"#99aabb","danger":"#aabbcc","dangerText":"#bbccdd","success":"#ccddee","track":"#ddeeff"}}"##,
        )
        .unwrap();
        let palette = ui_palette(&theme);
        assert_eq!(palette.text, 0x112233);
        assert_eq!(palette.panel, 0x445566);
        assert_eq!(palette.track, 0xddeeff);
    }

    #[test]
    fn dark_backgrounds_derive_a_dark_shell_palette() {
        let theme = ThemeTokens::parse(
            r##"{"schemaVersion":1,"boardWood":"#2e2a24","boardLine":"#8a7a5a","starPoint":"#a89264","stoneBlack":"#0f0f0f","stoneWhite":"#ececec","background":"#1b1b1b"}"##,
        )
        .unwrap();
        let palette = ui_palette(&theme);
        assert_eq!(palette.text, 0xf5f5f7);
        assert_eq!(palette.panel, 0x252528);
    }
}
