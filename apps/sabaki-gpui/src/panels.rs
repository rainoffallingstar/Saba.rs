//! Panel rendering for the shell, split out of `main.rs` so the shell keeps
//! only state, actions and assembly. Every function is a pure view over
//! `ShellApp` state plus precomputed values; listeners are built with
//! `cx.listener` against `ShellApp` handlers.

use gpui::{
    App, Context, Div, FocusHandle, MouseButton, MouseDownEvent, StatefulInteractiveElement,
    Window, div, prelude::*, px, rgb,
};

use sabaki_domain_core::GameSnapshot;

use crate::engine_console::best_analysis_winrate;
use crate::goban_view::render_goban;
use crate::markup::{MarkupTool, render_markup_toolbar};
use crate::navigation::NavigationAvailability;
use crate::node_inspector::NodeInspectorMetadata;
use crate::settings_form::{SettingRow, display_setting_value};
use crate::variation_tree::{VariationTreeLayout, render_variation_tree};
use crate::{BOARD_PIXEL_SIZE, BOARD_WINDOW_OFFSET_X, BOARD_WINDOW_OFFSET_Y, ShellApp};

/// The top-left title and subtitle block.
pub fn render_header(snapshot: &GameSnapshot, status: &str) -> Div {
    div()
        .child(
            div()
                .absolute()
                .left(px(24.0))
                .top(px(16.0))
                .text_lg()
                .child("Sabaki GPUI shell"),
        )
        .child(
            div()
                .absolute()
                .left(px(24.0))
                .top(px(48.0))
                .text_base()
                .child(format!(
                    "{} moves · board {}x{} · {status}",
                    snapshot.moves.len(),
                    snapshot.board.width,
                    snapshot.board.height
                )),
        )
}

/// The markup toolbar and navigation bar row.
pub fn render_toolbar_row(
    active_tool: MarkupTool,
    availability: NavigationAvailability,
    position: &str,
    cx: &Context<ShellApp>,
) -> Div {
    let weak_shell = cx.entity().downgrade();
    let on_tool_clicked = move |tool: &MarkupTool, _window: &mut Window, cx: &mut App| {
        weak_shell
            .update(cx, |shell, cx| shell.on_tool_selected(*tool, cx))
            .ok();
    };
    div()
        .absolute()
        .left(px(24.0))
        .top(px(524.0))
        .flex()
        .items_center()
        .gap_3()
        .child(render_markup_toolbar(active_tool, on_tool_clicked))
        .child(crate::navigation_bar(
            availability,
            position,
            cx.listener(ShellApp::on_navigate_first),
            cx.listener(ShellApp::on_navigate_previous),
            cx.listener(ShellApp::on_navigate_next),
            cx.listener(ShellApp::on_navigate_last),
        ))
}

/// The status bar: status text, file state, recent files, recovery, external
/// file status and the benchmark line.
pub fn render_status_bar(
    shell: &ShellApp,
    status: &str,
    dirty_label: &str,
    path_label: &str,
    external_status: &sabaki_host::ExternalFileStatusDto,
) -> Div {
    div()
        .absolute()
        .left(px(24.0))
        .top(px(560.0))
        .flex()
        .flex_col()
        .gap_1()
        .text_sm()
        .text_color(rgb(0x444444))
        .child(format!("status: {status}"))
        .child(format!("{dirty_label} · {path_label}"))
        .child(format!(
            "recent: {}",
            if shell.recent_files.list().is_empty() {
                "none".to_owned()
            } else {
                shell
                    .recent_files
                    .list()
                    .into_iter()
                    .map(|entry| entry.display_name)
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        ))
        .child(format!(
            "recovery: {}",
            if shell.autosave.info().is_available {
                "available"
            } else {
                "none"
            }
        ))
        .child(format!(
            "external: {} {}",
            format!("{:?}", external_status.status).to_lowercase(),
            external_status.display_name.as_deref().unwrap_or("")
        ))
        .child(format!("benchmark: {}", shell.benchmark))
        .child(format!("large-game: {}", shell.large_game_benchmark))
}

/// The restore/discard recovery buttons shown while a recovery candidate is
/// available.
pub fn render_recovery_buttons(shell: &ShellApp, cx: &Context<ShellApp>) -> Div {
    if shell.autosave.info().is_available {
        div()
            .absolute()
            .left(px(24.0))
            .top(px(524.0))
            .flex()
            .gap_2()
            .child(
                div()
                    .px_2()
                    .py_1()
                    .border_1()
                    .border_color(rgb(0x8a6d3b))
                    .rounded(px(4.0))
                    .bg(rgb(0xf7ecd8))
                    .child("restore recovery")
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(ShellApp::on_restore_recovery),
                    ),
            )
            .child(
                div()
                    .px_2()
                    .py_1()
                    .border_1()
                    .border_color(rgb(0xc0392b))
                    .rounded(px(4.0))
                    .bg(rgb(0xf5d6d6))
                    .child("discard recovery")
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(ShellApp::on_discard_recovery),
                    ),
            )
    } else {
        div()
    }
}

/// The reload/keep-local actions shown while an external-file conflict is
/// pending.
pub fn render_external_conflict_buttons(external_conflict: bool, cx: &Context<ShellApp>) -> Div {
    if external_conflict {
        div()
            .absolute()
            .left(px(24.0))
            .top(px(620.0))
            .flex()
            .gap_2()
            .child(
                div()
                    .px_2()
                    .py_1()
                    .border_1()
                    .border_color(rgb(0x8a6d3b))
                    .rounded(px(4.0))
                    .bg(rgb(0xf7ecd8))
                    .child("reload external")
                    .on_mouse_down(MouseButton::Left, cx.listener(ShellApp::on_reload_external)),
            )
            .child(
                div()
                    .px_2()
                    .py_1()
                    .border_1()
                    .border_color(rgb(0xc0392b))
                    .rounded(px(4.0))
                    .bg(rgb(0xf5d6d6))
                    .child("keep local")
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(ShellApp::on_keep_local_external),
                    ),
            )
    } else {
        div()
    }
}

/// The plugin list panel: installed plugins with enable/disable, grant,
/// native authorization and command dispatch, plus the declarative widget
/// demo panel.
pub fn render_plugins_panel(shell: &ShellApp, cx: &Context<ShellApp>) -> Div {
    div()
        .absolute()
        .left(px(480.0))
        .top(px(96.0))
        .flex()
        .flex_col()
        .gap_2()
        .text_base()
        .child("plugins")
        .child(if shell.installed_plugins.is_empty() {
            div()
                .text_sm()
                .text_color(rgb(0x999999))
                .child("no plugins installed — add a plugin folder to the install root")
        } else {
            div()
                .flex()
                .flex_col()
                .gap_1()
                .text_sm()
                .children(shell.installed_plugins.iter().map(|plugin| {
                    let plugin_id = plugin.plugin_id.clone();
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
                        .gap_1()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(format!(
                                    "{} v{} {}",
                                    plugin.name,
                                    plugin.version,
                                    if plugin.enabled { "" } else { "(disabled)" }
                                ))
                                .child(
                                    div()
                                        .px_2()
                                        .py_1()
                                        .border_1()
                                        .border_color(if plugin.enabled {
                                            rgb(0x8a6d3b)
                                        } else {
                                            rgb(0x666666)
                                        })
                                        .rounded(px(4.0))
                                        .bg(if plugin.enabled {
                                            rgb(0xf7ecd8)
                                        } else {
                                            rgb(0xe8e8e8)
                                        })
                                        .child(if plugin.enabled { "disable" } else { "enable" })
                                        .on_mouse_down(MouseButton::Left, toggle_listener),
                                )
                                .child(if plugin.missing_permissions.is_empty() {
                                    div()
                                } else {
                                    div()
                                        .px_2()
                                        .py_1()
                                        .border_1()
                                        .border_color(rgb(0x8a6d3b))
                                        .rounded(px(4.0))
                                        .bg(rgb(0xf7ecd8))
                                        .child("grant & enable")
                                        .on_mouse_down(MouseButton::Left, grant_listener)
                                })
                                .child(if plugin.native_runtime && !plugin.native_authorized {
                                    div()
                                        .px_2()
                                        .py_1()
                                        .border_1()
                                        .border_color(rgb(0xc0392b))
                                        .rounded(px(4.0))
                                        .bg(rgb(0xf5d6d6))
                                        .child("authorize native")
                                        .on_mouse_down(MouseButton::Left, authorize_listener)
                                } else {
                                    div()
                                }),
                        )
                        .child(if plugin.enabled {
                            div().flex().flex_col().gap_1().children(
                                plugin.command_ids.iter().zip(plugin.commands.iter()).map(
                                    |(command_id, title)| {
                                        let plugin_id = plugin_id.clone();
                                        let command_id = command_id.clone();
                                        div()
                                            .px_1()
                                            .py_1()
                                            .border_1()
                                            .border_color(rgb(0x8a6d3b))
                                            .rounded(px(4.0))
                                            .bg(rgb(0xffffff))
                                            .child(title.clone())
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
                        } else {
                            div().text_color(rgb(0x999999)).child(format!(
                                "needs: {}",
                                if plugin.missing_permissions.is_empty() {
                                    "nothing — enable to use".to_owned()
                                } else {
                                    plugin.missing_permissions.join(", ")
                                }
                            ))
                        })
                }))
        })
        .child(format!("panel: {}", shell.panel.panel_title))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .text_sm()
                .children(shell.panel.widgets.iter().map(|widget| {
                    div().child(match widget {
                        crate::plugin_contribution::PanelWidget::Label { text } => text.clone(),
                        crate::plugin_contribution::PanelWidget::Value { label, value } => {
                            format!("{label}: {value}")
                        }
                        crate::plugin_contribution::PanelWidget::Button { id, title } => {
                            format!("[button:{id}] {title}")
                        }
                        crate::plugin_contribution::PanelWidget::Select {
                            id,
                            options,
                            selected,
                        } => format!("[select:{id}] {} (={:?})", options.join("/"), selected),
                    })
                })),
        )
        .child(div().mt_2().text_sm().child(format!(
            "theme tokens: wood #{:06x} stones #{:06x}/#{:06x}",
            shell.theme.board_wood_color().rgb_u32(),
            shell.theme.stone_black_color().rgb_u32(),
            shell.theme.stone_white_color().rgb_u32()
        )))
}

/// The variation tree panel.
pub fn render_variation_tree_panel(
    layout: &VariationTreeLayout,
    on_node_clicked: impl Fn(&sabaki_domain_core::NodeId, &mut Window, &mut App) + 'static,
) -> Div {
    div()
        .absolute()
        .left(px(760.0))
        .top(px(96.0))
        .flex()
        .flex_col()
        .gap_2()
        .text_base()
        .child("variation tree")
        .child(render_variation_tree(layout, on_node_clicked))
}

/// The engine console panel: engine list and management, analyze/engine-move
/// actions, analysis candidates and winrate bar, and the console transcript.
pub fn render_engine_panel(shell: &ShellApp, cx: &Context<ShellApp>) -> Div {
    div()
        .absolute()
        .left(px(760.0))
        .top(px(340.0))
        .flex()
        .flex_col()
        .gap_2()
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
                        let connected = shell.engine_session.is_some();
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(format!("{} ({})", record.name, record.path))
                            .child(if connected {
                                div().text_color(rgb(0x2e6b34)).child("connected")
                            } else {
                                let connect_name = name.clone();
                                div()
                                        .px_2()
                                        .py_1()
                                        .border_1()
                                        .border_color(rgb(0x8a6d3b))
                                        .rounded(px(4.0))
                                        .bg(rgb(0xf7ecd8))
                                        .child("connect")
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(move |shell,
                                                        _: &MouseDownEvent,
                                                        _: &mut Window,
                                                        cx: &mut Context<ShellApp>| {
                                                shell.on_engine_connect(&connect_name, cx);
                                            }),
                                        )
                            })
                            .child({
                                let remove_name = name.clone();
                                div()
                                        .px_2()
                                        .py_1()
                                        .border_1()
                                        .border_color(rgb(0xc0392b))
                                        .rounded(px(4.0))
                                        .bg(rgb(0xf5d6d6))
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
                        .border_color(rgb(0x8a6d3b))
                        .rounded(px(4.0))
                        .bg(rgb(0xffffff))
                        .text_color(rgb(0x222222))
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
                                .border_color(rgb(0x8a6d3b))
                                .rounded(px(4.0))
                                .bg(rgb(0xf7ecd8))
                                .child("analyze")
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(ShellApp::on_analyze),
                                ),
                        )
                        .child(if shell.engine_session.is_some() {
                            div()
                                .flex()
                                .gap_2()
                                .child(
                                    div()
                                        .px_2()
                                        .py_1()
                                        .border_1()
                                        .border_color(rgb(0x8a6d3b))
                                        .rounded(px(4.0))
                                        .bg(rgb(0xf7ecd8))
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
                                        .border_color(rgb(0xc0392b))
                                        .rounded(px(4.0))
                                        .bg(rgb(0xf5d6d6))
                                        .child("disconnect")
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(ShellApp::on_engine_disconnect),
                                        ),
                                )
                        } else {
                            div()
                        }),
                ),
        )
        .child(if shell.analysis.is_empty() {
            div()
        } else {
            div()
                .flex()
                .flex_col()
                .gap_1()
                .text_sm()
                .child("analysis")
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
                                .bg(rgb(0xdddddd))
                                .child(
                                    div()
                                        .h_full()
                                        .w(
                                            px(140.0
                                                * best_analysis_winrate(&shell.analysis) as f32),
                                        )
                                        .bg(rgb(0x222222)),
                                ),
                        )
                        .child("white"),
                )
        })
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .text_sm()
                .child(div().flex().flex_col().gap_1().children(
                    shell.engine_log.iter().rev().take(8).map(|entry| {
                        let color = if entry.success {
                            rgb(0x2e6b34)
                        } else {
                            rgb(0xc0392b)
                        };
                        div()
                            .text_color(color)
                            .child(format!("{} → {}", entry.command, entry.response))
                    }),
                ))
                .child(
                    div()
                        .track_focus(&shell.engine_input_focus_handle)
                        .px_2()
                        .py_1()
                        .border_1()
                        .border_color(rgb(0x8a6d3b))
                        .rounded(px(4.0))
                        .bg(rgb(0xffffff))
                        .text_color(rgb(0x222222))
                        .child(shell.engine_draft.clone().to_string())
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(ShellApp::on_engine_input_focus),
                        )
                        .on_key_down(cx.listener(ShellApp::on_engine_key_down)),
                ),
        )
}

/// The node inspector panel: title, comment editor, property table and
/// variation actions.
pub fn render_node_inspector_panel(
    metadata: &NodeInspectorMetadata,
    shell: &ShellApp,
    cx: &Context<ShellApp>,
) -> Div {
    div()
        .absolute()
        .left(px(480.0))
        .top(px(340.0))
        .flex()
        .flex_col()
        .gap_2()
        .text_base()
        .child(format!(
            "node inspector · {} · {}",
            metadata.title, metadata.node_id
        ))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .text_sm()
                .child("comment")
                .child(
                    div()
                        .track_focus(&shell.comment_focus_handle)
                        .px_2()
                        .py_1()
                        .border_1()
                        .border_color(rgb(0x8a6d3b))
                        .rounded(px(4.0))
                        .bg(rgb(0xffffff))
                        .text_color(rgb(0x222222))
                        .child(if shell.comment_draft.is_empty() {
                            metadata.comment.clone()
                        } else {
                            shell.comment_draft.clone().to_string()
                        })
                        .on_mouse_down(MouseButton::Left, cx.listener(ShellApp::on_comment_focus))
                        .on_key_down(cx.listener(ShellApp::on_comment_key_down)),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .text_sm()
                .children(metadata.properties.iter().map(|row| {
                    div()
                        .flex()
                        .gap_2()
                        .child(format!("{}:", row.name))
                        .child(row.value.clone())
                }))
                .child(if metadata.properties.is_empty() {
                    div().text_color(rgb(0x999999)).child("no other properties")
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
                        .px_2()
                        .py_1()
                        .border_1()
                        .border_color(rgb(0x8a6d3b))
                        .rounded(px(4.0))
                        .bg(rgb(0xf7ecd8))
                        .child("promote")
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(ShellApp::on_variation_promote),
                        ),
                )
                .child(
                    div()
                        .px_2()
                        .py_1()
                        .border_1()
                        .border_color(rgb(0xc0392b))
                        .rounded(px(4.0))
                        .bg(rgb(0xf5d6d6))
                        .child("remove")
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
) -> Div {
    div()
        .absolute()
        .left(px(480.0))
        .top(px(500.0))
        .flex()
        .flex_col()
        .gap_2()
        .text_base()
        .child("settings")
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .text_sm()
                .child("theme")
                .child(
                    div()
                        .flex()
                        .gap_1()
                        .children(crate::THEME_CHOICES.iter().map(|choice| {
                            let is_active = *choice == shell.theme_choice;
                            div()
                            .px_2()
                            .py_1()
                            .border_1()
                            .border_color(rgb(0x8a6d3b))
                            .rounded(px(4.0))
                            .bg(if is_active { rgb(0xf7ecd8) } else { rgb(0xe8e0d4) })
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
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .text_sm()
                .child("board size")
                .child(
                    div()
                        .flex()
                        .gap_1()
                        .children(crate::BOARD_SIZE_OPTIONS.iter().map(|size| {
                            let is_active = *size == shell.board_size;
                            div()
                            .px_2()
                            .py_1()
                            .border_1()
                            .border_color(rgb(0x8a6d3b))
                            .rounded(px(4.0))
                            .bg(if is_active { rgb(0xf7ecd8) } else { rgb(0xe8e0d4) })
                            .child(size.to_string())
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
                .gap_1()
                .text_sm()
                .child("mode")
                .child(
                    div().flex().gap_1().child(
                        div()
                            .px_2()
                            .py_1()
                            .border_1()
                            .border_color(rgb(0x8a6d3b))
                            .rounded(px(4.0))
                            .bg(if shell.scoring_mode {
                                rgb(0xf7ecd8)
                            } else {
                                rgb(0xe8e0d4)
                            })
                            .child(if shell.scoring_mode {
                                "scoring: on"
                            } else {
                                "scoring: off"
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(ShellApp::on_scoring_mode_toggle),
                            ),
                    ),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .text_sm()
                .child("preferences")
                .child(
                    div()
                        .id("settings-form")
                        .h(px(210.0))
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .children(settings_rows.iter().map(|row| {
                            let is_editing =
                                shell.settings_editing_key.as_deref() == Some(row.key.as_str());
                            render_setting_row(
                                row,
                                is_editing,
                                &shell.settings_draft,
                                &shell.settings_input_focus_handle,
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
    shell: &ShellApp,
    cx: &Context<ShellApp>,
) -> Div {
    let board = &snapshot.board;
    let options = crate::goban_view::GobanRenderOptions {
        show_coordinates: shell
            .settings
            .get_bool("view.show_coordinates")
            .unwrap_or(false),
        show_move_numbers: shell
            .settings
            .get_bool("view.show_move_numbers")
            .unwrap_or(false),
        move_numbers: crate::goban_view::move_numbers_from_moves(&snapshot.moves),
        score_overrides: snapshot.score_overrides.clone(),
    };
    div()
        .child(
            div()
                .absolute()
                .left(px(BOARD_WINDOW_OFFSET_X))
                .top(px(BOARD_WINDOW_OFFSET_Y))
                .child(render_goban(board, BOARD_PIXEL_SIZE, theme, &options))
                .on_mouse_down(MouseButton::Left, cx.listener(ShellApp::on_board_clicked)),
        )
        .child(if let Some(vertex) = best_move {
            let (x, y) = crate::goban_view::intersection_position(
                board,
                BOARD_PIXEL_SIZE,
                vertex.column,
                vertex.row,
            );
            div()
                .absolute()
                .left(px(BOARD_WINDOW_OFFSET_X + x - 8.0))
                .top(px(BOARD_WINDOW_OFFSET_Y + y - 8.0))
                .size(px(16.0))
                .rounded_full()
                .border_2()
                .border_color(rgb(0xc0392b))
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
                .gap_2()
                .child(row.label.to_owned())
                .child(
                    div()
                        .px_2()
                        .py_1()
                        .border_1()
                        .border_color(rgb(0x8a6d3b))
                        .rounded(px(4.0))
                        .bg(if is_on { rgb(0xf7ecd8) } else { rgb(0xe8e0d4) })
                        .child(if is_on { "on" } else { "off" })
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
                .gap_2()
                .child(row.label.to_owned())
                .child(if is_editing {
                    div()
                        .track_focus(focus_handle)
                        .px_2()
                        .py_1()
                        .border_1()
                        .border_color(rgb(0x8a6d3b))
                        .rounded(px(4.0))
                        .bg(rgb(0xffffff))
                        .text_color(rgb(0x222222))
                        .child(draft.to_owned())
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(ShellApp::on_settings_input_focus),
                        )
                        .on_key_down(cx.listener(ShellApp::on_settings_key_down))
                } else {
                    div()
                        .px_2()
                        .py_1()
                        .border_1()
                        .border_color(rgb(0x8a6d3b))
                        .rounded(px(4.0))
                        .bg(rgb(0xf0eadc))
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
