//! Plugin configuration dialogs and the pinned-plugin manager.
//!
//! Extracted from `panels/mod.rs` during the architecture convergence: these
//! render the built-in plugin surfaces (KataGo setup, Fox sync, position-to-SGF,
//! generic declarative panels) and the pinned-plugin manager. They are used by
//! the bottom deck and the left engine sidebar's "引擎与工具" section.

use gpui::{
    Context, Div, FontWeight, InteractiveElement, MouseButton, MouseDownEvent, div, prelude::*, px,
    rgb,
};
use gpui_component::badge::Badge;
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::scroll::ScrollableElement;
use gpui_component::{Disableable, Selectable, Sizable};

use super::icon_label;
use crate::ShellApp;
use crate::icons::{self, ShellIcon};
use crate::native_text_input::NativeInputBinding;
use crate::plugin_panel::PluginPanelEntry;

pub(crate) fn plugin_icon(plugin_id: &str) -> ShellIcon {
    if plugin_id.contains("katago") {
        ShellIcon::Cpu
    } else if plugin_id.contains("fox") {
        ShellIcon::Globe
    } else if plugin_id.contains("position") {
        ShellIcon::BarChart
    } else if plugin_id.contains("sgf") {
        ShellIcon::Save
    } else {
        ShellIcon::Puzzle
    }
}

/// Helper returning a human-readable summary of the plugin's capabilities.
pub(crate) fn plugin_description(plugin_id: &str) -> &'static str {
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

pub(crate) fn render_katago_dialog(shell: &ShellApp, cx: &Context<ShellApp>) -> Div {
    let analysis_connected = shell
        .engine_controller
        .is_attached(crate::engine_console::EngineRole::Analysis);
    // HumanSL rank ladder, weakest → strongest (20k … 9d). The current profile
    // maps to an index on this ladder so the compact stepper can move one step.
    let human_sl_ranks: Vec<String> = ryusei_host::human_sl_profiles()
        .into_iter()
        .filter(|profile| profile.starts_with("rank_"))
        .collect();
    let human_sl_current = shell
        .settings
        .get_str("katago.human_sl_profile")
        .unwrap_or("rank_5k")
        .to_owned();
    let human_sl_index = human_sl_ranks
        .iter()
        .position(|profile| *profile == human_sl_current)
        .unwrap_or(14);
    let human_sl_label = human_sl_ranks
        .get(human_sl_index)
        .and_then(|profile| profile.strip_prefix("rank_"))
        .unwrap_or("5k")
        .to_owned();
    let human_sl_prev = human_sl_index
        .checked_sub(1)
        .and_then(|i| human_sl_ranks.get(i).cloned());
    let human_sl_next = human_sl_ranks.get(human_sl_index + 1).cloned();
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap_2p5()
        .child(
            div()
                .p_2p5()
                .rounded_md()
                .bg(rgb(shell.palette.panel))
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
                        .text_color(rgb(shell.palette.muted))
                        .child("支持自动下载 KataGo 原生引擎，并根据本机硬件自动匹配 Metal (Apple Silicon) / OpenCL / CUDA 加速。"),
                ),
        )
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
                                .text_xs()
                                .font_weight(FontWeight::BOLD)
                                .text_color(rgb(shell.palette.text))
                                .child("本机与官网版本"),
                        )
                        .child(
                            Button::new("katago-refresh-metadata-btn")
                                .small()
                                .ghost()
                                .label("↻ 刷新官网")
                                .on_click(cx.listener(|shell, _, _, cx| {
                                    shell.refresh_katago_panel(cx);
                                })),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .gap_3()
                        .child(
                            div()
                                .flex_1()
                                .text_xs()
                                .text_color(rgb(shell.palette.text_secondary))
                                .child(format!(
                                    "本机 KataGo: {}",
                                    shell.katago_local
                                        .as_ref()
                                        .and_then(|local| local.version.as_deref())
                                        .unwrap_or("未检测到"),
                                )),
                        )
                        .child(
                            div()
                                .flex_1()
                                .text_xs()
                                .text_color(rgb(shell.palette.text_secondary))
                                .child(format!(
                                    "官网最新: {}",
                                    shell
                                        .katago_release
                                        .as_ref()
                                        .map(|release| release.version.as_str())
                                        .unwrap_or("未刷新"),
                                )),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .text_xs()
                                .text_color(rgb(shell.palette.muted))
                                .child(shell.katago_panel_status.clone()),
                        )
                        .child(
                            Button::new("katago-update-binary-btn")
                                .small()
                                .outline()
                                .label(if cfg!(target_os = "macos") {
                                    "brew 升级"
                                } else {
                                    "更新引擎"
                                })
                                .tooltip(if cfg!(target_os = "macos") {
                                    "执行 brew upgrade katago 升级本机引擎"
                                } else {
                                    "下载并安装官网最新发布版本"
                                })
                                .disabled(
                                    !(cfg!(target_os = "macos")
                                        || cfg!(target_os = "windows")
                                        || cfg!(target_os = "linux")),
                                )
                                .on_click(cx.listener(|shell, _, _, cx| {
                                    shell.update_katago_binary_from_panel(cx);
                                })),
                        ),
                ),
        )
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
                .max_h(px(190.0))
                .overflow_y_scrollbar()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(shell.palette.text))
                        .child("官网权重与统一模型入口"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(shell.palette.muted))
                        .child(format!(
                            "统一模型: {}",
                            shell
                                .katago_local
                                .as_ref()
                                .and_then(|local| local.unified_model.file_name())
                                .and_then(|name| name.to_str())
                                .unwrap_or("尚未初始化"),
                        )),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(shell.palette.muted))
                        .child(format!(
                            "HumanSL profile: {}",
                            shell
                                .settings
                                .get_str("katago.human_sl_profile")
                                .unwrap_or("rank_5k"),
                        )),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .child(
                            Button::new("human-sl-prev")
                                .xsmall()
                                .outline()
                                .disabled(human_sl_prev.is_none())
                                .label("更弱")
                                .on_click(cx.listener(move |shell, _, _, cx| {
                                    if let Some(profile) = human_sl_prev.clone() {
                                        shell.set_human_sl_profile(&profile, cx);
                                    }
                                })),
                        )
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .items_center()
                                .justify_center()
                                .gap_1()
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(shell.palette.muted))
                                        .child("HumanSL 档位"),
                                )
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(rgb(shell.palette.accent))
                                        .child(human_sl_label.clone()),
                                ),
                        )
                        .child(
                            Button::new("human-sl-next")
                                .xsmall()
                                .outline()
                                .disabled(human_sl_next.is_none())
                                .label("更强")
                                .on_click(cx.listener(move |shell, _, _, cx| {
                                    if let Some(profile) = human_sl_next.clone() {
                                        shell.set_human_sl_profile(&profile, cx);
                                    }
                                })),
                        ),
                )
                .children(shell.katago_weights.iter().enumerate().map(|(index, weight)| {
                    let name = weight.name.clone();
                    let activate_name = name.clone();
                    let downloaded = weight.installed;
                    let active = weight.active;
                    let human_sl = ryusei_host::is_human_sl_weight_name(&name);
                    // Display only the leading 10 characters so long network
                    // file names never push the trailing action buttons off-screen.
                    let short_name = if name.chars().count() > 10 {
                        format!("{}…", name.chars().take(10).collect::<String>())
                    } else {
                        name.clone()
                    };
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_xs()
                                .text_color(rgb(if active { 0x62d68a } else { 0xc5c5ca }))
                                .child(format!(
                                    "{}{}{}",
                                    if active { "● " } else { "○ " },
                                    short_name,
                                    if human_sl {
                                        " · HumanSL"
                                    } else if downloaded {
                                        " · 已下载"
                                    } else {
                                        " · 官网"
                                    },
                                )),
                        )
                        .child(if downloaded {
                            Button::new(("katago-activate-weight", index))
                                .small()
                                .ghost()
                                .tooltip(name.clone())
                                .label(if human_sl {
                                    "启用"
                                } else if active {
                                    "使用中"
                                } else {
                                    "设为当前"
                                })
                                .disabled(active && !human_sl)
                                .on_click(cx.listener(move |shell, _, _, cx| {
                                    shell.activate_katago_weight(&activate_name, cx);
                                }))
                        } else {
                            Button::new(("katago-download-weight", index))
                                .small()
                                .outline()
                                .tooltip(name.clone())
                                .label("下载")
                                .on_click(cx.listener(move |shell, _, _, cx| {
                                    shell.download_katago_weight_asset(&name, cx);
                                }))
                        })
                }))
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(shell.palette.muted))
                        .child("标准权重使用 active-model.bin.gz；HumanSL 会保留标准模型，并以 -human-model 和专用配置启动。"),
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
                        .child(icon_label(ShellIcon::Sparkles, "一键配置", shell.palette.muted))
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
                        .child(if analysis_connected {
                            icon_label(ShellIcon::Stop, "断开分析", shell.palette.muted)
                        } else {
                            icon_label(ShellIcon::Plug, "连接为分析引擎", shell.palette.muted)
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
                ),
        )
}

pub(crate) fn render_fox_sync_dialog(shell: &ShellApp, cx: &Context<ShellApp>) -> Div {
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap_2p5()
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
                        .w_full()
                        .track_focus(&shell.text_inputs.fox_query_focus_handle)
                        .tab_index(8)
                        .key_context("FoxQueryInput")
                        .px_3()
                        .py_1p5()
                        .rounded_md()
                        .border_1()
                        .border_color(rgb(
                            if shell.active_text_input == Some(crate::ActiveTextInput::FoxQuery) {
                                shell.palette.accent
                            } else {
                                shell.palette.border
                            },
                        ))
                        .bg(rgb(shell.palette.panel))
                        .text_xs()
                        .text_color(rgb(shell.palette.text))
                        .child(if shell.text_inputs.fox_query_input.text().is_empty() {
                            div()
                                .text_color(rgb(shell.palette.subtle))
                                .child("输入野狐用户名或 ID (按 Enter 搜索)")
                        } else {
                            div()
                                .text_color(rgb(shell.palette.text))
                                .child(shell.text_inputs.fox_query_input.text().to_owned())
                        })
                        .child(NativeInputBinding::new(
                            shell.text_inputs.fox_query_focus_handle.clone(),
                            cx.entity().clone(),
                        ))
                        .on_mouse_down(MouseButton::Left, cx.listener(ShellApp::on_fox_query_focus))
                        .on_key_down(cx.listener(ShellApp::on_fox_query_key_down)),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_1p5()
                .child(
                    Button::new("fox-sync-recent-btn")
                        .small()
                        .outline()
                        .child(icon_label(
                            ShellIcon::RefreshCw,
                            "同步最近对局",
                            shell.palette.muted,
                        ))
                        .on_click(cx.listener(|shell, _, _, cx| {
                            shell.on_plugin_command(
                                "org.ryusei.fox-kifu-sync",
                                "fox.query_games",
                                cx,
                            );
                        })),
                )
                .when(
                    !shell.text_inputs.fox_query_input.text().trim().is_empty(),
                    |this| {
                        this.child(
                            Button::new("fox-search-btn")
                                .small()
                                .primary()
                                .child(icon_label(
                                    ShellIcon::Magnify,
                                    "查询用户",
                                    shell.palette.muted,
                                ))
                                .on_click(cx.listener(|shell, _, _, cx| {
                                    shell.fetch_fox_query(cx);
                                })),
                        )
                    },
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
                        .text_color(rgb(shell.palette.subtle))
                        .child(shell.fox_query_status.clone()),
                )
                .children(
                    shell
                        .fox_recent_games
                        .iter()
                        .enumerate()
                        .map(|(index, game)| {
                            let chess_id = game.chess_id.clone();
                            let save_chess_id = chess_id.clone();
                            div()
                                .p_2()
                                .rounded_md()
                                .bg(rgb(shell.palette.panel))
                                .border_1()
                                .border_color(rgb(shell.palette.border))
                                .cursor_pointer()
                                .hover(|style| style.border_color(rgb(shell.palette.accent)))
                                .flex()
                                .flex_col()
                                .gap_0p5()
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |shell, _, _, cx| {
                                        shell.open_fox_game(&chess_id, cx);
                                    }),
                                )
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
                                                    "{} ({}) vs {} ({})",
                                                    game.black_name,
                                                    game.black_rank,
                                                    game.white_name,
                                                    game.white_rank
                                                )),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(rgb(shell.palette.muted))
                                                .child(format!("#{}", index + 1)),
                                        ),
                                )
                                .child(div().text_xs().text_color(rgb(shell.palette.muted)).child(
                                    format!(
                                        "{} · {} · {} 手",
                                        game.result, game.date, game.moves_count
                                    ),
                                ))
                                .child(
                                    div().mt_1().flex().justify_end().child(
                                        div()
                                            .px_2()
                                            .py_1()
                                            .rounded_md()
                                            .text_xs()
                                            .text_color(rgb(shell.palette.accent))
                                            .hover(|style| style.bg(rgb(shell.palette.input)))
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(move |shell, _, _, cx| {
                                                    cx.stop_propagation();
                                                    shell.save_fox_game_to_library(
                                                        &save_chess_id,
                                                        cx,
                                                    );
                                                }),
                                            )
                                            .child("保存到棋谱库"),
                                    ),
                                )
                        }),
                ),
        )
}

pub(crate) fn render_position_to_sgf_dialog(shell: &ShellApp, cx: &Context<ShellApp>) -> Div {
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap_2p5()
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
                        .child(icon_label(
                            ShellIcon::Upload,
                            "复制完整对局 SGF 到剪贴板",
                            shell.palette.muted,
                        ))
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
                        .child(icon_label(
                            ShellIcon::Target,
                            "仅复制当前死活/初始局面 (AB/AW)",
                            shell.palette.muted,
                        ))
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
                        .child(icon_label(
                            ShellIcon::Film,
                            "导出动画 GIF 棋谱…",
                            shell.palette.muted,
                        ))
                        .on_click(cx.listener(|shell, _, _, cx| {
                            shell.on_export_gif_action(cx);
                        })),
                )
                .child(
                    Button::new("export-png-modal-btn")
                        .small()
                        .ghost()
                        .child(icon_label(
                            ShellIcon::Image,
                            "导出当前局面高清图 (PNG)…",
                            shell.palette.muted,
                        ))
                        .on_click(cx.listener(|shell, _, _, cx| {
                            shell.export_current_position_png(cx);
                        })),
                ),
        )
}

pub(crate) fn render_generic_plugin_dialog(
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
                .border_color(rgb(shell.palette.border))
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(shell.palette.text))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_1p5()
                                .child(icons::icon(
                                    plugin_icon(&plugin.plugin_id),
                                    14.0,
                                    shell.palette.text,
                                ))
                                .child(div().child(plugin.name.clone())),
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
                                .child(icons::icon(ShellIcon::Close, 13.0, shell.palette.muted))
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

pub(crate) fn render_pinned_plugins_manager(shell: &ShellApp, cx: &Context<ShellApp>) -> Div {
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
                .border_color(rgb(shell.palette.border))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::BOLD)
                                .text_color(rgb(shell.palette.text))
                                .child(icon_label(
                                    ShellIcon::Puzzle,
                                    "插件栏管理",
                                    shell.palette.text,
                                )),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(shell.palette.muted))
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
                                .child(icons::icon(ShellIcon::Close, 13.0, shell.palette.muted))
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
                                                .child(
                                                    div()
                                                        .flex()
                                                        .items_center()
                                                        .gap_1p5()
                                                        .child(icons::icon(
                                                            plugin_icon(&plugin_id),
                                                            13.0,
                                                            shell.palette.text,
                                                        ))
                                                        .child(div().child(plugin.name.clone())),
                                                ),
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
