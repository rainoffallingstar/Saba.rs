//! Panel rendering for the shell, split out of `main.rs` so the shell keeps
//! only state, actions and assembly. Every function is a pure view over
//! `ShellApp` state plus precomputed values; listeners are built with
//! `cx.listener` against `ShellApp` handlers.

use std::rc::Rc;

use gpui::{
    App, Context, Div, FontWeight, InteractiveElement, MouseButton, MouseDownEvent, PathBuilder,
    Stateful, StatefulInteractiveElement, Window, canvas, div, hsla, point, prelude::*, px, rgb,
};

use ryusei_domain_core::{GameMode, GameSnapshot};

use gpui_component::badge::Badge;
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::{Selectable, Sizable};

use crate::ShellApp;
use crate::engine_console::{best_analysis_winrate, parse_gtp_vertex};
use crate::goban_view::{
    pv_preview_points, render_goban, render_goban_click_layer, render_goban_with_id,
};
use crate::layout::SplitPane;
use crate::native_text_input::NativeInputBinding;
use crate::node_inspector::NodeInspectorMetadata;
use crate::plugin_panel::PluginPanelEntry;
use crate::theme::UiPalette;
use crate::variation_tree::{VariationTreeLayout, render_variation_tree};
use crate::winrate_graph::{GraphPlotPoint, WinrateGraphMetric};

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
    let is_black_turn = snapshot.board.next_player == ryusei_domain_core::Color::Black;
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
                    Button::new("export-gif-toolbar-button")
                        .small()
                        .ghost()
                        .label("🎬 导出 GIF")
                        .tooltip("导出当前对局为动画 GIF 棋谱 (Cmd+Shift+G)")
                        .on_click(cx.listener(|shell, _, window, cx| {
                            shell.on_export_gif_action(&MouseDownEvent::default(), window, cx);
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

fn render_katago_dialog(shell: &ShellApp, cx: &Context<ShellApp>) -> Div {
    let analysis_connected = shell
        .engine_controller
        .is_attached(crate::engine_console::EngineRole::Analysis);
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap_2p5()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .pb_2()
                .border_b_1()
                .border_color(rgb(0x2a2a30))
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(0xf2f2f5))
                        .child("⚡ KataGo AI 引擎与模型配置"),
                )
                .child(
                    div()
                        .id("plugin-menu-close")
                        .debug_selector(|| "plugin-menu-close".to_owned())
                        .child(
                            Button::new("plugin-menu-close-btn")
                                .small()
                                .ghost()
                                .label("✕")
                                .on_click(cx.listener(|shell, _, window, cx| {
                                    shell.close_plugin_popover(
                                        &MouseDownEvent::default(),
                                        window,
                                        cx,
                                    );
                                })),
                        ),
                ),
        )
        .child(
            div()
                .p_2p5()
                .rounded_md()
                .bg(rgb(0x121214))
                .border_1()
                .border_color(rgb(0x26262c))
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
                                .text_color(rgb(shell.palette.subtle))
                                .child("引擎运行状态:"),
                        )
                        .child(
                            Badge::new().small().child(if analysis_connected {
                                "● Kata-Analyze 已连接"
                            } else {
                                "○ 待连接"
                            }),
                        ),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(0x9a9a9a))
                        .child("支持自动下载 KataGo 原生引擎，并根据本机硬件自动匹配 Metal (Apple Silicon) / OpenCL / CUDA 加速。"),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    Button::new("katago-setup-action-btn")
                        .small()
                        .primary()
                        .label("⚡ 一键配置 / 诊断环境与模型")
                        .tooltip("自动检查环境、下载最新模型并配置引擎")
                        .on_click(cx.listener(|shell, _, _, cx| {
                            shell.on_plugin_command(
                                "org.ryusei.katago-setup-hub",
                                "katago.setup",
                                cx,
                            );
                        })),
                )
                .child(
                    Button::new("katago-connect-analysis-btn")
                        .small()
                        .outline()
                        .label(if analysis_connected {
                            "⏹ 断开分析"
                        } else {
                            "🔌 连接为分析引擎"
                        })
                        .on_click(cx.listener(move |shell, _, window, cx| {
                            if shell
                                .engine_controller
                                .is_attached(crate::engine_console::EngineRole::Analysis)
                            {
                                shell.on_engine_disconnect(
                                    crate::engine_console::EngineRole::Analysis,
                                    &MouseDownEvent::default(),
                                    window,
                                    cx,
                                );
                            } else {
                                shell.on_engine_connect(
                                    crate::engine_console::EngineRole::Analysis,
                                    cx,
                                );
                            }
                        })),
                )
                .child(
                    Button::new("katago-open-terminal-btn")
                        .small()
                        .ghost()
                        .label("💻 GTP 终端")
                        .on_click(cx.listener(|shell, _, window, cx| {
                            shell.toggle_gtp_terminal(&MouseDownEvent::default(), window, cx);
                        })),
                ),
        )
}

fn render_fox_sync_dialog(shell: &ShellApp, cx: &Context<ShellApp>) -> Div {
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap_2p5()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .pb_2()
                .border_b_1()
                .border_color(rgb(0x2a2a30))
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(0xf2f2f5))
                        .child("🦊 野狐围棋对局同步与查询"),
                )
                .child(
                    div()
                        .id("plugin-menu-close")
                        .debug_selector(|| "plugin-menu-close".to_owned())
                        .child(
                            Button::new("plugin-menu-close-btn")
                                .small()
                                .ghost()
                                .label("✕")
                                .on_click(cx.listener(|shell, _, window, cx| {
                                    shell.close_plugin_popover(
                                        &MouseDownEvent::default(),
                                        window,
                                        cx,
                                    );
                                })),
                        ),
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
                .flex_col()
                .gap_1p5()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(shell.palette.subtle))
                        .child("野狐对局用户查询"),
                )
                .child(
                    div()
                        .flex()
                        .gap_1p5()
                        .child(
                            div()
                                .flex_1()
                                .track_focus(&shell.fox_query_focus_handle)
                                .key_context("FoxQueryInput")
                                .px_3()
                                .py_1p5()
                                .rounded_md()
                                .border_1()
                                .border_color(rgb(
                                    if shell.active_text_input
                                        == Some(crate::ActiveTextInput::FoxQuery)
                                    {
                                        0x38bdf8
                                    } else {
                                        0x26262c
                                    },
                                ))
                                .bg(rgb(0x121214))
                                .text_xs()
                                .text_color(rgb(0xf4f4f5))
                                .child(if shell.fox_query_input.text().is_empty() {
                                    div()
                                        .text_color(rgb(0x71717a))
                                        .child("输入野狐用户名或用户 ID (按 Enter 搜索)")
                                } else {
                                    div()
                                        .text_color(rgb(0xf4f4f5))
                                        .child(shell.fox_query_input.text().to_owned())
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
                            Button::new("fox-search-btn")
                                .small()
                                .primary()
                                .label("🔍 查询")
                                .on_click(cx.listener(|shell, _, _, cx| {
                                    shell.fetch_fox_query(cx);
                                })),
                        ),
                ),
        )
        .child(
            div().flex().gap_1p5().child(
                Button::new("fox-sync-recent-btn")
                    .small()
                    .outline()
                    .label("🔄 同步最近对局")
                    .on_click(cx.listener(|shell, _, _, cx| {
                        shell.on_plugin_command("org.ryusei.fox-kifu-sync", "fox.query_games", cx);
                    })),
            ),
        )
}

fn render_position_to_sgf_dialog(shell: &ShellApp, cx: &Context<ShellApp>) -> Div {
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap_2p5()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .pb_2()
                .border_b_1()
                .border_color(rgb(0x2a2a30))
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(0xf2f2f5))
                        .child("📋 局面转 SGF / 剪贴板导出"),
                )
                .child(
                    div()
                        .id("plugin-menu-close")
                        .debug_selector(|| "plugin-menu-close".to_owned())
                        .child(
                            Button::new("plugin-menu-close-btn")
                                .small()
                                .ghost()
                                .label("✕")
                                .on_click(cx.listener(|shell, _, window, cx| {
                                    shell.close_plugin_popover(
                                        &MouseDownEvent::default(),
                                        window,
                                        cx,
                                    );
                                })),
                        ),
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
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(shell.palette.subtle))
                        .child("当前盘面导出选项:"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(shell.palette.text))
                        .child(format!(
                            "棋盘路数: {} × {} · 已落子: {} 手",
                            shell.host.snapshot().board.width,
                            shell.host.snapshot().board.height,
                            shell.host.snapshot().moves.len()
                        )),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1p5()
                .child(
                    Button::new("export-full-sgf-btn")
                        .small()
                        .primary()
                        .label("📋 复制完整对局 SGF 到剪贴板")
                        .on_click(cx.listener(|shell, _, _, cx| {
                            shell.on_plugin_command(
                                "org.ryusei.position-to-sgf",
                                "export.clipboard_sgf",
                                cx,
                            );
                        })),
                )
                .child(
                    Button::new("export-pos-sgf-btn")
                        .small()
                        .outline()
                        .label("🎯 仅复制当前死活/初始局面 (AB/AW)")
                        .on_click(cx.listener(|shell, _, _, cx| {
                            shell.on_plugin_command(
                                "org.ryusei.position-to-sgf",
                                "export.clipboard_position",
                                cx,
                            );
                        })),
                )
                .child(
                    Button::new("export-gif-modal-btn")
                        .small()
                        .ghost()
                        .label("🎬 导出动画 GIF 棋谱…")
                        .on_click(cx.listener(|shell, _, window, cx| {
                            shell.on_export_gif_action(&MouseDownEvent::default(), window, cx);
                        })),
                ),
        )
}

fn render_generic_plugin_dialog(
    plugin: &PluginPanelEntry,
    shell: &ShellApp,
    cx: &Context<ShellApp>,
) -> Div {
    let plugin_id = plugin.plugin_id.clone();
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap_2p5()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .pb_2()
                .border_b_1()
                .border_color(rgb(0x2a2a30))
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(0xf2f2f5))
                        .child(format!(
                            "{} {}",
                            plugin_icon(&plugin.plugin_id),
                            plugin.name
                        )),
                )
                .child(
                    div()
                        .id("plugin-menu-close")
                        .debug_selector(|| "plugin-menu-close".to_owned())
                        .child(
                            Button::new("plugin-menu-close-btn")
                                .small()
                                .ghost()
                                .label("✕")
                                .on_click(cx.listener(|shell, _, window, cx| {
                                    shell.close_plugin_popover(
                                        &MouseDownEvent::default(),
                                        window,
                                        cx,
                                    );
                                })),
                        ),
                ),
        )
        .child(
            div()
                .p_2p5()
                .rounded_md()
                .bg(rgb(shell.palette.input))
                .text_xs()
                .text_color(rgb(shell.palette.subtle))
                .child(plugin_description(&plugin.plugin_id)),
        )
        .children((!plugin.commands.is_empty()).then(|| {
            div().flex().flex_col().gap_1p5().children(
                plugin
                    .command_ids
                    .iter()
                    .zip(plugin.commands.iter())
                    .enumerate()
                    .map(|(idx, (command_id, title))| {
                        let pid = plugin_id.clone();
                        let cid = command_id.clone();
                        Button::new(("generic-plugin-cmd", idx))
                            .small()
                            .primary()
                            .label(title.clone())
                            .on_click(cx.listener(move |shell, _, _, cx| {
                                shell.on_plugin_command(&pid, &cid, cx);
                            }))
                    }),
            )
        }))
}

fn render_pinned_plugins_manager(shell: &ShellApp, cx: &Context<ShellApp>) -> Div {
    let pinned_ids = shell.pinned_plugin_ids();
    div()
        .w_full()
        .max_h(px(420.0))
        .flex()
        .flex_col()
        .gap_2()
        .child(
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
                        .flex_col()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::BOLD)
                                .text_color(rgb(0xf2f2f5))
                                .child("🧩 插件栏管理"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(0x9a9a9a))
                                .child("选择固定到底部操作栏的快捷插件"),
                        ),
                )
                .child(
                    div()
                        .id("plugin-menu-close")
                        .debug_selector(|| "plugin-menu-close".to_owned())
                        .child(
                            Button::new("plugin-menu-close-btn")
                                .small()
                                .ghost()
                                .label("✕")
                                .on_click(cx.listener(|shell, _, window, cx| {
                                    shell.close_plugin_popover(
                                        &MouseDownEvent::default(),
                                        window,
                                        cx,
                                    );
                                })),
                        ),
                ),
        )
        .child(if shell.installed_plugins.is_empty() {
            div()
                .id("plugin-list-empty")
                .p_3()
                .rounded_md()
                .bg(rgb(shell.palette.input))
                .text_xs()
                .text_color(rgb(shell.palette.subtle))
                .child("暂无已安装的扩展插件。可在顶部菜单【Plugins】中安装。")
        } else {
            div()
                .id("plugin-list-scroll")
                .flex()
                .flex_col()
                .gap_1p5()
                .overflow_y_scroll()
                .children(
                    shell
                        .installed_plugins
                        .iter()
                        .enumerate()
                        .map(|(idx, plugin)| {
                            let plugin_id = plugin.plugin_id.clone();
                            let is_pinned = pinned_ids.contains(&plugin_id);
                            let toggle_id = plugin_id.clone();
                            let desc = plugin_description(&plugin_id);

                            div()
                                .p_2()
                                .rounded_md()
                                .bg(rgb(shell.palette.input))
                                .border_1()
                                .border_color(rgb(if is_pinned {
                                    shell.palette.accent
                                } else {
                                    shell.palette.border
                                }))
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap_2()
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .min_w_0()
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
                                                .text_xs()
                                                .text_color(rgb(shell.palette.subtle))
                                                .child(desc),
                                        ),
                                )
                                .child(
                                    Button::new(("pin-toggle", idx))
                                        .small()
                                        .ghost()
                                        .selected(is_pinned)
                                        .label(if is_pinned {
                                            "📌 已固定"
                                        } else {
                                            "固定到栏"
                                        })
                                        .on_click(cx.listener(move |shell, _, _, cx| {
                                            shell.toggle_plugin_pinned(&toggle_id, cx);
                                        })),
                                )
                        }),
                )
        })
}

/// Renders a compact WinrateGraph from the current variation's persisted and
/// Renders an OGS-style Winrate & Score Lead Graph with coordinate ticks,
/// reference baselines, split advantage shading, and move number markers.
pub fn render_winrate_graph_panel(
    points: &[GraphPlotPoint],
    metric: WinrateGraphMetric,
    height: f32,
    palette: UiPalette,
    on_node_clicked: impl Fn(&ryusei_domain_core::NodeId, &mut Window, &mut App) + 'static,
    cx: &Context<ShellApp>,
) -> Stateful<Div> {
    let on_node_clicked = Rc::new(on_node_clicked);
    let has_values = points.iter().any(|point| point.y.is_some());
    let last_index = points.len().saturating_sub(1).max(1) as f32;
    let total_moves = points.len();

    let y_labels = match metric {
        WinrateGraphMetric::Winrate => ["100%", "75%", "50%", "25%", "0%"],
        WinrateGraphMetric::ScoreLead => ["+30", "+15", "0.0", "-15", "-30"],
    };

    // Calculate move number ticks along X-axis
    let x_step = if total_moves <= 50 {
        10
    } else if total_moves <= 150 {
        25
    } else if total_moves <= 300 {
        50
    } else {
        100
    };

    div()
        .id("winrate-graph-panel")
        .debug_selector(|| "winrate-graph-panel".to_owned())
        .flex_none()
        .h(px(height))
        .min_h_0()
        .flex()
        .flex_col()
        .gap_1p5()
        .p_2p5()
        .rounded_md()
        .bg(rgb(palette.panel))
        .border_1()
        .border_color(rgb(palette.border))
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
                                .label("胜率走势 (%)")
                                .on_click(cx.listener(|shell, _, _, cx| {
                                    shell.set_winrate_metric(WinrateGraphMetric::Winrate, cx);
                                })),
                        )
                        .child(
                            Button::new("metric-tab-score")
                                .xsmall()
                                .ghost()
                                .selected(metric == WinrateGraphMetric::ScoreLead)
                                .label("目差走势 (目)")
                                .on_click(cx.listener(|shell, _, _, cx| {
                                    shell.set_winrate_metric(WinrateGraphMetric::ScoreLead, cx);
                                })),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(rgb(0x0ea5e9))
                                .child(if metric == WinrateGraphMetric::Winrate {
                                    "黑优 ↑ / 白优 ↓"
                                } else {
                                    "+黑领先 / -白领先"
                                }),
                        )
                        .children(
                            has_values
                                .then(|| Badge::new().small().child(format!("{} 手", total_moves))),
                        ),
                ),
        )
        .child(if has_values {
            let graph_points = points.to_vec();
            let graph_accent = 0x0ea5e9; // OGS / KaTrain sky blue
            let graph_danger = 0xef4444; // Blunder red

            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .gap_1()
                        .child(
                            // Y-Axis Coordinate Labels (Left Column)
                            div()
                                .w(px(32.0))
                                .h_full()
                                .flex()
                                .flex_col()
                                .justify_between()
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(rgb(0x9a9a9a))
                                .child(div().text_color(rgb(0x0ea5e9)).child(y_labels[0]))
                                .child(div().child(y_labels[1]))
                                .child(div().text_color(rgb(0xd4d4d8)).child(y_labels[2]))
                                .child(div().child(y_labels[3]))
                                .child(div().text_color(rgb(0xf59e0b)).child(y_labels[4])),
                        )
                        .child(
                            // Graph Plot Canvas & Scrub Area
                            div()
                                .id("winrate-graph-plot")
                                .relative()
                                .flex_1()
                                .h_full()
                                .rounded_md()
                                .bg(rgb(0x121214))
                                .border_1()
                                .border_color(rgb(0x26262c))
                                .child(
                                    canvas(
                                        |_, _, _| (),
                                        move |bounds, (), window, _cx| {
                                            // 25% and 75% reference dashed grid lines
                                            let mut grid_25 = PathBuilder::stroke(px(1.0))
                                                .dash_array(&[px(2.0), px(4.0)]);
                                            grid_25.move_to(point(
                                                bounds.origin.x,
                                                bounds.origin.y + bounds.size.height * 0.25,
                                            ));
                                            grid_25.line_to(point(
                                                bounds.origin.x + bounds.size.width,
                                                bounds.origin.y + bounds.size.height * 0.25,
                                            ));
                                            if let Ok(path) = grid_25.build() {
                                                window.paint_path(path, hsla(0.0, 0.0, 1.0, 0.07));
                                            }

                                            let mut grid_75 = PathBuilder::stroke(px(1.0))
                                                .dash_array(&[px(2.0), px(4.0)]);
                                            grid_75.move_to(point(
                                                bounds.origin.x,
                                                bounds.origin.y + bounds.size.height * 0.75,
                                            ));
                                            grid_75.line_to(point(
                                                bounds.origin.x + bounds.size.width,
                                                bounds.origin.y + bounds.size.height * 0.75,
                                            ));
                                            if let Ok(path) = grid_75.build() {
                                                window.paint_path(path, hsla(0.0, 0.0, 1.0, 0.07));
                                            }

                                            // 50% / 0.0 Center Baseline
                                            let mut baseline = PathBuilder::stroke(px(1.2));
                                            baseline.move_to(point(
                                                bounds.origin.x,
                                                bounds.origin.y + bounds.size.height * 0.5,
                                            ));
                                            baseline.line_to(point(
                                                bounds.origin.x + bounds.size.width,
                                                bounds.origin.y + bounds.size.height * 0.5,
                                            ));
                                            if let Ok(path) = baseline.build() {
                                                window.paint_path(path, hsla(0.0, 0.0, 1.0, 0.22));
                                            }

                                            let valid: Vec<(f32, f32, u32, bool, bool)> =
                                                graph_points
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
                                                        Some((
                                                            x,
                                                            y,
                                                            color,
                                                            point.is_current,
                                                            point.is_blunder,
                                                        ))
                                                    })
                                                    .collect();

                                            if valid.len() >= 2 {
                                                // OGS-style shaded area fills relative to center baseline (y = 0.5)
                                                // Upper Black advantage area
                                                let mut upper_area = PathBuilder::fill();
                                                if let Some(&(first_x, _, _, _, _)) = valid.first()
                                                {
                                                    upper_area.move_to(point(
                                                        bounds.origin.x
                                                            + bounds.size.width * first_x,
                                                        bounds.origin.y + bounds.size.height * 0.5,
                                                    ));
                                                    for &(x, y, _, _, _) in &valid {
                                                        let clamped_y = y.min(0.5);
                                                        upper_area.line_to(point(
                                                            bounds.origin.x + bounds.size.width * x,
                                                            bounds.origin.y
                                                                + bounds.size.height * clamped_y,
                                                        ));
                                                    }
                                                    if let Some(&(last_x, _, _, _, _)) =
                                                        valid.last()
                                                    {
                                                        upper_area.line_to(point(
                                                            bounds.origin.x
                                                                + bounds.size.width * last_x,
                                                            bounds.origin.y
                                                                + bounds.size.height * 0.5,
                                                        ));
                                                    }
                                                    upper_area.close();
                                                    if let Ok(path) = upper_area.build() {
                                                        window.paint_path(
                                                            path,
                                                            hsla(0.55, 0.85, 0.5, 0.18),
                                                        );
                                                    }
                                                }

                                                // Lower White advantage area
                                                let mut lower_area = PathBuilder::fill();
                                                if let Some(&(first_x, _, _, _, _)) = valid.first()
                                                {
                                                    lower_area.move_to(point(
                                                        bounds.origin.x
                                                            + bounds.size.width * first_x,
                                                        bounds.origin.y + bounds.size.height * 0.5,
                                                    ));
                                                    for &(x, y, _, _, _) in &valid {
                                                        let clamped_y = y.max(0.5);
                                                        lower_area.line_to(point(
                                                            bounds.origin.x + bounds.size.width * x,
                                                            bounds.origin.y
                                                                + bounds.size.height * clamped_y,
                                                        ));
                                                    }
                                                    if let Some(&(last_x, _, _, _, _)) =
                                                        valid.last()
                                                    {
                                                        lower_area.line_to(point(
                                                            bounds.origin.x
                                                                + bounds.size.width * last_x,
                                                            bounds.origin.y
                                                                + bounds.size.height * 0.5,
                                                        ));
                                                    }
                                                    lower_area.close();
                                                    if let Ok(path) = lower_area.build() {
                                                        window.paint_path(
                                                            path,
                                                            hsla(0.08, 0.85, 0.5, 0.15),
                                                        );
                                                    }
                                                }

                                                // Main smooth curve line
                                                let mut path = PathBuilder::stroke(px(2.0));
                                                for (index, (x, y, _, _, _)) in
                                                    valid.iter().enumerate()
                                                {
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

                                            // Render node dots & current move indicator
                                            for (x, y, color, is_current, is_blunder) in valid {
                                                let center = point(
                                                    bounds.origin.x + bounds.size.width * x,
                                                    bounds.origin.y + bounds.size.height * y,
                                                );

                                                if is_current {
                                                    // Vertical cursor line at active move
                                                    let mut cursor_line =
                                                        PathBuilder::stroke(px(1.5));
                                                    cursor_line
                                                        .move_to(point(center.x, bounds.origin.y));
                                                    cursor_line.line_to(point(
                                                        center.x,
                                                        bounds.origin.y + bounds.size.height,
                                                    ));
                                                    if let Ok(path) = cursor_line.build() {
                                                        window.paint_path(
                                                            path,
                                                            hsla(0.55, 0.9, 0.6, 0.65),
                                                        );
                                                    }
                                                }

                                                let dot_radius = if is_current {
                                                    px(4.5)
                                                } else if is_blunder {
                                                    px(3.5)
                                                } else {
                                                    px(2.0)
                                                };
                                                window.paint_quad(gpui::quad(
                                                    gpui::Bounds {
                                                        origin: point(
                                                            center.x - dot_radius,
                                                            center.y - dot_radius,
                                                        ),
                                                        size: gpui::size(
                                                            dot_radius * 2.0,
                                                            dot_radius * 2.0,
                                                        ),
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
                                                    move |_: &MouseDownEvent,
                                                          window: &mut Window,
                                                          cx: &mut App| {
                                                        handler(&node_id, window, cx);
                                                    },
                                                )
                                        })),
                                ),
                        ),
                )
                .child(
                    // X-Axis Move Number Ticks (Bottom Row)
                    div()
                        .pl(px(36.0))
                        .pr_1()
                        .flex()
                        .justify_between()
                        .text_xs()
                        .text_color(rgb(0x71717a))
                        .child("0")
                        .children((1..=4).filter_map(|i| {
                            let move_val = i * x_step;
                            (move_val < total_moves).then(|| div().child(move_val.to_string()))
                        }))
                        .child(format!("{total_moves}")),
                )
        } else {
            div()
                .flex_1()
                .min_h_0()
                .rounded_md()
                .bg(rgb(palette.input))
                .border_1()
                .border_color(rgb(palette.border))
                .p_3()
                .flex()
                .items_center()
                .justify_center()
                .text_xs()
                .text_color(rgb(palette.subtle))
                .child("暂无胜率记录。连接分析引擎或开启自动分析后实时生成走势图。")
        })
}

pub fn render_variation_tree_panel(
    layout: &VariationTreeLayout,
    grid_size: f32,
    node_size: f32,
    palette: UiPalette,
    shell: &ShellApp,
    cx: &Context<ShellApp>,
    on_node_clicked: impl Fn(&ryusei_domain_core::NodeId, &mut Window, &mut App) + 'static,
    on_node_context_requested: impl Fn(&ryusei_domain_core::NodeId, &mut Window, &mut App) + 'static,
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
    node_id: &ryusei_domain_core::NodeId,
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

    let scoring_result = ryusei_domain_core::scoring::score_board(
        &snapshot.board,
        Some(komi),
        &snapshot.score_overrides,
    );

    let katago_estimate = shell
        .analysis
        .first()
        .and_then(|e| e.ownership.as_deref())
        .and_then(|ownership| {
            ryusei_host::estimate_territory(
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
        "About Ryusei",
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
                            .child("Ryusei"),
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

/// The integrated bottom deck panel (second screen) revealed by clicking toolbar buttons.
pub fn render_bottom_deck_panel(
    _snapshot: &GameSnapshot,
    shell: &ShellApp,
    cx: &Context<ShellApp>,
) -> Stateful<Div> {
    let active_tab = shell.active_bottom_tab();

    div()
        .id("bottom-deck-panel")
        .debug_selector(|| "bottom-deck-panel".to_owned())
        .h(px(280.0))
        .flex_none()
        .flex()
        .flex_col()
        .border_t_1()
        .border_color(rgb(shell.palette.accent))
        .bg(rgb(0x18181c))
        .shadow_lg()
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            cx.stop_propagation();
        })
        .child(
            // Header Tab Switcher Bar inside the Deck
            div()
                .flex()
                .items_center()
                .justify_between()
                .px_3()
                .py_1p5()
                .border_b_1()
                .border_color(rgb(0x2a2a30))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(
                            Button::new("deck-tab-gtp")
                                .small()
                                .ghost()
                                .selected(active_tab == crate::BottomDeckTab::GtpTerminal)
                                .label("💻 GTP 终端")
                                .on_click(cx.listener(|shell, _, _, cx| {
                                    shell.switch_bottom_tab(crate::BottomDeckTab::GtpTerminal, cx);
                                })),
                        )
                        .child(
                            Button::new("deck-tab-katago")
                                .small()
                                .ghost()
                                .selected(active_tab == crate::BottomDeckTab::KataGo)
                                .label("⚡ KataGo 配置")
                                .on_click(cx.listener(|shell, _, _, cx| {
                                    shell.switch_bottom_tab(crate::BottomDeckTab::KataGo, cx);
                                })),
                        )
                        .child(
                            Button::new("deck-tab-fox")
                                .small()
                                .ghost()
                                .selected(active_tab == crate::BottomDeckTab::FoxSync)
                                .label("🦊 野狐对局")
                                .on_click(cx.listener(|shell, _, _, cx| {
                                    shell.switch_bottom_tab(crate::BottomDeckTab::FoxSync, cx);
                                })),
                        )
                        .child(
                            Button::new("deck-tab-sgf")
                                .small()
                                .ghost()
                                .selected(active_tab == crate::BottomDeckTab::PositionSgf)
                                .label("📋 局面转 SGF")
                                .on_click(cx.listener(|shell, _, _, cx| {
                                    shell.switch_bottom_tab(crate::BottomDeckTab::PositionSgf, cx);
                                })),
                        )
                        .child(
                            Button::new("deck-tab-plugins")
                                .small()
                                .ghost()
                                .selected(active_tab == crate::BottomDeckTab::PluginManager)
                                .label("🧩 插件管理")
                                .on_click(cx.listener(|shell, _, _, cx| {
                                    shell
                                        .switch_bottom_tab(crate::BottomDeckTab::PluginManager, cx);
                                })),
                        ),
                )
                .child(
                    div()
                        .id("plugin-menu-close")
                        .debug_selector(|| "plugin-menu-close".to_owned())
                        .child(
                            Button::new("plugin-menu-close-btn")
                                .small()
                                .ghost()
                                .label("✕ 收起第二屏")
                                .on_click(cx.listener(|shell, _, _, cx| {
                                    shell.close_bottom_deck(cx);
                                })),
                        ),
                ),
        )
        .child(
            div()
                .id("bottom-deck-body")
                .flex_1()
                .min_h_0()
                .p_3()
                .overflow_y_scroll()
                .child(match active_tab {
                    crate::BottomDeckTab::GtpTerminal => render_gtp_terminal_body(shell, cx),
                    crate::BottomDeckTab::KataGo => render_katago_dialog(shell, cx),
                    crate::BottomDeckTab::FoxSync => render_fox_sync_dialog(shell, cx),
                    crate::BottomDeckTab::PositionSgf => render_position_to_sgf_dialog(shell, cx),
                    crate::BottomDeckTab::PluginManager => render_pinned_plugins_manager(shell, cx),
                    crate::BottomDeckTab::Generic(ref other_id) => {
                        if let Some(plugin) = shell
                            .installed_plugins
                            .iter()
                            .find(|p| &p.plugin_id == other_id)
                        {
                            render_generic_plugin_dialog(plugin, shell, cx)
                        } else {
                            render_pinned_plugins_manager(shell, cx)
                        }
                    }
                }),
        )
}

fn render_gtp_terminal_body(shell: &ShellApp, cx: &Context<ShellApp>) -> Div {
    let selected_role = shell
        .active_console_role
        .unwrap_or(crate::engine_console::EngineRole::Analysis);
    let is_attached = shell.engine_controller.is_attached(selected_role);
    let assigned_name = shell.engine_roles.get(selected_role);
    let selected_label = assigned_name
        .map(|name| format!("{} · {name}", selected_role.label()))
        .unwrap_or_else(|| format!("{} (点击连接自动探测)", selected_role.label()));

    div()
        .w_full()
        .h_full()
        .flex()
        .flex_col()
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
                        .bg(rgb(shell.palette.input))
                        .border_1()
                        .border_color(rgb(shell.palette.border))
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
    let rule_config = ryusei_host::GameRuleConfig::from_root_properties(
        &snapshot.root_properties,
        snapshot.board.width,
    );
    let evaluations = ryusei_host::compute_game_move_evaluations(&snapshot);
    let summary = ryusei_host::GameAnalyticsSummary::from_evaluations(&evaluations);

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

    let winrate_metric = crate::winrate_graph::WinrateGraphMetric::from_setting(
        shell.settings.get_str("board.analysis_type"),
    );
    let live_player_winrate =
        crate::engine_console::best_analysis_entry(&shell.analysis).map(|entry| entry.winrate);
    let live_score_lead = crate::engine_console::best_analysis_entry(&shell.analysis)
        .and_then(|entry| entry.score_lead)
        .filter(|lead| lead.is_finite());

    // Compute winrate graph history points for embedded OGS graph card
    let history = crate::winrate_graph::winrate_history(
        &snapshot,
        live_player_winrate,
        live_score_lead,
        snapshot.board.next_player,
    );
    let graph_points = crate::winrate_graph::graph_plot_points(
        &history,
        winrate_metric,
        shell
            .settings
            .get_bool("view.winrategraph_invert")
            .unwrap_or(false),
        shell
            .settings
            .get("view.winrategraph_blunderthreshold")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(5.0),
        5.0,
    );

    let weak_shell = cx.entity().downgrade();
    let on_node_clicked =
        move |node_id: &ryusei_domain_core::NodeId, _: &mut Window, cx: &mut App| {
            weak_shell
                .update(cx, |shell, cx| shell.navigate_to_node(node_id.clone(), cx))
                .ok();
        };

    div()
        .id("engine-sidebar")
        .debug_selector(|| "engine-sidebar".to_owned())
        .size_full()
        .min_h_0()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap_2p5()
        .p_3()
        .bg(rgb(shell.palette.input))
        .text_xs()
        // Card 1: AI 局面评估与好坏棋统计 (整合规则与贴目信息，移除 sgf2gif 字样)
        .child(
            div()
                .p_2p5()
                .rounded_md()
                .bg(rgb(shell.palette.panel))
                .border_1()
                .border_color(rgb(shell.palette.border))
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_0p5()
                                .child(
                                    div()
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(rgb(shell.palette.text))
                                        .child("AI 局面评估"),
                                )
                                .child(div().text_xs().text_color(rgb(shell.palette.muted)).child(
                                    format!(
                                        "{} · 贴目 {:.1}目{}",
                                        rule_config.ruleset.label(),
                                        rule_config.komi,
                                        if rule_config.handicap >= 2 {
                                            format!(" (让{}子)", rule_config.handicap)
                                        } else {
                                            String::new()
                                        }
                                    ),
                                )),
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
                                                "⏹ 停止复盘"
                                            } else {
                                                "⏩ 全盘复盘"
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
                // Real-time Winrate & Score Lead
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
                                .child(format!(
                                    "胜率: 黑 {:.1}% · 白 {:.1}%",
                                    live_winrate * 100.0,
                                    (1.0 - live_winrate) * 100.0
                                )),
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
                                        .map(|s| {
                                            if s >= 0.0 {
                                                format!("黑领先 +{:.1} 目", s)
                                            } else {
                                                format!("白领先 +{:.1} 目", -s)
                                            }
                                        })
                                        .unwrap_or_else(|| "均势 0.0 目".to_owned()),
                                ),
                        ),
                )
                // Dual Winrate Bar (Black vs White)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(div().text_color(rgb(shell.palette.subtle)).child("黑"))
                        .child(
                            div()
                                .flex_1()
                                .h(px(8.0))
                                .rounded(px(2.0))
                                .bg(rgb(0xf4f4f5))
                                .child(
                                    div()
                                        .h_full()
                                        .w(px((live_winrate as f32).clamp(0.0, 1.0) * 160.0))
                                        .bg(rgb(0x18181b)),
                                ),
                        )
                        .child(div().text_color(rgb(shell.palette.subtle)).child("白")),
                )
                // Current Move Quality evaluation (if any)
                .children(current_eval.map(|ev| {
                    div()
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
                }))
                // KaTrain Move Quality Classification Table
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .pt_1()
                        .border_t_1()
                        .border_color(rgb(shell.palette.border))
                        .child(
                            div()
                                .flex()
                                .justify_between()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(shell.palette.muted))
                                .child("好坏棋评级")
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
                            "🟡 疑问 (Inaccuracy)",
                            summary.black_inaccuracy_count,
                            summary.white_inaccuracy_count,
                            0xf59e0b,
                            shell,
                        ))
                        .child(render_stat_row(
                            "🟠 错着 (Mistake)",
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
                ),
        )
        // Card 2: 胜率与目差走势图卡片 (OGS 风格，置于 AI 评估卡片下方)
        .child(render_winrate_graph_panel(
            &graph_points,
            winrate_metric,
            210.0,
            shell.palette,
            on_node_clicked,
            cx,
        ))
        // Card 3: AI 候选点推荐卡片 (置于胜率图卡片下方，不显示长串 PV 文本，点击即预览)
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
                                .child(format!("AI 候选点 · {}", candidates.len())),
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
                            let wr_pct = entry.winrate * 100.0;
                            let score_str = entry
                                .score_lead
                                .map(|s| format!("{:+.1}目", s))
                                .unwrap_or_default();

                            let is_active_preview = shell.hovered_candidate_vertex.as_deref()
                                == hover_vertex.as_deref();
                            let click_vertex = hover_vertex.clone();

                            div()
                                .p_2()
                                .rounded_md()
                                .cursor_pointer()
                                .border_1()
                                .border_color(if is_active_preview {
                                    rgb(0x38bdf8)
                                } else if is_best {
                                    rgb(shell.palette.accent)
                                } else {
                                    rgb(shell.palette.border)
                                })
                                .bg(if is_active_preview {
                                    rgb(shell.palette.button_active)
                                } else if is_best {
                                    rgb(shell.palette.input)
                                } else {
                                    rgb(shell.palette.panel)
                                })
                                .hover(|style| style.bg(rgb(shell.palette.button_active)))
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_1p5()
                                        .child(
                                            div()
                                                .font_weight(FontWeight::BOLD)
                                                .text_color(if is_active_preview {
                                                    rgb(0x38bdf8)
                                                } else if is_best {
                                                    rgb(shell.palette.accent)
                                                } else {
                                                    rgb(shell.palette.text)
                                                })
                                                .child(format!("#{} {}", idx + 1, vtx)),
                                        )
                                        .children(
                                            is_active_preview
                                                .then(|| Badge::new().small().child("👁 预览中")),
                                        ),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(rgb(shell.palette.subtle))
                                        .child(format!(
                                            "{:.1}% · {} · {}v",
                                            wr_pct, score_str, entry.visits
                                        )),
                                )
                                .id(("candidate-row", idx))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |shell, _, _, cx| {
                                        if shell.hovered_candidate_vertex.as_deref()
                                            == click_vertex.as_deref()
                                        {
                                            shell.set_hovered_candidate(None, cx);
                                        } else {
                                            shell.set_hovered_candidate(click_vertex.clone(), cx);
                                        }
                                    }),
                                )
                                .on_hover(cx.listener(
                                    move |shell, is_hovering: &bool, _window, cx| {
                                        if *is_hovering {
                                            shell.set_hovered_candidate(hover_vertex.clone(), cx);
                                        }
                                    },
                                ))
                        }),
                    )
                }),
        )
}

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
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(shell.palette.subtle))
                                    .child("💬 局面解说 / 评论 (Markdown)"),
                            )
                            .child(div().text_xs().text_color(rgb(shell.palette.muted)).child(
                                if metadata.comment.is_empty() {
                                    "可直接输入 Markdown"
                                } else {
                                    "已渲染解说"
                                },
                            )),
                    )
                    .child(
                        div()
                            .id("node-comment-input-box")
                            .debug_selector(|| "node-comment-input-box".to_owned())
                            .track_focus(&shell.comment_focus_handle)
                            .key_context("CommentInput")
                            .px_3()
                            .py_2()
                            .min_h(px(90.0))
                            .border_1()
                            .border_color(rgb(
                                if shell.active_text_input == Some(crate::ActiveTextInput::Comment)
                                {
                                    0x38bdf8
                                } else {
                                    shell.palette.border
                                },
                            ))
                            .rounded_md()
                            .bg(rgb(shell.palette.input))
                            .text_color(rgb(shell.palette.text))
                            .text_xs()
                            .child(if shell.comment_input.text().is_empty() {
                                if metadata.comment.is_empty() {
                                    div()
                                        .text_color(rgb(0x71717a))
                                        .child("点击此处直接输入 Markdown 局面解说 / 棋谱评论...")
                                } else {
                                    div()
                                        .text_color(rgb(shell.palette.text))
                                        .child(metadata.comment.clone())
                                }
                            } else {
                                div()
                                    .text_color(rgb(shell.palette.text))
                                    .child(shell.comment_input.text().to_owned())
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
}

/// Compact right-sidebar board for the currently hovered engine candidate.
/// It is intentionally read-only: the real SGF document and host revision are
/// never changed by hovering. The PV is resolved from the live candidate list,
/// so streaming analysis updates the numbered ghost sequence in place.
pub fn render_analysis_preview_panel(
    snapshot: &GameSnapshot,
    theme: &crate::theme::ThemeTokens,
    shell: &ShellApp,
    _cx: &Context<ShellApp>,
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
    let response_entry = selected;
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
            entry.vertex.clone().unwrap_or_else(|| "pass".to_owned()),
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
                .child(if let Some((vertex, _)) = preview.as_ref() {
                    div().child(Badge::new().small().child(format!("选点 {vertex}")))
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
    best_move: Option<ryusei_domain_core::Vertex>,
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
        .map(|(start, end)| ryusei_domain_core::BoardLineSnapshot {
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
                    .map(|(column, row)| ryusei_domain_core::Vertex { column, row })?;
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
    let evaluations = ryusei_host::compute_game_move_evaluations(snapshot);
    let mut eval_dots = std::collections::BTreeMap::new();
    for eval in &evaluations {
        if let Some(vtx_str) = eval.played_vertex.as_deref()
            && let Some(vtx) = parse_gtp_vertex(board.width, vtx_str)
        {
            eval_dots.insert(
                ryusei_domain_core::Vertex {
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
        move |vertex: ryusei_domain_core::Vertex,
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
        move |vertex: ryusei_domain_core::Vertex,
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
        move |vertex: ryusei_domain_core::Vertex,
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
