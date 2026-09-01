//! Theme token types and the derived shell UI palette.
//!
//! The canonical theme definitions live in `ryusei-host::theme_workflow`
//! (design §8.2): the host validates theme packages and tokens before any
//! render layer applies them. This module re-exports those types and derives
//! readable light/dark shell colors from the validated background color.
//!
//! Two layers of tokens exist:
//! - [`UiPalette`] — semantic *colors* that follow the active theme. Values
//!   that a theme package omits fall back to the design-prototype defaults
//!   (`sabaki-web-prototype.html` `:root` tokens).
//! - [`type_scale`], [`radius`], [`motion`] — theme-invariant typography,
//!   corner-radius and motion constants frozen from the design prototype so
//!   UI code stops scattering raw literals.

use ryusei_host::parse_hex_color;
pub use ryusei_host::{ThemeColor, ThemeTokens};

/// Semantic shell colors that follow the active theme.
///
/// The core thirteen fields mirror `ShellThemeTokens`; the extended fields are
/// either taken from a theme package (when it provides them) or derived from
/// the core colors using design-prototype-aligned defaults. All values are
/// `0xRRGGBB`.
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
    // ── Extended semantic tokens (design prototype alignment) ─────────────
    /// Secondary foreground (design `--fg-2` / dark `#d2d2d7`).
    pub text_secondary: u32,
    /// Softer hairline divider (design `--border-soft`).
    pub border_soft: u32,
    /// Accent hover lift (design `--accent-hover` `#0077ed`).
    pub accent_hover: u32,
    /// Accent pressed state (design `--accent-active` `#0066cc`).
    pub accent_active: u32,
    /// Warning / inaccuracy amber (design `--warn` `#eab308`).
    pub warn: u32,
    /// Mistake orange on the move-quality ramp (between warn and danger).
    pub mistake: u32,
    /// Raised surface for cards/menus/toasts above `panel`.
    pub elevated: u32,
    /// Graph axis / log-command / crosshair info blue.
    pub info: u32,
}

impl UiPalette {
    /// Whether this palette is derived for a dark background. Used to pick
    /// sensible translucent overlays (light strokes read differently on dark).
    pub fn is_dark(&self) -> bool {
        relative_luminance_u32(self.panel) < 0.45
    }
}

// Design-prototype defaults (`sabaki-web-prototype.html` `:root`), used when a
// theme package does not pin an extended token. Light values come from `:root`
// (L12-27), dark values from `[data-theme="dark"]` (L56-68).
mod proto {
    pub const ACCENT: u32 = 0x0071e3;
    pub const ACCENT_HOVER: u32 = 0x0077ed;
    pub const ACCENT_ACTIVE: u32 = 0x0066cc;
    pub const SUCCESS: u32 = 0x16a34a;
    pub const WARN: u32 = 0xdd9d06; // amber that stays legible on light surfaces
    pub const MISTAKE: u32 = 0xf97316;
    pub const INFO: u32 = 0x0ea5e9;

    pub mod light {
        pub const TEXT_SECONDARY: u32 = 0x424245;
        pub const BORDER_SOFT: u32 = 0xe8e8ed;
        pub const ELEVATED: u32 = 0xffffff;
    }
    pub mod dark {
        pub const TEXT_SECONDARY: u32 = 0xd2d2d7;
        pub const BORDER_SOFT: u32 = 0x2a2a2f;
        pub const ELEVATED: u32 = 0x2c2c2e;
    }
}

pub fn ui_palette(theme: &ThemeTokens) -> UiPalette {
    if let Some(shell) = &theme.shell {
        // Host validation makes these infallible before any render work starts.
        let color = |value: &str| {
            parse_hex_color(value)
                .expect("validated shell color")
                .rgb_u32()
        };
        let opt =
            |value: &Option<String>, fallback: u32| value.as_deref().map(color).unwrap_or(fallback);
        let accent = color(&shell.accent);
        let success = color(&shell.success);
        let danger_text = color(&shell.danger_text);
        let dark = relative_luminance_u32(color(&shell.panel)) < 0.45;
        let (text_secondary, border_soft, elevated) = if dark {
            (
                proto::dark::TEXT_SECONDARY,
                proto::dark::BORDER_SOFT,
                proto::dark::ELEVATED,
            )
        } else {
            (
                proto::light::TEXT_SECONDARY,
                proto::light::BORDER_SOFT,
                proto::light::ELEVATED,
            )
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
            accent,
            danger: color(&shell.danger),
            danger_text,
            success,
            track: color(&shell.track),
            text_secondary: opt(&shell.text_secondary, text_secondary),
            border_soft: opt(&shell.border_soft, border_soft),
            accent_hover: opt(&shell.accent_hover, proto::ACCENT_HOVER),
            accent_active: opt(&shell.accent_active, proto::ACCENT_ACTIVE),
            warn: opt(&shell.warn, proto::WARN),
            mistake: opt(&shell.mistake, proto::MISTAKE),
            elevated: opt(&shell.elevated, elevated),
            info: opt(&shell.info, proto::INFO),
        };
    }

    // Schema v1 packages only define board tokens. Preserve their compatibility
    // by deriving a readable shell palette from the background luminance, then
    // layering the design-prototype extended defaults on top.
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
            text_secondary: proto::dark::TEXT_SECONDARY,
            border_soft: proto::dark::BORDER_SOFT,
            accent_hover: proto::ACCENT_HOVER,
            accent_active: proto::ACCENT_ACTIVE,
            warn: proto::WARN,
            mistake: proto::MISTAKE,
            elevated: proto::dark::ELEVATED,
            info: proto::INFO,
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
            accent: proto::ACCENT,
            danger: 0xfee2e2,
            danger_text: 0xdc2626,
            success: proto::SUCCESS,
            track: 0xe5e5ea,
            text_secondary: proto::light::TEXT_SECONDARY,
            border_soft: proto::light::BORDER_SOFT,
            accent_hover: proto::ACCENT_HOVER,
            accent_active: proto::ACCENT_ACTIVE,
            warn: proto::WARN,
            mistake: proto::MISTAKE,
            elevated: proto::light::ELEVATED,
            info: proto::INFO,
        }
    }
}

/// Typography scale frozen from the design prototype (`--text-xs` … `--text-2xl`,
/// `sabaki-web-prototype.html` L33-38). Values are logical pixels.
#[allow(dead_code)] // design-token contract; consumed incrementally
pub mod type_scale {
    pub const XS: f32 = 11.0;
    pub const SM: f32 = 13.0;
    pub const BASE: f32 = 14.0;
    pub const LG: f32 = 17.0;
    pub const XL: f32 = 21.0;
    pub const XXL: f32 = 28.0;
}

/// Corner radii frozen from the design prototype (`--radius-sm/md/lg/pill`,
/// `sabaki-web-prototype.html` L40-43). Values are logical pixels.
#[allow(dead_code)] // design-token contract; consumed incrementally
pub mod radius {
    pub const SM: f32 = 6.0;
    pub const MD: f32 = 10.0;
    pub const LG: f32 = 16.0;
    pub const PILL: f32 = 980.0;
}

/// Motion durations frozen from the design prototype (`--motion-fast/base`,
/// `sabaki-web-prototype.html` L51-53). Values are milliseconds.
#[allow(dead_code)] // design-token contract; consumed incrementally
pub mod motion {
    pub const FAST_MS: u64 = 140;
    pub const BASE_MS: u64 = 220;
}

/// Board container corner radius, from the design prototype board container
/// (`border-radius: 14px`, `sabaki-web-prototype.html` L708). Sits between
/// `radius::MD` and `radius::LG` and is specific to the goban frame.
pub const BOARD_RADIUS: f32 = 14.0;

/// How the goban container should be framed (gradient, ring/border, shadow
/// strength). Derived from the active theme's board wood so the three built-in
/// skins (榧木 kaya / 白墨 studio / 曜黑 midnight) match the design prototype
/// `.board-container-*` rules (`sabaki-web-prototype.html` L235-247).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoardSkin {
    /// Linear-gradient stand-in for the design's radial kaya gradient. `Some`
    /// for the kaya skin, `None` for flat studio/midnight surfaces.
    pub gradient: Option<(u32, u32)>,
    /// Container border / ring color. `None` means no visible border (studio /
    /// midnight use an inset ring that the flat border already approximates).
    pub border: Option<u32>,
    /// Whether to cast a drop shadow under the board.
    pub shadow: bool,
}

impl BoardSkin {
    /// Picks the skin from the board-wood luminance: light warm wood → kaya
    /// gradient + border; very dark wood → midnight ring; anything else → flat
    /// studio surface.
    pub fn from_theme(theme: &ThemeTokens) -> Self {
        let wood = theme.board_wood_color();
        let lum = relative_luminance(wood);
        let is_dark = lum < 0.18;
        let is_light_wood = !is_dark && lum > 0.30;
        if is_dark {
            // midnight: inset ring only, strong shadow.
            BoardSkin {
                gradient: None,
                border: None,
                shadow: true,
            }
        } else if is_light_wood {
            // kaya: warm gradient approximating the radial highlight, plus a
            // thin wood border and a soft shadow.
            BoardSkin {
                gradient: Some((0xf1dcab, 0xd8b46e)),
                border: Some(0xc9a45e),
                shadow: true,
            }
        } else {
            // studio: flat surface, hairline ring, gentle shadow.
            BoardSkin {
                gradient: None,
                border: Some(0xd2d2d7),
                shadow: true,
            }
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

fn relative_luminance_u32(color: u32) -> f32 {
    relative_luminance(ThemeColor {
        red: ((color >> 16) & 0xff) as u8,
        green: ((color >> 8) & 0xff) as u8,
        blue: (color & 0xff) as u8,
    })
}

#[cfg(test)]
mod tests {
    use super::{UiPalette, ui_palette};
    use ryusei_host::ThemeTokens;

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

    #[test]
    fn extended_tokens_fall_back_to_design_prototype_defaults() {
        // A schema v2 package without the extended tokens still gets
        // prototype-aligned accent / warn / elevated values.
        let theme = ThemeTokens::parse(
            r##"{"schemaVersion":2,"boardWood":"#d9a866","boardLine":"#4a2f12","starPoint":"#3a2410","stoneBlack":"#1a1a1a","stoneWhite":"#ffffff","background":"#f5f0e8","shell":{"text":"#1d1d1f","muted":"#6e6e73","subtle":"#86868b","panel":"#ffffff","input":"#f0f0f3","border":"#e5e5ea","button":"#ebebef","buttonActive":"#dedee4","accent":"#007aff","danger":"#fee2e2","dangerText":"#dc2626","success":"#16a34a","track":"#e5e5ea"}}"##,
        )
        .unwrap();
        let palette = ui_palette(&theme);
        assert_eq!(palette.warn, 0xdd9d06);
        assert_eq!(palette.text_secondary, 0x424245);
        assert_eq!(palette.border_soft, 0xe8e8ed);
        assert_eq!(palette.elevated, 0xffffff);
        assert_eq!(palette.accent_hover, 0x0077ed);
        assert!(!palette.is_dark());
    }

    #[test]
    fn extended_tokens_can_be_pinned_by_a_theme_package() {
        let theme = ThemeTokens::parse(
            r##"{"schemaVersion":2,"boardWood":"#d9a866","boardLine":"#4a2f12","starPoint":"#3a2410","stoneBlack":"#1a1a1a","stoneWhite":"#ffffff","background":"#1b1b1b","shell":{"text":"#f5f5f7","muted":"#98989d","subtle":"#636366","panel":"#252528","input":"#1e1e20","border":"#38383a","button":"#2c2c2e","buttonActive":"#3a3a3c","accent":"#0a84ff","danger":"#3d1c1c","dangerText":"#ff453a","success":"#30d158","track":"#38383a","textSecondary":"#d2d2d7","warn":"#ffd60a","elevated":"#2c2c2e"}}"##,
        )
        .unwrap();
        let palette = ui_palette(&theme);
        assert_eq!(palette.text_secondary, 0xd2d2d7);
        assert_eq!(palette.warn, 0xffd60a);
        assert_eq!(palette.elevated, 0x2c2c2e);
        assert!(palette.is_dark());
    }

    #[test]
    fn palette_is_dark_detects_dark_panels() {
        let mut palette = ui_palette(&ThemeTokens::default());
        assert!(!palette.is_dark());
        palette.panel = 0x252528;
        assert!(UiPalette { ..palette }.is_dark());
    }
}
