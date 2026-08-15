//! Mode-specific bar rendered between the goban and the navigation toolbar.
//!
//! M1 keeps the mode switch functional for `Play`, `Edit`, `Scoring`, and
//! `Estimator`. Guess/Find/Autoplay slots remain in the parity plan until their
//! backing workflows land.

use gpui::{
    App, Context, Div, InteractiveElement, MouseButton, ParentElement, Styled, Window, div, px, rgb,
};
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
    }
}

fn mode_button(mode: GameMode, active: bool, palette: UiPalette, cx: &Context<ShellApp>) -> Div {
    div()
        .px_2()
        .py_1()
        .border_1()
        .border_color(rgb(palette.accent))
        .rounded(px(4.0))
        .bg(if active {
            rgb(palette.button)
        } else {
            rgb(palette.button_active)
        })
        .text_sm()
        .child(mode_label(mode).to_owned())
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |shell, event, window, cx| {
                shell.on_mode_selected(mode, event, window, cx);
            }),
        )
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
        .gap_2()
        .px_2()
        .py_1()
        .border_1()
        .border_color(rgb(palette.border))
        .rounded(px(6.0))
        .bg(rgb(palette.panel));

    for mode in [
        GameMode::Play,
        GameMode::Edit,
        GameMode::Scoring,
        GameMode::Estimator,
    ] {
        bar = bar.child(mode_button(mode, active_mode == mode, palette, cx));
    }

    match active_mode {
        GameMode::Play => {
            let info = player_bar_info(snapshot);
            let player = |name: String, rank: String, active: bool| {
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_sm()
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
                    div()
                        .id("pass-button")
                        .debug_selector(|| "pass-button".to_owned())
                        .px_2()
                        .py_1()
                        .border_1()
                        .border_color(rgb(palette.accent))
                        .rounded(px(4.0))
                        .bg(rgb(palette.button))
                        .text_sm()
                        .child("Pass")
                        .on_mouse_down(MouseButton::Left, cx.listener(ShellApp::on_pass)),
                )
                .child(
                    div()
                        .px_2()
                        .py_1()
                        .border_1()
                        .border_color(rgb(palette.danger_text))
                        .rounded(px(4.0))
                        .bg(rgb(palette.danger))
                        .text_sm()
                        .child("Resign")
                        .on_mouse_down(MouseButton::Left, cx.listener(ShellApp::on_resign)),
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
            bar = bar.child(div().text_sm().text_color(rgb(palette.muted)).child(
                if active_mode == GameMode::Scoring {
                    "Please select dead stones."
                } else {
                    "Toggle group status (heuristic estimate)."
                },
            ));
            bar = bar.child(
                div()
                    .text_xs()
                    .text_color(rgb(palette.accent))
                    .child(crate::markup::scoring_summary(snapshot)),
            );
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
