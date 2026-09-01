//! Lucide monoline SVG icons, embedded from the design prototype.
//!
//! The design prototype (`sabaki-web-prototype.html` L335-411) ships a set of
//! Lucide-style monoline glyphs (24×24 viewBox, `stroke-width: 1.8`, round
//! caps/joins, `currentColor` stroke). We embed the same glyphs so the native
//! shell matches the design instead of substituting CJK characters or emoji.
//!
//! Glyphs are recolored at render time by GPUI's monochrome SVG pipeline from
//! the element's text color, so a single source works across themes.

use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString, Styled as _};

/// Wraps inner SVG content in a 24×24 monoline document using `currentColor`.
macro_rules! svg_doc {
    ($inner:expr) => {
        concat!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">"#,
            $inner,
            "</svg>"
        )
    };
}

const BOOK_OPEN: &str = svg_doc!(
    r#"<path d="M2 3h6a4 4 0 0 1 4 4v14a3 3 0 0 0-3-3H2z"/><path d="M22 3h-6a4 4 0 0 0-4 4v14a3 3 0 0 1 3-3h7z"/>"#
);
const SWORDS: &str = svg_doc!(
    r#"<polyline points="14.5 17.5 3 6 3 3 6 3 17.5 14.5"/><line x1="13" x2="19" y1="19" y2="13"/><line x1="16" x2="20" y1="16" y2="20"/><line x1="19" x2="21" y1="21" y2="19"/>"#
);
const RADIO: &str = svg_doc!(
    r#"<circle cx="12" cy="12" r="2"/><path d="M16.24 7.76a6 6 0 0 1 0 8.49m-8.48-.01a6 6 0 0 1 0-8.49m11.31-2.82a10 10 0 0 1 0 14.14m-14.14 0a10 10 0 0 1 0-14.14"/>"#
);
const VOLUME_2: &str = svg_doc!(
    r#"<polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5"/><path d="M15.54 8.46a5 5 0 0 1 0 7.07"/><path d="M19.07 4.93a5 5 0 0 1 0 7.07 7.07"/>"#
);
const VOLUME_X: &str = svg_doc!(
    r#"<polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5"/><line x1="22" x2="16" y1="9" y2="15"/><line x1="16" x2="22" y1="15" y2="9"/>"#
);
const SETTINGS: &str = svg_doc!(
    r#"<path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 0-2-2v-.18a2 2 0 0 0-1-1.73l-.43-.25a2 2 0 0 1-2 0l.15-.08a2 2 0 0 0 2.73-.73l.22-.38a2 2 0 0 0-.73-2.73l-.15-.1a2 2 0 0 0-1-1.72v-.51a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"/><circle cx="12" cy="12" r="3"/>"#
);
const PLAY: &str = svg_doc!(r#"<polygon points="6 3 20 12 6 21 6 3"/>"#);
const PAUSE: &str = svg_doc!(
    r#"<rect width="4" height="16" x="6" y="4"/><rect width="4" height="16" x="14" y="4"/>"#
);
const CHEVRON_LEFT: &str = svg_doc!(r#"<path d="m15 18-6-6 6-6"/>"#);
const CHEVRON_RIGHT: &str = svg_doc!(r#"<path d="m9 18 6 6-6-6"/>"#);
const CHEVRONS_LEFT: &str = svg_doc!(r#"<path d="m11 17-5-5 5-5"/><path d="m18 17-5-5 5-5"/>"#);
const CHEVRONS_RIGHT: &str = svg_doc!(r#"<path d="m6 17 5 5 5-5"/><path d="m13 17 5 5-5 5"/>"#);
const TRENDING_UP: &str = svg_doc!(
    r#"<polyline points="22 7 13.5 15.5 8.5 10.5 2 17"/><polyline points="16 7 22 7 22 13"/>"#
);
const GIT_BRANCH: &str = svg_doc!(
    r#"<line x1="6" x2="6" y1="3" y2="15"/><circle cx="18" cy="6" r="3"/><circle cx="6" cy="18" r="3"/><path d="M18 9a9 9 0 0 1-9 9"/>"#
);
const TERMINAL: &str =
    svg_doc!(r#"<polyline points="4 17 10 11 4 5"/><line x1="12" x2="20" y1="19" y2="19"/>"#);
const CPU: &str = svg_doc!(
    r#"<rect width="16" height="16" x="4" y="4" rx="2"/><rect width="6" height="6" x="9" y="9" rx="1"/><path d="M15 2v2"/><path d="M15 20v2"/><path d="M2 15h2"/><path d="M2 9h2"/><path d="M20 15h2"/><path d="M20 9h2"/><path d="M9 2v2"/><path d="M9 20v2"/>"#
);
const SPARKLES: &str = svg_doc!(
    r#"<path d="m12 3-1.912 5.813a2 2 0 0 1-1.275 1.275L3 12l5.813 1.912a2 2 0 0 1 1.275 1.275L12 21l1.912-5.813a2 2 0 0 1 1.275-1.275L21 12l-5.813-1.912a2 2 0 0 1-1.275-1.275L12 3Z"/>"#
);
const SHARE: &str = svg_doc!(
    r#"<circle cx="18" cy="5" r="3"/><circle cx="6" cy="12" r="3"/><circle cx="18" cy="19" r="3"/><line x1="8.59" x2="15.42" y1="13.51" y2="17.49"/><line x1="15.41" x2="8.59" y1="6.51" y2="10.49"/>"#
);
const INFO: &str =
    svg_doc!(r#"<circle cx="12" cy="12" r="10"/><path d="M12 16v-4"/><path d="M12 8h.01"/>"#);
const CHECK: &str = svg_doc!(r#"<polyline points="20 6 9 17 4 12"/>"#);
const REFRESH_CW: &str = svg_doc!(
    r#"<path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8"/><path d="M21 3v5h-5"/><path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16"/><path d="M16 16h5v5"/>"#
);
const UPLOAD: &str = svg_doc!(
    r#"<path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="17 8 12 3 7 8"/><line x1="12" x2="15" y1="3" y2="15"/>"#
);
const PANEL_LEFT: &str =
    svg_doc!(r#"<rect width="18" height="18" x="3" y="3" rx="2"/><path d="M9 3v18"/>"#);
const PANEL_RIGHT: &str =
    svg_doc!(r#"<rect width="18" height="18" x="3" y="3" rx="2"/><path d="M15 3v18"/>"#);
const PANEL_BOTTOM: &str =
    svg_doc!(r#"<rect width="18" height="18" x="3" y="3" rx="2"/><path d="M3 15h18"/>"#);
const ALERT_TRIANGLE: &str = svg_doc!(
    r#"<path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3Z"/><path d="M12 9v4"/><path d="M12 17h.01"/>"#
);
const LIGHTBULB: &str = svg_doc!(
    r#"<path d="M15 14c.2-1 .7-1.7 1.5-2.5 1-.9 1.5-2.2 1.5-3.5A6 6 0 0 0 6 8c0 1 .2 2.2 1.5 3.5.7.7 1.3 1.5 1.5 2.5"/><path d="M9 18h6"/><path d="M10 22h4"/>"#
);
const LIBRARY: &str =
    svg_doc!(r#"<path d="m16 6 4 14"/><path d="M12 6v14"/><path d="M8 8v12"/><path d="M4 4v16"/>"#);
const FILM: &str = svg_doc!(
    r#"<rect width="18" height="18" x="3" y="3" rx="2"/><path d="M7 3v18"/><path d="M3 7.5h4"/><path d="M3 12h18"/><path d="M3 16.5h4"/><path d="M17 3v18"/><path d="M17 7.5h4"/><path d="M17 16.5h4"/>"#
);
const GLOBE: &str = svg_doc!(
    r#"<circle cx="12" cy="12" r="10"/><path d="M12 2a14.5 14.5 0 0 0 0 20 14.5 14.5 0 0 0 0-20"/><path d="M2 12h20"/>"#
);
const DICE: &str = svg_doc!(
    r#"<rect width="18" height="18" x="3" y="3" rx="2" ry="2"/><path d="M16 8h.01"/><path d="M8 8h.01"/><path d="M8 16h.01"/><path d="M16 16h.01"/><path d="M12 12h.01"/>"#
);
const PUZZLE: &str = svg_doc!(
    r#"<path d="M19.439 7.85c-.049.322.059.648.289.878l1.568 1.568c.47.47.706 1.087.706 1.704s-.235 1.233-.706 1.704l-1.611 1.611a.98.98 0 0 1-.837.276c-.47-.07-.802-.48-.968-.925a2.501 2.501 0 1 0-3.214 3.214c.446.166.855.497.925.968a.979.979 0 0 1-.276.837l-1.61 1.61a2.404 2.404 0 0 1-1.705.707 2.402 2.402 0 0 1-1.704-.706l-1.568-1.568a1.026 1.026 0 0 0-.877-.29c-.493.074-.84.504-1.02.968a2.5 2.5 0 1 1-3.237-3.237c.464-.18.894-.527.967-1.02a1.026 1.026 0 0 0-.289-.877l-1.568-1.568A2.402 2.402 0 0 1 1.998 12c0-.617.236-1.234.706-1.704L4.23 8.77c.24-.24.581-.353.917-.303.515.077.877.528 1.073 1.01a2.5 2.5 0 1 0 3.259-3.259c-.482-.196-.933-.558-1.01-1.073-.05-.336.062-.676.303-.917l1.525-1.525A2.402 2.402 0 0 1 12 1.998c.617 0 1.234.236 1.704.706l1.568 1.568c.23.23.556.338.877.29.493-.074.84-.504 1.02-.968a2.5 2.5 0 1 1 3.237 3.237c-.464.18-.894.527-.967 1.02Z"/>"#
);
const GRID: &str = svg_doc!(
    r#"<rect width="18" height="18" x="3" y="3" rx="2"/><path d="M3 9h18"/><path d="M3 15h18"/><path d="M9 3v18"/><path d="M15 3v18"/>"#
);
const MESSAGE: &str =
    svg_doc!(r#"<path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/>"#);
const BAR_CHART: &str = svg_doc!(
    r#"<line x1="12" x2="12" y1="20" y2="10"/><line x1="18" x2="18" y1="20" y2="4"/><line x1="6" x2="6" y1="20" y2="16"/>"#
);
const PLUG: &str = svg_doc!(
    r#"<path d="M12 22v-5"/><path d="M9 8V2"/><path d="M15 8V2"/><path d="M18 8v5a4 4 0 0 1-4 4h-4a4 4 0 0 1-4-4V8Z"/>"#
);
const STOP: &str = svg_doc!(r#"<rect width="14" height="14" x="5" y="5" rx="2"/>"#);
const FLAG: &str = svg_doc!(
    r#"<path d="M4 15s1-1 4-1 5 2 8 2 4-1 4-1V3s-1 1-4 1-5-2-8-2-4 1-4 1z"/><line x1="4" x2="4" y1="22" y2="15"/>"#
);
const EYE: &str = svg_doc!(
    r#"<path d="M2 12s3-7 10-7 10 7 10 7-3 7-10 7-10-7-10-7Z"/><circle cx="12" cy="12" r="3"/>"#
);
const CLOSE: &str = svg_doc!(r#"<path d="M18 6 6 18"/><path d="m6 6 12 12"/>"#);
const MAGNIFY: &str = svg_doc!(r#"<circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/>"#);
const SAVE: &str = svg_doc!(
    r#"<path d="M19 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11l5 5v11a2 2 0 0 1-2 2z"/><polyline points="17 21 17 13 7 13 7 21"/><polyline points="7 3 7 8 15 8"/>"#
);
const IMAGE: &str = svg_doc!(
    r#"<rect width="18" height="18" x="3" y="3" rx="2" ry="2"/><circle cx="9" cy="9" r="2"/><path d="m21 15-3.086-3.086a2 2 0 0 0-2.828 0L6 21"/>"#
);
const TARGET: &str = svg_doc!(
    r#"<circle cx="12" cy="12" r="10"/><circle cx="12" cy="12" r="6"/><circle cx="12" cy="12" r="2"/>"#
);
const PIN: &str = svg_doc!(
    r#"<path d="M12 17v5"/><path d="M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V6h1a2 2 0 0 0 0-4H8a2 2 0 0 0 0 4h1z"/>"#
);
const MENU: &str = svg_doc!(
    r#"<line x1="4" x2="20" y1="6" y2="6"/><line x1="4" x2="20" y1="12" y2="12"/><line x1="4" x2="20" y1="18" y2="18"/>"#
);

/// Embedded icon bytes keyed by their `icons/<name>.svg` asset path.
fn lookup(path: &str) -> Option<&'static str> {
    Some(match path {
        "icons/book-open.svg" => BOOK_OPEN,
        "icons/swords.svg" => SWORDS,
        "icons/radio.svg" => RADIO,
        "icons/volume-2.svg" => VOLUME_2,
        "icons/volume-x.svg" => VOLUME_X,
        "icons/settings.svg" => SETTINGS,
        "icons/play.svg" => PLAY,
        "icons/pause.svg" => PAUSE,
        "icons/chevron-left.svg" => CHEVRON_LEFT,
        "icons/chevron-right.svg" => CHEVRON_RIGHT,
        "icons/chevrons-left.svg" => CHEVRONS_LEFT,
        "icons/chevrons-right.svg" => CHEVRONS_RIGHT,
        "icons/trending-up.svg" => TRENDING_UP,
        "icons/git-branch.svg" => GIT_BRANCH,
        "icons/terminal.svg" => TERMINAL,
        "icons/cpu.svg" => CPU,
        "icons/sparkles.svg" => SPARKLES,
        "icons/share.svg" => SHARE,
        "icons/info.svg" => INFO,
        "icons/check.svg" => CHECK,
        "icons/refresh-cw.svg" => REFRESH_CW,
        "icons/upload.svg" => UPLOAD,
        "icons/panel-left.svg" => PANEL_LEFT,
        "icons/panel-right.svg" => PANEL_RIGHT,
        "icons/panel-bottom.svg" => PANEL_BOTTOM,
        "icons/alert-triangle.svg" => ALERT_TRIANGLE,
        "icons/lightbulb.svg" => LIGHTBULB,
        "icons/library.svg" => LIBRARY,
        "icons/film.svg" => FILM,
        "icons/globe.svg" => GLOBE,
        "icons/dice.svg" => DICE,
        "icons/puzzle.svg" => PUZZLE,
        "icons/grid.svg" => GRID,
        "icons/message.svg" => MESSAGE,
        "icons/bar-chart.svg" => BAR_CHART,
        "icons/plug.svg" => PLUG,
        "icons/stop.svg" => STOP,
        "icons/flag.svg" => FLAG,
        "icons/eye.svg" => EYE,
        "icons/close.svg" => CLOSE,
        "icons/magnify.svg" => MAGNIFY,
        "icons/save.svg" => SAVE,
        "icons/image.svg" => IMAGE,
        "icons/target.svg" => TARGET,
        "icons/pin.svg" => PIN,
        "icons/menu.svg" => MENU,
        _ => return None,
    })
}

/// Embedded icon assets served to GPUI's SVG renderer. Register with
/// `Application::new().with_assets(EmbeddedIcons)`.
pub struct EmbeddedIcons;

impl AssetSource for EmbeddedIcons {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(lookup(path).map(|svg| Cow::Borrowed(svg.as_bytes())))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        if path == "icons" || path == "icons/" {
            Ok(vec![
                "icons/book-open.svg".into(),
                "icons/swords.svg".into(),
                "icons/radio.svg".into(),
                "icons/volume-2.svg".into(),
                "icons/volume-x.svg".into(),
                "icons/settings.svg".into(),
                "icons/play.svg".into(),
                "icons/pause.svg".into(),
                "icons/chevron-left.svg".into(),
                "icons/chevron-right.svg".into(),
                "icons/chevrons-left.svg".into(),
                "icons/chevrons-right.svg".into(),
                "icons/trending-up.svg".into(),
                "icons/git-branch.svg".into(),
                "icons/terminal.svg".into(),
                "icons/cpu.svg".into(),
                "icons/sparkles.svg".into(),
                "icons/share.svg".into(),
                "icons/info.svg".into(),
                "icons/check.svg".into(),
                "icons/refresh-cw.svg".into(),
                "icons/upload.svg".into(),
                "icons/panel-left.svg".into(),
                "icons/panel-right.svg".into(),
                "icons/panel-bottom.svg".into(),
                "icons/alert-triangle.svg".into(),
                "icons/lightbulb.svg".into(),
                "icons/library.svg".into(),
                "icons/film.svg".into(),
                "icons/globe.svg".into(),
                "icons/dice.svg".into(),
                "icons/puzzle.svg".into(),
                "icons/grid.svg".into(),
                "icons/message.svg".into(),
                "icons/bar-chart.svg".into(),
                "icons/plug.svg".into(),
                "icons/stop.svg".into(),
                "icons/flag.svg".into(),
                "icons/eye.svg".into(),
                "icons/close.svg".into(),
                "icons/magnify.svg".into(),
                "icons/save.svg".into(),
                "icons/image.svg".into(),
                "icons/target.svg".into(),
                "icons/pin.svg".into(),
                "icons/menu.svg".into(),
            ])
        } else {
            Ok(vec![])
        }
    }
}

/// Semantic icon names used across the shell, decoupled from asset paths.
/// Variants are a token set consumed incrementally, so unused ones are allowed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub enum ShellIcon {
    BookOpen,
    Swords,
    Radio,
    Volume2,
    VolumeX,
    Settings,
    Play,
    Pause,
    ChevronLeft,
    ChevronRight,
    ChevronsLeft,
    ChevronsRight,
    TrendingUp,
    GitBranch,
    Terminal,
    Cpu,
    Sparkles,
    Share,
    Info,
    Check,
    RefreshCw,
    Upload,
    PanelLeft,
    PanelRight,
    PanelBottom,
    AlertTriangle,
    Lightbulb,
    Library,
    Film,
    Globe,
    Dice,
    Puzzle,
    Grid,
    Message,
    BarChart,
    Plug,
    Stop,
    Flag,
    Eye,
    Close,
    Magnify,
    Save,
    Image,
    Target,
    Pin,
    Menu,
}

impl ShellIcon {
    pub fn path(self) -> &'static str {
        match self {
            Self::BookOpen => "icons/book-open.svg",
            Self::Swords => "icons/swords.svg",
            Self::Radio => "icons/radio.svg",
            Self::Volume2 => "icons/volume-2.svg",
            Self::VolumeX => "icons/volume-x.svg",
            Self::Settings => "icons/settings.svg",
            Self::Play => "icons/play.svg",
            Self::Pause => "icons/pause.svg",
            Self::ChevronLeft => "icons/chevron-left.svg",
            Self::ChevronRight => "icons/chevron-right.svg",
            Self::ChevronsLeft => "icons/chevrons-left.svg",
            Self::ChevronsRight => "icons/chevrons-right.svg",
            Self::TrendingUp => "icons/trending-up.svg",
            Self::GitBranch => "icons/git-branch.svg",
            Self::Terminal => "icons/terminal.svg",
            Self::Cpu => "icons/cpu.svg",
            Self::Sparkles => "icons/sparkles.svg",
            Self::Share => "icons/share.svg",
            Self::Info => "icons/info.svg",
            Self::Check => "icons/check.svg",
            Self::RefreshCw => "icons/refresh-cw.svg",
            Self::Upload => "icons/upload.svg",
            Self::PanelLeft => "icons/panel-left.svg",
            Self::PanelRight => "icons/panel-right.svg",
            Self::PanelBottom => "icons/panel-bottom.svg",
            Self::AlertTriangle => "icons/alert-triangle.svg",
            Self::Lightbulb => "icons/lightbulb.svg",
            Self::Library => "icons/library.svg",
            Self::Film => "icons/film.svg",
            Self::Globe => "icons/globe.svg",
            Self::Dice => "icons/dice.svg",
            Self::Puzzle => "icons/puzzle.svg",
            Self::Grid => "icons/grid.svg",
            Self::Message => "icons/message.svg",
            Self::BarChart => "icons/bar-chart.svg",
            Self::Plug => "icons/plug.svg",
            Self::Stop => "icons/stop.svg",
            Self::Flag => "icons/flag.svg",
            Self::Eye => "icons/eye.svg",
            Self::Close => "icons/close.svg",
            Self::Magnify => "icons/magnify.svg",
            Self::Save => "icons/save.svg",
            Self::Image => "icons/image.svg",
            Self::Target => "icons/target.svg",
            Self::Pin => "icons/pin.svg",
            Self::Menu => "icons/menu.svg",
        }
    }
}

/// Renders a monoline icon as a GPUI element tinted by `color`.
///
/// The icon inherits the design's monoline strokes; `size` is logical pixels.
pub fn icon(icon: ShellIcon, size: f32, color: u32) -> gpui::Svg {
    gpui::svg()
        .path(icon.path())
        .size(gpui::px(size))
        .text_color(gpui::rgb(color))
}

#[cfg(test)]
mod tests {
    use super::{EmbeddedIcons, lookup};
    use gpui::AssetSource;

    #[test]
    fn every_lookup_entry_is_a_well_formed_svg_document() {
        for path in [
            "icons/book-open.svg",
            "icons/settings.svg",
            "icons/play.svg",
            "icons/panel-left.svg",
            "icons/cpu.svg",
            "icons/trending-up.svg",
        ] {
            let svg = lookup(path).expect("icon must exist");
            assert!(svg.starts_with("<svg"), "{path} must open an svg tag");
            assert!(svg.trim_end().ends_with("</svg>"), "{path} must close");
            assert!(svg.contains("stroke=\"currentColor\""));
        }
    }

    #[test]
    fn asset_source_serves_embedded_icons() {
        let bytes = EmbeddedIcons
            .load("icons/git-branch.svg")
            .unwrap()
            .expect("git-branch must load");
        assert!(bytes.starts_with(b"<svg"));
        assert!(EmbeddedIcons.load("icons/missing.svg").unwrap().is_none());
        assert!(!EmbeddedIcons.list("icons").unwrap().is_empty());
    }
}
