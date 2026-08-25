//! Mode-specific bar rendered between the goban and the navigation toolbar.
//!
//! Every M1 mode has a concrete bar. Find jumps to a selected move, Guess
//! verifies the next variation move, and Autoplay advances the active line.
//! The bar is intentionally not rendered in the current screenshot-matched
//! layout; it remains available for keyboard-triggered overlays and tests.
#![allow(dead_code)]

use gpui::{
    App, Context, Div, FontWeight, InteractiveElement, ParentElement, Styled, Window, div, rgb,
};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::{Selectable, Sizable};
use sabaki_domain_core::{Color, GameMode, GameSnapshot};

use crate::ShellApp;
use crate::markup::{MarkupTool, render_markup_toolbar};
use crate::theme::UiPalette;

/// Player block data for the PlayBar.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlayerBarInfo {
    pub black_name: String,
    pub black_rank: String,
    pub white_name: String,
    pub white_rank: String,
    pub black_to_move: bool,
}

pub fn player_bar_info(snapshot: &GameSnapshot) -> PlayerBarInfo {
    let property = |key: &str| {
        snapshot
            .root_properties
            .get(key)
            .and_then(|values| values.first())
            .cloned()
            .unwrap_or_default()
    };
    PlayerBarInfo {
        black_name: if property("PB").is_empty() {
            "Black".to_owned()
        } else {
            property("PB")
        },
        black_rank: property("BR"),
        white_name: if property("PW").is_empty() {
            "White".to_owned()
        } else {
            property("PW")
        },
        white_rank: property("WR"),
        black_to_move: snapshot.board.next_player == Color::Black,
    }
}

pub fn mode_label(mode: GameMode) -> &'static str {
    match mode {
        GameMode::Play => "Play",
        GameMode::Edit => "Edit",
        GameMode::Scoring => "Scoring",
        GameMode::Estimator => "Estimate",
        GameMode::Find => "Find",
        GameMode::Guess => "Guess",
        GameMode::Autoplay => "Autoplay",
    }
}

fn mode_button(
    mode: GameMode,
    active: bool,
    _palette: UiPalette,
    cx: &Context<ShellApp>,
) -> Button {
    let mode_idx: usize = match mode {
        GameMode::Play => 0,
        GameMode::Edit => 1,
        GameMode::Scoring => 2,
        GameMode::Estimator => 3,
        GameMode::Find => 4,
        GameMode::Guess => 5,
        GameMode::Autoplay => 6,
    };
    Button::new(("mode-btn", mode_idx))
        .small()
        .ghost()
        .selected(active)
        .label(mode_label(mode).to_owned())
        .on_click(cx.listener(move |shell, _, window, cx| {
            shell.on_mode_selected(mode, &gpui::MouseDownEvent::default(), window, cx);
        }))
}

fn mode_action_button(label: &'static str, _palette: UiPalette, cx: &Context<ShellApp>) -> Button {
    Button::new(label)
        .small()
        .primary()
        .label(label.to_owned())
        .on_click(cx.listener(|shell, _, window, cx| {
            shell.on_mode_action(&gpui::MouseDownEvent::default(), window, cx);
        }))
}

pub fn render_mode_bar(
    snapshot: &GameSnapshot,
    shell: &ShellApp,
    palette: UiPalette,
    cx: &Context<ShellApp>,
) -> Div {
    let active_mode = shell.mode;
    let mut bar = div()
        .debug_selector(|| "mode-bar".to_owned())
        .flex()
        .flex_wrap()
        .items_center()
        .gap_1()
        .p_1()
        .border_1()
        .border_color(rgb(palette.border))
        .rounded_lg()
        .bg(rgb(palette.panel));

    for mode in [
        GameMode::Play,
        GameMode::Edit,
        GameMode::Scoring,
        GameMode::Estimator,
        GameMode::Find,
        GameMode::Guess,
        GameMode::Autoplay,
    ] {
        bar = bar.child(mode_button(mode, active_mode == mode, palette, cx));
    }

    bar = bar.child(div().text_xs().text_color(rgb(palette.border)).child("|"));

    match active_mode {
        GameMode::Play => {
            let info = player_bar_info(snapshot);
            let player = |name: String, rank: String, active: bool| {
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .py_0p5()
                    .rounded_md()
                    .bg(if active {
                        rgb(palette.input)
                    } else {
                        rgb(palette.panel)
                    })
                    .border_1()
                    .border_color(rgb(if active {
                        palette.border
                    } else {
                        palette.panel
                    }))
                    .text_xs()
                    .text_color(rgb(if active { palette.accent } else { palette.text }))
                    .child(name)
                    .child(if rank.is_empty() {
                        div()
                    } else {
                        div().text_xs().text_color(rgb(palette.muted)).child(rank)
                    })
                    .child(if active { "●" } else { "" })
            };
            bar = bar
                .child(player(info.black_name, info.black_rank, info.black_to_move))
                .child(player(
                    info.white_name,
                    info.white_rank,
                    !info.black_to_move,
                ))
                .child(
                    Button::new("mode-bar-pass")
                        .small()
                        .ghost()
                        .label("Pass")
                        .on_click(cx.listener(|shell, _, window, cx| {
                            shell.on_pass(&gpui::MouseDownEvent::default(), window, cx);
                        })),
                )
                .child(
                    Button::new("mode-bar-resign")
                        .small()
                        .ghost()
                        .danger()
                        .label("Resign")
                        .on_click(cx.listener(|shell, _, window, cx| {
                            shell.on_resign(&gpui::MouseDownEvent::default(), window, cx);
                        })),
                );
        }
        GameMode::Edit => {
            let weak_shell = cx.entity().downgrade();
            let on_tool_clicked = move |tool: &MarkupTool, _: &mut Window, cx: &mut App| {
                weak_shell
                    .update(cx, |shell, cx| shell.on_tool_selected(*tool, cx))
                    .ok();
            };
            bar = bar.child(render_markup_toolbar(
                shell.active_tool,
                palette,
                on_tool_clicked,
            ));
        }
        GameMode::Scoring | GameMode::Estimator => {
            bar = bar.child(div().text_xs().text_color(rgb(palette.muted)).child(
                if active_mode == GameMode::Scoring {
                    "Please select dead stones."
                } else {
                    "Toggle group status (heuristic estimate)."
                },
            ));
            bar = bar.child(
                div()
                    .px_2()
                    .py_0p5()
                    .rounded_md()
                    .bg(rgb(palette.input))
                    .border_1()
                    .border_color(rgb(palette.border))
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(palette.accent))
                    .child(crate::markup::scoring_summary(snapshot)),
            );
        }
        GameMode::Find => {
            bar = bar
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(palette.muted))
                        .child("Click an intersection to jump to its first occurrence."),
                )
                .child(mode_action_button("Find move", palette, cx));
        }
        GameMode::Guess => {
            bar = bar
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(palette.muted))
                        .child("Click the next move in the active variation."),
                )
                .child(mode_action_button("Show instructions", palette, cx));
        }
        GameMode::Autoplay => {
            bar = bar
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(palette.muted))
                        .child("Advance through the active variation one move at a time."),
                )
                .child(mode_action_button("Next move", palette, cx));
        }
    }

    bar
}

#[cfg(test)]
mod tests {
    use super::{mode_label, player_bar_info};
    use sabaki_domain_core::{Color, GameMode, GameSnapshot};

    fn empty_snapshot() -> GameSnapshot {
        sabaki_domain_core::GameDocument::new(19, 19)
            .unwrap()
            .snapshot()
    }

    #[test]
    fn mode_labels_cover_implemented_modes() {
        assert_eq!(mode_label(GameMode::Play), "Play");
        assert_eq!(mode_label(GameMode::Edit), "Edit");
        assert_eq!(mode_label(GameMode::Scoring), "Scoring");
        assert_eq!(mode_label(GameMode::Estimator), "Estimate");
    }

    #[test]
    fn play_bar_falls_back_to_black_and_white() {
        let info = player_bar_info(&empty_snapshot());
        assert_eq!(info.black_name, "Black");
        assert_eq!(info.white_name, "White");
        assert_eq!(info.black_rank, "");
        assert_eq!(info.white_rank, "");
        assert!(info.black_to_move);
    }

    #[test]
    fn play_bar_reads_sgf_player_properties() {
        let snapshot = sabaki_domain_core::GameDocument::from_sgf(
            "(;GM[1]FF[4]SZ[19]PB[Cho]BR[9d]PW[Lee]WR[9d];B[pd])",
        )
        .unwrap()
        .snapshot();
        let info = player_bar_info(&snapshot);
        assert_eq!(info.black_name, "Cho");
        assert_eq!(info.black_rank, "9d");
        assert_eq!(info.white_name, "Lee");
        assert_eq!(info.white_rank, "9d");
        assert_eq!(snapshot.board.next_player, Color::White);
        assert!(!info.black_to_move);
    }
}
