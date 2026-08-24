//! Panel rendering for the shell, split out of `main.rs` so the shell keeps
//! only state, actions and assembly. Every function is a pure view over
//! `ShellApp` state plus precomputed values; listeners are built with
//! `cx.listener` against `ShellApp` handlers.

use std::rc::Rc;

use gpui::{
    App, Context, Div, FocusHandle, FontWeight, InteractiveElement, MouseButton, MouseDownEvent,
    Stateful, StatefulInteractiveElement, Window, div, hsla, prelude::*, px, rgb,
};

use sabaki_domain_core::{GameMode, GameSnapshot};

use crate::engine_console::{best_analysis_winrate, parse_gtp_vertex};
use crate::goban_view::{render_goban, render_goban_click_layer};
use crate::layout::SplitPane;
use crate::native_text_input::NativeInputBinding;
use crate::navigation::NavigationAvailability;
use crate::node_inspector::{NodeAnnotation, NodeInspectorMetadata};
use crate::plugin_contribution::PanelWidget;
use crate::plugin_panel::PluginPanelEntry;
use crate::settings_form::{SettingRow, display_setting_value};
use crate::theme::UiPalette;
use crate::variation_tree::{VariationTreeLayout, render_variation_tree};
use crate::winrate_graph::{GraphPlotPoint, WinrateGraphMetric, graph_index_from_x};
use crate::{BOARD_PIXEL_SIZE, ShellApp};

/// Renders a native macOS sidebar toggle icon (matching SF Symbol `sidebar.left` / `sidebar.right`).
fn mac_sidebar_icon(is_left: bool, active: bool, color: u32, fill: u32) -> Div {
    let bar = div()
        .w(px(4.5))
        .h_full()
        .bg(if active { rgb(fill) } else { rgb(0x00000000) });
    let pane = div().flex_1().h_full();
    div()
        .w(px(15.0))
        .h(px(12.0))
        .rounded(px(2.5))
        .border_1()
        .border_color(rgb(color))
        .flex()
        .overflow_hidden()
        .children(if is_left {
            vec![bar.border_r_1().border_color(rgb(color)), pane]
        } else {
            vec![pane, bar.border_l_1().border_color(rgb(color))]
        })
}

/// Renders a native macOS info circle icon (matching SF Symbol `info.circle`).
fn mac_info_icon(color: u32) -> Div {
    div()
        .size(px(14.0))
        .rounded_full()
        .border_1()
        .border_color(rgb(color))
        .flex()
        .items_center()
        .justify_center()
        .text_xs()
        .font_weight(FontWeight::BOLD)
        .text_color(rgb(color))
        .child("i")
}

/// The native macOS titlebar matching the reference texture with sidebar toggle icons.
pub fn render_titlebar(
    show_left_sidebar: bool,
    show_right_sidebar: bool,
    palette: UiPalette,
    cx: &Context<ShellApp>,
) -> Stateful<Div> {
    let icon_color = palette.muted;
    let active_color = palette.accent;
    let fill_color = palette.accent;

    div()
        .id("titlebar")
        .debug_selector(|| "titlebar".to_owned())
        .flex_none()
        .h(px(40.0))
        .flex()
        .items_center()
        .justify_between()
        .px_3()
        .border_b_1()
        .border_color(rgb(palette.border))
        .bg(rgb(palette.panel))
        // Left: traffic lights spacer + Left sidebar toggle icon + history arrows
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .child(div().w(px(72.0)))
                .child(
                    div()
                        .id("left-sidebar-toggle")
                        .debug_selector(|| "left-sidebar-toggle".to_owned())
                        .w(px(30.0))
                        .h(px(26.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(5.0))
                        .cursor_pointer()
                        .bg(if show_left_sidebar {
                            rgb(palette.button_active)
                        } else {
                            rgb(palette.panel)
                        })
                        .hover(|style| style.bg(rgb(palette.button)))
                        .child(mac_sidebar_icon(
                            true,
                            show_left_sidebar,
                            if show_left_sidebar {
                                active_color
                            } else {
                                icon_color
                            },
                            fill_color,
                        ))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(ShellApp::on_toggle_left_sidebar),
                        ),
                )
                .child(
                    div()
                        .w(px(26.0))
                        .h(px(26.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(5.0))
                        .cursor_pointer()
                        .text_base()
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(palette.muted))
                        .hover(|style| style.bg(rgb(palette.button)).text_color(rgb(palette.text)))
                        .child("‹")
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(ShellApp::on_navigate_previous),
                        ),
                )
                .child(
                    div()
                        .w(px(26.0))
                        .h(px(26.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(5.0))
                        .cursor_pointer()
                        .text_base()
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(palette.muted))
                        .hover(|style| style.bg(rgb(palette.button)).text_color(rgb(palette.text)))
                        .child("›")
                        .on_mouse_down(MouseButton::Left, cx.listener(ShellApp::on_navigate_next)),
                ),
        )
        // Center: clean native title
        .child(
            div()
                .font_weight(FontWeight::MEDIUM)
                .text_sm()
                .text_color(rgb(palette.text))
                .child("Sabaki"),
        )
        // Right: Info icon + Right sidebar toggle icon
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .child(
                    div()
                        .w(px(26.0))
                        .h(px(26.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(5.0))
                        .cursor_pointer()
                        .hover(|style| style.bg(rgb(palette.button)))
                        .child(mac_info_icon(palette.muted))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|shell, _, _, cx| shell.open_game_info(cx)),
                        ),
                )
                .child(
                    div()
                        .id("right-sidebar-toggle")
                        .debug_selector(|| "right-sidebar-toggle".to_owned())
                        .w(px(30.0))
                        .h(px(26.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(5.0))
                        .cursor_pointer()
                        .bg(if show_right_sidebar {
                            rgb(palette.button_active)
                        } else {
                            rgb(palette.panel)
                        })
                        .hover(|style| style.bg(rgb(palette.button)))
                        .child(mac_sidebar_icon(
                            false,
                            show_right_sidebar,
                            if show_right_sidebar {
                                active_color
                            } else {
                                icon_color
                            },
                            fill_color,
                        ))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(ShellApp::on_toggle_right_sidebar),
                        ),
                ),
        )
}

/// The navigation and pane-toggle row.
#[allow(dead_code)]
pub fn render_toolbar_row(
    availability: NavigationAvailability,
    position: &str,
    show_left_sidebar: bool,
    show_right_sidebar: bool,
    palette: UiPalette,
    cx: &Context<ShellApp>,
) -> Div {
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_2()
        .child(
            div()
                .id("left-sidebar-toggle")
                .debug_selector(|| "left-sidebar-toggle".to_owned())
                .px_3()
                .py_1()
                .rounded_md()
                .cursor_pointer()
                .border_1()
                .border_color(rgb(if show_left_sidebar {
                    palette.accent
                } else {
                    palette.border
                }))
                .bg(if show_left_sidebar {
                    rgb(palette.button_active)
                } else {
                    rgb(palette.panel)
                })
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgb(if show_left_sidebar {
                    palette.accent
                } else {
                    palette.muted
                }))
                .hover(|style| {
                    if !show_left_sidebar {
                        style.bg(rgb(palette.button)).text_color(rgb(palette.text))
                    } else {
                        style
                    }
                })
                .child(if show_left_sidebar {
                    "⚙ Engines"
                } else {
                    "Engines"
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(ShellApp::on_toggle_left_sidebar),
                ),
        )
        .child(crate::navigation_bar(
            availability,
            position,
            palette,
            cx.listener(ShellApp::on_navigate_first),
            cx.listener(ShellApp::on_navigate_previous),
            cx.listener(ShellApp::on_navigate_next),
            cx.listener(ShellApp::on_navigate_last),
        ))
        .child(
            div()
                .id("right-sidebar-toggle")
                .debug_selector(|| "right-sidebar-toggle".to_owned())
                .px_3()
                .py_1()
                .rounded_md()
                .cursor_pointer()
                .border_1()
                .border_color(rgb(if show_right_sidebar {
                    palette.accent
                } else {
                    palette.border
                }))
                .bg(if show_right_sidebar {
                    rgb(palette.button_active)
                } else {
                    rgb(palette.panel)
                })
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgb(if show_right_sidebar {
                    palette.accent
                } else {
                    palette.muted
                }))
                .hover(|style| {
                    if !show_right_sidebar {
                        style.bg(rgb(palette.button)).text_color(rgb(palette.text))
                    } else {
                        style
                    }
                })
                .child(if show_right_sidebar {
                    "📊 Panels"
                } else {
                    "Panels"
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(ShellApp::on_toggle_right_sidebar),
                ),
        )
}

/// Renders the draggable divider between the center pane and a side pane.
pub fn render_split_handle(pane: SplitPane, palette: UiPalette, cx: &Context<ShellApp>) -> Div {
    div()
        .debug_selector(move || pane.debug_selector().to_owned())
        .flex_none()
        .w(px(5.0))
        .h_full()
        .bg(rgb(palette.border))
        .cursor_col_resize()
        .hover(|style| style.bg(rgb(palette.accent)))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(
                move |shell, event: &MouseDownEvent, _window: &mut Window, cx| {
                    shell.begin_split_drag(pane, f32::from(event.position.x), cx);
                },
            ),
        )
}

/// The dark player bar at the bottom of the window, mirroring the original
/// Electron Sabaki footer: black background, white player names with ranks,
/// a centered ⚫⚪ turn toggle, and the hamburger drawer menu on the right.
pub fn render_player_bar(
    snapshot: &GameSnapshot,
    status: &str,
    _palette: UiPalette,
    shell: &ShellApp,
    cx: &Context<ShellApp>,
) -> Stateful<Div> {
    let property = |key: &str| {
        snapshot
            .root_properties
            .get(key)
            .and_then(|values| values.first())
            .cloned()
            .unwrap_or_default()
    };
    let black_name = if property("PB").is_empty() {
        "Black".to_owned()
    } else {
        property("PB")
    };
    let black_rank = property("BR");
    let white_name = if property("PW").is_empty() {
        "White".to_owned()
    } else {
        property("PW")
    };
    let white_rank = property("WR");
    let is_black_turn = snapshot.board.next_player == sabaki_domain_core::Color::Black;

    div()
        .id("player-bar")
        .debug_selector(|| "player-bar".to_owned())
        .flex_none()
        .h(px(36.0))
        .flex()
        .items_center()
        .justify_between()
        .px_4()
        .bg(rgb(0x141414))
        .text_xs()
        .text_color(rgb(0xf2f2f2))
        // Left: Black player + rank
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(div().child("●"))
                .child(div().font_weight(FontWeight::SEMIBOLD).child(black_name))
                .children((!black_rank.is_empty()).then(|| {
                    div()
                        .text_color(rgb(0x9a9a9a))
                        .child(format!("({black_rank})"))
                })),
        )
        // Center: turn indicator + pass / resign actions + recovery chip
        .child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .w(px(22.0))
                                .h(px(22.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_full()
                                .bg(if is_black_turn {
                                    rgb(0x333333)
                                } else {
                                    rgb(0xffffff)
                                })
                                .border_1()
                                .border_color(rgb(if is_black_turn { 0x555555 } else { 0xbbbbbb }))
                                .child(if is_black_turn {
                                    div().text_color(rgb(0xffffff)).child("B")
                                } else {
                                    div().text_color(rgb(0x111111)).child("W")
                                }),
                        )
                        .child(div().text_color(rgb(0x9a9a9a)).child(if status.is_empty() {
                            if is_black_turn {
                                "Black to move".to_owned()
                            } else {
                                "White to move".to_owned()
                            }
                        } else {
                            status.to_owned()
                        })),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .id("pass-button")
                                .debug_selector(|| "pass-button".to_owned())
                                .px_3()
                                .py_0p5()
                                .rounded_md()
                                .cursor_pointer()
                                .text_color(rgb(0xe8e8e8))
                                .hover(|style| style.bg(rgb(0x2a2a2a)))
                                .child("Pass")
                                .on_mouse_down(MouseButton::Left, cx.listener(ShellApp::on_pass)),
                        )
                        .child(
                            div()
                                .px_3()
                                .py_0p5()
                                .rounded_md()
                                .cursor_pointer()
                                .text_color(rgb(0xe8b3b3))
                                .hover(|style| style.bg(rgb(0x3a2222)))
                                .child("Resign")
                                .on_mouse_down(MouseButton::Left, cx.listener(ShellApp::on_resign)),
                        ),
                )
                .children(
                    (shell.mode == GameMode::Scoring || shell.mode == GameMode::Estimator).then(
                        || {
                            div()
                                .flex()
                                .items_center()
                                .gap_1p5()
                                .px_2p5()
                                .py_0p5()
                                .rounded_md()
                                .cursor_pointer()
                                .bg(rgb(0x3a2210))
                                .border_1()
                                .border_color(rgb(0x8a5520))
                                .text_color(rgb(0xf2a860))
                                .hover(|style| style.bg(rgb(0x4a2a14)))
                                .child("🏁 退出点目 (返回落子)")
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|shell, _, _, cx| {
                                        shell.set_mode(GameMode::Play, cx)
                                    }),
                                )
                        },
                    ),
                )
                .children(shell.autosave.info().is_available.then(|| {
                    div()
                        .id("player-bar-restore-recovery")
                        .flex()
                        .items_center()
                        .gap_1()
                        .px_2()
                        .py_0p5()
                        .rounded_md()
                        .cursor_pointer()
                        .bg(rgb(0x352810))
                        .border_1()
                        .border_color(rgb(0x7a5818))
                        .text_color(rgb(0xf2cf78))
                        .hover(|style| style.bg(rgb(0x4a3a18)))
                        .child("⚡ Restore")
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(ShellApp::on_restore_recovery),
                        )
                })),
        )
        // Right: White player + rank + pinned plugin buttons + 🧩 plugin menu + hamburger menu
        .child(
            div()
                .flex()
                .items_center()
                .gap_1p5()
                .children((!white_rank.is_empty()).then(|| {
                    div()
                        .text_color(rgb(0x9a9a9a))
                        .child(format!("({white_rank})"))
                }))
                .child(div().font_weight(FontWeight::SEMIBOLD).child(white_name))
                .child(div().child("○"))
                // Pinned plugin quick buttons directly on the bar
                .children(
                    shell
                        .installed_plugins
                        .iter()
                        .filter(|plugin| {
                            plugin.enabled && shell.pinned_plugin_ids().contains(&plugin.plugin_id)
                        })
                        .enumerate()
                        .map(|(plugin_idx, plugin)| {
                            let plugin_id = plugin.plugin_id.clone();
                            let is_active =
                                shell.active_plugin_popover.as_ref() == Some(&plugin_id);
                            let icon = plugin_icon(&plugin_id);
                            let label = if plugin_id.contains("katago") {
                                "KataGo"
                            } else if plugin_id.contains("fox") {
                                "野狐"
                            } else if plugin_id.contains("position") {
                                "局面"
                            } else if plugin_id.contains("sgf") {
                                "导出"
                            } else {
                                plugin.name.as_str()
                            };
                            div()
                                .id(("pinned-plugin", plugin_idx))
                                .px_2()
                                .py_0p5()
                                .cursor_pointer()
                                .rounded_md()
                                .bg(if is_active {
                                    rgb(0x2a2a3a)
                                } else {
                                    rgb(0x1e1e1e)
                                })
                                .border_1()
                                .border_color(if is_active {
                                    rgb(shell.palette.accent)
                                } else {
                                    rgb(0x3a3a3a)
                                })
                                .text_color(if is_active {
                                    rgb(0x8ec5ff)
                                } else {
                                    rgb(0xe8e8e8)
                                })
                                .hover(|style| {
                                    style
                                        .bg(rgb(0x2e2e2e))
                                        .border_color(rgb(shell.palette.accent))
                                })
                                .child(format!("{icon} {label}"))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |shell, _, _, cx| {
                                        shell.toggle_plugin_popover(&plugin_id, cx);
                                    }),
                                )
                        }),
                )
                .child(
                    div()
                        .id("gtp-terminal-button")
                        .debug_selector(|| "gtp-terminal-button".to_owned())
                        .px_2()
                        .py_0p5()
                        .cursor_pointer()
                        .rounded_md()
                        .bg(if shell.gtp_terminal_open {
                            rgb(0x2a2a3a)
                        } else {
                            rgb(0x1e1e1e)
                        })
                        .border_1()
                        .border_color(if shell.gtp_terminal_open {
                            rgb(shell.palette.accent)
                        } else {
                            rgb(0x3a3a3a)
                        })
                        .text_color(rgb(if shell.gtp_terminal_open {
                            0x8ec5ff
                        } else {
                            0xe8e8e8
                        }))
                        .hover(|style| {
                            style
                                .bg(rgb(0x2e2e2e))
                                .border_color(rgb(shell.palette.accent))
                        })
                        .child("💻 GTP 终端")
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(ShellApp::toggle_gtp_terminal),
                        ),
                )
                .child(
                    div()
                        .id("plugin-menu-button")
                        .debug_selector(|| "plugin-menu-button".to_owned())
                        .px_2()
                        .py_0p5()
                        .cursor_pointer()
                        .rounded_md()
                        .bg(if shell.active_plugin_popover.as_deref() == Some("all") {
                            rgb(0x2a2a3a)
                        } else {
                            rgb(0x1e1e1e)
                        })
                        .border_1()
                        .border_color(if shell.active_plugin_popover.as_deref() == Some("all") {
                            rgb(shell.palette.accent)
                        } else {
                            rgb(0x3a3a3a)
                        })
                        .text_color(rgb(
                            if shell.active_plugin_popover.as_deref() == Some("all") {
                                0x8ec5ff
                            } else {
                                0xe8e8e8
                            },
                        ))
                        .hover(|style| style.bg(rgb(0x2a2a2a)))
                        .child("🧩 插件")
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|shell, _, _, cx| shell.toggle_plugin_popover("all", cx)),
                        ),
                )
                .child(
                    div()
                        .id("drawer-menu-button")
                        .debug_selector(|| "drawer-menu-button".to_owned())
                        .px_2()
                        .py_0p5()
                        .cursor_pointer()
                        .rounded_md()
                        .text_color(rgb(0xe8e8e8))
                        .hover(|style| style.bg(rgb(0x2a2a2a)))
                        .child("☰")
                        .on_mouse_down(MouseButton::Left, cx.listener(ShellApp::open_side_menu)),
                ),
        )
}

/// The restore/discard recovery buttons shown while a recovery candidate is
/// available.
#[allow(dead_code)]
pub fn render_recovery_buttons(shell: &ShellApp, cx: &Context<ShellApp>) -> Div {
    if shell.autosave.info().is_available {
        div()
            .w_full()
            .max_w(px(BOARD_PIXEL_SIZE))
            .p_2()
            .rounded_lg()
            .bg(rgb(shell.palette.input))
            .border_1()
            .border_color(rgb(shell.palette.accent))
            .flex()
            .items_center()
            .gap_3()
            .child(
                div().flex_1().min_w_0().overflow_hidden().child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(shell.palette.text))
                        .child("⚡ Unsaved recovery snapshot found from prior session"),
                ),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .px_3()
                            .py_1()
                            .rounded_md()
                            .cursor_pointer()
                            .border_1()
                            .border_color(rgb(shell.palette.accent))
                            .bg(rgb(shell.palette.button))
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(shell.palette.text))
                            .hover(|style| style.bg(rgb(shell.palette.button_active)))
                            .child("Restore Recovery")
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(ShellApp::on_restore_recovery),
                            ),
                    )
                    .child(
                        div()
                            .px_3()
                            .py_1()
                            .rounded_md()
                            .cursor_pointer()
                            .border_1()
                            .border_color(rgb(shell.palette.danger_text))
                            .bg(rgb(shell.palette.danger))
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(shell.palette.danger_text))
                            .child("Discard")
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(ShellApp::on_discard_recovery),
                            ),
                    ),
            )
    } else {
        div()
    }
}

/// The reload/keep-local actions shown while an external-file conflict is
/// pending.
#[allow(dead_code)]
pub fn render_external_conflict_buttons(
    external_conflict: bool,
    palette: UiPalette,
    cx: &Context<ShellApp>,
) -> Div {
    if external_conflict {
        div()
            .w_full()
            .max_w(px(BOARD_PIXEL_SIZE))
            .p_2()
            .rounded_lg()
            .bg(rgb(palette.input))
            .border_1()
            .border_color(rgb(palette.danger_text))
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(palette.text))
                    .child("⚠️ The current file on disk has changed externally"),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .px_3()
                            .py_1()
                            .rounded_md()
                            .cursor_pointer()
                            .border_1()
                            .border_color(rgb(palette.accent))
                            .bg(rgb(palette.button))
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(palette.text))
                            .hover(|style| style.bg(rgb(palette.button_active)))
                            .child("Reload from Disk")
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(ShellApp::on_reload_external),
                            ),
                    )
                    .child(
                        div()
                            .px_3()
                            .py_1()
                            .rounded_md()
                            .cursor_pointer()
                            .border_1()
                            .border_color(rgb(palette.border))
                            .bg(rgb(palette.panel))
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(palette.muted))
                            .hover(|style| style.bg(rgb(palette.button)))
                            .child("Keep Local Changes")
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(ShellApp::on_keep_local_external),
                            ),
                    ),
            )
    } else {
        div()
    }
}

/// Helper returning an emoji/icon representing the plugin domain.
fn plugin_icon(plugin_id: &str) -> &'static str {
    if plugin_id.contains("katago") {
        "⚡"
    } else if plugin_id.contains("fox") {
        "🦊"
    } else if plugin_id.contains("position") {
        "📊"
    } else if plugin_id.contains("sgf") {
        "💾"
    } else {
        "🧩"
    }
}

/// Helper returning a human-readable summary of the plugin's capabilities.
fn plugin_description(plugin_id: &str) -> &'static str {
    if plugin_id.contains("katago") {
        "KataGo AI 硬件检测、一键配置与权重模型快速下载"
    } else if plugin_id.contains("fox") {
        "野狐围棋对局查询与 SGF 历史棋谱自动同步"
    } else if plugin_id.contains("position") {
        "全局局面死活判定与形势智能检查"
    } else if plugin_id.contains("sgf") {
        "棋谱导出与多引擎分析注释打包"
    } else {
        "Sabaki 扩展组件"
    }
}

/// Legacy inline plugin panel retained only as a view fixture while the compact
/// overlay menu owns the production UI.
#[allow(dead_code)]
pub fn render_plugins_panel(shell: &ShellApp, cx: &Context<ShellApp>) -> Stateful<Div> {
    let enabled_plugins: Vec<&PluginPanelEntry> = shell
        .installed_plugins
        .iter()
        .filter(|plugin| plugin.enabled)
        .collect();

    let mut panel = div()
        .id("plugins-panel")
        .debug_selector(|| "plugins-panel".to_owned())
        .flex_none()
        .max_h(px(260.0))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div().flex().items_center().gap_1().child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(shell.palette.subtle))
                            .child("PLUGINS & EXTENSIONS (扩展组件)"),
                    ),
                )
                .child(
                    div()
                        .cursor_pointer()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(shell.palette.accent))
                        .hover(|style| style.underline())
                        .child("⚙️ 插件设置")
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|shell, _, _, cx| shell.open_preferences(cx)),
                        ),
                ),
        );

    if enabled_plugins.is_empty() {
        panel = panel.child(
            div()
                .p_3()
                .rounded_lg()
                .bg(rgb(shell.palette.input))
                .border_1()
                .border_color(rgb(shell.palette.border))
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(shell.palette.subtle))
                        .child("暂无启用的扩展组件，可在偏好设置中启用"),
                )
                .child(
                    div()
                        .px_2p5()
                        .py_1()
                        .rounded_md()
                        .bg(rgb(shell.palette.button))
                        .border_1()
                        .border_color(rgb(shell.palette.border))
                        .cursor_pointer()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(shell.palette.text))
                        .hover(|style| style.bg(rgb(shell.palette.button_active)))
                        .child("⚙️ 前往偏好设置管理插件...")
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|shell, _, _, cx| shell.open_preferences(cx)),
                        ),
                ),
        );
    } else {
        panel = panel.child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .children(enabled_plugins.iter().map(|plugin| {
                    let plugin_id = plugin.plugin_id.clone();
                    let icon = plugin_icon(&plugin_id);
                    let desc = plugin_description(&plugin_id);

                    div()
                        .p_2p5()
                        .rounded_lg()
                        .bg(rgb(shell.palette.input))
                        .border_1()
                        .border_color(rgb(shell.palette.border))
                        .flex()
                        .flex_col()
                        .gap_1p5()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_1p5()
                                        .child(div().text_sm().child(icon))
                                        .child(
                                            div()
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_xs()
                                                .text_color(rgb(shell.palette.text))
                                                .child(plugin.name.clone()),
                                        ),
                                )
                                .child(
                                    div()
                                        .px_1p5()
                                        .py_0p5()
                                        .rounded_sm()
                                        .bg(rgb(shell.palette.panel))
                                        .border_1()
                                        .border_color(rgb(shell.palette.border))
                                        .text_xs()
                                        .text_color(rgb(shell.palette.muted))
                                        .child(format!("v{}", plugin.version)),
                                ),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(shell.palette.subtle))
                                .child(desc),
                        )
                        .children(plugin_id.contains("fox").then(|| {
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(
                                    div()
                                        .track_focus(&shell.fox_query_focus_handle)
                                        .px_2()
                                        .py_1()
                                        .rounded_md()
                                        .border_1()
                                        .border_color(rgb(shell.palette.accent))
                                        .bg(rgb(shell.palette.panel))
                                        .text_xs()
                                        .text_color(rgb(shell.palette.text))
                                        .child(if shell.fox_query_input.text().is_empty() {
                                            "输入野狐用户名或 ID，按 Enter 查询最新棋谱".to_owned()
                                        } else {
                                            shell.fox_query_input.text().to_owned()
                                        })
                                        .child(NativeInputBinding::new(
                                            shell.fox_query_focus_handle.clone(),
                                            cx.entity().clone(),
                                        ))
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(ShellApp::on_fox_query_focus),
                                        )
                                        .on_key_down(cx.listener(ShellApp::on_fox_query_key_down)),
                                )
                                .child(
                                    div()
                                        .px_2()
                                        .py_1()
                                        .rounded_md()
                                        .cursor_pointer()
                                        .bg(rgb(shell.palette.button))
                                        .border_1()
                                        .border_color(rgb(shell.palette.border))
                                        .text_xs()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(rgb(shell.palette.text))
                                        .hover(|style| style.bg(rgb(shell.palette.button_active)))
                                        .child("查询并导入最新对局 →")
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(|shell, _, _, cx| {
                                                shell.fetch_fox_query(cx)
                                            }),
                                        ),
                                )
                        }))
                        .children(plugin.process_status.as_ref().map(|status| {
                            let color = if status.starts_with("running") {
                                rgb(shell.palette.success)
                            } else if status.starts_with("auto-disabled")
                                || status.starts_with("crashed")
                            {
                                rgb(shell.palette.danger_text)
                            } else {
                                rgb(shell.palette.subtle)
                            };
                            div()
                                .flex()
                                .items_center()
                                .gap_1()
                                .child(div().text_xs().text_color(color).child("●"))
                                .child(div().text_xs().text_color(color).child(status.clone()))
                        }))
                        .children((!plugin.commands.is_empty()).then(|| {
                            div().flex().flex_col().gap_1().children(
                                plugin.command_ids.iter().zip(plugin.commands.iter()).map(
                                    |(command_id, title)| {
                                        let plugin_id = plugin_id.clone();
                                        let command_id = command_id.clone();
                                        div()
                                            .px_2p5()
                                            .py_1p5()
                                            .rounded_md()
                                            .bg(rgb(shell.palette.button))
                                            .border_1()
                                            .border_color(rgb(shell.palette.border))
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .hover(|style| {
                                                style
                                                    .bg(rgb(shell.palette.button_active))
                                                    .border_color(rgb(shell.palette.accent))
                                            })
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(rgb(shell.palette.text))
                                                    .child(title.clone()),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(rgb(shell.palette.accent))
                                                    .child("执行 →"),
                                            )
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(move |shell,
                                                            _: &MouseDownEvent,
                                                            _window: &mut Window,
                                                            cx: &mut Context<ShellApp>| {
                                                    shell.on_plugin_command(
                                                        &plugin_id,
                                                        &command_id,
                                                        cx,
                                                    )
                                                }),
                                            )
                                    },
                                ),
                            )
                        }))
                })),
        );
    }

    // Append declarative UI panel contributions if any
    let mut panel_count = 0;
    let mut panel_section = div().flex().flex_col().gap_2().text_xs();
    for entry in shell
        .installed_plugins
        .iter()
        .filter(|entry| entry.enabled && entry.ui_panel_granted && !entry.panels.is_empty())
    {
        let plugin_id = entry.plugin_id.clone();
        for panel_item in &entry.panels {
            panel_count += 1;
            let panel_plugin_id = plugin_id.clone();
            let panel_title = panel_item.panel_title.clone();
            panel_section =
                panel_section
                    .child(
                        div()
                            .mt_1()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(shell.palette.subtle))
                            .child(panel_title.clone()),
                    )
                    .child(div().flex().flex_col().gap_1().children(
                        panel_item.widgets.iter().map(|widget| match widget {
                            PanelWidget::Label { text } => div().child(text.clone()),
                            PanelWidget::Value { label, value } => {
                                div().child(format!("{label}: {value}"))
                            }
                            PanelWidget::Button { id, title } => {
                                let command_plugin_id = panel_plugin_id.clone();
                                let command_id = id.clone();
                                div()
                                .px_3()
                                .py_1()
                                .border_1()
                                .border_color(rgb(shell.palette.accent))
                                .rounded_md()
                                .bg(rgb(shell.palette.button))
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .cursor_pointer()
                                .hover(|style| style.bg(rgb(shell.palette.button_active)))
                                .child(title.clone())
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(
                                        move |shell,
                                              _: &MouseDownEvent,
                                              _window: &mut Window,
                                              cx: &mut Context<ShellApp>| {
                                            shell.on_plugin_command(
                                                &command_plugin_id,
                                                &command_id,
                                                cx,
                                            );
                                        },
                                    ),
                                )
                            }
                            PanelWidget::Select {
                                id,
                                options,
                                selected,
                            } => div().child(format!(
                                "[select:{id}] {} (={})",
                                options.join("/"),
                                selected.as_deref().unwrap_or("none")
                            )),
                        }),
                    ));
        }
    }
    if panel_count > 0 {
        panel = panel.child(panel_section);
    }

    panel
}

/// Compact overlay menu for optional extensions. It uses transient screen space
/// above the player bar rather than consuming the right game-inspection panel.
pub fn render_plugin_menu(shell: &ShellApp, cx: &Context<ShellApp>) -> Stateful<Div> {
    let active_target = shell.active_plugin_popover.as_deref().unwrap_or("all");
    let is_all = active_target == "all";
    let pinned_ids = shell.pinned_plugin_ids();

    let enabled: Vec<&PluginPanelEntry> = shell
        .installed_plugins
        .iter()
        .filter(|plugin| plugin.enabled && (is_all || plugin.plugin_id == active_target))
        .collect();

    div()
        .id("plugin-menu")
        .debug_selector(|| "plugin-menu".to_owned())
        .absolute()
        .bottom(px(42.0))
        .left_0()
        .w_full()
        .flex()
        .justify_center()
        .child(
            div()
                .w(px(340.0))
                .max_h(px(380.0))
                .flex()
                .flex_col()
                .gap_2()
                .p_3()
                .rounded_lg()
                .border_1()
                .border_color(rgb(shell.palette.accent))
                .bg(rgb(shell.palette.panel))
                .shadow_lg()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(shell.palette.text))
                                .child(if is_all {
                                    "🧩 扩展组件中心"
                                } else if active_target.contains("fox") {
                                    "🦊 野狐围棋对局查询"
                                } else if active_target.contains("katago") {
                                    "⚡ KataGo AI 引擎与模型"
                                } else {
                                    "🧩 扩展组件"
                                }),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .cursor_pointer()
                                        .text_xs()
                                        .text_color(rgb(shell.palette.accent))
                                        .hover(|style| style.underline())
                                        .child("⚙️ 设置")
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(|shell, _, _, cx| {
                                                shell.open_preferences(cx)
                                            }),
                                        ),
                                )
                                .child(
                                    div()
                                        .cursor_pointer()
                                        .text_xs()
                                        .text_color(rgb(shell.palette.muted))
                                        .hover(|style| style.text_color(rgb(shell.palette.text)))
                                        .child("✕ 关闭")
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(ShellApp::close_plugin_popover),
                                        ),
                                ),
                        ),
                )
                .children(is_all.then(|| {
                    div()
                        .text_xs()
                        .text_color(rgb(shell.palette.subtle))
                        .child("提示：点击各插件的 📌 按钮可固定/取消固定到工具栏")
                }))
                .child(if enabled.is_empty() {
                    div()
                        .p_2()
                        .rounded_md()
                        .bg(rgb(shell.palette.input))
                        .text_xs()
                        .text_color(rgb(shell.palette.subtle))
                        .child("暂无启用的扩展组件，请到设置中启用插件。")
                } else {
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .children(enabled.iter().map(|plugin| {
                            let plugin_id = plugin.plugin_id.clone();
                            let is_pinned = pinned_ids.contains(&plugin_id);
                            let toggle_id = plugin_id.clone();

                            div()
                                .p_2()
                                .rounded_md()
                                .bg(rgb(shell.palette.input))
                                .border_1()
                                .border_color(rgb(shell.palette.border))
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .child(
                                            div()
                                                .text_xs()
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(rgb(shell.palette.text))
                                                .child(format!(
                                                    "{} {}",
                                                    plugin_icon(&plugin_id),
                                                    plugin.name
                                                )),
                                        )
                                        .child(
                                            div()
                                                .cursor_pointer()
                                                .px_1p5()
                                                .py_0p5()
                                                .rounded_sm()
                                                .border_1()
                                                .border_color(if is_pinned {
                                                    rgb(shell.palette.accent)
                                                } else {
                                                    rgb(shell.palette.border)
                                                })
                                                .bg(if is_pinned {
                                                    rgb(shell.palette.button_active)
                                                } else {
                                                    rgb(shell.palette.button)
                                                })
                                                .text_xs()
                                                .text_color(if is_pinned {
                                                    rgb(shell.palette.accent)
                                                } else {
                                                    rgb(shell.palette.muted)
                                                })
                                                .hover(|style| {
                                                    style.bg(rgb(shell.palette.button_active))
                                                })
                                                .child(if is_pinned {
                                                    "📌 已固定"
                                                } else {
                                                    "📌 固定到栏"
                                                })
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(move |shell, _, _, cx| {
                                                        shell.toggle_plugin_pinned(&toggle_id, cx);
                                                    }),
                                                ),
                                        ),
                                )
                                .children(plugin_id.contains("fox").then(|| {
                                    div()
                                        .flex()
                                        .gap_1()
                                        .child(
                                            div()
                                                .flex_1()
                                                .track_focus(&shell.fox_query_focus_handle)
                                                .px_2()
                                                .py_1()
                                                .rounded_md()
                                                .border_1()
                                                .border_color(rgb(shell.palette.accent))
                                                .bg(rgb(shell.palette.panel))
                                                .text_xs()
                                                .text_color(rgb(shell.palette.text))
                                                .child(if shell.fox_query_input.text().is_empty() {
                                                    "野狐用户名或 ID".to_owned()
                                                } else {
                                                    shell.fox_query_input.text().to_owned()
                                                })
                                                .child(NativeInputBinding::new(
                                                    shell.fox_query_focus_handle.clone(),
                                                    cx.entity().clone(),
                                                ))
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(ShellApp::on_fox_query_focus),
                                                )
                                                .on_key_down(
                                                    cx.listener(ShellApp::on_fox_query_key_down),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .px_2()
                                                .py_1()
                                                .rounded_md()
                                                .cursor_pointer()
                                                .bg(rgb(shell.palette.button))
                                                .border_1()
                                                .border_color(rgb(shell.palette.border))
                                                .text_xs()
                                                .text_color(rgb(shell.palette.text))
                                                .hover(|style| {
                                                    style.bg(rgb(shell.palette.button_active))
                                                })
                                                .child("查询")
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(|shell, _, _, cx| {
                                                        shell.fetch_fox_query(cx)
                                                    }),
                                                ),
                                        )
                                }))
                                .children((!plugin.commands.is_empty()).then(|| {
                                    div().flex().flex_wrap().gap_1().children(
                                        plugin.command_ids.iter().zip(plugin.commands.iter()).map(
                                            |(command_id, title)| {
                                                let plugin_id = plugin_id.clone();
                                                let command_id = command_id.clone();
                                                div()
                                                    .px_2()
                                                    .py_1()
                                                    .rounded_md()
                                                    .cursor_pointer()
                                                    .bg(rgb(shell.palette.button))
                                                    .border_1()
                                                    .border_color(rgb(shell.palette.border))
                                                    .text_xs()
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(rgb(shell.palette.text))
                                                    .hover(|style| {
                                                        style.bg(rgb(shell.palette.button_active))
                                                    })
                                                    .child(title.clone())
                                                    .on_mouse_down(
                                                        MouseButton::Left,
                                                        cx.listener(move |shell, _, _, cx| {
                                                            shell.on_plugin_command(
                                                                &plugin_id,
                                                                &command_id,
                                                                cx,
                                                            )
                                                        }),
                                                    )
                                            },
                                        ),
                                    )
                                }))
                        }))
                }),
        )
}

/// Renders a compact WinrateGraph from the current variation's persisted and
/// live analysis values. The point handlers route only node ids back to the
/// shell, keeping this view independent of engine sessions.
pub fn render_winrate_graph_panel(
    points: &[GraphPlotPoint],
    metric: WinrateGraphMetric,
    height: f32,
    palette: UiPalette,
    on_node_clicked: impl Fn(&sabaki_domain_core::NodeId, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let on_node_clicked = Rc::new(on_node_clicked);
    let has_values = points.iter().any(|point| point.y.is_some());
    let width = 236.0_f32;
    let graph_height = (height - 44.0).max(32.0);
    let last_index = points.len().saturating_sub(1).max(1) as f32;
    div()
        .id("winrate-graph-panel")
        .debug_selector(|| "winrate-graph-panel".to_owned())
        .flex_none()
        .h(px(height))
        .min_h_0()
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(palette.subtle))
                        .child(metric.label().to_uppercase()),
                )
                .children(has_values.then(|| {
                    div()
                        .text_xs()
                        .text_color(rgb(palette.accent))
                        .child(format!("{} points", points.len()))
                })),
        )
        .child(if has_values {
            div()
                .id("winrate-graph-plot")
                .relative()
                .w_full()
                .flex_1()
                .min_h_0()
                .rounded_md()
                .bg(rgb(palette.input))
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .w_full()
                        .h_full()
                        .children(points.iter().enumerate().map(|(index, _)| {
                            let target_index = graph_index_from_x(
                                (index as f32 + 0.5) * width / points.len().max(1) as f32,
                                width,
                                points.len(),
                            )
                            .expect("plot has at least one point");
                            let handler = on_node_clicked.clone();
                            let node_id = points[target_index].node_id.clone();
                            div()
                                .id(("winrate-scrub-cell", index))
                                .absolute()
                                .left(px(width * index as f32 / points.len().max(1) as f32))
                                .top_0()
                                .w(px(width / points.len().max(1) as f32))
                                .h_full()
                                .cursor_pointer()
                                .on_mouse_down(
                                    MouseButton::Left,
                                    move |_: &MouseDownEvent, window: &mut Window, cx: &mut App| {
                                        handler(&node_id, window, cx);
                                    },
                                )
                        })),
                )
                .children(points.iter().enumerate().filter_map(|(index, point)| {
                    let y = graph_height * point.y? as f32;
                    let x = width * index as f32 / last_index;
                    let color = if point.is_current || point.is_blunder {
                        palette.danger_text
                    } else {
                        palette.accent
                    };
                    let handler = on_node_clicked.clone();
                    let node_id = point.node_id.clone();
                    Some(
                        div()
                            .id(("winrate-point", index))
                            .debug_selector(move || format!("winrate-point-{index}"))
                            .absolute()
                            .left(px(x.max(0.0)))
                            .top(px(y.max(0.0)))
                            .size(px(if point.is_current { 8.0 } else { 6.0 }))
                            .rounded_full()
                            .bg(rgb(color))
                            .cursor_pointer()
                            .on_mouse_down(
                                MouseButton::Left,
                                move |_: &MouseDownEvent, window: &mut Window, cx: &mut App| {
                                    handler(&node_id, window, cx);
                                },
                            ),
                    )
                }))
        } else {
            div()
                .id("winrate-graph-empty")
                .debug_selector(|| "winrate-graph-empty".to_owned())
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .rounded_md()
                .bg(rgb(palette.input))
                .text_xs()
                .text_color(rgb(palette.subtle))
                .child("No winrate data on this variation")
        })
}

/// Renders the LizzieYZY-style blunder and mistake inspection panel in the right sidebar.
pub fn render_blunder_list_panel(
    blunders: &[sabaki_host::BlunderEntry],
    palette: UiPalette,
    cx: &Context<ShellApp>,
) -> Stateful<Div> {
    if blunders.is_empty() {
        return div()
            .id("blunder-list-panel")
            .debug_selector(|| "blunder-list-panel".to_owned());
    }

    div()
        .id("blunder-list-panel")
        .debug_selector(|| "blunder-list-panel".to_owned())
        .flex()
        .flex_col()
        .gap_1p5()
        .p_3()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(palette.subtle))
                        .child("BLUNDERS & MISTAKES"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(palette.danger_text))
                        .child(format!("{} found", blunders.len())),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .children(blunders.iter().map(|b| {
                    let node_id = b.node_id.clone();
                    let color_label = match b.player {
                        sabaki_domain_core::Color::Black => "Black",
                        sabaki_domain_core::Color::White => "White",
                    };
                    let vertex_label = b.played_vertex.as_deref().unwrap_or("Pass");
                    let drop_pct = (b.winrate_drop * 100.0).round();
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .px_2()
                        .py_1()
                        .rounded(px(4.0))
                        .bg(rgb(palette.input))
                        .hover(|style| style.bg(rgb(palette.button)))
                        .cursor_pointer()
                        .text_xs()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_1()
                                .child(b.grade.badge())
                                .child(
                                    div()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(rgb(palette.text))
                                        .child(format!(
                                            "#{}: {color_label} {vertex_label}",
                                            b.move_number
                                        )),
                                ),
                        )
                        .child(
                            div()
                                .text_color(rgb(palette.danger_text))
                                .child(format!("-{drop_pct:.0}%")),
                        )
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |shell, _, _, cx| {
                                shell.navigate_to_node(node_id.clone(), cx);
                            }),
                        )
                })),
        )
}

#[expect(
    clippy::too_many_arguments,
    reason = "P2 will replace direct ShellApp callbacks with a variation-tree controller"
)]
pub fn render_variation_tree_panel(
    layout: &VariationTreeLayout,
    grid_size: f32,
    node_size: f32,
    palette: UiPalette,
    shell: &ShellApp,
    cx: &Context<ShellApp>,
    on_node_clicked: impl Fn(&sabaki_domain_core::NodeId, &mut Window, &mut App) + 'static,
    on_node_context_requested: impl Fn(&sabaki_domain_core::NodeId, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id("variation-tree-panel")
        .debug_selector(|| "variation-tree-panel".to_owned())
        .relative()
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(palette.subtle))
                        .child("VARIATION TREE"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(palette.muted))
                        .child(format!("{} nodes", layout.nodes.len())),
                ),
        )
        .child(render_variation_tree(
            layout,
            grid_size,
            node_size,
            palette,
            on_node_clicked,
            on_node_context_requested,
        ))
        .child(
            if let Some(node_id) = shell.game_graph_context_node.as_ref() {
                render_game_graph_context_menu(node_id, shell, cx)
            } else {
                div().id("game-graph-context-menu-hidden")
            },
        )
}

/// Renders a node-specific GameGraph context menu. It deliberately has a small
/// command surface while native popup support remains outside GPUI 0.2.2.
fn render_game_graph_context_menu(
    node_id: &sabaki_domain_core::NodeId,
    shell: &ShellApp,
    cx: &Context<ShellApp>,
) -> Stateful<Div> {
    let hotspot_enabled = shell
        .host
        .snapshot()
        .nodes
        .iter()
        .find(|node| &node.id == node_id)
        .is_some_and(|node| node.properties.contains_key("HO"));
    div()
        .id("game-graph-context-menu")
        .debug_selector(|| "game-graph-context-menu".to_owned())
        .absolute()
        .top(px(32.0))
        .left(px(8.0))
        .w(px(176.0))
        .flex()
        .flex_col()
        .gap_1()
        .p_2()
        .border_1()
        .border_color(rgb(shell.palette.accent))
        .rounded(px(4.0))
        .bg(rgb(shell.palette.panel))
        .text_sm()
        .child(node_id.clone())
        .child(
            div()
                .id("game-graph-context-navigate")
                .debug_selector(|| "game-graph-context-navigate".to_owned())
                .px_2()
                .py_1()
                .cursor_pointer()
                .child("jump to node")
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(ShellApp::navigate_game_graph_context_node),
                ),
        )
        .child(
            div()
                .id("game-graph-context-hotspot")
                .debug_selector(|| "game-graph-context-hotspot".to_owned())
                .px_2()
                .py_1()
                .cursor_pointer()
                .child(if hotspot_enabled {
                    "remove hotspot"
                } else {
                    "add hotspot"
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(ShellApp::toggle_game_graph_context_hotspot),
                ),
        )
        .child(
            div()
                .id("game-graph-context-close")
                .px_2()
                .py_1()
                .cursor_pointer()
                .child("close")
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(ShellApp::close_game_graph_context_menu),
                ),
        )
}

/// Renders the Preferences drawer over the workspace. Settings ownership moves
/// out of the game/comment sidebar so the latter can stay focused on the game.
pub fn render_preferences_drawer(
    rows: &[SettingRow],
    shell: &ShellApp,
    cx: &Context<ShellApp>,
) -> Stateful<Div> {
    div()
        .id("preferences-drawer-overlay")
        .debug_selector(|| "preferences-drawer-overlay".to_owned())
        .absolute()
        .top(px(40.0))
        .left_0()
        .size_full()
        .bg(hsla(0.0, 0.0, 0.0, 0.45))
        .on_mouse_down(MouseButton::Left, cx.listener(ShellApp::close_drawer))
        .child(
            div()
                .id("preferences-drawer")
                .debug_selector(|| "preferences-drawer".to_owned())
                .absolute()
                .top_0()
                .right_0()
                .h_full()
                .w(px(380.0))
                .max_w_full()
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap_3()
                .p_4()
                .border_l_1()
                .border_color(rgb(shell.palette.border))
                .bg(rgb(shell.palette.panel))
                .on_mouse_down(MouseButton::Left, |_, _, _| {})
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .pb_2()
                        .border_b_1()
                        .border_color(rgb(shell.palette.border))
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_base()
                                .text_color(rgb(shell.palette.text))
                                .child("Preferences"),
                        )
                        .child(
                            div()
                                .id("preferences-close")
                                .debug_selector(|| "preferences-close".to_owned())
                                .px_3()
                                .py_1()
                                .border_1()
                                .border_color(rgb(shell.palette.border))
                                .rounded_md()
                                .bg(rgb(shell.palette.input))
                                .cursor_pointer()
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(rgb(shell.palette.text))
                                .hover(|style| style.bg(rgb(shell.palette.button)))
                                .child("✕ Close")
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(ShellApp::close_drawer),
                                ),
                        ),
                )
                .child(render_settings_panel(rows, shell, cx)),
        )
}

fn render_readonly_drawer(
    drawer_id: &'static str,
    drawer_index: usize,
    title: &'static str,
    content: Div,
    shell: &ShellApp,
    cx: &Context<ShellApp>,
) -> Stateful<Div> {
    div()
        .id(("drawer-overlay", drawer_index))
        .debug_selector(move || format!("{drawer_id}-drawer-overlay"))
        .absolute()
        .top(px(40.0))
        .left_0()
        .size_full()
        .bg(hsla(0.0, 0.0, 0.0, 0.45))
        .on_mouse_down(MouseButton::Left, cx.listener(ShellApp::close_drawer))
        .child(
            div()
                .id(("drawer", drawer_index))
                .debug_selector(move || format!("{drawer_id}-drawer"))
                .absolute()
                .top_0()
                .right_0()
                .h_full()
                .w(px(380.0))
                .max_w_full()
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap_3()
                .p_4()
                .border_l_1()
                .border_color(rgb(shell.palette.border))
                .bg(rgb(shell.palette.panel))
                .on_mouse_down(MouseButton::Left, |_, _, _| {})
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .pb_2()
                        .border_b_1()
                        .border_color(rgb(shell.palette.border))
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_base()
                                .text_color(rgb(shell.palette.text))
                                .child(title),
                        )
                        .child(
                            div()
                                .id(("drawer-close", drawer_index))
                                .debug_selector(move || format!("{drawer_id}-drawer-close"))
                                .px_3()
                                .py_1()
                                .border_1()
                                .border_color(rgb(shell.palette.border))
                                .rounded_md()
                                .bg(rgb(shell.palette.input))
                                .cursor_pointer()
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(rgb(shell.palette.text))
                                .hover(|style| style.bg(rgb(shell.palette.button)))
                                .child("✕ Close")
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(ShellApp::close_drawer),
                                ),
                        ),
                )
                .child(content),
        )
}

/// Shows non-editable game metadata from the host snapshot.
pub fn render_game_info_drawer(
    snapshot: &GameSnapshot,
    shell: &ShellApp,
    cx: &Context<ShellApp>,
) -> Stateful<Div> {
    let property = |key: &str| {
        snapshot
            .root_properties
            .get(key)
            .and_then(|values| values.first())
            .cloned()
            .unwrap_or_default()
    };
    let row = |label: &'static str, val: String| {
        div()
            .flex()
            .items_center()
            .justify_between()
            .p_2()
            .rounded_md()
            .bg(rgb(shell.palette.input))
            .border_1()
            .border_color(rgb(shell.palette.border))
            .child(
                div()
                    .font_weight(FontWeight::MEDIUM)
                    .text_xs()
                    .text_color(rgb(shell.palette.subtle))
                    .child(label),
            )
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_xs()
                    .text_color(rgb(shell.palette.text))
                    .child(if val.is_empty() { "-".to_owned() } else { val }),
            )
    };
    render_readonly_drawer(
        "game-info",
        1,
        "Game Information",
        div()
            .flex()
            .flex_col()
            .gap_1p5()
            .child(row(
                "Board Size",
                format!("{} × {}", snapshot.board.width, snapshot.board.height),
            ))
            .child(row("Total Moves", format!("{}", snapshot.moves.len())))
            .child(row("Komi", property("KM")))
            .child(row("Black Player", property("PB")))
            .child(row("White Player", property("PW")))
            .child(row("Result", property("RE")))
            .child(row(
                "Source File",
                snapshot
                    .file_state
                    .path
                    .as_deref()
                    .unwrap_or("Unsaved Game")
                    .to_owned(),
            )),
        shell,
        cx,
    )
}

/// Shows the current deterministic scoring result, KataGo territory breakdown, and override count.
pub fn render_score_drawer(
    snapshot: &GameSnapshot,
    shell: &ShellApp,
    cx: &Context<ShellApp>,
) -> Stateful<Div> {
    let komi = snapshot
        .root_properties
        .get("KM")
        .and_then(|v| v.first())
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(7.5);

    let scoring_result = sabaki_domain_core::scoring::score_board(
        &snapshot.board,
        Some(komi),
        &snapshot.score_overrides,
    );

    let katago_estimate = shell
        .analysis
        .first()
        .and_then(|e| e.ownership.as_deref())
        .and_then(|ownership| {
            sabaki_host::estimate_territory(
                ownership,
                scoring_result.black_captured,
                scoring_result.white_captured,
                komi,
                0.50,
            )
        });

    render_readonly_drawer(
        "score",
        2,
        "Score & Territory Estimate",
        div()
            .flex()
            .flex_col()
            .gap_2()
            .children(katago_estimate.as_ref().map(|est| {
                div()
                    .p_3()
                    .rounded_lg()
                    .bg(rgb(shell.palette.input))
                    .border_1()
                    .border_color(rgb(shell.palette.accent))
                    .flex()
                    .flex_col()
                    .gap_1p5()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(shell.palette.accent))
                            .child("KATAGO AI 形势判断 (领地热力图估算)"),
                    )
                    .child(
                        div()
                            .text_base()
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(shell.palette.text))
                            .child(est.summary_text()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(shell.palette.muted))
                            .child(format!(
                                "黑方: 盘面领地 {:.1} 目 + 提子 {} = {:.1} 目",
                                est.black_territory, est.black_prisoners, est.black_total
                            )),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(shell.palette.muted))
                            .child(format!(
                                "白方: 盘面领地 {:.1} 目 + 提子 {} + 贴目 {:.1} = {:.1} 目",
                                est.white_territory, est.white_prisoners, est.komi, est.white_total
                            )),
                    )
            }))
            .child(
                div()
                    .p_3()
                    .rounded_lg()
                    .bg(rgb(shell.palette.input))
                    .border_1()
                    .border_color(rgb(shell.palette.border))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(shell.palette.subtle))
                            .child("RULES ESTIMATE (规则点目)"),
                    )
                    .child(
                        div()
                            .text_base()
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(shell.palette.text))
                            .child(crate::markup::scoring_summary(snapshot)),
                    ),
            )
            .child(
                div()
                    .p_2p5()
                    .rounded_md()
                    .bg(rgb(shell.palette.input))
                    .border_1()
                    .border_color(rgb(shell.palette.border))
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_xs()
                    .child(
                        div()
                            .text_color(rgb(shell.palette.subtle))
                            .child("Manual dead/alive overrides"),
                    )
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(shell.palette.text))
                            .child(format!("{}", snapshot.score_overrides.len())),
                    ),
            )
            .child(
                div()
                    .p_2()
                    .text_xs()
                    .text_color(rgb(shell.palette.muted))
                    .child("Tip: switch to Scoring mode on the board to click and toggle stone group status."),
            ),
        shell,
        cx,
    )
}

/// Shows the native client's build and architecture identity.
pub fn render_about_drawer(shell: &ShellApp, cx: &Context<ShellApp>) -> Stateful<Div> {
    render_readonly_drawer(
        "about",
        3,
        "About Saba.rs",
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .p_3()
                    .rounded_lg()
                    .bg(rgb(shell.palette.input))
                    .border_1()
                    .border_color(rgb(shell.palette.border))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .font_weight(FontWeight::BOLD)
                            .text_sm()
                            .text_color(rgb(shell.palette.accent))
                            .child("Saba.rs"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(shell.palette.muted))
                            .child("An elegant Go board and SGF editor built with Rust & GPUI."),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(shell.palette.subtle))
                    .child("Native GPUI implementation with full SGF fidelity, multi-engine GTP support, and plugin ecosystem."),
            ),
        shell,
        cx,
    )
}

/// Renders the vertical splitter between the roster and the GTP console.
pub fn render_peer_list_split_handle(palette: UiPalette, cx: &Context<ShellApp>) -> Stateful<Div> {
    div()
        .id("peer-list-splitter")
        .debug_selector(|| "peer-list-splitter".to_owned())
        .flex_none()
        .w_full()
        .h(px(5.0))
        .bg(rgb(palette.border))
        .cursor_row_resize()
        .hover(|style| style.bg(rgb(palette.accent)))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|shell, event: &MouseDownEvent, _window: &mut Window, cx| {
                shell.begin_split_drag(SplitPane::PeerList, f32::from(event.position.y), cx);
            }),
        )
}

/// Renders one vertically draggable right-sidebar divider.
pub fn render_right_sidebar_split_handle(
    pane: SplitPane,
    palette: UiPalette,
    cx: &Context<ShellApp>,
) -> Stateful<Div> {
    let selector = pane.debug_selector();
    div()
        .id(selector)
        .debug_selector(move || selector.to_owned())
        .flex_none()
        .w_full()
        .h(px(5.0))
        .bg(rgb(palette.border))
        .cursor_row_resize()
        .hover(|style| style.bg(rgb(palette.accent)))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(
                move |shell, event: &MouseDownEvent, _window: &mut Window, cx| {
                    shell.begin_split_drag(pane, f32::from(event.position.y), cx);
                },
            ),
        )
}

/// The upper half of the engines sidebar. It mirrors Sabaki's `PeerList`:
/// configured engines can be selected, attached, and assigned analysis/black/
/// white roles. Role selection is explicit even though this client currently
/// owns one process session at a time.
pub fn render_engine_roster_panel(shell: &ShellApp, cx: &Context<ShellApp>) -> Stateful<Div> {
    div()
        .id("engine-roster")
        .debug_selector(|| "engine-roster".to_owned())
        .size_full()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(shell.palette.subtle))
                        .child("ENGINES ROSTER"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(shell.palette.muted))
                        .child(format!("{} configured", shell.engine_store.list().len())),
                ),
        )
        .child(div().flex().flex_col().gap_1().children(
            shell.engine_store.list().iter().map(|record| {
                let name = record.name.clone();
                let selected_role = shell
                    .active_console_role
                    .filter(|role| shell.engine_roles.get(*role) == Some(name.as_str()));
                let connected =
                    selected_role.is_some_and(|role| shell.engine_controller.is_attached(role));
                let selected = selected_role.is_some();
                let roles = [
                    crate::engine_console::EngineRole::Analysis,
                    crate::engine_console::EngineRole::Black,
                    crate::engine_console::EngineRole::White,
                ];
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .p_2()
                    .border_1()
                    .border_color(rgb(if selected {
                        shell.palette.accent
                    } else {
                        shell.palette.border
                    }))
                    .rounded_md()
                    .bg(rgb(if selected {
                        shell.palette.button_active
                    } else {
                        shell.palette.input
                    }))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .child(
                                        div()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_xs()
                                            .text_color(rgb(shell.palette.text))
                                            .child(record.name.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(rgb(shell.palette.muted))
                                            .child(format!("({})", record.path)),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .child(
                                        div()
                                            .px_1()
                                            .py_0p5()
                                            .rounded_sm()
                                            .bg(rgb(if connected {
                                                shell.palette.success
                                            } else {
                                                shell.palette.button
                                            }))
                                            .text_xs()
                                            .text_color(rgb(if connected {
                                                0xffffff
                                            } else {
                                                shell.palette.muted
                                            }))
                                            .child(if connected { "attached" } else { "detached" }),
                                    )
                                    .children((!connected).then(|| {
                                        let connect_name = name.clone();
                                        div()
                                            .px_2()
                                            .py_0p5()
                                            .rounded_md()
                                            .cursor_pointer()
                                            .border_1()
                                            .border_color(rgb(shell.palette.accent))
                                            .bg(rgb(shell.palette.button))
                                            .text_xs()
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(rgb(shell.palette.text))
                                            .hover(|style| style.bg(rgb(shell.palette.button_active)))
                                            .child("attach")
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(move |shell,
                                                                  _: &MouseDownEvent,
                                                                  _: &mut Window,
                                                                  cx: &mut Context<ShellApp>| {
                                                    let _ = &connect_name;
                                                    let role = shell.active_console_role.unwrap_or(
                                                        crate::engine_console::EngineRole::Analysis,
                                                    );
                                                    shell.on_engine_connect(role, cx);
                                                }),
                                            )
                                    }))
                                    .child({
                                        let remove_name = name.clone();
                                        div()
                                            .px_2()
                                            .py_0p5()
                                            .rounded_md()
                                            .cursor_pointer()
                                            .border_1()
                                            .border_color(rgb(shell.palette.danger_text))
                                            .bg(rgb(shell.palette.danger))
                                            .text_xs()
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(rgb(shell.palette.danger_text))
                                            .child("remove")
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(move |shell,
                                                                  _: &MouseDownEvent,
                                                                  _: &mut Window,
                                                                  cx: &mut Context<ShellApp>| {
                                                    shell.on_engine_remove(&remove_name, cx);
                                                }),
                                            )
                                    }),
                            ),
                    )
                    .child(div().flex().gap_1().children(roles.into_iter().map(|role| {
                        let role_name = name.clone();
                        let active = shell.engine_roles.get(role) == Some(name.as_str());
                        div()
                            .px_2()
                            .py_0p5()
                            .rounded_md()
                            .cursor_pointer()
                            .border_1()
                            .border_color(rgb(if active {
                                shell.palette.accent
                            } else {
                                shell.palette.border
                            }))
                            .bg(rgb(if active {
                                shell.palette.button_active
                            } else {
                                shell.palette.button
                            }))
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(if active {
                                shell.palette.text
                            } else {
                                shell.palette.muted
                            }))
                            .child(if active && shell.engine_controller.is_attached(role) {
                                format!("{} ●", role.label())
                            } else if active {
                                format!("{} ○", role.label())
                            } else {
                                role.label().to_owned()
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |shell,
                                                  _: &MouseDownEvent,
                                                  _: &mut Window,
                                                  cx: &mut Context<ShellApp>| {
                                    shell.on_engine_role_toggled(role, &role_name, cx);
                                }),
                            )
                    })))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(
                            move |shell,
                                  event: &MouseDownEvent,
                                  window: &mut Window,
                                  cx: &mut Context<ShellApp>| {
                                shell.on_engine_selected(&name, event, window, cx);
                            },
                        ),
                    )
            }),
        ))
        .child(
            div()
                .track_focus(&shell.engine_spec_focus_handle)
                .px_3()
                .py_1()
                .border_1()
                .border_color(rgb(shell.palette.border))
                .rounded_md()
                .bg(rgb(shell.palette.input))
                .text_xs()
                .text_color(rgb(shell.palette.text))
                .child(if shell.engine_spec_draft.is_empty() {
                    "Add engine: Name | path | args | commands".to_owned()
                } else {
                    shell.engine_spec_draft.to_string()
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(ShellApp::on_engine_spec_focus),
                )
                .on_key_down(cx.listener(ShellApp::on_engine_spec_key_down)),
        )
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap_1()
                .child(
                    div()
                        .px_3()
                        .py_1()
                        .rounded_md()
                        .cursor_pointer()
                        .border_1()
                        .border_color(rgb(shell.palette.accent))
                        .bg(rgb(shell.palette.button))
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(shell.palette.text))
                        .hover(|style| style.bg(rgb(shell.palette.button_active)))
                        .child("analyze")
                        .on_mouse_down(MouseButton::Left, cx.listener(ShellApp::on_analyze)),
                )
                .children(shell.analysis_task.is_some().then(|| {
                    div()
                        .px_3()
                        .py_1()
                        .rounded_md()
                        .cursor_pointer()
                        .border_1()
                        .border_color(rgb(shell.palette.danger_text))
                        .bg(rgb(shell.palette.danger))
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(shell.palette.danger_text))
                        .child("stop")
                        .on_mouse_down(MouseButton::Left, cx.listener(ShellApp::on_analysis_stop))
                }))
                .children((shell.engine_controller.any_attached()).then(|| {
                    div()
                        .px_3()
                        .py_1()
                        .rounded_md()
                        .cursor_pointer()
                        .border_1()
                        .border_color(rgb(shell.palette.accent))
                        .bg(rgb(shell.palette.button))
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(shell.palette.text))
                        .hover(|style| style.bg(rgb(shell.palette.button_active)))
                        .child("engine move")
                        .on_mouse_down(MouseButton::Left, cx.listener(ShellApp::on_engine_move))
                }))
                .children((shell.engine_controller.any_attached()).then(|| {
                    div()
                        .px_3()
                        .py_1()
                        .rounded_md()
                        .cursor_pointer()
                        .border_1()
                        .border_color(rgb(shell.palette.danger_text))
                        .bg(rgb(shell.palette.danger))
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(shell.palette.danger_text))
                        .child("detach")
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(
                                |shell,
                                 event: &MouseDownEvent,
                                 window: &mut Window,
                                 cx: &mut Context<ShellApp>| {
                                    if let Some(role) = shell.active_console_role {
                                        shell.on_engine_disconnect(role, event, window, cx);
                                    }
                                },
                            ),
                        )
                })),
        )
}

pub fn render_gtp_console_panel(shell: &ShellApp, cx: &Context<ShellApp>) -> Stateful<Div> {
    let selected = shell
        .active_console_role
        .and_then(|role| {
            shell
                .engine_roles
                .get(role)
                .map(|name| format!("{} · {name}", role.label()))
        })
        .unwrap_or_else(|| "no engine role selected".to_owned());
    div()
        .id("gtp-console")
        .debug_selector(|| "gtp-console".to_owned())
        .size_full()
        .min_h_0()
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(shell.palette.subtle))
                        .child("GTP CONSOLE"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(shell.palette.accent))
                        .child(selected.clone()),
                ),
        )
        .child(
            if shell
                .settings
                .get_bool("gtp.console_log_enabled")
                .unwrap_or(true)
            {
                div()
                    .id("gtp-transcript")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_2()
                    .rounded_md()
                    .bg(rgb(shell.palette.input))
                    .border_1()
                    .border_color(rgb(shell.palette.border))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .text_xs()
                    .children(shell.engine_log.iter().map(|entry| {
                        let color = if entry.success {
                            rgb(shell.palette.success)
                        } else {
                            rgb(shell.palette.danger_text)
                        };
                        div()
                            .text_color(color)
                            .child(format!("{} → {}", entry.command, entry.response))
                    }))
            } else {
                div()
                    .id("gtp-transcript-disabled")
                    .flex_1()
                    .text_xs()
                    .text_color(rgb(shell.palette.subtle))
                    .child("GTP console logging disabled")
            },
        )
        .child(
            div()
                .track_focus(&shell.engine_input_focus_handle)
                .px_3()
                .py_1p5()
                .border_1()
                .border_color(rgb(shell.palette.accent))
                .rounded_md()
                .bg(rgb(shell.palette.input))
                .text_color(rgb(shell.palette.text))
                .text_xs()
                .child(if shell.gtp_input.text().is_empty() {
                    format!("{selected}>")
                } else {
                    shell.gtp_input.text().to_owned()
                })
                .child(NativeInputBinding::new(
                    shell.engine_input_focus_handle.clone(),
                    cx.entity().clone(),
                ))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(ShellApp::on_engine_input_focus),
                )
                .on_key_down(cx.listener(ShellApp::on_engine_key_down)),
        )
}

/// The pull-up bottom GTP terminal drawer, toggled from the bottom player bar.
pub fn render_gtp_terminal_drawer(shell: &ShellApp, cx: &Context<ShellApp>) -> Stateful<Div> {
    let selected_role = shell
        .active_console_role
        .unwrap_or(crate::engine_console::EngineRole::Analysis);
    let is_attached = shell.engine_controller.is_attached(selected_role);
    let assigned_name = shell.engine_roles.get(selected_role);
    let selected_label = assigned_name
        .map(|name| format!("{} · {name}", selected_role.label()))
        .unwrap_or_else(|| format!("{} (未配置引擎)", selected_role.label()));

    div()
        .id("gtp-terminal-drawer")
        .debug_selector(|| "gtp-terminal-drawer".to_owned())
        .absolute()
        .bottom(px(42.0))
        .left(px(16.0))
        .right(px(16.0))
        .flex()
        .justify_center()
        .child(
            div()
                .w_full()
                .max_w(px(780.0))
                .h(px(320.0))
                .flex()
                .flex_col()
                .p_3()
                .rounded_lg()
                .border_1()
                .border_color(rgb(shell.palette.accent))
                .bg(rgb(0x18181c))
                .shadow_lg()
                .child(
                    // Header: Title + Role selection tabs + Connect/Disconnect + Clear + Close
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .pb_2()
                        .border_b_1()
                        .border_color(rgb(0x2a2a30))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(rgb(0xf2f2f5))
                                        .child("💻 GTP 终端"),
                                )
                                // Role tabs
                                .children([
                                    crate::engine_console::EngineRole::Analysis,
                                    crate::engine_console::EngineRole::Black,
                                    crate::engine_console::EngineRole::White,
                                ].into_iter().map(|role| {
                                    let is_active = shell.active_console_role == Some(role)
                                        || (shell.active_console_role.is_none() && role == crate::engine_console::EngineRole::Analysis);
                                    let assigned = shell.engine_roles.get(role);
                                    let attached = shell.engine_controller.is_attached(role);
                                    div()
                                        .cursor_pointer()
                                        .px_2()
                                        .py_0p5()
                                        .rounded_md()
                                        .border_1()
                                        .border_color(if is_active { rgb(shell.palette.accent) } else { rgb(0x3a3a42) })
                                        .bg(if is_active { rgb(0x262630) } else { rgb(0x1e1e24) })
                                        .text_xs()
                                        .text_color(if is_active { rgb(0x8ec5ff) } else { rgb(0x9a9a9a) })
                                        .child(format!(
                                            "{} {}{}",
                                            if attached { "●" } else { "○" },
                                            role.label(),
                                            assigned.map(|n| {
                                                let clean = n.trim_matches('(').trim_matches(')');
                                                format!(": {clean}")
                                            }).unwrap_or_default()
                                        ))
                                        .on_mouse_down(MouseButton::Left, cx.listener(move |shell, _, _, cx| {
                                            shell.active_console_role = Some(role);
                                            cx.notify();
                                        }))
                                }))
                                .child(
                                    div()
                                        .cursor_pointer()
                                        .px_2()
                                        .py_0p5()
                                        .rounded_md()
                                        .border_1()
                                        .border_color(if is_attached { rgb(shell.palette.danger_text) } else { rgb(shell.palette.accent) })
                                        .bg(if is_attached { rgb(shell.palette.danger) } else { rgb(shell.palette.button) })
                                        .text_xs()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(if is_attached { rgb(shell.palette.danger_text) } else { rgb(shell.palette.accent) })
                                        .hover(|style| style.bg(rgb(shell.palette.button_active)))
                                        .child(if is_attached { "⏹ 断开引擎" } else { "🔌 连接引擎" })
                                        .on_mouse_down(MouseButton::Left, cx.listener(move |shell, event, window, cx| {
                                            if shell.engine_controller.is_attached(selected_role) {
                                                shell.on_engine_disconnect(selected_role, event, window, cx);
                                            } else {
                                                shell.on_engine_connect(selected_role, cx);
                                            }
                                        }))
                                )
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .cursor_pointer()
                                        .text_xs()
                                        .text_color(rgb(0x9a9a9a))
                                        .hover(|style| style.text_color(rgb(0xf2f2f5)))
                                        .child("清空日志")
                                        .on_mouse_down(MouseButton::Left, cx.listener(|shell, _, _, cx| {
                                            shell.engine_log.clear();
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    div()
                                        .cursor_pointer()
                                        .text_xs()
                                        .text_color(rgb(0x9a9a9a))
                                        .hover(|style| style.text_color(rgb(0xf2f2f5)))
                                        .child("✕ 关闭")
                                        .on_mouse_down(MouseButton::Left, cx.listener(ShellApp::toggle_gtp_terminal)),
                                )
                        )
                )
                .child(
                    // Transcript body
                    div()
                        .id("gtp-transcript")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .my_2()
                        .p_2()
                        .rounded_md()
                        .bg(rgb(0x121214))
                        .border_1()
                        .border_color(rgb(0x26262c))
                        .flex()
                        .flex_col()
                        .gap_1()
                        .text_xs()
                        .children(if shell.engine_log.is_empty() {
                            vec![
                                div()
                                    .text_color(rgb(0x6e6e78))
                                    .child(format!("GTP 终端就绪 [{selected_label}]。在下方输入框中输入指令或点击快捷操作。"))
                            ]
                        } else {
                            shell.engine_log.iter().map(|entry| {
                                let color = if entry.success {
                                    rgb(0x34c759)
                                } else {
                                    rgb(0xff453a)
                                };
                                div()
                                    .flex()
                                    .gap_2()
                                    .child(div().text_color(rgb(0x8ec5ff)).child(format!("> {}", entry.command)))
                                    .child(div().text_color(color).child(format!("= {}", entry.response)))
                            }).collect()
                        })
                )
                // Quick commands + Input row
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1p5()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_1p5()
                                .child(
                                    div()
                                        .cursor_pointer()
                                        .px_2()
                                        .py_0p5()
                                        .rounded_sm()
                                        .bg(rgb(0x24242c))
                                        .border_1()
                                        .border_color(rgb(shell.palette.accent))
                                        .text_xs()
                                        .text_color(rgb(0x8ec5ff))
                                        .hover(|style| style.bg(rgb(0x30303c)).text_color(rgb(0xffffff)))
                                        .child(if shell.analysis_task.is_some() { "⏹ 停止分析" } else { "⚡ 启动分析" })
                                        .on_mouse_down(MouseButton::Left, cx.listener(|shell, _, _, cx| {
                                            if shell.analysis_task.is_some() {
                                                shell.stop_analysis(cx);
                                            } else {
                                                shell.start_analysis(cx);
                                            }
                                        }))
                                )
                                .child(
                                    div()
                                        .cursor_pointer()
                                        .px_2()
                                        .py_0p5()
                                        .rounded_sm()
                                        .bg(rgb(0x24242c))
                                        .border_1()
                                        .border_color(rgb(0x383844))
                                        .text_xs()
                                        .text_color(rgb(0xd0d0d8))
                                        .hover(|style| style.bg(rgb(0x30303c)).text_color(rgb(0xffffff)))
                                        .child("🎲 AI落子 (genmove)")
                                        .on_mouse_down(MouseButton::Left, cx.listener(|shell, _, _, cx| {
                                            shell.generate_engine_move(cx);
                                        }))
                                )
                                .child(
                                    div()
                                        .cursor_pointer()
                                        .px_2()
                                        .py_0p5()
                                        .rounded_sm()
                                        .bg(rgb(0x24242c))
                                        .border_1()
                                        .border_color(rgb(0x383844))
                                        .text_xs()
                                        .text_color(rgb(0xd0d0d8))
                                        .hover(|style| style.bg(rgb(0x30303c)).text_color(rgb(0xffffff)))
                                        .child("🔄 清空棋盘")
                                        .on_mouse_down(MouseButton::Left, cx.listener(|shell, _, _, cx| {
                                            shell.send_engine_command("clear_board", cx);
                                        }))
                                )
                                .child(
                                    div()
                                        .cursor_pointer()
                                        .px_2()
                                        .py_0p5()
                                        .rounded_sm()
                                        .bg(rgb(0x24242c))
                                        .border_1()
                                        .border_color(rgb(0x383844))
                                        .text_xs()
                                        .text_color(rgb(0xd0d0d8))
                                        .hover(|style| style.bg(rgb(0x30303c)).text_color(rgb(0xffffff)))
                                        .child("📋 列出指令")
                                        .on_mouse_down(MouseButton::Left, cx.listener(|shell, _, _, cx| {
                                            shell.send_engine_command("list_commands", cx);
                                        }))
                                )
                        )
                        .child(
                            div()
                                .flex()
                                .gap_2()
                                .child(
                                    div()
                                        .flex_1()
                                        .track_focus(&shell.engine_input_focus_handle)
                                        .px_3()
                                        .py_1p5()
                                        .rounded_md()
                                        .border_1()
                                        .border_color(rgb(shell.palette.accent))
                                        .bg(rgb(0x121214))
                                        .text_xs()
                                        .text_color(rgb(0xf5f5f7))
                                        .child(if shell.gtp_input.text().is_empty() {
                                            "输入 GTP 指令 (如: name, genmove, kata-analyze, boardsize 19 等)，按 Enter 发送".to_owned()
                                        } else {
                                            shell.gtp_input.text().to_owned()
                                        })
                                        .child(NativeInputBinding::new(
                                            shell.engine_input_focus_handle.clone(),
                                            cx.entity().clone(),
                                        ))
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(ShellApp::on_engine_input_focus),
                                        )
                                        .on_key_down(cx.listener(ShellApp::on_engine_key_down)),
                                )
                                .child(
                                    div()
                                        .cursor_pointer()
                                        .px_3()
                                        .py_1p5()
                                        .rounded_md()
                                        .bg(rgb(shell.palette.button))
                                        .border_1()
                                        .border_color(rgb(shell.palette.border))
                                        .text_xs()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(rgb(shell.palette.text))
                                        .hover(|style| style.bg(rgb(shell.palette.button_active)))
                                        .child("发送 ↵")
                                        .on_mouse_down(MouseButton::Left, cx.listener(|shell, _, _, cx| {
                                            let draft = shell.gtp_input.text().to_owned();
                                            shell.send_engine_command(&draft, cx);
                                            shell.gtp_input.set_text("");
                                            cx.notify();
                                        })),
                                )
                        )
                )
        )
}

/// The M2 left sidebar combines Sabaki's upper engine roster and lower GTP
/// transcript with a persisted vertical split.
pub fn render_left_engine_sidebar(shell: &ShellApp, cx: &Context<ShellApp>) -> Stateful<Div> {
    div()
        .id("engine-sidebar")
        .debug_selector(|| "engine-sidebar".to_owned())
        .size_full()
        .min_h_0()
        .flex()
        .flex_col()
        .child(
            div()
                .flex_none()
                .h(px(shell.peer_list_height))
                .min_h_0()
                .child(render_engine_roster_panel(shell, cx)),
        )
        .child(render_peer_list_split_handle(shell.palette, cx))
        .child(
            div()
                .flex_1()
                .min_h_0()
                .pt_2()
                .child(render_gtp_console_panel(shell, cx)),
        )
}

/// Legacy combined engine panel retained temporarily for downstream UI callers.
#[allow(dead_code)]
pub fn render_engine_panel(shell: &ShellApp, cx: &Context<ShellApp>) -> Stateful<Div> {
    div()
        .id("engine-panel")
        .debug_selector(|| "engine-panel".to_owned())
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .border_1()
        .border_color(rgb(shell.palette.border))
        .rounded(px(6.0))
        .bg(rgb(shell.palette.panel))
        .text_base()
        .child("engine console")
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .text_sm()
                .child("engines")
                .child(div().flex().flex_col().gap_1().children(
                    shell.engine_store.list().iter().map(|record| {
                        let name = record.name.clone();
                        let connected = shell.engine_controller.any_attached();
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(format!("{} ({})", record.name, record.path))
                            .child(if connected {
                                div()
                                    .text_color(rgb(shell.palette.success))
                                    .child("connected")
                            } else {
                                let connect_name = name.clone();
                                div()
                                        .px_2()
                                        .py_1()
                                        .border_1()
                                        .border_color(rgb(shell.palette.accent))
                                        .rounded(px(4.0))
                                        .bg(rgb(shell.palette.button))
                                        .child("connect")
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(move |shell,
                                                        _: &MouseDownEvent,
                                                        _: &mut Window,
                                                        cx: &mut Context<ShellApp>| {
                                                let _ = &connect_name;
                                                let role = shell.active_console_role.unwrap_or(
                                                    crate::engine_console::EngineRole::Analysis,
                                                );
                                                shell.on_engine_connect(role, cx);
                                            }),
                                        )
                            })
                            .child({
                                let remove_name = name.clone();
                                div()
                                        .px_2()
                                        .py_1()
                                        .border_1()
                                        .border_color(rgb(shell.palette.danger_text))
                                        .rounded(px(4.0))
                                        .bg(rgb(shell.palette.danger))
                                        .child("remove")
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(move |shell,
                                                        _: &MouseDownEvent,
                                                        _: &mut Window,
                                                        cx: &mut Context<ShellApp>| {
                                                shell.on_engine_remove(&remove_name, cx);
                                            }),
                                        )
                            })
                    }),
                ))
                .child(
                    div()
                        .track_focus(&shell.engine_spec_focus_handle)
                        .px_2()
                        .py_1()
                        .border_1()
                        .border_color(rgb(shell.palette.accent))
                        .rounded(px(4.0))
                        .bg(rgb(shell.palette.input))
                        .text_color(rgb(shell.palette.text))
                        .child(if shell.engine_spec_draft.is_empty() {
                            "Name | path | args | commands".to_owned()
                        } else {
                            shell.engine_spec_draft.to_string()
                        })
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(ShellApp::on_engine_spec_focus),
                        )
                        .on_key_down(cx.listener(ShellApp::on_engine_spec_key_down)),
                )
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            div()
                                .px_2()
                                .py_1()
                                .border_1()
                                .border_color(rgb(shell.palette.accent))
                                .rounded(px(4.0))
                                .bg(rgb(shell.palette.button))
                                .child("analyze")
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(ShellApp::on_analyze),
                                ),
                        )
                        .child(if shell.analysis_task.is_some() {
                            div()
                                .px_2()
                                .py_1()
                                .border_1()
                                .border_color(rgb(shell.palette.danger_text))
                                .rounded(px(4.0))
                                .bg(rgb(shell.palette.danger))
                                .child("stop analysis")
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(ShellApp::on_analysis_stop),
                                )
                        } else {
                            div()
                        })
                        .child(if shell.engine_controller.any_attached() {
                            div()
                                .flex()
                                .gap_2()
                                .child(
                                    div()
                                        .px_2()
                                        .py_1()
                                        .border_1()
                                        .border_color(rgb(shell.palette.accent))
                                        .rounded(px(4.0))
                                        .bg(rgb(shell.palette.button))
                                        .child("engine move")
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(ShellApp::on_engine_move),
                                        ),
                                )
                                .child(
                                    div()
                                        .px_2()
                                        .py_1()
                                        .border_1()
                                        .border_color(rgb(shell.palette.danger_text))
                                        .rounded(px(4.0))
                                        .bg(rgb(shell.palette.danger))
                                        .child("disconnect")
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(|shell,
                                                         event: &MouseDownEvent,
                                                         window: &mut Window,
                                                         cx: &mut Context<ShellApp>| {
                                                if let Some(role) = shell.active_console_role {
                                                    shell.on_engine_disconnect(role, event, window, cx);
                                                }
                                            }),
                                        ),
                                )
                        } else {
                            div()
                        }),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .p_2()
                .rounded(px(4.0))
                .bg(rgb(shell.palette.input))
                .border_1()
                .border_color(rgb(shell.palette.border))
                .text_sm()
                .child(if shell.analysis_task.is_some() {
                    "LIVE KATAGO ANALYSIS · searching"
                } else if shell.analysis.is_empty() {
                    "ANALYSIS · connect an Analysis engine to start"
                } else {
                    "ANALYSIS · latest candidates"
                })
                .child(if shell.analysis.is_empty() {
                    div()
                        .text_xs()
                        .text_color(rgb(shell.palette.subtle))
                        .child("分析结果会显示在棋盘推荐点、此候选列表和胜率图中。")
                } else {
            div()
                .flex()
                .flex_col()
                .gap_1()
                .text_sm()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .children(shell.analysis.iter().take(3).map(|entry| {
                            let winrate_percent = (entry.winrate * 100.0) as i64;
                            div().child(format!(
                                "{} {}v {}% {}",
                                entry.vertex.as_deref().unwrap_or("?"),
                                entry.visits,
                                winrate_percent,
                                entry
                                    .score_lead
                                    .map(|lead| format!("{:+.1}", lead))
                                    .unwrap_or_default()
                            ))
                        })),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child("black")
                        .child(
                            div()
                                .flex()
                                .w(px(140.0))
                                .h(px(10.0))
                                .rounded(px(2.0))
                                .bg(rgb(shell.palette.track))
                                .child(
                                    div()
                                        .h_full()
                                        .w(px(140.0
                                            * best_analysis_winrate(
                                                &shell.analysis,
                                                shell.host.snapshot().board.next_player,
                                            ) as f32))
                                        .bg(rgb(shell.palette.text)),
                                ),
                        )
                        .child("white"),
                )
                })
        )
        .child(
            if shell
                .settings
                .get_bool("gtp.console_log_enabled")
                .unwrap_or(true)
            {
                div().flex().flex_col().gap_1().text_sm().child(
                    div().flex().flex_col().gap_1().children(
                        shell.engine_log.iter().rev().take(8).map(|entry| {
                            let color = if entry.success {
                                rgb(shell.palette.success)
                            } else {
                                rgb(shell.palette.danger_text)
                            };
                            div()
                                .text_color(color)
                                .child(format!("{} → {}", entry.command, entry.response))
                        }),
                    ),
                )
            } else {
                div()
                    .text_sm()
                    .text_color(rgb(shell.palette.subtle))
                    .child("GTP console logging disabled")
            },
        )
        .child(
            div()
                .track_focus(&shell.engine_input_focus_handle)
                .px_2()
                .py_1()
                .border_1()
                .border_color(rgb(shell.palette.accent))
                .rounded(px(4.0))
                .bg(rgb(shell.palette.input))
                .text_color(rgb(shell.palette.text))
                .child(shell.engine_draft.clone().to_string())
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(ShellApp::on_engine_input_focus),
                )
                .on_key_down(cx.listener(ShellApp::on_engine_key_down)),
        )
}

/// The node inspector panel: title, comment editor, property table and
/// variation actions.
pub fn render_node_inspector_panel(
    metadata: &NodeInspectorMetadata,
    shell: &ShellApp,
    cx: &Context<ShellApp>,
) -> Stateful<Div> {
    div()
        .id("node-inspector-panel")
        .debug_selector(|| "node-inspector-panel".to_owned())
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(shell.palette.subtle))
                        .child("NODE INSPECTOR"),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .px_2()
                        .py_0p5()
                        .rounded_sm()
                        .bg(rgb(shell.palette.input))
                        .text_xs()
                        .text_color(rgb(shell.palette.accent))
                        .child("✏️"),
                ),
        )
        .child(
            div()
                .track_focus(&shell.node_title_focus_handle)
                .px_3()
                .py_1p5()
                .border_1()
                .border_color(rgb(shell.palette.border))
                .rounded_md()
                .bg(rgb(shell.palette.input))
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(shell.palette.text))
                .child(if shell.node_title_input.text().is_empty() {
                    metadata.title.clone()
                } else {
                    shell.node_title_input.text().to_owned()
                })
                .on_mouse_down(MouseButton::Left, cx.listener(ShellApp::on_node_title_focus))
                .on_key_down(cx.listener(ShellApp::on_node_title_key_down))
                .child(NativeInputBinding::new(
                    shell.node_title_focus_handle.clone(),
                    cx.entity(),
                )),
        )
        .child(
            if shell
                .settings
                .get_bool("view.show_comments")
                .unwrap_or(true)
            {
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(shell.palette.subtle))
                            .child("Comment"),
                    )
                    .child(
                        div()
                            .track_focus(&shell.comment_focus_handle)
                            .px_3()
                            .py_2()
                            .border_1()
                            .border_color(rgb(shell.palette.border))
                            .rounded_md()
                            .bg(rgb(shell.palette.input))
                            .text_color(rgb(shell.palette.text))
                            .text_xs()
                            .child(if shell.comment_input.text().is_empty() {
                                if metadata.comment.is_empty() {
                                    "No comment on this move. Click to add a note...".to_owned()
                                } else {
                                    metadata.comment.clone()
                                }
                            } else {
                                shell.comment_input.text().to_owned()
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(ShellApp::on_comment_focus),
                            )
                            .on_key_down(cx.listener(ShellApp::on_comment_key_down))
                            .child(NativeInputBinding::new(
                                shell.comment_focus_handle.clone(),
                                cx.entity(),
                            )),
                    )
            } else {
                div()
                    .text_xs()
                    .text_color(rgb(shell.palette.subtle))
                    .child("comments hidden — enable view.show_comments to edit")
            },
        )
        .child(
            div()
                .id("commentbox-annotations")
                .debug_selector(|| "commentbox-annotations".to_owned())
                .flex()
                .flex_col()
                .gap_1p5()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(shell.palette.subtle))
                        .child("Move Evaluation"),
                )
                .child(div().flex().flex_wrap().gap_1().children(NodeAnnotation::MOVE.map(|annotation| {
                    let active = metadata.move_annotation == Some(annotation);
                    div()
                        .px_2()
                        .py_1()
                        .border_1()
                        .border_color(rgb(if active { shell.palette.accent } else { shell.palette.border }))
                        .rounded_md()
                        .bg(rgb(if active { shell.palette.button_active } else { shell.palette.input }))
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(if active { shell.palette.text } else { shell.palette.muted }))
                        .cursor_pointer()
                        .hover(|style| {
                            if !active {
                                style.bg(rgb(shell.palette.button)).text_color(rgb(shell.palette.text))
                            } else {
                                style
                            }
                        })
                        .child(annotation.label())
                        .on_mouse_down(MouseButton::Left, cx.listener(move |shell, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<ShellApp>| shell.on_node_annotation(annotation, cx)))
                })))
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(shell.palette.subtle))
                        .child("Position Evaluation"),
                )
                .child(div().flex().flex_wrap().gap_1().children(NodeAnnotation::POSITION.map(|annotation| {
                    let active = metadata.position_annotation == Some(annotation);
                    div()
                        .px_2()
                        .py_1()
                        .border_1()
                        .border_color(rgb(if active { shell.palette.accent } else { shell.palette.border }))
                        .rounded_md()
                        .bg(rgb(if active { shell.palette.button_active } else { shell.palette.input }))
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(if active { shell.palette.text } else { shell.palette.muted }))
                        .cursor_pointer()
                        .hover(|style| {
                            if !active {
                                style.bg(rgb(shell.palette.button)).text_color(rgb(shell.palette.text))
                            } else {
                                style
                            }
                        })
                        .child(annotation.label())
                        .on_mouse_down(MouseButton::Left, cx.listener(move |shell, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<ShellApp>| shell.on_node_annotation(annotation, cx)))
                })))
                .child(
                    div()
                        .id("commentbox-hotspot")
                        .px_2()
                        .py_1()
                        .border_1()
                        .border_color(rgb(if metadata.hotspot { shell.palette.accent } else { shell.palette.border }))
                        .rounded_md()
                        .bg(rgb(if metadata.hotspot { shell.palette.button_active } else { shell.palette.input }))
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(if metadata.hotspot { shell.palette.accent } else { shell.palette.muted }))
                        .cursor_pointer()
                        .hover(|style| {
                            if !metadata.hotspot {
                                style.bg(rgb(shell.palette.button)).text_color(rgb(shell.palette.text))
                            } else {
                                style
                            }
                        })
                        .child(if metadata.hotspot { "⭐ Hotspot" } else { "☆ Hotspot" })
                        .on_mouse_down(MouseButton::Left, cx.listener(|shell, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<ShellApp>| shell.on_hotspot_toggle(cx))),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .text_xs()
                .children(metadata.properties.iter().map(|row| {
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            div()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(rgb(shell.palette.subtle))
                                .child(format!("{}:", row.name)),
                        )
                        .child(div().text_color(rgb(shell.palette.text)).child(row.value.clone()))
                }))
                .child(if metadata.properties.is_empty() {
                    div()
                        .text_color(rgb(shell.palette.subtle))
                        .child("no other properties")
                } else {
                    div()
                }),
        )
        .child(if metadata.can_edit_variation {
            div()
                .flex()
                .gap_2()
                .child(
                    div()
                        .px_3()
                        .py_1()
                        .rounded_md()
                        .cursor_pointer()
                        .border_1()
                        .border_color(rgb(shell.palette.accent))
                        .bg(rgb(shell.palette.button))
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(shell.palette.text))
                        .hover(|style| style.bg(rgb(shell.palette.button_active)))
                        .child("Promote")
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(ShellApp::on_variation_promote),
                        ),
                )
                .child(
                    div()
                        .px_3()
                        .py_1()
                        .rounded_md()
                        .cursor_pointer()
                        .border_1()
                        .border_color(rgb(shell.palette.danger_text))
                        .bg(rgb(shell.palette.danger))
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(shell.palette.danger_text))
                        .child("Remove")
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(ShellApp::on_variation_remove),
                        ),
                )
        } else {
            div()
        })
}

/// The settings panel: theme choices, board size options and the key-table
/// driven preferences form.
pub fn render_settings_panel(
    settings_rows: &[SettingRow],
    shell: &ShellApp,
    cx: &Context<ShellApp>,
) -> Stateful<Div> {
    div()
        .id("settings-panel")
        .debug_selector(|| "settings-panel".to_owned())
        .flex()
        .flex_col()
        .gap_3()
        .p_3()
        .border_1()
        .border_color(rgb(shell.palette.border))
        .rounded_lg()
        .bg(rgb(shell.palette.panel))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1p5()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(shell.palette.subtle))
                        .child("THEME SELECTION"),
                )
                .child(
                    div()
                        .flex()
                        .gap_1p5()
                        .children(crate::THEME_CHOICES.iter().map(|choice| {
                            let is_active = *choice == shell.theme_choice;
                            div()
                                .px_3()
                                .py_1p5()
                                .border_1()
                                .border_color(rgb(if is_active {
                                    shell.palette.accent
                                } else {
                                    shell.palette.border
                                }))
                                .rounded_md()
                                .cursor_pointer()
                                .bg(if is_active {
                                    rgb(shell.palette.button_active)
                                } else {
                                    rgb(shell.palette.input)
                                })
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(rgb(if is_active {
                                    shell.palette.text
                                } else {
                                    shell.palette.muted
                                }))
                                .hover(|style| {
                                    if !is_active {
                                        style.bg(rgb(shell.palette.button)).text_color(rgb(shell.palette.text))
                                    } else {
                                        style
                                    }
                                })
                                .child(choice.label().to_owned())
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |shell,
                                                _: &MouseDownEvent,
                                                _: &mut Window,
                                                cx: &mut Context<ShellApp>| {
                                        shell.on_theme_selected(*choice, cx);
                                    }),
                                )
                        })),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .children(shell.installed_themes.iter().map(|theme| {
                            let apply_id = theme.manifest.id.clone();
                            let uninstall_id = theme.manifest.id.clone();
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .p_2()
                                .rounded_md()
                                .bg(rgb(shell.palette.input))
                                .border_1()
                                .border_color(rgb(shell.palette.border))
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(FontWeight::MEDIUM)
                                        .cursor_pointer()
                                        .text_color(rgb(shell.palette.text))
                                        .child(format!(
                                            "{} v{} (installed)",
                                            theme.manifest.name, theme.manifest.version
                                        ))
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(move |shell,
                                                        _: &MouseDownEvent,
                                                        _: &mut Window,
                                                        cx: &mut Context<ShellApp>| {
                                                shell.on_installed_theme_selected(&apply_id, cx);
                                            }),
                                        ),
                                )
                                .child(
                                    div()
                                        .px_2()
                                        .py_0p5()
                                        .border_1()
                                        .border_color(rgb(shell.palette.danger_text))
                                        .rounded_md()
                                        .bg(rgb(shell.palette.danger))
                                        .text_xs()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(rgb(shell.palette.danger_text))
                                        .cursor_pointer()
                                        .child("uninstall")
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(move |shell,
                                                        _: &MouseDownEvent,
                                                        _: &mut Window,
                                                        cx: &mut Context<ShellApp>| {
                                                shell.on_theme_uninstall(&uninstall_id, cx);
                                            }),
                                        ),
                                )
                        })),
                )
                .child(
                    div()
                        .px_3()
                        .py_1p5()
                        .border_1()
                        .border_color(rgb(shell.palette.accent))
                        .rounded_md()
                        .cursor_pointer()
                        .bg(rgb(shell.palette.button))
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(shell.palette.text))
                        .hover(|style| style.bg(rgb(shell.palette.button_active)))
                        .child("+ Install theme from folder…")
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(
                                |shell,
                                 _: &MouseDownEvent,
                                 _: &mut Window,
                                 cx: &mut Context<ShellApp>| {
                                    shell.on_theme_install(cx);
                                },
                            ),
                        ),
                )
                .child(
                    shell
                        .legacy_asar_themes
                        .iter()
                        .map(|path| {
                            div().text_xs().text_color(rgb(shell.palette.danger_text)).child(format!(
                                "{}: legacy .asar theme — migration only, not loaded",
                                path.file_name()
                                    .and_then(|name| name.to_str())
                                    .unwrap_or("unknown")
                            ))
                        })
                        .collect::<Vec<_>>()
                        .into_iter()
                        .fold(div().flex().flex_col().gap_1(), |container, line| {
                            container.child(line)
                        }),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1p5()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(shell.palette.subtle))
                        .child("DEFAULT BOARD SIZE"),
                )
                .child(
                    div()
                        .flex()
                        .gap_1p5()
                        .children(crate::BOARD_SIZE_OPTIONS.iter().map(|size| {
                            let is_active = *size == shell.board_size;
                            div()
                                .px_3()
                                .py_1()
                                .border_1()
                                .border_color(rgb(if is_active {
                                    shell.palette.accent
                                } else {
                                    shell.palette.border
                                }))
                                .rounded_md()
                                .cursor_pointer()
                                .bg(if is_active {
                                    rgb(shell.palette.button_active)
                                } else {
                                    rgb(shell.palette.input)
                                })
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(rgb(if is_active {
                                    shell.palette.text
                                } else {
                                    shell.palette.muted
                                }))
                                .hover(|style| {
                                    if !is_active {
                                        style.bg(rgb(shell.palette.button)).text_color(rgb(shell.palette.text))
                                    } else {
                                        style
                                    }
                                })
                                .child(format!("{size} × {size}"))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |shell,
                                                _: &MouseDownEvent,
                                                _: &mut Window,
                                                cx: &mut Context<ShellApp>| {
                                        shell.on_board_size_selected(*size, cx);
                                    }),
                                )
                        })),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1p5()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(shell.palette.subtle))
                        .child("SCORING MODE"),
                )
                .child(
                    div().flex().gap_1().child(
                        div()
                            .px_3()
                            .py_1()
                            .border_1()
                            .border_color(rgb(if shell.mode == GameMode::Scoring {
                                shell.palette.accent
                            } else {
                                shell.palette.border
                            }))
                            .rounded_md()
                            .cursor_pointer()
                            .bg(if shell.mode == GameMode::Scoring {
                                rgb(shell.palette.button_active)
                            } else {
                                rgb(shell.palette.input)
                            })
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(if shell.mode == GameMode::Scoring {
                                shell.palette.text
                            } else {
                                shell.palette.muted
                            }))
                            .hover(|style| style.bg(rgb(shell.palette.button)))
                            .child(if shell.mode == GameMode::Scoring {
                                "Scoring: ON"
                            } else {
                                "Scoring: OFF"
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(ShellApp::on_scoring_mode_toggle),
                            ),
                    ),
                )
                .child(if shell.mode == GameMode::Scoring {
                    div()
                        .text_xs()
                        .text_color(rgb(shell.palette.accent))
                        .child(crate::markup::scoring_summary(&shell.host.snapshot()))
                } else {
                    div()
                }),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1p5()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(shell.palette.subtle))
                                .child("PLUGINS & EXTENSIONS (插件管理)"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(shell.palette.muted))
                                .child(format!("{} installed", shell.installed_plugins.len())),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .children(shell.installed_plugins.iter().map(|plugin| {
                            let plugin_id = plugin.plugin_id.clone();
                            let icon = plugin_icon(&plugin_id);
                            let desc = plugin_description(&plugin_id);
                            let toggle_listener = {
                                let plugin_id = plugin.plugin_id.clone();
                                cx.listener(
                                    move |shell,
                                          _: &MouseDownEvent,
                                          _window: &mut Window,
                                          _cx: &mut Context<ShellApp>| {
                                        shell.on_plugin_toggle(&plugin_id)
                                    },
                                )
                            };
                            let grant_listener = {
                                let plugin_id = plugin.plugin_id.clone();
                                cx.listener(
                                    move |shell,
                                          _: &MouseDownEvent,
                                          _window: &mut Window,
                                          cx: &mut Context<ShellApp>| {
                                        shell.on_plugin_grant(&plugin_id, cx)
                                    },
                                )
                            };
                            let authorize_listener = {
                                let plugin_id = plugin.plugin_id.clone();
                                cx.listener(
                                    move |shell,
                                          _: &MouseDownEvent,
                                          _window: &mut Window,
                                          cx: &mut Context<ShellApp>| {
                                        shell.on_plugin_authorize(&plugin_id, cx)
                                    },
                                )
                            };
                            div()
                                .flex()
                                .flex_col()
                                .gap_1p5()
                                .p_2p5()
                                .rounded_md()
                                .bg(rgb(shell.palette.input))
                                .border_1()
                                .border_color(rgb(shell.palette.border))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap_1p5()
                                                .child(div().text_sm().child(icon))
                                                .child(
                                                    div()
                                                        .font_weight(FontWeight::SEMIBOLD)
                                                        .text_xs()
                                                        .text_color(rgb(shell.palette.text))
                                                        .child(format!("{} v{}", plugin.name, plugin.version)),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap_1()
                                                .children((!plugin.missing_permissions.is_empty()).then(|| {
                                                    div()
                                                        .px_2()
                                                        .py_0p5()
                                                        .rounded_md()
                                                        .cursor_pointer()
                                                        .border_1()
                                                        .border_color(rgb(shell.palette.accent))
                                                        .bg(rgb(shell.palette.button))
                                                        .text_xs()
                                                        .font_weight(FontWeight::MEDIUM)
                                                        .text_color(rgb(shell.palette.text))
                                                        .hover(|style| style.bg(rgb(shell.palette.button_active)))
                                                        .child("🛡️ 授予权限")
                                                        .on_mouse_down(MouseButton::Left, grant_listener)
                                                }))
                                                .children((plugin.native_runtime && !plugin.native_authorized).then(|| {
                                                    div()
                                                        .px_2()
                                                        .py_0p5()
                                                        .rounded_md()
                                                        .cursor_pointer()
                                                        .border_1()
                                                        .border_color(rgb(shell.palette.danger_text))
                                                        .bg(rgb(shell.palette.danger))
                                                        .text_xs()
                                                        .font_weight(FontWeight::MEDIUM)
                                                        .text_color(rgb(shell.palette.danger_text))
                                                        .child("⚠️ 授权原生进程")
                                                        .on_mouse_down(MouseButton::Left, authorize_listener)
                                                }))
                                                .child(
                                                    div()
                                                        .px_2p5()
                                                        .py_1()
                                                        .rounded_md()
                                                        .cursor_pointer()
                                                        .border_1()
                                                        .border_color(if plugin.enabled {
                                                            rgb(shell.palette.accent)
                                                        } else {
                                                            rgb(shell.palette.border)
                                                        })
                                                        .bg(if plugin.enabled {
                                                            rgb(shell.palette.button_active)
                                                        } else {
                                                            rgb(shell.palette.panel)
                                                        })
                                                        .text_xs()
                                                        .font_weight(FontWeight::MEDIUM)
                                                        .text_color(rgb(if plugin.enabled {
                                                            shell.palette.text
                                                        } else {
                                                            shell.palette.muted
                                                        }))
                                                        .hover(|style| style.bg(rgb(shell.palette.button)))
                                                        .child(if plugin.enabled { "已启用 (Disable)" } else { "已禁用 (Enable)" })
                                                        .on_mouse_down(MouseButton::Left, toggle_listener),
                                                ),
                                        ),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(shell.palette.subtle))
                                        .child(desc),
                                )
                        })),
                )
                .child(
                    div()
                        .px_3()
                        .py_1p5()
                        .border_1()
                        .border_color(rgb(shell.palette.accent))
                        .rounded_md()
                        .cursor_pointer()
                        .bg(rgb(shell.palette.button))
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(shell.palette.text))
                        .hover(|style| style.bg(rgb(shell.palette.button_active)))
                        .child("+ 导入插件 ZIP 压缩包 (Install from ZIP)…")
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(ShellApp::on_install_plugin_zip),
                        ),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1p5()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(shell.palette.subtle))
                        .child("DETAILED PREFERENCES"),
                )
                .child(
                    div()
                        .id("settings-form")
                        .h(px(220.0))
                        .overflow_y_scroll()
                        .p_2()
                        .rounded_md()
                        .bg(rgb(shell.palette.input))
                        .border_1()
                        .border_color(rgb(shell.palette.border))
                        .flex()
                        .flex_col()
                        .gap_1p5()
                        .children(settings_rows.iter().map(|row| {
                            let is_editing =
                                shell.settings_editing_key.as_deref() == Some(row.key.as_str());
                            render_setting_row(
                                row,
                                is_editing,
                                &shell.settings_draft,
                                &shell.settings_input_focus_handle,
                                shell.palette,
                                cx,
                            )
                        })),
                ),
        )
}

/// The goban plus the analysis best-move ring overlay. Rendering options come
/// from the shell settings (`view.show_coordinates`, `view.show_move_numbers`)
/// and the document state (move numbers, scoring overrides).
pub fn render_goban_area(
    snapshot: &GameSnapshot,
    theme: &crate::theme::ThemeTokens,
    best_move: Option<sabaki_domain_core::Vertex>,
    board_pixel_size: f32,
    shell: &ShellApp,
    cx: &Context<ShellApp>,
) -> Stateful<Div> {
    let board = &snapshot.board;
    let line_preview = shell
        .line_start
        .zip(shell.hovered_vertex)
        .filter(|(start, end)| start != end)
        .filter(|_| shell.active_tool.is_line_tool() && shell.mode == GameMode::Edit)
        .map(|(start, end)| sabaki_domain_core::BoardLineSnapshot {
            start,
            end,
            line_type: if shell.active_tool == crate::markup::MarkupTool::Arrow {
                "arrow".to_owned()
            } else {
                "line".to_owned()
            },
        });
    let analysis_candidates = if shell
        .settings
        .get_bool("board.show_analysis")
        .unwrap_or(true)
    {
        shell
            .analysis
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| {
                let vertex = entry
                    .vertex
                    .as_deref()
                    .and_then(|value| parse_gtp_vertex(board.width, value))
                    .map(|(column, row)| sabaki_domain_core::Vertex { column, row })?;
                Some(crate::goban_view::AnalysisCandidate {
                    vertex,
                    label: format!("{}v {:.0}%", entry.visits, entry.winrate * 100.0),
                    is_best: index == 0 || Some(vertex) == best_move,
                })
            })
            .take(5)
            .collect()
    } else {
        Vec::new()
    };
    let hover_stone_color = matches!(
        shell.mode,
        GameMode::Play | GameMode::Guess | GameMode::Autoplay
    )
    .then_some(board.next_player);
    let options = crate::goban_view::GobanRenderOptions {
        show_coordinates: shell
            .settings
            .get_bool("view.show_coordinates")
            .unwrap_or(false),
        coordinates_type: shell
            .settings
            .get_str("view.coordinates_type")
            .unwrap_or("A1")
            .to_owned(),
        show_move_numbers: shell
            .settings
            .get_bool("view.show_move_numbers")
            .unwrap_or(false),
        move_numbers: crate::goban_view::move_numbers_for_snapshot(
            snapshot,
            shell
                .settings
                .get_str("view.move_numbers_type")
                .unwrap_or("start"),
        ),
        line_preview,
        hovered_vertex: shell.hovered_vertex,
        hover_stone_color,
        analysis_candidates,
        ownership: shell
            .analysis
            .first()
            .and_then(|entry| entry.ownership.clone()),
        score_overrides: snapshot.score_overrides.clone(),
        show_next_moves: shell
            .settings
            .get_bool("view.show_next_moves")
            .unwrap_or(true),
        show_siblings: shell
            .settings
            .get_bool("view.show_siblings")
            .unwrap_or(true),
        show_move_colorization: shell
            .settings
            .get_bool("view.show_move_colorization")
            .unwrap_or(true),
    };
    let best_move = if shell
        .settings
        .get_bool("board.show_analysis")
        .unwrap_or(true)
    {
        best_move
    } else {
        None
    };
    let weak_shell = cx.entity().downgrade();
    let on_vertex_mouse_down = Rc::new(
        move |vertex: sabaki_domain_core::Vertex,
              _: &MouseDownEvent,
              _window: &mut Window,
              cx: &mut App| {
            weak_shell
                .update(cx, |shell, cx| shell.on_board_vertex_mouse_down(vertex, cx))
                .ok();
        },
    );
    let weak_shell = cx.entity().downgrade();
    let on_vertex_mouse_move = Rc::new(
        move |vertex: sabaki_domain_core::Vertex,
              _: &gpui::MouseMoveEvent,
              _window: &mut Window,
              cx: &mut App| {
            weak_shell
                .update(cx, |shell, cx| shell.on_board_vertex_mouse_move(vertex, cx))
                .ok();
        },
    );
    let weak_shell = cx.entity().downgrade();
    let on_vertex_mouse_up = Rc::new(
        move |vertex: sabaki_domain_core::Vertex,
              _: &gpui::MouseUpEvent,
              _window: &mut Window,
              cx: &mut App| {
            weak_shell
                .update(cx, |shell, cx| shell.on_board_vertex_mouse_up(vertex, cx))
                .ok();
        },
    );
    let board_element =
        render_goban(board, board_pixel_size, theme, &options).child(render_goban_click_layer(
            board,
            board_pixel_size,
            on_vertex_mouse_down,
            on_vertex_mouse_move,
            on_vertex_mouse_up,
        ));
    div()
        .id("goban-area")
        .relative()
        .size(px(board_pixel_size))
        .child(board_element)
        .child(if let Some(vertex) = best_move {
            let (x, y) = crate::goban_view::intersection_position(
                board,
                board_pixel_size,
                vertex.column,
                vertex.row,
            );
            div()
                .absolute()
                .left(px(x - 8.0))
                .top(px(y - 8.0))
                .size(px(16.0))
                .rounded_full()
                .border_2()
                .border_color(rgb(shell.palette.danger_text))
        } else {
            div()
        })
}

/// Renders one settings row: a toggle pill for booleans, a click-to-edit text
/// row for every other kind. The editing row shows the draft in a focused
/// input box; Enter commits, Esc reverts.
pub fn render_setting_row(
    row: &SettingRow,
    is_editing: bool,
    draft: &str,
    focus_handle: &FocusHandle,
    palette: UiPalette,
    cx: &Context<ShellApp>,
) -> Div {
    match row.kind {
        sabaki_host::SettingKind::Boolean => {
            let is_on = row
                .value
                .as_ref()
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            let row_for_listener = row.clone();
            div()
                .flex()
                .items_center()
                .justify_between()
                .p_2()
                .rounded_md()
                .bg(rgb(palette.panel))
                .border_1()
                .border_color(rgb(palette.border))
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(palette.text))
                        .child(row.label.to_owned()),
                )
                .child(
                    div()
                        .px_2p5()
                        .py_0p5()
                        .rounded_full()
                        .cursor_pointer()
                        .border_1()
                        .border_color(rgb(if is_on {
                            palette.accent
                        } else {
                            palette.border
                        }))
                        .bg(if is_on {
                            rgb(palette.button_active)
                        } else {
                            rgb(palette.input)
                        })
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(if is_on { palette.accent } else { palette.muted }))
                        .hover(|style| style.bg(rgb(palette.button)))
                        .child(if is_on { "ON" } else { "OFF" })
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(
                                move |shell,
                                      event: &MouseDownEvent,
                                      window: &mut Window,
                                      cx: &mut Context<ShellApp>| {
                                    shell.on_settings_toggle(&row_for_listener, event, window, cx)
                                },
                            ),
                        ),
                )
        }
        _ => {
            let row_for_listener = row.clone();
            div()
                .flex()
                .items_center()
                .justify_between()
                .p_2()
                .rounded_md()
                .bg(rgb(palette.panel))
                .border_1()
                .border_color(rgb(palette.border))
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(palette.text))
                        .child(row.label.to_owned()),
                )
                .child(if is_editing {
                    div()
                        .track_focus(focus_handle)
                        .px_2p5()
                        .py_1()
                        .border_1()
                        .border_color(rgb(palette.accent))
                        .rounded_md()
                        .bg(rgb(palette.input))
                        .text_xs()
                        .text_color(rgb(palette.text))
                        .child(draft.to_owned())
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(ShellApp::on_settings_input_focus),
                        )
                        .on_key_down(cx.listener(ShellApp::on_settings_key_down))
                } else {
                    div()
                        .px_2p5()
                        .py_1()
                        .border_1()
                        .border_color(rgb(palette.border))
                        .rounded_md()
                        .bg(rgb(palette.input))
                        .text_xs()
                        .cursor_pointer()
                        .hover(|style| style.bg(rgb(palette.button)))
                        .child(display_setting_value(row.value.as_ref()))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(
                                move |shell,
                                      event: &MouseDownEvent,
                                      window: &mut Window,
                                      cx: &mut Context<ShellApp>| {
                                    shell.on_settings_row_clicked(
                                        &row_for_listener,
                                        event,
                                        window,
                                        cx,
                                    )
                                },
                            ),
                        )
                })
        }
    }
}
