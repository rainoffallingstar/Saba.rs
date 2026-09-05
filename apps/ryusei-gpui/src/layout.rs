//! Pure layout math for the three-pane shell.
//!
//! The split pane drag interaction lives in `ShellApp`, while this module keeps
//! the size calculations and settings fallbacks deterministic and testable.

use ryusei_host::SettingsStore;

/// A draggable divider between the center pane and one of the side panes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SplitPane {
    Left,
    Right,
    #[allow(dead_code)]
    PeerList,
    #[allow(dead_code)]
    WinrateGraph,
    Properties,
}

impl SplitPane {
    pub fn debug_selector(self) -> &'static str {
        match self {
            Self::Left => "left-splitter",
            Self::Right => "right-splitter",
            Self::PeerList => "peer-list-splitter",
            Self::WinrateGraph => "winrate-graph-splitter",
            Self::Properties => "properties-splitter",
        }
    }
}

/// Computes the new pane size from the drag start state.
///
/// `start_position` and `current_position` are window-global X coordinates.
/// Dragging the left divider to the right grows the left pane; dragging the
/// right divider to the right shrinks the right pane.
pub fn pane_size_for_drag(
    start_size: f32,
    start_position: f32,
    current_position: f32,
    pane: SplitPane,
) -> f32 {
    let delta = current_position - start_position;
    match pane {
        SplitPane::Left | SplitPane::PeerList | SplitPane::WinrateGraph => start_size + delta,
        SplitPane::Right | SplitPane::Properties => start_size - delta,
    }
}

pub fn clamp_pane_size(size: f32, min_size: f32, max_size: f32) -> f32 {
    size.clamp(min_size, max_size.max(min_size))
}

/// Reads a persisted pane width from the settings store with the original
/// Sabaki fallbacks. The returned value is clamped against the persisted
/// minimum and a conservative maximum.
pub fn pane_size_from_settings(
    settings: &SettingsStore,
    width_key: &str,
    min_width_key: &str,
    fallback: f32,
) -> f32 {
    let width = settings
        .get(width_key)
        .and_then(serde_json::Value::as_f64)
        .map(|value| value as f32)
        .unwrap_or(fallback);
    let min_width = settings
        .get(min_width_key)
        .and_then(serde_json::Value::as_f64)
        .map(|value| value as f32)
        .unwrap_or(100.0);
    clamp_pane_size(width, min_width, 800.0)
}

/// The right pane hosts the AI preview and the comments/node inspector. The
/// variation tree moved to the bottom deck, so `show_graph` no longer affects
/// right-pane visibility.
pub fn right_pane_visible(show_comments: bool, show_analysis_preview: bool) -> bool {
    show_comments || show_analysis_preview
}

#[cfg(test)]
mod tests {
    use super::{
        SplitPane, clamp_pane_size, pane_size_for_drag, pane_size_from_settings, right_pane_visible,
    };
    use ryusei_host::SettingsStore;
    use serde_json::json;

    #[test]
    fn dragging_dividers_moves_the_correct_pane() {
        assert_eq!(
            pane_size_for_drag(250.0, 100.0, 150.0, SplitPane::Left),
            300.0
        );
        assert_eq!(
            pane_size_for_drag(200.0, 100.0, 150.0, SplitPane::Right),
            150.0
        );
        assert_eq!(
            pane_size_for_drag(200.0, 100.0, 80.0, SplitPane::Right),
            220.0
        );
        assert_eq!(
            pane_size_for_drag(130.0, 100.0, 150.0, SplitPane::PeerList),
            180.0
        );
        assert_eq!(
            pane_size_for_drag(90.0, 100.0, 150.0, SplitPane::WinrateGraph),
            140.0
        );
        assert_eq!(
            pane_size_for_drag(180.0, 100.0, 70.0, SplitPane::Properties),
            210.0
        );
    }

    #[test]
    fn pane_sizes_are_clamped_to_the_persisted_minimum() {
        assert_eq!(clamp_pane_size(40.0, 100.0, 800.0), 100.0);
        assert_eq!(clamp_pane_size(900.0, 100.0, 800.0), 800.0);
        assert_eq!(clamp_pane_size(250.0, 100.0, 800.0), 250.0);
    }

    #[test]
    fn pane_size_settings_fall_back_and_clamp() {
        let mut settings = SettingsStore::default();
        settings
            .set("view.leftsidebar_width", json!(420.0))
            .unwrap();
        settings
            .set("view.leftsidebar_minwidth", json!(120.0))
            .unwrap();
        assert_eq!(
            pane_size_from_settings(
                &settings,
                "view.leftsidebar_width",
                "view.leftsidebar_minwidth",
                250.0,
            ),
            420.0
        );

        let empty = SettingsStore::default();
        assert_eq!(
            pane_size_from_settings(&empty, "view.sidebar_width", "view.sidebar_minwidth", 200.0,),
            200.0
        );
    }

    #[test]
    fn right_pane_visibility_matches_upstream_inference() {
        assert!(!right_pane_visible(false, false));
        assert!(right_pane_visible(true, false));
        assert!(right_pane_visible(false, true));
        assert!(right_pane_visible(true, true));
    }
}
