//! Panel rendering for the shell, split out of `main.rs` so the shell keeps
//! only state, actions and assembly. Every function is a pure view over
//! `ShellApp` state plus precomputed values; listeners are built with
//! `cx.listener` against `ShellApp` handlers.

use std::rc::Rc;

use gpui::{
    App, Context, Div, FocusHandle, FontWeight, InteractiveElement, MouseButton, MouseDownEvent,
    PathBuilder, Stateful, StatefulInteractiveElement, Window, canvas, div, hsla, point,
    prelude::*, px, rgb,
};

use sabaki_domain_core::{GameMode, GameSnapshot, Vertex};

use gpui_component::badge::Badge;
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::switch::Switch;
use gpui_component::{Selectable, Sizable};

use crate::engine_console::{best_analysis_winrate, parse_gtp_vertex};
use crate::goban_view::{
    pv_preview_points, render_goban, render_goban_click_layer, render_goban_with_id,
};
use crate::layout::SplitPane;
use crate::native_text_input::NativeInputBinding;
use crate::navigation::NavigationAvailability;
use crate::node_inspector::{NodeAnnotation, NodeInspectorMetadata};
use crate::plugin_contribution::PanelWidget;
use crate::plugin_panel::PluginPanelEntry;
use crate::settings_form::{SettingRow, display_setting_value};
use crate::theme::UiPalette;
use crate::variation_tree::{VariationTreeLayout, render_variation_tree};
use crate::winrate_graph::{GraphPlotPoint, WinrateGraphMetric};
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
                    Button::new("left-sidebar-toggle")
                        .small()
                        .ghost()
                        .tooltip("切换引擎侧栏 (Cmd+Shift+B)")
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
                        .on_click(cx.listener(|shell, _, window, cx| {
                            shell.on_toggle_left_sidebar(&MouseDownEvent::default(), window, cx);
                        })),
                )
                .child(
                    Button::new("titlebar-navigate-prev")
                        .small()
                        .ghost()
                        .label("‹")
                        .tooltip("上一手 (Left)")
                        .on_click(cx.listener(|shell, _, window, cx| {
                            shell.on_navigate_previous(&MouseDownEvent::default(), window, cx);
                        })),
                )
                .child(
                    Button::new("titlebar-navigate-next")
                        .small()
                        .ghost()
                        .label("›")
                        .tooltip("下一手 (Right)")
                        .on_click(cx.listener(|shell, _, window, cx| {
                            shell.on_navigate_next(&MouseDownEvent::default(), window, cx);
                        })),
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
                    Button::new("titlebar-game-info")
                        .small()
                        .ghost()
                        .tooltip("对局信息 (Cmd+I)")
                        .child(mac_info_icon(palette.muted))
                        .on_click(cx.listener(|shell, _, _, cx| shell.open_game_info(cx))),
                )
                .child(
                    Button::new("right-sidebar-toggle")
                        .small()
                        .ghost()
                        .tooltip("切换侧栏变化与分析面板")
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
                        .on_click(cx.listener(|shell, _, window, cx| {
                            shell.on_toggle_right_sidebar(&MouseDownEvent::default(), window, cx);
                        })),
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
    let show_coordinates = shell
        .settings
        .get_bool("view.show_coordinates")
        .unwrap_or(false);
    let show_move_numbers = shell
        .settings
        .get_bool("view.show_move_numbers")
        .unwrap_or(false);

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
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            cx.stop_propagation();
        })
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
                                .child(
                                    Button::new("pass-btn")
                                        .small()
                                        .ghost()
                                        .label("Pass")
                                        .tooltip("停一手 (Pass)")
                                        .on_click(cx.listener(|shell, _, window, cx| {
                                            shell.on_pass(&MouseDownEvent::default(), window, cx);
                                        })),
                                ),
                        )
                        .child(
                            Button::new("resign-button")
                                .small()
                                .ghost()
                                .danger()
                                .label("Resign")
                                .tooltip("认输 (Resign)")
                                .on_click(cx.listener(|shell, _, window, cx| {
                                    shell.on_resign(&MouseDownEvent::default(), window, cx);
                                })),
                        ),
                )
                .children(
                    (shell.mode == GameMode::Scoring || shell.mode == GameMode::Estimator).then(
                        || {
                            Button::new("exit-scoring-button")
                                .small()
                                .warning()
                                .label("🏁 退出点目 (返回落子)")
                                .on_click(
                                    cx.listener(|shell, _, _, cx| {
                                        shell.set_mode(GameMode::Play, cx)
                                    }),
                                )
                        },
                    ),
                )
                .children(shell.autosave.info().is_available.then(|| {
                    Button::new("player-bar-restore-recovery")
                        .small()
                        .warning()
                        .label("⚡ Restore")
                        .tooltip("恢复未保存的崩溃局面")
                        .on_click(cx.listener(|shell, _, window, cx| {
                            shell.on_restore_recovery(&MouseDownEvent::default(), window, cx);
                        }))
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
                .child(
                    Button::new("toggle-coordinates-button")
                        .small()
                        .ghost()
                        .selected(show_coordinates)
                        .label("坐标")
                        .tooltip("显示/隐藏棋盘坐标 (Cmd+Shift+C)")
                        .on_click(cx.listener(|shell, _, _, cx| {
                            shell.toggle_view_setting(
                                "view.show_coordinates",
                                "board coordinates",
                                cx,
                            )
                        })),
                )
                .child(
                    Button::new("toggle-move-numbers-button")
                        .small()
                        .ghost()
                        .selected(show_move_numbers)
                        .label("手数")
                        .tooltip("显示/隐藏落子手数 (Cmd+Shift+M)")
                        .on_click(cx.listener(|shell, _, _, cx| {
                            shell.toggle_view_setting("view.show_move_numbers", "move numbers", cx)
                        })),
                )
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
                            let pid = plugin_id.clone();
                            Button::new(("pinned-plugin", plugin_idx))
                                .small()
                                .ghost()
                                .selected(is_active)
                                .label(format!("{icon} {label}"))
                                .on_click(cx.listener(move |shell, _, _, cx| {
                                    shell.toggle_plugin_popover(&pid, cx);
                                }))
                        }),
                )
                .child(
                    Button::new("gtp-terminal-button")
                        .small()
                        .ghost()
                        .selected(shell.gtp_terminal_open)
                        .label("💻 GTP 终端")
                        .tooltip("切换 KataGo / GTP 控制台 (Cmd+T)")
                        .on_click(cx.listener(|shell, _, window, cx| {
                            shell.toggle_gtp_terminal(&MouseDownEvent::default(), window, cx);
                        })),
                )
                .child(
                    Button::new("plugin-menu-button")
                        .small()
                        .ghost()
                        .selected(shell.active_plugin_popover.as_deref() == Some("all"))
                        .label("🧩 插件")
                        .tooltip("已安装插件与市场")
                        .on_click(cx.listener(|shell, _, _, cx| {
                            shell.toggle_plugin_popover("all", cx);
                        })),
                )
                .child(
                    Button::new("drawer-menu-button")
                        .small()
                        .ghost()
                        .label("☰")
                        .tooltip("主菜单 (Cmd+, 首选项)")
                        .on_click(cx.listener(|shell, _, window, cx| {
                            shell.open_side_menu(&MouseDownEvent::default(), window, cx);
                        })),
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
    cx: &Context<ShellApp>,
) -> Stateful<Div> {
    let on_node_clicked = Rc::new(on_node_clicked);
    let has_values = points.iter().any(|point| point.y.is_some());
    // The plot is fluid with the right sidebar. The previous fixed 236px
    // coordinate placed the live endpoint outside the default ~200px pane.
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
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(
                            Button::new("metric-tab-winrate")
                                .xsmall()
                                .ghost()
                                .selected(metric == WinrateGraphMetric::Winrate)
                                .label("胜率走势")
                                .on_click(cx.listener(|shell, _, _, cx| {
                                    shell.set_winrate_metric(WinrateGraphMetric::Winrate, cx);
                                })),
                        )
                        .child(
                            Button::new("metric-tab-score")
                                .xsmall()
                                .ghost()
                                .selected(metric == WinrateGraphMetric::ScoreLead)
                                .label("目差走势")
                                .on_click(cx.listener(|shell, _, _, cx| {
                                    shell.set_winrate_metric(WinrateGraphMetric::ScoreLead, cx);
                                })),
                        ),
                )
                .children(has_values.then(|| {
                    Badge::new()
                        .small()
                        .child(format!("{} 手记录", points.len()))
                })),
        )
        .child(if has_values {
            let graph_points = points.to_vec();
            let graph_accent = 0x0ea5e9; // KaTrain cyan/sky blue
            let graph_danger = 0xef4444; // KaTrain blunder red
            div()
                .id("winrate-graph-plot")
                .relative()
                .w_full()
                .flex_1()
                .min_h_0()
                .rounded_md()
                .bg(rgb(palette.input))
                .child(
                    canvas(
                        |_, _, _| (),
                        move |bounds, (), window, _cx| {
                            // 50% even-game centerline
                            let mut baseline =
                                PathBuilder::stroke(px(1.0)).dash_array(&[px(3.0), px(3.0)]);
                            baseline.move_to(point(
                                bounds.origin.x,
                                bounds.origin.y + bounds.size.height * 0.5,
                            ));
                            baseline.line_to(point(
                                bounds.origin.x + bounds.size.width,
                                bounds.origin.y + bounds.size.height * 0.5,
                            ));
                            if let Ok(path) = baseline.build() {
                                window.paint_path(path, hsla(0.0, 0.0, 1.0, 0.12));
                            }

                            let valid: Vec<(f32, f32, u32, bool, bool)> = graph_points
                                .iter()
                                .enumerate()
                                .filter_map(|(index, point)| {
                                    let y = point.y? as f32;
                                    let x = if graph_points.len() <= 1 {
                                        0.5
                                    } else {
                                        index as f32 / last_index
                                    };
                                    let color = if point.is_blunder {
                                        graph_danger
                                    } else {
                                        graph_accent
                                    };
                                    Some((x, y, color, point.is_current, point.is_blunder))
                                })
                                .collect();

                            if valid.len() >= 2 {
                                // Shaded area fill under the curve (KaTrain smooth area)
                                let mut area = PathBuilder::fill();
                                if let Some(&(first_x, _, _, _, _)) = valid.first() {
                                    area.move_to(point(
                                        bounds.origin.x + bounds.size.width * first_x,
                                        bounds.origin.y + bounds.size.height,
                                    ));
                                    for &(x, y, _, _, _) in &valid {
                                        area.line_to(point(
                                            bounds.origin.x + bounds.size.width * x,
                                            bounds.origin.y + bounds.size.height * y,
                                        ));
                                    }
                                    if let Some(&(last_x, _, _, _, _)) = valid.last() {
                                        area.line_to(point(
                                            bounds.origin.x + bounds.size.width * last_x,
                                            bounds.origin.y + bounds.size.height,
                                        ));
                                    }
                                    area.close();
                                    if let Ok(path) = area.build() {
                                        window.paint_path(path, hsla(0.55, 0.85, 0.45, 0.16));
                                    }
                                }

                                // Main winrate stroke line
                                let mut path = PathBuilder::stroke(px(2.0));
                                for (index, (x, y, _, _, _)) in valid.iter().enumerate() {
                                    let position = point(
                                        bounds.origin.x + bounds.size.width * *x,
                                        bounds.origin.y + bounds.size.height * *y,
                                    );
                                    if index == 0 {
                                        path.move_to(position);
                                    } else {
                                        path.line_to(position);
                                    }
                                }
                                if let Ok(path) = path.build() {
                                    window.paint_path(path, rgb(graph_accent));
                                }
                            }

                            // KaTrain-style node dots & blunder indicators
                            for (x, y, color, is_current, is_blunder) in valid {
                                let center = point(
                                    bounds.origin.x + bounds.size.width * x,
                                    bounds.origin.y + bounds.size.height * y,
                                );
                                let dot_radius = if is_current {
                                    px(4.5)
                                } else if is_blunder {
                                    px(3.5)
                                } else {
                                    px(2.0)
                                };
                                window.paint_quad(gpui::quad(
                                    gpui::Bounds {
                                        origin: point(center.x - dot_radius, center.y - dot_radius),
                                        size: gpui::size(dot_radius * 2.0, dot_radius * 2.0),
                                    },
                                    dot_radius,
                                    rgb(color),
                                    if is_current { px(1.5) } else { px(0.0) },
                                    rgb(0xffffff),
                                    gpui::BorderStyle::default(),
                                ));
                            }
                        },
                    )
                    .absolute()
                    .top_0()
                    .left_0()
                    .w_full()
                    .h_full(),
                )
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .w_full()
                        .h_full()
                        .flex()
                        .children(points.iter().enumerate().map(|(index, _)| {
                            let handler = on_node_clicked.clone();
                            let node_id = points[index].node_id.clone();
                            div()
                                .id(("winrate-scrub-cell", index))
                                .flex_1()
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
                                .child(
                                    Button::new("preferences-close-btn")
                                        .small()
                                        .ghost()
                                        .label("✕ Close")
                                        .on_click(cx.listener(|shell, _, window, cx| {
                                            shell.close_drawer(
                                                &MouseDownEvent::default(),
                                                window,
                                                cx,
                                            );
                                        })),
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
                                .child(
                                    Button::new(("drawer-close-btn", drawer_index))
                                        .small()
                                        .ghost()
                                        .label("✕ Close")
                                        .on_click(cx.listener(|shell, _, window, cx| {
                                            shell.close_drawer(
                                                &MouseDownEvent::default(),
                                                window,
                                                cx,
                                            );
                                        })),
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
#[allow(dead_code)]
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
#[allow(dead_code)]
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
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .children(shell.engine_store.list().iter().map(|record| {
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
                                .flex_col()
                                .gap_0p5()
                                .min_w_0()
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .gap_1()
                                        .child(
                                            div()
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_xs()
                                                .text_color(rgb(shell.palette.text))
                                                .flex_1()
                                                .min_w_0()
                                                .truncate()
                                                .child(record.name.clone()),
                                        )
                                        .child(
                                            div()
                                                .px_1p5()
                                                .py_0p5()
                                                .rounded_sm()
                                                .flex_none()
                                                .whitespace_nowrap()
                                                .bg(rgb(if connected {
                                                    shell.palette.button_active
                                                } else {
                                                    shell.palette.panel
                                                }))
                                                .text_xs()
                                                .text_color(rgb(if connected {
                                                    shell.palette.success
                                                } else {
                                                    shell.palette.muted
                                                }))
                                                .child(if connected {
                                                    "● 已连接"
                                                } else {
                                                    "○ 离线"
                                                }),
                                        ),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(shell.palette.subtle))
                                        .truncate()
                                        .child(record.path.clone()),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(div().flex().gap_1().children(roles.into_iter().map(
                                    |role| {
                                        let role_name = name.clone();
                                        let active =
                                            shell.engine_roles.get(role) == Some(name.as_str());
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
                                    },
                                )))
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
                        )
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
                })),
        )
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

#[allow(dead_code)]
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
                .flex_col()
                .gap_0p5()
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
                        .truncate()
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
        .unwrap_or_else(|| format!("{} (点击连接自动探测)", selected_role.label()));

    div()
        .id("gtp-terminal-drawer")
        .debug_selector(|| "gtp-terminal-drawer".to_owned())
        .absolute()
        .bottom(px(42.0))
        .left(px(16.0))
        .right(px(16.0))
        .flex()
        .justify_center()
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            // Stop propagation to prevent accidental clicks under the drawer
            cx.stop_propagation();
        })
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
                        .gap_2()
                        .pb_2()
                        .border_b_1()
                        .border_color(rgb(0x2a2a30))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_1p5()
                                .min_w_0()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(rgb(0xf2f2f5))
                                        .flex_none()
                                        .child("💻 GTP 终端"),
                                )
                                // Role tabs (short labels only; the full engine
                                // name is shown in the transcript status line)
                                .children([
                                    crate::engine_console::EngineRole::Analysis,
                                    crate::engine_console::EngineRole::Black,
                                    crate::engine_console::EngineRole::White,
                                ].into_iter().enumerate().map(|(idx, role)| {
                                    let is_active = shell.active_console_role == Some(role)
                                        || (shell.active_console_role.is_none() && role == crate::engine_console::EngineRole::Analysis);
                                    let attached = shell.engine_controller.is_attached(role);
                                    Button::new(("gtp-role", idx))
                                        .small()
                                        .ghost()
                                        .selected(is_active)
                                        .label(format!(
                                            "{} {}",
                                            if attached { "●" } else { "○" },
                                            role.label(),
                                        ))
                                        .on_click(cx.listener(move |shell, _, _, cx| {
                                            shell.active_console_role = Some(role);
                                            cx.notify();
                                        }))
                                }))
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .flex_none()
                                .child(
                                    Button::new("gtp-attach-toggle")
                                        .small()
                                        .outline()
                                        .label(if is_attached { "⏹ 断开" } else { "🔌 连接" })
                                        .on_click(cx.listener(move |shell, _, window, cx| {
                                            if shell.engine_controller.is_attached(selected_role) {
                                                shell.on_engine_disconnect(selected_role, &MouseDownEvent::default(), window, cx);
                                            } else {
                                                shell.on_engine_connect(selected_role, cx);
                                            }
                                        }))
                                )
                                .child(
                                    Button::new("gtp-clear-log")
                                        .small()
                                        .ghost()
                                        .label("清空")
                                        .on_click(cx.listener(|shell, _, _, cx| {
                                            shell.engine_log.clear();
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    Button::new("gtp-close-terminal")
                                        .small()
                                        .ghost()
                                        .label("✕")
                                        .on_click(cx.listener(|shell, _, window, cx| {
                                            shell.toggle_gtp_terminal(&MouseDownEvent::default(), window, cx);
                                        })),
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
                                    Button::new("gtp-start-analysis")
                                        .xsmall()
                                        .outline()
                                        .label(if shell.analysis_task.is_some() { "⏹ 停止分析" } else { "⚡ 启动分析" })
                                        .on_click(cx.listener(|shell, _, _, cx| {
                                            if shell.analysis_task.is_some() {
                                                shell.stop_analysis(cx);
                                            } else {
                                                shell.start_analysis(cx);
                                            }
                                        }))
                                )
                                // Analysis visits quick-switch (KataGo maxVisits)
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_1()
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(rgb(0x9a9a9a))
                                                .child("访问量:"),
                                        )
                                        .children([100u64, 500, 1000, 0].into_iter().enumerate().map(|(idx, visits)| {
                                            let current = shell
                                                .settings
                                                .get("engines.analysis_max_visits")
                                                .and_then(serde_json::Value::as_u64)
                                                .unwrap_or(500);
                                            let is_active = current == visits;
                                            Button::new(("visits-switch", idx))
                                                .xsmall()
                                                .ghost()
                                                .selected(is_active)
                                                .label(if visits == 0 { "无限".to_owned() } else { visits.to_string() })
                                                .on_click(cx.listener(move |shell, _, _, cx| {
                                                    shell.apply_analysis_visits(visits, cx);
                                                }))
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
                                    Button::new("gtp-clear-board")
                                        .xsmall()
                                        .ghost()
                                        .label("🔄 清空棋盘")
                                        .on_click(cx.listener(|shell, _, _, cx| {
                                            shell.send_engine_command("clear_board", cx);
                                        }))
                                )
                                .child(
                                    Button::new("gtp-list-commands")
                                        .xsmall()
                                        .ghost()
                                        .label("📋 列出指令")
                                        .on_click(cx.listener(|shell, _, _, cx| {
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
                                    Button::new("gtp-send-button")
                                        .small()
                                        .primary()
                                        .label("发送 ↵")
                                        .on_click(cx.listener(|shell, _, _, cx| {
                                            let draft = shell.gtp_input.text().to_owned();
                                            shell.send_engine_command(&draft, cx);
                                            shell.gtp_input.set_text("");
                                            cx.notify();
                                        })),
                                ),
                        )
                )
        )
}

fn render_stat_row(
    label: &'static str,
    black: usize,
    white: usize,
    color: u32,
    shell: &ShellApp,
) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .child(
            div()
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgb(color))
                .child(label),
        )
        .child(
            div()
                .text_color(rgb(shell.palette.text))
                .child(black.to_string()),
        )
        .child(
            div()
                .text_color(rgb(shell.palette.text))
                .child(white.to_string()),
        )
}

/// The KaTrain-style KataGo AI Analysis Panel on the left sidebar.
pub fn render_left_engine_sidebar(shell: &ShellApp, cx: &Context<ShellApp>) -> Stateful<Div> {
    let snapshot = shell.host.snapshot();
    let rule_config = sabaki_host::GameRuleConfig::from_root_properties(
        &snapshot.root_properties,
        snapshot.board.width,
    );
    let evaluations = sabaki_host::compute_game_move_evaluations(&snapshot);
    let summary = sabaki_host::GameAnalyticsSummary::from_evaluations(&evaluations);

    // Current node move evaluation (if any)
    let current_eval = evaluations
        .iter()
        .find(|e| e.node_id == snapshot.current_node_id);

    // Candidates sorted by visits descending
    let mut candidates = shell.analysis.clone();
    candidates.sort_by_key(|e| std::cmp::Reverse(e.visits));

    let live_winrate = if !shell.analysis.is_empty() {
        best_analysis_winrate(&shell.analysis, snapshot.board.next_player)
    } else {
        0.50
    };

    let best_score_lead = candidates.first().and_then(|e| e.score_lead);
    let total_visits: u64 = candidates.iter().map(|e| e.visits).sum();

    div()
        .id("engine-sidebar")
        .debug_selector(|| "engine-sidebar".to_owned())
        .size_full()
        .min_h_0()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .bg(rgb(shell.palette.input))
        .text_xs()
        // Section 1: Header with Rule, Komi, Handicap
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .p_2()
                .rounded_md()
                .bg(rgb(shell.palette.panel))
                .border_1()
                .border_color(rgb(shell.palette.border))
                .child(
                    div()
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(shell.palette.text))
                        .child(format!("📊 {}", rule_config.ruleset.label())),
                )
                .child(div().text_color(rgb(shell.palette.muted)).child(format!(
                    "贴目 {:.1}目{}",
                    rule_config.komi,
                    if rule_config.handicap >= 2 {
                        format!(" (让{}子)", rule_config.handicap)
                    } else {
                        String::new()
                    }
                ))),
        )
        // Section 2: AI Live Scorecard (Winrate & Score Lead)
        .child(
            div()
                .p_2p5()
                .rounded_md()
                .bg(rgb(shell.palette.panel))
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
                                .font_weight(FontWeight::BOLD)
                                .text_color(rgb(shell.palette.subtle))
                                .child("AI 局面评估"),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_1p5()
                                .child(
                                    Button::new("whole-game-review-button")
                                        .xsmall()
                                        .outline()
                                        .label(
                                            if shell
                                                .batch_review_progress
                                                .is_some_and(|p| p.is_running)
                                            {
                                                "⏹ 停止全盘复盘"
                                            } else {
                                                "⏩ 全盘 AI 复盘"
                                            },
                                        )
                                        .on_click(cx.listener(|shell, _, _, cx| {
                                            shell.start_whole_game_review(cx);
                                        })),
                                )
                                .child(Badge::new().small().child(
                                    if shell.analysis_task.is_some() {
                                        "● 分析中"
                                    } else if shell.analysis_enabled {
                                        "○ 自动分析"
                                    } else {
                                        "○ 已暂停"
                                    },
                                )),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_base()
                                .font_weight(FontWeight::BOLD)
                                .text_color(rgb(shell.palette.text))
                                .child(format!("胜率: {:.1}%", live_winrate * 100.0)),
                        )
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(if best_score_lead.unwrap_or(0.0) >= 0.0 {
                                    shell.palette.accent
                                } else {
                                    shell.palette.danger_text
                                }))
                                .child(
                                    best_score_lead
                                        .map(|s| format!("领先: {:+.1} 目", s))
                                        .unwrap_or_else(|| "领先: 0.0 目".to_owned()),
                                ),
                        ),
                )
                // Winrate bar
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child("黑")
                        .child(
                            div()
                                .flex_1()
                                .h(px(8.0))
                                .rounded(px(2.0))
                                .bg(rgb(shell.palette.track))
                                .child(
                                    div()
                                        .h_full()
                                        .w(px(live_winrate as f32 * 140.0))
                                        .bg(rgb(shell.palette.text)),
                                ),
                        )
                        .child("白"),
                )
                // Move quality badge
                .children(current_eval.map(|ev| {
                    div()
                        .mt_1()
                        .p_1p5()
                        .rounded_md()
                        .bg(rgb(shell.palette.input))
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .font_weight(FontWeight::BOLD)
                                .text_color(rgb(ev.quality.color_u32()))
                                .child(format!("{} {}", ev.quality.badge(), ev.quality.label())),
                        )
                        .child(
                            div()
                                .text_color(rgb(shell.palette.muted))
                                .child(format!("损失 {:.1}目", ev.points_lost)),
                        )
                })),
        )
        // Section 3: AI Candidate Moves Table
        .child(
            div()
                .p_2p5()
                .rounded_md()
                .bg(rgb(shell.palette.panel))
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
                                .font_weight(FontWeight::BOLD)
                                .text_color(rgb(shell.palette.subtle))
                                .child(format!("候选点 · {}", candidates.len())),
                        )
                        .child(
                            div()
                                .text_color(rgb(shell.palette.muted))
                                .child(format!("{} visits", total_visits)),
                        ),
                )
                .child(if candidates.is_empty() {
                    div()
                        .p_2()
                        .text_color(rgb(shell.palette.subtle))
                        .child("自动分析开启后，候选点和变化会在这里实时更新。")
                } else {
                    div().flex().flex_col().gap_1().children(
                        candidates.iter().take(6).enumerate().map(|(idx, entry)| {
                            let vtx = entry.vertex.as_deref().unwrap_or("pass");
                            let is_best = idx == 0;
                            let hover_vertex = entry.vertex.clone();
                            let trial_vertex = entry.vertex.clone();
                            let wr_pct = entry.winrate * 100.0;
                            let score_str = entry
                                .score_lead
                                .map(|s| format!("{:+.1}目", s))
                                .unwrap_or_default();
                            let pv_str = if entry.pv.is_empty() {
                                String::new()
                            } else {
                                format!(
                                    "PV: {}",
                                    entry
                                        .pv
                                        .iter()
                                        .take(4)
                                        .cloned()
                                        .collect::<Vec<_>>()
                                        .join(" → ")
                                )
                            };

                            div()
                                .p_1p5()
                                .rounded_md()
                                .border_1()
                                .border_color(if is_best {
                                    rgb(shell.palette.accent)
                                } else {
                                    rgb(shell.palette.border)
                                })
                                .bg(if is_best {
                                    rgb(shell.palette.button_active)
                                } else {
                                    rgb(shell.palette.input)
                                })
                                .flex()
                                .flex_col()
                                .gap_0p5()
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .child(
                                            div()
                                                .font_weight(FontWeight::BOLD)
                                                .text_color(if is_best {
                                                    rgb(shell.palette.accent)
                                                } else {
                                                    rgb(shell.palette.text)
                                                })
                                                .child(format!("#{} {}", idx + 1, vtx)),
                                        )
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap_1()
                                                .child(
                                                    div()
                                                        .text_color(rgb(shell.palette.text))
                                                        .child(format!(
                                                            "{:.1}% · {} · {}v",
                                                            wr_pct, score_str, entry.visits
                                                        )),
                                                )
                                                .children((!entry.pv.is_empty()).then(|| {
                                                    let pv_clone = entry.pv.clone();
                                                    Button::new(("branch-btn", idx))
                                                        .xsmall()
                                                        .outline()
                                                        .label("+ 分支")
                                                        .tooltip(
                                                            "将此 AI 推荐变化图作为新分支加入棋谱",
                                                        )
                                                        .on_click(cx.listener(
                                                            move |shell, _, _, cx| {
                                                                shell.on_branch_candidate_pv(
                                                                    &pv_clone, cx,
                                                                );
                                                            },
                                                        ))
                                                }))
                                                .children(trial_vertex.map(|vertex| {
                                                    let vertex_for_handler = vertex.clone();
                                                    Button::new(("trial-btn", idx))
                                                        .xsmall()
                                                        .primary()
                                                        .label("试下")
                                                        .tooltip(
                                                            "在棋盘上试下此点并让 AI 给出即时应对",
                                                        )
                                                        .on_click(cx.listener(
                                                            move |shell, _, _, cx| {
                                                                shell.on_trial_candidate(
                                                                    &vertex_for_handler,
                                                                    cx,
                                                                );
                                                            },
                                                        ))
                                                })),
                                        ),
                                )
                                .children((!pv_str.is_empty()).then(|| {
                                    div().text_color(rgb(shell.palette.subtle)).child(pv_str)
                                }))
                                .id(("candidate-row", idx))
                                .on_hover(cx.listener(
                                    move |shell, is_hovering: &bool, _window, cx| {
                                        if *is_hovering {
                                            shell.set_hovered_candidate(hover_vertex.clone(), cx);
                                        } else if let Some(vertex) = hover_vertex.as_deref() {
                                            shell.clear_hovered_candidate_if(vertex, cx);
                                        }
                                    },
                                ))
                        }),
                    )
                }),
        )
        // Section 4: Full-Game Review Statistics Summary (KaTrain / sgf2gif)
        .child(
            div()
                .p_2p5()
                .rounded_md()
                .bg(rgb(shell.palette.panel))
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
                                .font_weight(FontWeight::BOLD)
                                .text_color(rgb(shell.palette.subtle))
                                .child("整局好坏棋复盘 (SGF2GIF 统计)"),
                        )
                        .child(
                            div()
                                .text_color(rgb(shell.palette.muted))
                                .child(format!("共 {} 手", summary.total_moves)),
                        ),
                )
                // KaTrain Category Counts Grid
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .flex()
                                .justify_between()
                                .text_color(rgb(shell.palette.muted))
                                .child("评级")
                                .child("黑棋")
                                .child("白棋"),
                        )
                        .child(render_stat_row(
                            "🌟 最佳 (Best)",
                            summary.black_best_count,
                            summary.white_best_count,
                            0x10b981,
                            shell,
                        ))
                        .child(render_stat_row(
                            "🟢 好手 (Good)",
                            summary.black_good_count,
                            summary.white_good_count,
                            0x0ea5e9,
                            shell,
                        ))
                        .child(render_stat_row(
                            "🟡 略亏 (Inaccuracy)",
                            summary.black_inaccuracy_count,
                            summary.white_inaccuracy_count,
                            0xf59e0b,
                            shell,
                        ))
                        .child(render_stat_row(
                            "🟠 疑问 (Mistake)",
                            summary.black_mistake_count,
                            summary.white_mistake_count,
                            0xf97316,
                            shell,
                        ))
                        .child(render_stat_row(
                            "🔴 恶手 (Blunder)",
                            summary.black_blunder_count,
                            summary.white_blunder_count,
                            0xef4444,
                            shell,
                        )),
                )
                // Averages
                .child(
                    div()
                        .mt_1()
                        .p_1()
                        .rounded_sm()
                        .bg(rgb(shell.palette.input))
                        .flex()
                        .justify_between()
                        .child(div().child(format!("黑均损: {:.1}目", summary.black_avg_loss)))
                        .child(div().child(format!("白均损: {:.1}目", summary.white_avg_loss))),
                )
                // Export GIF Button
                .child(
                    Button::new("export-gif-button")
                        .small()
                        .outline()
                        .label("🎬 导出动画 GIF 棋谱 (sgf2gif)...")
                        .on_click(cx.listener(|shell, _, window, cx| {
                            shell.on_export_gif_action(&MouseDownEvent::default(), window, cx);
                        })),
                ),
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
                    shell.engine_store.list().iter().enumerate().map(|(idx, record)| {
                        let name = record.name.clone();
                        let connected = shell.engine_controller.any_attached();
                        let connect_name = name.clone();
                        let remove_name = name.clone();
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(format!("{} ({})", record.name, record.path))
                            .child(if connected {
                                div().child(Badge::new().small().child("connected"))
                            } else {
                                div().child(
                                    Button::new(("engine-connect", idx))
                                        .xsmall()
                                        .outline()
                                        .label("connect")
                                        .on_click(cx.listener(move |shell, _, _, cx| {
                                            let _ = &connect_name;
                                            let role = shell.active_console_role.unwrap_or(
                                                crate::engine_console::EngineRole::Analysis,
                                            );
                                            shell.on_engine_connect(role, cx);
                                        }))
                                )
                            })
                            .child(
                                Button::new(("engine-remove", idx))
                                    .xsmall()
                                    .danger()
                                    .label("remove")
                                    .on_click(cx.listener(move |shell, _, _, cx| {
                                        shell.on_engine_remove(&remove_name, cx);
                                    }))
                            )
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
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(ShellApp::on_node_title_focus),
                )
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
                .child(
                    div().flex().flex_wrap().gap_1().children(
                        NodeAnnotation::MOVE
                            .iter()
                            .enumerate()
                            .map(|(idx, annotation)| {
                                let ann = *annotation;
                                let active = metadata.move_annotation == Some(ann);
                                Button::new(("move-eval", idx))
                                    .small()
                                    .ghost()
                                    .selected(active)
                                    .label(ann.label())
                                    .on_click(cx.listener(move |shell, _, _, cx| {
                                        shell.on_node_annotation(ann, cx)
                                    }))
                            }),
                    ),
                )
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(shell.palette.subtle))
                        .child("Position Evaluation"),
                )
                .child(
                    div().flex().flex_wrap().gap_1().children(
                        NodeAnnotation::POSITION
                            .iter()
                            .enumerate()
                            .map(|(idx, annotation)| {
                                let ann = *annotation;
                                let active = metadata.position_annotation == Some(ann);
                                Button::new(("pos-eval", idx))
                                    .small()
                                    .ghost()
                                    .selected(active)
                                    .label(ann.label())
                                    .on_click(cx.listener(move |shell, _, _, cx| {
                                        shell.on_node_annotation(ann, cx)
                                    }))
                            }),
                    ),
                )
                .child(
                    Button::new("commentbox-hotspot")
                        .small()
                        .ghost()
                        .selected(metadata.hotspot)
                        .label(if metadata.hotspot {
                            "⭐ Hotspot"
                        } else {
                            "☆ Hotspot"
                        })
                        .on_click(cx.listener(|shell, _, _, cx| shell.on_hotspot_toggle(cx))),
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
                        .child(
                            div()
                                .text_color(rgb(shell.palette.text))
                                .child(row.value.clone()),
                        )
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
                    Button::new("variation-promote")
                        .small()
                        .outline()
                        .label("Promote")
                        .on_click(cx.listener(|shell, _, window, cx| {
                            shell.on_variation_promote(&MouseDownEvent::default(), window, cx);
                        })),
                )
                .child(
                    Button::new("variation-remove")
                        .small()
                        .danger()
                        .label("Remove")
                        .on_click(cx.listener(|shell, _, window, cx| {
                            shell.on_variation_remove(&MouseDownEvent::default(), window, cx);
                        })),
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
                        .children(
                            crate::THEME_CHOICES
                                .iter()
                                .enumerate()
                                .map(|(idx, choice)| {
                                    let is_active = *choice == shell.theme_choice;
                                    let ch = *choice;
                                    Button::new(("theme-choice", idx))
                                        .small()
                                        .ghost()
                                        .selected(is_active)
                                        .label(choice.label().to_owned())
                                        .on_click(cx.listener(move |shell, _, _, cx| {
                                            shell.on_theme_selected(ch, cx);
                                        }))
                                }),
                        ),
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
                            div()
                                .text_xs()
                                .text_color(rgb(shell.palette.danger_text))
                                .child(format!(
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
                    div().flex().gap_1p5().children(
                        crate::BOARD_SIZE_OPTIONS
                            .iter()
                            .enumerate()
                            .map(|(idx, size)| {
                                let is_active = *size == shell.board_size;
                                let s = *size;
                                Button::new(("board-size-choice", idx))
                                    .small()
                                    .ghost()
                                    .selected(is_active)
                                    .label(format!("{size} × {size}"))
                                    .on_click(cx.listener(move |shell, _, _, cx| {
                                        shell.on_board_size_selected(s, cx);
                                    }))
                            }),
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
                        .child("SCORING MODE"),
                )
                .child(
                    div().flex().gap_1().child(
                        Button::new("scoring-mode-toggle-button")
                            .small()
                            .ghost()
                            .selected(shell.mode == GameMode::Scoring)
                            .label(if shell.mode == GameMode::Scoring {
                                "Scoring: ON"
                            } else {
                                "Scoring: OFF"
                            })
                            .on_click(cx.listener(|shell, _, window, cx| {
                                shell.on_scoring_mode_toggle(
                                    &MouseDownEvent::default(),
                                    window,
                                    cx,
                                );
                            })),
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
                .child(div().flex().flex_col().gap_2().children(
                    shell.installed_plugins.iter().map(|plugin| {
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
                                                    .child(format!(
                                                        "{} v{}",
                                                        plugin.name, plugin.version
                                                    )),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_1()
                                            .children(
                                                (!plugin.missing_permissions.is_empty()).then(
                                                    || {
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
                                                            .hover(|style| {
                                                                style.bg(rgb(shell
                                                                    .palette
                                                                    .button_active))
                                                            })
                                                            .child("🛡️ 授予权限")
                                                            .on_mouse_down(
                                                                MouseButton::Left,
                                                                grant_listener,
                                                            )
                                                    },
                                                ),
                                            )
                                            .children(
                                                (plugin.native_runtime
                                                    && !plugin.native_authorized)
                                                    .then(|| {
                                                        div()
                                                            .px_2()
                                                            .py_0p5()
                                                            .rounded_md()
                                                            .cursor_pointer()
                                                            .border_1()
                                                            .border_color(rgb(shell
                                                                .palette
                                                                .danger_text))
                                                            .bg(rgb(shell.palette.danger))
                                                            .text_xs()
                                                            .font_weight(FontWeight::MEDIUM)
                                                            .text_color(rgb(shell
                                                                .palette
                                                                .danger_text))
                                                            .child("⚠️ 授权原生进程")
                                                            .on_mouse_down(
                                                                MouseButton::Left,
                                                                authorize_listener,
                                                            )
                                                    }),
                                            )
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
                                                    .hover(|style| {
                                                        style.bg(rgb(shell.palette.button))
                                                    })
                                                    .child(if plugin.enabled {
                                                        "已启用 (Disable)"
                                                    } else {
                                                        "已禁用 (Enable)"
                                                    })
                                                    .on_mouse_down(
                                                        MouseButton::Left,
                                                        toggle_listener,
                                                    ),
                                            ),
                                    ),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(shell.palette.subtle))
                                    .child(desc),
                            )
                    }),
                ))
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

fn vertex_label(vertex: Option<Vertex>, board_width: usize) -> String {
    vertex.map_or_else(
        || "pass".to_owned(),
        |vertex| {
            let column =
                (b'A' + vertex.column as u8 + usize::from(vertex.column >= 8) as u8) as char;
            format!("{column}{}", board_width.saturating_sub(vertex.row))
        },
    )
}

/// Compact right-sidebar board for the currently hovered engine candidate.
/// It is intentionally read-only: the real SGF document and host revision are
/// never changed by hovering. The PV is resolved from the live candidate list,
/// so streaming analysis updates the numbered ghost sequence in place.
pub fn render_analysis_preview_panel(
    snapshot: &GameSnapshot,
    theme: &crate::theme::ThemeTokens,
    shell: &ShellApp,
    cx: &Context<ShellApp>,
) -> Stateful<Div> {
    let selected = shell
        .hovered_candidate_vertex
        .as_deref()
        .and_then(|vertex| {
            shell
                .analysis
                .iter()
                .find(|entry| entry.vertex.as_deref() == Some(vertex))
        });
    let response_entry = selected.or_else(|| {
        shell
            .trial_move
            .as_ref()
            .and_then(|_| crate::engine_console::best_analysis_entry(&shell.analysis))
    });
    let preview = response_entry.map(|entry| {
        let mut pv_preview = if let Some(trial_move) = shell.trial_move.as_ref() {
            trial_move
                .vertex
                .map(|vertex| vec![(vertex, trial_move.color, 1)])
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let response = pv_preview_points(
            snapshot.board.width,
            shell
                .trial_move
                .as_ref()
                .map_or(snapshot.board.next_player, |trial| trial.color.opponent()),
            &entry.pv,
            8,
        );
        let offset = pv_preview.len();
        pv_preview.extend(
            response
                .into_iter()
                .enumerate()
                .map(|(index, (vertex, color, _))| (vertex, color, offset + index + 1)),
        );
        (
            if let Some(trial_move) = shell.trial_move.as_ref() {
                format!(
                    "试下 {}",
                    vertex_label(trial_move.vertex, snapshot.board.width)
                )
            } else {
                entry.vertex.clone().unwrap_or_else(|| "pass".to_owned())
            },
            pv_preview,
        )
    });
    let preview_board_size = (shell.right_sidebar_width - 24.0).clamp(180.0, 360.0);
    let preview_panel_height = preview_board_size + 42.0;
    let preview_stones = preview
        .as_ref()
        .map(|(_, pv_preview)| pv_preview.clone())
        .unwrap_or_default();
    let mut preview_board = snapshot.board.clone();
    if preview.is_none() {
        // An idle preview is deliberately an empty board, not a duplicate of
        // the main goban. This makes the panel read as an analysis viewport.
        preview_board.sign_map = vec![vec![0; preview_board.width]; preview_board.height];
        preview_board.current_vertex = None;
    }
    let board = {
        let options = crate::goban_view::GobanRenderOptions {
            show_coordinates: false,
            coordinates_type: "A1".to_owned(),
            pv_preview: preview_stones,
            ..Default::default()
        };
        render_goban_with_id(
            "analysis-preview-goban",
            &preview_board,
            preview_board_size,
            theme,
            &options,
        )
    };

    div()
        .id("analysis-preview-panel")
        .debug_selector(|| "analysis-preview-panel".to_owned())
        .flex_none()
        .h(px(preview_panel_height))
        .p_2p5()
        .border_b_1()
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
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(shell.palette.subtle))
                        .child("AI 变化预览"),
                )
                .child(if shell.trial_move.is_some() {
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(
                            div().text_xs().text_color(rgb(shell.palette.accent)).child(
                                preview
                                    .as_ref()
                                    .map(|(vertex, _)| vertex.clone())
                                    .unwrap_or_else(|| "试下".to_owned()),
                            ),
                        )
                        .child(
                            Button::new("exit-trial-button")
                                .xsmall()
                                .danger()
                                .label("退出")
                                .tooltip("退出试下局面并恢复当前对局分析")
                                .on_click(
                                    cx.listener(|shell, _, _, cx| shell.clear_trial_move(cx)),
                                ),
                        )
                } else if let Some((vertex, _)) = preview.as_ref() {
                    div().child(Badge::new().small().child(vertex.clone()))
                } else {
                    div().child(Badge::new().small().child("待机"))
                }),
        )
        .child(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .items_center()
                .justify_center()
                .child(board),
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
    let best_move = shell.trial_move.is_none().then_some(best_move).flatten();
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
    let analysis_candidates = if shell.trial_move.is_none()
        && shell
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
                    winrate_percent: entry.winrate * 100.0,
                    visits: entry.visits,
                    score_lead: entry.score_lead,
                    is_best: index == 0 || Some(vertex) == best_move,
                })
            })
            .take(8)
            .collect()
    } else {
        Vec::new()
    };
    let hover_stone_color = matches!(
        shell.mode,
        GameMode::Play | GameMode::Guess | GameMode::Autoplay
    )
    .then_some(board.next_player);
    let evaluations = sabaki_host::compute_game_move_evaluations(snapshot);
    let mut eval_dots = std::collections::BTreeMap::new();
    for eval in &evaluations {
        if let Some(vtx_str) = eval.played_vertex.as_deref()
            && let Some(vtx) = parse_gtp_vertex(board.width, vtx_str)
        {
            eval_dots.insert(
                sabaki_domain_core::Vertex {
                    column: vtx.0,
                    row: vtx.1,
                },
                eval.quality,
            );
        }
    }

    let mut pv_preview = shell
        .hovered_candidate_vertex
        .as_deref()
        .and_then(|vertex| {
            shell
                .analysis
                .iter()
                .find(|entry| entry.vertex.as_deref() == Some(vertex))
                .map(|entry| pv_preview_points(board.width, board.next_player, &entry.pv, 6))
        })
        .unwrap_or_default();
    if let Some(trial_move) = shell.trial_move.as_ref() {
        let mut trial_preview = trial_move
            .vertex
            .map_or_else(Vec::new, |vertex| vec![(vertex, trial_move.color, 1)]);
        let response = shell
            .analysis
            .first()
            .map(|entry| pv_preview_points(board.width, trial_move.color.opponent(), &entry.pv, 6))
            .unwrap_or_default();
        trial_preview.extend(
            response
                .into_iter()
                .enumerate()
                .map(|(index, (vertex, color, _))| (vertex, color, index + 2)),
        );
        pv_preview = trial_preview;
    }

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
        eval_dots,
        pv_preview,
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
                    Switch::new(gpui::SharedString::from(row.key.clone()))
                        .small()
                        .checked(is_on)
                        .on_click(cx.listener(
                            move |shell,
                                  _checked: &bool,
                                  window: &mut Window,
                                  cx: &mut Context<ShellApp>| {
                                shell.on_settings_toggle(
                                    &row_for_listener,
                                    &MouseDownEvent::default(),
                                    window,
                                    cx,
                                )
                            },
                        )),
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
