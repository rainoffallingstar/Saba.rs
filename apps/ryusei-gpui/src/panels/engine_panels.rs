//! Engine & analysis panels: the GTP terminal, engine manager, left engine
//! sidebar, engine-config section, node inspector and the right analysis
//! preview. Extracted from `panels/mod.rs` during the architecture convergence.

use gpui::{
    Context, Div, FontWeight, InteractiveElement, MouseButton, MouseDownEvent, Stateful,
    StatefulInteractiveElement, div, prelude::*, px, rgb,
};
use gpui_component::badge::Badge;
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::scroll::ScrollableElement;
use gpui_component::{Selectable, Sizable};

use ryusei_domain_core::GameSnapshot;

use super::{
    focus_ring, icon_label, render_fox_sync_dialog, render_katago_dialog,
    render_position_to_sgf_dialog,
};
use crate::ShellApp;
use crate::engine_console::best_analysis_winrate;
use crate::goban_view::{pv_preview_points, render_goban_with_id};
use crate::icons::{self, ShellIcon};
use crate::native_text_input::NativeInputBinding;
use crate::node_inspector::NodeInspectorMetadata;

pub(crate) fn render_engine_manager(shell: &ShellApp, cx: &Context<ShellApp>) -> Div {
    let editing = shell.engine_spec_editing_name.as_deref();
    div()
        .w_full()
        .h_full()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(shell.palette.text))
                        .child("GTP 引擎管理"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(shell.palette.muted))
                        .child("保存配置不会启动进程。分配角色后，在 GTP 终端连接该角色。"),
                ),
        )
        .child(
            div()
                .flex()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .track_focus(&shell.text_inputs.engine_spec_focus_handle)
                        .tab_index(2)
                        .px_3()
                        .py_1p5()
                        .rounded_md()
                        .border_1()
                        .border_color(rgb(shell.palette.accent))
                        .bg(rgb(shell.palette.input))
                        .text_xs()
                        .text_color(rgb(shell.palette.text))
                        .child(if shell.text_inputs.engine_spec_input.text().is_empty() {
                            "名称 | 可执行路径 | 参数 | 启动命令".to_owned()
                        } else {
                            shell.text_inputs.engine_spec_input.text().to_owned()
                        })
                        .child(NativeInputBinding::new(
                            shell.text_inputs.engine_spec_focus_handle.clone(),
                            cx.entity().clone(),
                        ))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(ShellApp::on_engine_spec_focus),
                        )
                        .on_key_down(cx.listener(ShellApp::on_engine_spec_key_down)),
                )
                .child(
                    Button::new("engine-spec-choose-file")
                        .small()
                        .outline()
                        .label("选择文件")
                        .on_click(cx.listener(|shell, _, _, cx| {
                            shell.choose_engine_executable(cx);
                        })),
                )
                .child(
                    Button::new("engine-spec-save")
                        .small()
                        .primary()
                        .label(if editing.is_some() {
                            "保存修改"
                        } else {
                            "添加引擎"
                        })
                        .on_click(cx.listener(|shell, _, _, cx| {
                            shell.save_engine_spec(cx);
                        })),
                ),
        )
        .child(
            div()
                .flex_1()
                .min_h_0()
                .overflow_y_scrollbar()
                .flex()
                .flex_col()
                .gap_1p5()
                .children(shell.engine_store.list().iter().map(|record| {
                    let name = record.name.clone();
                    let edit_name = name.clone();
                    let test_name = name.clone();
                    let remove_name = name.clone();
                    let path = record.path.clone();
                    let details = if record.args.is_empty() {
                        record.commands.clone().unwrap_or_default()
                    } else if let Some(commands) = &record.commands {
                        format!("{} · {}", record.args, commands)
                    } else {
                        record.args.clone()
                    };
                    div()
                        .p_2()
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
                                .gap_2()
                                .child(
                                    div()
                                        .min_w_0()
                                        .flex_1()
                                        .text_sm()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(rgb(shell.palette.text))
                                        .child(name.clone()),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .gap_1()
                                        .child(
                                            Button::new(gpui::SharedString::from(format!(
                                                "engine-edit-{name}"
                                            )))
                                            .xsmall()
                                            .outline()
                                            .label("编辑")
                                            .on_click(cx.listener(move |shell, _, _, cx| {
                                                shell.edit_engine_spec(&edit_name, cx);
                                            })),
                                        )
                                        .child(
                                            Button::new(gpui::SharedString::from(format!(
                                                "engine-test-{name}"
                                            )))
                                            .xsmall()
                                            .outline()
                                            .label("测试")
                                            .tooltip("启动进程并执行 GTP 握手，然后立即停止")
                                            .on_click(cx.listener(move |shell, _, _, cx| {
                                                shell.test_engine_spec(&test_name, cx);
                                            })),
                                        )
                                        .child(
                                            Button::new(gpui::SharedString::from(format!(
                                                "engine-delete-{name}"
                                            )))
                                            .xsmall()
                                            .ghost()
                                            .label("删除")
                                            .on_click(cx.listener(move |shell, _, _, cx| {
                                                shell.remove_engine_spec(&remove_name, cx);
                                            })),
                                        ),
                                ),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(shell.palette.muted))
                                .child(path),
                        )
                        .when(!details.is_empty(), |this| {
                            this.child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(shell.palette.subtle))
                                    .child(details),
                            )
                        })
                        .child(
                            div().flex().flex_wrap().gap_1().children(
                                crate::engine_console::EngineRole::ALL
                                    .into_iter()
                                    .map(|role| {
                                        let selected =
                                            shell.engine_roles.get(role) == Some(name.as_str());
                                        let assign_name = name.clone();
                                        Button::new(gpui::SharedString::from(format!(
                                            "engine-role-{name}-{}",
                                            role.label()
                                        )))
                                        .xsmall()
                                        .outline()
                                        .selected(selected)
                                        .label(role.label())
                                        .on_click(
                                            cx.listener(move |shell, _, _, cx| {
                                                shell.assign_engine_role(role, &assign_name, cx);
                                            }),
                                        )
                                    }),
                            ),
                        )
                })),
        )
}

pub(crate) fn render_gtp_terminal_body(shell: &ShellApp, cx: &Context<ShellApp>) -> Div {
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
                .border_color(rgb(shell.palette.border))
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
                                .text_color(rgb(shell.palette.text))
                                .flex_none()
                                .child(icon_label(ShellIcon::Terminal, "GTP 终端", shell.palette.text)),
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
                                .child(if is_attached { icon_label(ShellIcon::Stop, "断开", shell.palette.muted) } else { icon_label(ShellIcon::Plug, "连接", shell.palette.muted) })
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
                                .child(icons::icon(ShellIcon::Close, 13.0, shell.palette.muted))
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
                                    .text_color(rgb(shell.palette.subtle))
                                    .child(format!("GTP 终端就绪 [{selected_label}]。在下方输入框中输入指令或点击快捷操作。"))
                            ]
                        } else {
                            shell.engine_log.iter().map(|entry| {
                                let color = if entry.success {
                                    rgb(shell.palette.success)
                                } else {
                                    rgb(shell.palette.danger_text)
                                };
                                div()
                                    .flex()
                                    .gap_2()
                                    .child(div().text_color(rgb(shell.palette.info)).child(format!("> {}", entry.command)))
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
                                        .child(if shell.analysis_task.is_some() { icon_label(ShellIcon::Stop, "停止分析", shell.palette.muted) } else { icon_label(ShellIcon::Sparkles, "启动分析", shell.palette.muted) })
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
                                                .text_color(rgb(shell.palette.muted))
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
                                        .bg(rgb(shell.palette.button))
                                        .border_1()
                                        .border_color(rgb(shell.palette.border))
                                        .text_xs()
                                        .text_color(rgb(shell.palette.text_secondary))
                                        .hover(|style| style.bg(rgb(shell.palette.button_active)).text_color(rgb(shell.palette.text)))
                                        .child(icon_label(ShellIcon::Dice, "AI落子 (genmove)", shell.palette.muted))
                                        .on_mouse_down(MouseButton::Left, cx.listener(|shell, _, _, cx| {
                                            shell.generate_engine_move(cx);
                                        }))
                                )
                                .child(
                                    Button::new("gtp-clear-board")
                                        .xsmall()
                                        .ghost()
                                        .child(icon_label(ShellIcon::RefreshCw, "清空棋盘", shell.palette.muted))
                                        .on_click(cx.listener(|shell, _, _, cx| {
                                            shell.send_engine_command("clear_board", cx);
                                        }))
                                )
                                .child(
                                    Button::new("gtp-list-commands")
                                        .xsmall()
                                        .ghost()
                                        .child(icon_label(ShellIcon::Terminal, "列出指令", shell.palette.muted))
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
                                        .track_focus(&shell.text_inputs.engine_input_focus_handle)
                                        .tab_index(3)
                                        .px_3()
                                        .py_1p5()
                                        .rounded_md()
                                        .border_1()
                                        .border_color(rgb(shell.palette.accent))
                                        .bg(rgb(shell.palette.panel))
                                        .text_xs()
                                        .text_color(rgb(shell.palette.text))
                                        .child(if shell.text_inputs.gtp_input.text().is_empty() {
                                            "输入 GTP 指令 (如: name, genmove, kata-analyze, boardsize 19 等)，按 Enter 发送".to_owned()
                                        } else {
                                            shell.text_inputs.gtp_input.text().to_owned()
                                        })
                                        .child(NativeInputBinding::new(
                                            shell.text_inputs.engine_input_focus_handle.clone(),
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
                                            let draft = shell.text_inputs.gtp_input.text().to_owned();
                                            shell.send_engine_command(&draft, cx);
                                            shell.text_inputs.gtp_input.set_text("");
                                            cx.notify();
                                        })),
                                ),
                        )
                )
}

#[allow(dead_code)]
pub(crate) fn render_stat_row(
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
    // Player metadata & Clocks
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

    let clock_state = shell.clock.state();
    let black_clock = crate::ui_format::format_clock(clock_state.black);
    let white_clock = crate::ui_format::format_clock(clock_state.white);
    let clocks_visible = !matches!(clock_state.control, ryusei_domain_core::TimeControl::None);

    let live_winrate = if !shell.analysis.is_empty() {
        best_analysis_winrate(&shell.analysis, snapshot.board.next_player)
    } else {
        0.50
    };
    let best_score_lead = shell.analysis.first().and_then(|e| e.score_lead);
    let left_tab = shell.left_sidebar_tab;

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
        // Top Switcher: [AI 局面评估] | [棋谱库]
        .child(
            div()
                .flex()
                .items_center()
                .p_0p5()
                .rounded_md()
                .bg(rgb(shell.palette.panel))
                .border_1()
                .border_color(rgb(shell.palette.border))
                .child(
                    Button::new("left-tab-ai")
                        .small()
                        .ghost()
                        .selected(left_tab == crate::LeftSidebarTab::AiEvaluation)
                        .child(icon_label(
                            ShellIcon::Sparkles,
                            "AI 局面评估",
                            if left_tab == crate::LeftSidebarTab::AiEvaluation {
                                shell.palette.accent
                            } else {
                                shell.palette.muted
                            },
                        ))
                        .on_click(cx.listener(|shell, _, _, cx| {
                            shell.set_left_sidebar_tab(crate::LeftSidebarTab::AiEvaluation, cx);
                        })),
                )
                .child(
                    Button::new("left-tab-library")
                        .small()
                        .ghost()
                        .selected(left_tab == crate::LeftSidebarTab::Library)
                        .child(icon_label(
                            ShellIcon::Library,
                            "棋谱库",
                            if left_tab == crate::LeftSidebarTab::Library {
                                shell.palette.accent
                            } else {
                                shell.palette.muted
                            },
                        ))
                        .on_click(cx.listener(|shell, _, _, cx| {
                            shell.set_left_sidebar_tab(crate::LeftSidebarTab::Library, cx);
                        })),
                ),
        )
        .children(match left_tab {
            crate::LeftSidebarTab::AiEvaluation => vec![
                // Card 1: 双方棋手、时钟与 AI 局面评估
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
                                    .child(
                                        div().text_xs().text_color(rgb(shell.palette.muted)).child(
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
                                        ),
                                    ),
                            )
                            .child(
                                Badge::new()
                                    .small()
                                    .child(if shell.analysis_task.is_some() {
                                        "● 分析中"
                                    } else if shell.analysis_enabled {
                                        "○ 自动分析"
                                    } else {
                                        "○ 已暂停"
                                    }),
                            ),
                    )
                    // Both Players & Active Clocks Display
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .p_2()
                            .rounded_md()
                            .bg(rgb(shell.palette.input))
                            .border_1()
                            .border_color(rgb(shell.palette.border))
                            // Black Player
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .p_1p5()
                                    .rounded_sm()
                                    .bg(if is_black_turn {
                                        rgb(shell.palette.button_active)
                                    } else {
                                        rgb(shell.palette.panel)
                                    })
                                    .border_1()
                                    .border_color(if is_black_turn {
                                        rgb(shell.palette.accent)
                                    } else {
                                        rgb(shell.palette.border)
                                    })
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .gap_1()
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap_1()
                                                    .min_w_0()
                                                    .child(
                                                        div()
                                                            .text_xs()
                                                            .font_weight(FontWeight::BOLD)
                                                            .text_color(if is_black_turn {
                                                                rgb(shell.palette.accent)
                                                            } else {
                                                                rgb(shell.palette.text)
                                                            })
                                                            .child("●"),
                                                    )
                                                    .child(
                                                        div()
                                                            .truncate()
                                                            .font_weight(FontWeight::SEMIBOLD)
                                                            .text_xs()
                                                            .text_color(rgb(shell.palette.text))
                                                            .child(black_name),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(rgb(shell.palette.subtle))
                                                    .child(format!(
                                                        "{}提",
                                                        snapshot.black_captures
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
                                                    .text_xs()
                                                    .text_color(rgb(shell.palette.muted))
                                                    .child(if black_rank.is_empty() {
                                                        "黑方".to_owned()
                                                    } else {
                                                        black_rank
                                                    }),
                                            )
                                            .child(
                                                div()
                                                    .px_1p5()
                                                    .py_0p5()
                                                    .rounded_sm()
                                                    .bg(rgb(shell.palette.input))
                                                    .font_weight(FontWeight::BOLD)
                                                    .text_xs()
                                                    .text_color(rgb(if is_black_turn {
                                                        shell.palette.warn
                                                    } else {
                                                        shell.palette.muted
                                                    }))
                                                    .child(if clocks_visible {
                                                        black_clock
                                                    } else {
                                                        "--:--".to_owned()
                                                    }),
                                            ),
                                    ),
                            )
                            // White Player
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .p_1p5()
                                    .rounded_sm()
                                    .bg(if !is_black_turn {
                                        rgb(shell.palette.button_active)
                                    } else {
                                        rgb(shell.palette.panel)
                                    })
                                    .border_1()
                                    .border_color(if !is_black_turn {
                                        rgb(shell.palette.accent)
                                    } else {
                                        rgb(shell.palette.border)
                                    })
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .gap_1()
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap_1()
                                                    .min_w_0()
                                                    .child(
                                                        div()
                                                            .text_xs()
                                                            .font_weight(FontWeight::BOLD)
                                                            .text_color(if !is_black_turn {
                                                                rgb(shell.palette.accent)
                                                            } else {
                                                                rgb(shell.palette.text)
                                                            })
                                                            .child("○"),
                                                    )
                                                    .child(
                                                        div()
                                                            .truncate()
                                                            .font_weight(FontWeight::SEMIBOLD)
                                                            .text_xs()
                                                            .text_color(rgb(shell.palette.text))
                                                            .child(white_name),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(rgb(shell.palette.subtle))
                                                    .child(format!(
                                                        "{}提",
                                                        snapshot.white_captures
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
                                                    .text_xs()
                                                    .text_color(rgb(shell.palette.muted))
                                                    .child(if white_rank.is_empty() {
                                                        "白方".to_owned()
                                                    } else {
                                                        white_rank
                                                    }),
                                            )
                                            .child(
                                                div()
                                                    .px_1p5()
                                                    .py_0p5()
                                                    .rounded_sm()
                                                    .bg(rgb(shell.palette.input))
                                                    .font_weight(FontWeight::BOLD)
                                                    .text_xs()
                                                    .text_color(rgb(if !is_black_turn {
                                                        shell.palette.warn
                                                    } else {
                                                        shell.palette.muted
                                                    }))
                                                    .child(if clocks_visible {
                                                        white_clock
                                                    } else {
                                                        "--:--".to_owned()
                                                    }),
                                            ),
                                    ),
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
                                    .bg(rgb(shell.palette.text))
                                    .child(
                                        div()
                                            .h_full()
                                            .w(px((live_winrate as f32).clamp(0.0, 1.0) * 160.0))
                                            .bg(rgb(shell.palette.text)),
                                    ),
                            )
                            .child(div().text_color(rgb(shell.palette.subtle)).child("白")),
                    ),
                // Section 2: 整局失误统计 (design: below the AI evaluation card)
                render_game_analytics_card(shell),
                // Section 3: 引擎与工具
                render_engine_config_section(shell, cx),
            ],
            crate::LeftSidebarTab::Library => vec![render_left_sidebar_library_panel(shell, cx)],
        })
}

/// Renders the integrated 棋谱库 (Library & Kifu browser) panel inside the left sidebar.
pub(crate) fn render_left_sidebar_library_panel(shell: &ShellApp, cx: &Context<ShellApp>) -> Div {
    let palette = shell.palette;
    let recent_files = shell.recent_files.list();
    let library_entries = &shell.library_entries;

    div()
        .flex()
        .flex_col()
        .gap_2p5()
        // Quick Import Actions
        .child(
            div()
                .p_2p5()
                .rounded_md()
                .bg(rgb(palette.panel))
                .border_1()
                .border_color(rgb(palette.border))
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(palette.text))
                        .child("棋谱导入与同步"),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_1p5()
                        .child(
                            Button::new("lib-btn-open-file")
                                .small()
                                .primary()
                                .child(icon_label(ShellIcon::Upload, "打开本地 SGF", 0xffffff))
                                .tooltip("选择本地 SGF/NGF/GIB/UGF 棋谱文件 (Cmd+O)")
                                .on_click(cx.listener(|shell, _, _, cx| {
                                    shell.open(cx);
                                })),
                        )
                        .child(
                            Button::new("lib-btn-fox-sync")
                                .small()
                                .ghost()
                                .child(icon_label(ShellIcon::Globe, "野狐对局导入", palette.muted))
                                .tooltip("搜索并同步野狐对局棋谱")
                                .on_click(cx.listener(|shell, _, _, cx| {
                                    shell.open_engine_config_panel(
                                        crate::EngineConfigPanel::FoxSync,
                                        cx,
                                    );
                                })),
                        )
                        .child(
                            Button::new("lib-btn-live-capture")
                                .small()
                                .ghost()
                                .child(icon_label(ShellIcon::Radio, "网络直播/链接", palette.muted))
                                .tooltip("从 Starriver/弈客/OGS 网址抓取棋谱")
                                .on_click(cx.listener(|shell, _, _, cx| {
                                    shell.open_live_capture(cx);
                                })),
                        )
                        .child(
                            Button::new("lib-btn-cloud-repo")
                                .small()
                                .ghost()
                                .child(icon_label(
                                    ShellIcon::Library,
                                    "GitHub 棋谱库",
                                    palette.muted,
                                ))
                                .tooltip("管理与同步 GitHub 开源 SGF 仓库")
                                .on_click(cx.listener(|shell, _, _, cx| {
                                    shell.open_library(cx);
                                })),
                        ),
                ),
        )
        // Recent local games (最近打开)
        .child(
            div()
                .p_2p5()
                .rounded_md()
                .bg(rgb(palette.panel))
                .border_1()
                .border_color(rgb(palette.border))
                .flex()
                .flex_col()
                .gap_1p5()
                .child(
                    div().flex().items_center().justify_between().child(
                        div()
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(palette.text))
                            .child(format!("最近对局 · {}", recent_files.len())),
                    ),
                )
                .child(if recent_files.is_empty() {
                    div()
                        .p_2()
                        .text_color(rgb(shell.palette.subtle))
                        .child("暂无最近打开的棋谱文件。")
                } else {
                    div().flex().flex_col().gap_1().children(
                        recent_files.into_iter().enumerate().map(|(idx, file)| {
                            let filename = file.display_name.clone();
                            let file_id = file.id.clone();
                            div()
                                .p_2()
                                .rounded_md()
                                .cursor_pointer()
                                .bg(rgb(palette.input))
                                .border_1()
                                .border_color(rgb(palette.border))
                                .hover(|style| {
                                    style
                                        .bg(rgb(palette.button_active))
                                        .border_color(rgb(palette.accent))
                                })
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_1p5()
                                        .min_w_0()
                                        .child(icons::icon(
                                            ShellIcon::BookOpen,
                                            13.0,
                                            palette.muted,
                                        ))
                                        .child(
                                            div()
                                                .truncate()
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_xs()
                                                .text_color(rgb(palette.text))
                                                .child(filename),
                                        ),
                                )
                                .id(("recent-file-item", idx))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |shell, _, _, cx| {
                                        shell.open_recent_file(&file_id, cx);
                                    }),
                                )
                        }),
                    )
                }),
        )
        // Library entries (已同步棋谱)
        .children((!library_entries.is_empty()).then(|| {
            div()
                .p_2p5()
                .rounded_md()
                .bg(rgb(palette.panel))
                .border_1()
                .border_color(rgb(palette.border))
                .flex()
                .flex_col()
                .gap_1p5()
                .child(
                    div()
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(palette.text))
                        .child(format!("棋谱库项目 · {}", library_entries.len())),
                )
                .child(
                    div().flex().flex_col().gap_1().children(
                        library_entries
                            .iter()
                            .take(15)
                            .enumerate()
                            .map(|(idx, entry)| {
                                let entry_path = entry.path.clone();
                                let title = entry.relative_path.clone();
                                div()
                                    .p_1p5()
                                    .rounded_md()
                                    .cursor_pointer()
                                    .bg(rgb(palette.input))
                                    .hover(|style| style.bg(rgb(palette.button_active)))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .truncate()
                                            .text_xs()
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(rgb(palette.text))
                                            .child(title),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(rgb(palette.subtle))
                                            .child(entry.source_id.clone()),
                                    )
                                    .id(("lib-entry-item", idx))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |shell, _, _, cx| {
                                            shell.open_library_entry(entry_path.clone(), cx);
                                        }),
                                    )
                            }),
                    ),
                )
        }))
}

/// Renders the "引擎与工具" section pinned to the bottom of the left engine
/// sidebar. Each row expands into its configuration panel inline.
pub(crate) fn render_engine_config_section(shell: &ShellApp, cx: &Context<ShellApp>) -> Div {
    let palette = shell.palette;
    let active = shell.engine_config_panel;

    let row = |panel: crate::EngineConfigPanel,
               id: &'static str,
               icon: ShellIcon,
               label: &'static str,
               shell: &ShellApp,
               cx: &Context<ShellApp>| {
        let is_open = active == Some(panel);
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .id(id)
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .px_2()
                    .py_1p5()
                    .rounded_md()
                    .cursor_pointer()
                    .border_1()
                    .border_color(rgb(if is_open {
                        palette.accent
                    } else {
                        palette.border
                    }))
                    .bg(rgb(if is_open {
                        palette.button_active
                    } else {
                        palette.input
                    }))
                    .hover(|style| style.bg(rgb(palette.button_active)))
                    .child(icons::icon(
                        icon,
                        14.0,
                        if is_open {
                            palette.accent
                        } else {
                            palette.muted
                        },
                    ))
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(palette.text))
                            .child(label),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |shell, _, _, cx| {
                            shell.toggle_engine_config_panel(panel, cx);
                        }),
                    ),
            )
            .when(is_open, |this| {
                this.child(match panel {
                    crate::EngineConfigPanel::KataGo => {
                        render_katago_dialog(shell, cx).into_any_element()
                    }
                    crate::EngineConfigPanel::Engines => {
                        render_engine_manager(shell, cx).into_any_element()
                    }
                    crate::EngineConfigPanel::FoxSync => {
                        render_fox_sync_dialog(shell, cx).into_any_element()
                    }
                    crate::EngineConfigPanel::PositionSgf => {
                        render_position_to_sgf_dialog(shell, cx).into_any_element()
                    }
                })
            })
    };

    div()
        .flex()
        .flex_col()
        .gap_1p5()
        .pt_2()
        .border_t_1()
        .border_color(rgb(palette.border))
        .child(
            div()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(palette.muted))
                .child("引擎与工具"),
        )
        .child(row(
            crate::EngineConfigPanel::KataGo,
            "engine-cfg-katago",
            ShellIcon::Cpu,
            "KataGo 引擎配置",
            shell,
            cx,
        ))
        .child(row(
            crate::EngineConfigPanel::Engines,
            "engine-cfg-engines",
            ShellIcon::Settings,
            "引擎管理",
            shell,
            cx,
        ))
        .child(row(
            crate::EngineConfigPanel::FoxSync,
            "engine-cfg-fox",
            ShellIcon::Globe,
            "野狐对局同步",
            shell,
            cx,
        ))
        .child(row(
            crate::EngineConfigPanel::PositionSgf,
            "engine-cfg-sgf",
            ShellIcon::Upload,
            "局面转 SGF",
            shell,
            cx,
        ))
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
                    .gap_1p5()
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
                                    .child(icon_label(ShellIcon::Message, "局面研讨与注释 (Markdown)", shell.palette.text)),
                            )
                            .child(div().text_xs().text_color(rgb(shell.palette.muted)).child(
                                if metadata.comment.is_empty() {
                                    "可直接输入"
                                } else {
                                    "已保存"
                                },
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(
                                Button::new("tag-joseki")
                                    .small()
                                    .ghost()
                                    .label("#定式")
                                    .on_click(cx.listener(|shell, _, _, cx| {
                                        shell.append_comment_tag("#定式", cx);
                                    })),
                            )
                            .child(
                                Button::new("tag-life-death")
                                    .small()
                                    .ghost()
                                    .label("#死活")
                                    .on_click(cx.listener(|shell, _, _, cx| {
                                        shell.append_comment_tag("#死活", cx);
                                    })),
                            )
                            .child(
                                Button::new("tag-mistake")
                                    .small()
                                    .ghost()
                                    .danger()
                                    .label("⚠️ 恶手")
                                    .on_click(cx.listener(|shell, _, _, cx| {
                                        shell.append_comment_tag("⚠️ 恶手", cx);
                                    })),
                            )
                            .child(
                                Button::new("tag-tesuji")
                                    .small()
                                    .ghost()
                                    .label("💡 妙手")
                                    .on_click(cx.listener(|shell, _, _, cx| {
                                        shell.append_comment_tag("💡 妙手", cx);
                                    })),
                            ),
                    )
                    .child(
                        // Edit / preview toggle (PRD §4.3 live Markdown preview).
                        div()
                            .flex()
                            .justify_end()
                            .child(
                                Button::new("comment-preview-toggle")
                                    .xsmall()
                                    .ghost()
                                    .selected(shell.comment_preview)
                                    .child(icon_label(
                                        ShellIcon::Eye,
                                        if shell.comment_preview { "编辑" } else { "预览" },
                                        shell.palette.muted,
                                    ))
                                    .tooltip("切换 Markdown 预览 / 编辑")
                                    .on_click(cx.listener(|shell, _, _, cx| {
                                        shell.toggle_comment_preview(cx);
                                    })),
                            ),
                    )
                    .child(if shell.comment_preview {
                        // Rendered Markdown preview of the current comment.
                        let source = if shell.text_inputs.comment_input.text().is_empty() {
                            metadata.comment.clone()
                        } else {
                            shell.text_inputs.comment_input.text().to_owned()
                        };
                        div()
                            .id("node-comment-preview")
                            .px_3()
                            .py_2()
                            .min_h(px(80.0))
                            .border_1()
                            .border_color(rgb(shell.palette.border))
                            .rounded_md()
                            .bg(rgb(shell.palette.panel))
                            .text_xs()
                            .child(if source.trim().is_empty() {
                                div()
                                    .text_color(rgb(shell.palette.subtle))
                                    .child("暂无注释。切换到「编辑」输入 Markdown 局面解说 / 棋谱评论。")
                                    .into_any_element()
                            } else {
                                crate::markdown::render_markdown(&source, shell.palette)
                                    .into_any_element()
                            })
                    } else {
                        div()
                            .id("node-comment-input-box")
                            .debug_selector(|| "node-comment-input-box".to_owned())
                            .track_focus(&shell.text_inputs.comment_focus_handle)
                            .tab_index(1)
                            .key_context("CommentInput")
                            .px_3()
                            .py_2()
                            .min_h(px(80.0))
                            .border_1()
                            .border_color(rgb(
                                if shell.active_text_input == Some(crate::ActiveTextInput::Comment)
                                {
                                    shell.palette.accent
                                } else {
                                    shell.palette.border
                                },
                            ))
                            .when(
                                shell.active_text_input == Some(crate::ActiveTextInput::Comment),
                                |this| this.shadow(vec![focus_ring(shell.palette.accent)]),
                            )
                            .rounded_md()
                            .bg(rgb(shell.palette.input))
                            .text_color(rgb(shell.palette.text))
                            .text_xs()
                            .child(if shell.text_inputs.comment_input.text().is_empty() {
                                if metadata.comment.is_empty() {
                                    div()
                                        .text_color(rgb(shell.palette.subtle))
                                        .child("点击此处直接输入 Markdown 局面解说 / 棋谱评论...")
                                } else {
                                    div()
                                        .text_color(rgb(shell.palette.text))
                                        .child(metadata.comment.clone())
                                }
                            } else {
                                div()
                                    .text_color(rgb(shell.palette.text))
                                    .child(shell.text_inputs.comment_input.text().to_owned())
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(ShellApp::on_comment_focus),
                            )
                            .on_key_down(cx.listener(ShellApp::on_comment_key_down))
                            .child(NativeInputBinding::new(
                                shell.text_inputs.comment_focus_handle.clone(),
                                cx.entity(),
                            ))
                    })
            } else {
                div()
                    .text_xs()
                    .text_color(rgb(shell.palette.subtle))
                    .child("comments hidden — enable view.show_comments to edit")
            },
        )
}

/// The whole-game mistake & accuracy statistics card (PRD §4.3). Rendered in
/// the left engine sidebar under the AI position evaluation card, so the
/// per-player comparison is always visible next to the live winrate readout.
pub(crate) fn render_game_analytics_card(shell: &ShellApp) -> Div {
    let evaluations = ryusei_host::compute_game_move_evaluations(&shell.host.snapshot());
    let summary = ryusei_host::GameAnalyticsSummary::from_evaluations(&evaluations);

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
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(shell.palette.text))
                .child(icon_label(
                    ShellIcon::BarChart,
                    "整局失误与精度统计",
                    shell.palette.text,
                )),
        )
        .child(
            div()
                .flex()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .p_2()
                        .rounded(px(4.0))
                        .bg(rgb(shell.palette.panel))
                        .border_1()
                        .border_color(rgb(shell.palette.border))
                        .flex()
                        .flex_col()
                        .gap_0p5()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::BOLD)
                                .child("● 黑棋 (Black)"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(shell.palette.muted))
                                .child(format!(
                                    "恶手: {} · 失误: {}",
                                    summary.black_blunder_count, summary.black_mistake_count
                                )),
                        )
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(shell.palette.success))
                                .child(format!("均损: {:.2}目", summary.black_avg_loss)),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .p_2()
                        .rounded(px(4.0))
                        .bg(rgb(shell.palette.panel))
                        .border_1()
                        .border_color(rgb(shell.palette.border))
                        .flex()
                        .flex_col()
                        .gap_0p5()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::BOLD)
                                .child("○ 白棋 (White)"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(shell.palette.muted))
                                .child(format!(
                                    "恶手: {} · 失误: {}",
                                    summary.white_blunder_count, summary.white_mistake_count
                                )),
                        )
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(shell.palette.danger_text))
                                .child(format!("均损: {:.2}目", summary.white_avg_loss)),
                        ),
                ),
        )
        // Per-player comparison bars (PRD §4.3: 双方恶手数/失误数对比条形图 +
        // 均损对比). Widths are normalised to the larger value.
        .child({
            let bar = |label: &'static str, value: f64, other: f64, color: u32, text: String| {
                let ratio = crate::ui_format::loss_ratio(value, other);
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_none()
                            .w(px(28.0))
                            .text_xs()
                            .text_color(rgb(shell.palette.muted))
                            .child(label),
                    )
                    .child(
                        div()
                            .flex_1()
                            .h(px(8.0))
                            .rounded(px(4.0))
                            .bg(rgb(shell.palette.track))
                            .child(
                                div()
                                    .h_full()
                                    .rounded(px(4.0))
                                    .bg(rgb(color))
                                    .w(gpui::relative(ratio)),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(rgb(color))
                            .child(text),
                    )
            };
            let compare = move |value_black: f64,
                                value_white: f64,
                                text_black: String,
                                text_white: String| {
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(bar(
                        "黑",
                        value_black,
                        value_white,
                        shell.palette.success,
                        text_black,
                    ))
                    .child(bar(
                        "白",
                        value_white,
                        value_black,
                        shell.palette.danger_text,
                        text_white,
                    ))
            };
            let blunder_cmp = compare(
                summary.black_blunder_count as f64,
                summary.white_blunder_count as f64,
                format!("{}", summary.black_blunder_count),
                format!("{}", summary.white_blunder_count),
            );
            let mistake_cmp = compare(
                summary.black_mistake_count as f64,
                summary.white_mistake_count as f64,
                format!("{}", summary.black_mistake_count),
                format!("{}", summary.white_mistake_count),
            );
            let loss_cmp = compare(
                summary.black_avg_loss,
                summary.white_avg_loss,
                format!("{:.2}", summary.black_avg_loss),
                format!("{:.2}", summary.white_avg_loss),
            );
            div()
                .flex()
                .flex_col()
                .gap_2()
                .pt_1()
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(shell.palette.subtle))
                        .child("恶手数对比"),
                )
                .child(blunder_cmp)
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(shell.palette.subtle))
                        .child("失误数对比"),
                )
                .child(mistake_cmp)
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(shell.palette.subtle))
                        .child("均损对比 (目)"),
                )
                .child(loss_cmp)
        })
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
    let candidates = &shell.analysis;
    let total_visits = candidates.first().map(|c| c.visits).unwrap_or(0);
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
    let preview_board_size = (shell.right_sidebar_width - 24.0).clamp(160.0, 320.0);
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
        .p_2p5()
        .border_b_1()
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
                        .items_center()
                        .gap_1p5()
                        .child(icons::icon(ShellIcon::Sparkles, 14.0, shell.palette.muted))
                        .child(
                            div()
                                .font_weight(FontWeight::BOLD)
                                .text_xs()
                                .text_color(rgb(shell.palette.text))
                                .child("AI 推荐选点 (Top 5)"),
                        ),
                )
                .child(Badge::new().small().child(if total_visits > 0 {
                    format!("{} visits", total_visits)
                } else {
                    "推演中".to_owned()
                })),
        )
        .child(div().flex().flex_col().gap_1p5().children(
            candidates.iter().take(5).enumerate().map(|(idx, entry)| {
                let rank = idx + 1;
                let vertex_str = entry.vertex.clone().unwrap_or_else(|| "pass".to_owned());
                let winrate_str = format!("{:.1}%", entry.winrate * 100.0);
                let lead_str = entry
                    .score_lead
                    .map(|l| format!("{:+.1}目", l))
                    .unwrap_or_default();
                // Policy-network prior probability (design: 先验概率 Prior %).
                let prior_str = entry.prior.map(|p| format!("先验 {:.0}%", p * 100.0));
                let is_hovered = shell.hovered_candidate_vertex.as_deref() == Some(&vertex_str);
                let is_top = rank == 1;

                let v_str_for_hover = vertex_str.clone();
                let v_str_for_move = vertex_str.clone();
                let v_str_for_click = vertex_str.clone();

                // Design: the top-1 candidate reads as a tinted accent card
                // (accent 8% background + accent ring); others stay neutral and
                // lift on hover. Hover also drives the on-board PV preview.
                let card_bg = if is_hovered {
                    shell.palette.button_active
                } else {
                    shell.palette.input // accent ring (is_top) or neutral
                };
                div()
                    .id(gpui::SharedString::from(format!("candidate-card-{rank}")))
                    .p_1p5()
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(if is_hovered || is_top {
                        shell.palette.accent
                    } else {
                        shell.palette.border
                    }))
                    .bg(rgb(card_bg))
                    .cursor_pointer()
                    .hover(|style| {
                        if is_top || is_hovered {
                            style
                        } else {
                            style.bg(rgb(shell.palette.button_active))
                        }
                    })
                    .on_mouse_move(cx.listener(move |shell, _, _, cx| {
                        shell.set_hovered_candidate(Some(v_str_for_move.clone()), cx);
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |shell, _, _, cx| {
                            shell.set_hovered_candidate(Some(v_str_for_hover.clone()), cx);
                        }),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .w(px(18.0))
                                            .h(px(18.0))
                                            .rounded_full()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_xs()
                                            .font_weight(FontWeight::BOLD)
                                            .bg(rgb(if is_top {
                                                shell.palette.accent
                                            } else {
                                                shell.palette.button
                                            }))
                                            .text_color(rgb(if is_top {
                                                0xffffff
                                            } else {
                                                shell.palette.text
                                            }))
                                            .child(rank.to_string()),
                                    )
                                    .child(
                                        div()
                                            .font_weight(FontWeight::BOLD)
                                            .text_xs()
                                            .text_color(rgb(shell.palette.text))
                                            .child(vertex_str.clone()),
                                    )
                                    .child(div().size(px(7.0)).rounded_full().bg(rgb(if is_top {
                                        shell.palette.success
                                    } else if rank <= 3 {
                                        shell.palette.info
                                    } else {
                                        shell.palette.muted
                                    }))),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(rgb(shell.palette.text))
                                            .child(winrate_str),
                                    )
                                    .children(prior_str.map(|prior| {
                                        div()
                                            .text_xs()
                                            .text_color(rgb(shell.palette.muted))
                                            .child(prior)
                                    }))
                                    .children((!lead_str.is_empty()).then(|| {
                                        div()
                                            .text_xs()
                                            .text_color(rgb(
                                                if entry.score_lead.unwrap_or(0.0) >= 0.0 {
                                                    shell.palette.success
                                                } else {
                                                    shell.palette.danger_text
                                                },
                                            ))
                                            .child(lead_str)
                                    }))
                                    .child(
                                        Button::new(gpui::SharedString::from(format!(
                                            "btn-pv-{rank}"
                                        )))
                                        .small()
                                        .ghost()
                                        .child(
                                            if shell.pv_animation.as_ref().is_some_and(
                                                |(vertex, _)| vertex == &v_str_for_click,
                                            ) {
                                                icon_label(
                                                    ShellIcon::Stop,
                                                    "停止",
                                                    shell.palette.muted,
                                                )
                                            } else {
                                                icon_label(
                                                    ShellIcon::Play,
                                                    "推演 PV",
                                                    shell.palette.muted,
                                                )
                                            },
                                        )
                                        .tooltip("400ms 逐手推演主变序列")
                                        .on_click(
                                            cx.listener(move |shell, _, _, cx| {
                                                shell.toggle_pv_animation(
                                                    v_str_for_click.clone(),
                                                    cx,
                                                );
                                            }),
                                        ),
                                    ),
                            ),
                    )
            }),
        ))
        .child(
            div()
                .flex()
                .items_center()
                .justify_center()
                .p_1()
                .child(board),
        )
}
