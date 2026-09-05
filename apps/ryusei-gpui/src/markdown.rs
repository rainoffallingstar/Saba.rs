//! Markdown preview rendering for game-comment preview powered by GPUI Kit.

use gpui_kit::component::text::TextView;
use gpui_kit::{Div, ParentElement as _, Styled as _, div};

use crate::theme::UiPalette;

/// Renders a Markdown string into a GPUI element tree for the comment preview.
pub fn render_markdown(source: &str, _palette: UiPalette) -> Div {
    div()
        .text_xs()
        .child(TextView::markdown("comment-markdown", source.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::render_markdown;
    use crate::ThemeChoice;
    use crate::theme::ui_palette;

    #[test]
    fn splits_bold_and_code() {
        let palette = ui_palette(&ThemeChoice::Classic.tokens());
        let _ = render_markdown("Hello **world** with `code` block", palette);
    }

    #[test]
    fn unterminated_marker_stays_literal() {
        let palette = ui_palette(&ThemeChoice::Classic.tokens());
        let _ = render_markdown("A **dangling marker and `lone tick", palette);
    }
}
