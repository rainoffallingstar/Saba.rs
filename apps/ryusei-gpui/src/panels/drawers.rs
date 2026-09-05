//! Right-side drawers (game info, score, library, live capture, OGS account,
//! profile, goals, preferences, about, review, match setup, export).
//!
//! Extracted from `panels/mod.rs` during the architecture convergence. All
//! drawers share the `render_readonly_drawer` scaffold (overlay + panel +
//! close control) and are pure views over `ShellApp` state.

use gpui::{
    Context, Div, FocusHandle, FontWeight, InteractiveElement, MouseButton, MouseDownEvent,
    Stateful, StatefulInteractiveElement, Window, div, hsla, prelude::*, px, rgb,
};
use gpui_component::badge::Badge;
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::checkbox::Checkbox;
use gpui_component::{Disableable, Selectable, Sizable};

use ryusei_domain_core::{GameSnapshot, MatchParticipants, PlayerKind, TimeControl};

use super::{focus_ring, icon_label};
use crate::ShellApp;
use crate::icons::ShellIcon;
use crate::native_text_input::NativeInputBinding;
use crate::settings_form::{SettingRow, editable_setting_value, panel_setting_rows};

pub(crate) fn render_readonly_drawer(
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
            // Slide the drawer panel in from the right edge over the base
            // motion duration, easing out like the design's sheet transitions.
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
                .on_mouse_down(MouseButton::Left, |_, _, cx| {
                    cx.stop_propagation();
                })
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
                                        .child(icon_label(
                                            ShellIcon::Close,
                                            "Close",
                                            shell.palette.muted,
                                        ))
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
    let current_ruleset = ryusei_host::GoRuleset::from_setting(
        snapshot
            .root_properties
            .get("RU")
            .and_then(|values| values.first())
            .map(String::as_str),
    );
    let handicap = snapshot
        .root_properties
        .get("HA")
        .and_then(|values| values.first())
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or_default();
    let default_komi = current_ruleset.default_komi(handicap);
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
            .child(row("Rules", property("RU")))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1p5()
                    .pt_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(shell.palette.subtle))
                            .child("规则集会写入当前 SGF，并在重新连接 KataGo 时生效。"),
                    )
                    .child(div().flex().flex_wrap().gap_1().children(
                        ryusei_host::GoRuleset::ALL.into_iter().map(|ruleset| {
                            let selected = property("RU") == ruleset.sgf_name();
                            Button::new(gpui::SharedString::from(format!(
                                "game-ruleset-{}",
                                ruleset.katago_name()
                            )))
                            .xsmall()
                            .outline()
                            .selected(selected)
                            .label(ruleset.sgf_name())
                            .tooltip(ruleset.label())
                            .on_click(cx.listener(
                                move |shell, _, _, cx| {
                                    shell.set_current_game_ruleset(ruleset, cx);
                                },
                            ))
                        }),
                    ))
                    .child(
                        Button::new("game-ruleset-default-komi")
                            .xsmall()
                            .outline()
                            .label(format!(
                                "按 {} 默认贴目设为 {:.1}",
                                current_ruleset.sgf_name(),
                                default_komi
                            ))
                            .tooltip("仅在明确点击后写入 KM；切换规则时不会自动改变贴目")
                            .on_click(cx.listener(|shell, _, _, cx| {
                                shell.apply_current_ruleset_default_komi(cx);
                            })),
                    ),
            )
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
/// Reads the `score.estimator_iterations` setting as the Monte-Carlo playout
/// count for life-and-death estimation (`0` = deterministic heuristic).
fn estimator_iterations(shell: &ShellApp) -> usize {
    shell
        .settings
        .get("score.estimator_iterations")
        .and_then(serde_json::Value::as_u64)
        .map(|value| value as usize)
        .unwrap_or(0)
}

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

    let scoring_rule = ryusei_domain_core::ScoringRule::from_sgf_ru(
        snapshot
            .root_properties
            .get("RU")
            .and_then(|values| values.first())
            .map(String::as_str),
    );
    let scoring_result = ryusei_domain_core::score_board_with_estimation(
        &snapshot.board,
        Some(komi),
        &snapshot.score_overrides,
        scoring_rule,
        estimator_iterations(shell),
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
                            .child(crate::markup::scoring_summary(
                                snapshot,
                                estimator_iterations(shell),
                            )),
                    )
                    .children((scoring_rule == ryusei_domain_core::ScoringRule::ChineseAncient).then(|| {
                        div()
                            .text_xs()
                            .text_color(rgb(shell.palette.muted))
                            .child(format!(
                                "中国古谱还棋头: 黑 {} 目 ({} 块)，白 {} 目 ({} 块)",
                                scoring_result.black_group_tax,
                                scoring_result.black_groups,
                                scoring_result.white_group_tax,
                                scoring_result.white_groups,
                            ))
                    })),
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

/// Lists library material available from recent local games and managed Git sources.
pub fn render_library_drawer(shell: &ShellApp, cx: &Context<ShellApp>) -> Stateful<Div> {
    let recent_files = shell.recent_files.list();
    let field = |id: &'static str,
                 label: &'static str,
                 placeholder: &'static str,
                 kind: crate::ActiveTextInput,
                 focus: &gpui::FocusHandle,
                 input: &crate::native_text_input::NativeTextInput| {
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(label),
            )
            .child(
                div()
                    .id(id)
                    .track_focus(focus)
                    .tab_index(6)
                    .key_context("LibrarySourceInput")
                    .p_2()
                    .border_1()
                    .border_color(rgb(if shell.active_text_input == Some(kind) {
                        shell.palette.accent
                    } else {
                        shell.palette.border
                    }))
                    .when(shell.active_text_input == Some(kind), |this| {
                        this.shadow(vec![focus_ring(shell.palette.accent)])
                    })
                    .bg(rgb(shell.palette.input))
                    .text_xs()
                    .child(if input.text().is_empty() {
                        div()
                            .text_color(rgb(shell.palette.muted))
                            .child(placeholder)
                    } else {
                        div()
                            .text_color(rgb(shell.palette.text))
                            .child(input.text().to_owned())
                    })
                    .child(NativeInputBinding::new(focus.clone(), cx.entity().clone()))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |shell, _, window, cx| {
                            shell.on_library_input_focus(kind, window, cx)
                        }),
                    )
                    .on_key_down(cx.listener(ShellApp::on_library_input_key_down)),
            )
    };
    let syncing = shell.library_task.is_some();
    render_readonly_drawer(
        "library",
        4,
        "棋谱库",
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(div().text_xs().text_color(rgb(shell.palette.muted)).child(
                "仅同步明确允许再分发的公开 GitHub SGF 仓库。配置会保存在本机，Git 不会弹出凭据提示。",
            ))
            .child(
                Button::new("library-save-current-game")
                    .small()
                    .primary()
                    .child(icon_label(
                        ShellIcon::Save,
                        "保存当前棋谱到棋谱库",
                        shell.palette.muted,
                    ))
                    .on_click(cx.listener(|shell, _, _, cx| {
                        let source = shell.current_record_source();
                        shell.save_current_game_to_library(source, Vec::new(), cx);
                    })),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .children(shell.library_sources.iter().map(|source| {
                        let source_id = source.id.clone();
                        Button::new(gpui::SharedString::from(format!("library-source-{source_id}")))
                            .small()
                            .ghost()
                            .selected(shell.library_selected_source.as_deref() == Some(source.id.as_str()))
                            .label(source.name.clone())
                            .on_click(cx.listener(move |shell, _, _, cx| shell.select_library_source(&source_id, cx)))
                    }))
                    .child(
                        Button::new("library-new-source")
                            .small()
                            .outline()
                            .label("+ 新来源")
                            .on_click(cx.listener(|shell, _, _, cx| shell.new_library_source(cx))),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_3()
                    .border_1()
                    .border_color(rgb(shell.palette.border))
                    .bg(rgb(shell.palette.panel))
                    .child(field("library-source-id", "来源 ID", "例如 pro-games", crate::ActiveTextInput::LibraryId, &shell.text_inputs.library_id_focus_handle, &shell.text_inputs.library_id_input))
                    .child(field("library-source-name", "名称", "棋谱库名称", crate::ActiveTextInput::LibraryName, &shell.text_inputs.library_name_focus_handle, &shell.text_inputs.library_name_input))
                    .child(field("library-source-url", "GitHub HTTPS URL", "https://github.com/owner/repository", crate::ActiveTextInput::LibraryGithubUrl, &shell.text_inputs.library_github_url_focus_handle, &shell.text_inputs.library_github_url_input))
                    .child(field("library-source-ref", "Git ref", "main", crate::ActiveTextInput::LibraryReference, &shell.text_inputs.library_reference_focus_handle, &shell.text_inputs.library_reference_input))
                    .child(field("library-license-name", "许可证名称", "例如 CC BY 4.0", crate::ActiveTextInput::LibraryLicenseName, &shell.text_inputs.library_license_name_focus_handle, &shell.text_inputs.library_license_name_input))
                    .child(field("library-license-url", "许可证证据 URL", "公开许可证页面", crate::ActiveTextInput::LibraryLicenseUrl, &shell.text_inputs.library_license_url_focus_handle, &shell.text_inputs.library_license_url_input))
                    .child(Checkbox::new("library-rights-confirmed").checked(shell.library_rights_confirmed).label("我已确认该来源明确允许棋谱再分发").on_click(cx.listener(|shell, checked: &bool, _, cx| shell.toggle_library_rights(*checked, cx))))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(
                                div()
                                    .flex()
                                    .gap_1()
                                    .child(Button::new("library-sync").small().primary().disabled(syncing).label(if syncing { "正在同步…" } else { "保存并同步" }).on_click(cx.listener(|shell, _, _, cx| shell.sync_library(cx))))
                                    .children(shell.library_selected_source.is_some().then(|| {
                                        Button::new("library-remove-source")
                                            .small()
                                            .danger()
                                            .label("移除配置")
                                            .disabled(syncing)
                                            .on_click(cx.listener(|shell, _, _, cx| shell.remove_selected_library_source(cx)))
                                    })),
                            )
                            .child(div().text_xs().text_color(rgb(shell.palette.muted)).child(shell.library_status.clone())),
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
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(shell.palette.subtle))
                            .child(format!("已同步棋谱（{}）", shell.library_entries.len())),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_1()
                            .child(
                                Button::new("library-view-gallery")
                                    .xsmall()
                                    .ghost()
                                    .selected(shell.library_view_mode == crate::LibraryViewMode::Gallery)
                                    .label("缩略图")
                                    .tooltip("以缩略图形式展示（对局名 + 盘面）")
                                    .on_click(cx.listener(|shell, _, _, cx| {
                                        if shell.library_view_mode != crate::LibraryViewMode::Gallery {
                                            shell.toggle_library_view_mode(cx);
                                        }
                                    })),
                            )
                            .child(
                                Button::new("library-view-list")
                                    .xsmall()
                                    .ghost()
                                    .selected(shell.library_view_mode == crate::LibraryViewMode::List)
                                    .label("列表")
                                    .tooltip("以列表形式展示（编号 + 黑方 + 白方 + 结果）")
                                    .on_click(cx.listener(|shell, _, _, cx| {
                                        if shell.library_view_mode != crate::LibraryViewMode::List {
                                            shell.toggle_library_view_mode(cx);
                                        }
                                    })),
                            ),
                    ),
            )
            .child(match shell.library_view_mode {
                crate::LibraryViewMode::Gallery => render_library_gallery(shell, cx),
                crate::LibraryViewMode::List => render_library_list(shell, cx),
            })
            .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(rgb(shell.palette.subtle)).child("最近打开"))
            .child(div().flex().flex_col().gap_1().children(recent_files.into_iter().map(|entry| {
                let entry_id = entry.id.clone();
                Button::new(gpui::SharedString::from(format!("library-recent-{}", entry.id)))
                    .small()
                    .ghost()
                    .label(if entry.is_missing {
                        format!("{}（文件不存在）", entry.display_name)
                    } else {
                        entry.display_name
                    })
                    .disabled(entry.is_missing)
                    .on_click(cx.listener(move |shell, _, _, cx| {
                        shell.open_recent_file(&entry_id, cx);
                    }) )
            }))),
        shell,
        cx,
    )
}

/// Gallery display form: a wrapping grid of cards, each showing a board
/// thumbnail and the game name. Thumbnails are rendered lazily (fingerprint
/// keyed) and cached in the shell.
fn render_library_gallery(shell: &ShellApp, cx: &Context<ShellApp>) -> Div {
    div().flex().flex_wrap().gap_2().items_start().children(
        shell.library_entries.iter().take(200).map(|entry| {
            let id = entry.entry_id();
            let path = entry.path.clone();
            let name = entry.metadata.display_name(&entry.relative_path.clone());
            let thumbnail = shell.library_thumbnail_image(&id);
            div()
                .id(gpui::SharedString::from(format!("library-gallery-{id}")))
                .debug_selector(move || format!("library-gallery-{id}"))
                .flex()
                .flex_col()
                .gap_1()
                .w(px(112.0))
                .p_1()
                .border_1()
                .border_color(rgb(shell.palette.border))
                .rounded_md()
                .bg(rgb(shell.palette.panel))
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |shell, _, _, cx| shell.open_library_entry(path.clone(), cx)),
                )
                .child(if let Some(image) = thumbnail {
                    gpui::img(image)
                        .w(px(104.0))
                        .h(px(104.0))
                        .object_fit(gpui::ObjectFit::Contain)
                        .into_any_element()
                } else {
                    div()
                        .w(px(104.0))
                        .h(px(104.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_xs()
                        .text_color(rgb(shell.palette.muted))
                        .child("…")
                        .into_any_element()
                })
                .child(
                    div()
                        .w(px(104.0))
                        .text_xs()
                        .text_color(rgb(shell.palette.text))
                        .child(name),
                )
        }),
    )
}

/// One row of the library list view: a stable game number plus the header
/// columns the user asked for (black, white, result). Kept as a pure value so
/// the renderer only paints and the row shape is testable without a window.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LibraryListRow {
    pub number: String,
    pub black: String,
    pub white: String,
    pub result: String,
    pub entry_id: String,
    pub path: std::path::PathBuf,
}

/// Builds list rows from the *whole* sorted library. The number is the entry's
/// rank within the library (1-based), never the rendered-slice index, so a
/// bounded render limit does not renumber the rows shown. Missing header
/// values render as an em dash rather than an empty cell.
pub(crate) fn library_list_rows(
    entries: &[ryusei_host::SgfLibraryEntry],
    record_numbers: &std::collections::HashMap<String, u64>,
) -> Vec<LibraryListRow> {
    const RENDER_LIMIT: usize = 200;
    entries
        .iter()
        .enumerate()
        .filter(|(index, _)| *index < RENDER_LIMIT)
        .map(|(index, entry)| {
            let entry_id = entry.entry_id();
            let number = record_numbers
                .get(&entry_id)
                .map(|number| number.to_string())
                .unwrap_or_else(|| (index + 1).to_string());
            LibraryListRow {
                number,
                black: entry
                    .metadata
                    .black
                    .clone()
                    .unwrap_or_else(|| "—".to_owned()),
                white: entry
                    .metadata
                    .white
                    .clone()
                    .unwrap_or_else(|| "—".to_owned()),
                result: entry
                    .metadata
                    .result
                    .clone()
                    .unwrap_or_else(|| "—".to_owned()),
                entry_id,
                path: entry.path.clone(),
            }
        })
        .collect()
}

/// List display form: rows of game number + black + white + result.
fn render_library_list(shell: &ShellApp, cx: &Context<ShellApp>) -> Div {
    div().flex().flex_col().gap_1().children(
        library_list_rows(&shell.library_entries, &shell.library_record_numbers)
            .into_iter()
            .map(|row| {
                let id = row.entry_id.clone();
                let path = row.path.clone();
                div()
                    .id(gpui::SharedString::from(format!("library-list-{id}")))
                    .debug_selector(move || format!("library-list-{id}"))
                    .flex()
                    .items_center()
                    .gap_2()
                    .p_2()
                    .border_1()
                    .border_color(rgb(shell.palette.border))
                    .rounded_md()
                    .bg(rgb(shell.palette.panel))
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |shell, _, _, cx| {
                            shell.open_library_entry(path.clone(), cx)
                        }),
                    )
                    .child(
                        div()
                            .w(px(24.0))
                            .flex_none()
                            .text_xs()
                            .text_color(rgb(shell.palette.muted))
                            .child(row.number),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .text_color(rgb(shell.palette.text))
                            .child(row.black),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .text_color(rgb(shell.palette.text))
                            .child(row.white),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(rgb(shell.palette.subtle))
                            .child(row.result),
                    )
            }),
    )
}

/// Captures a public StarRiver/live page into the active read-only session.
pub fn render_live_capture_drawer(shell: &ShellApp, cx: &Context<ShellApp>) -> Stateful<Div> {
    render_readonly_drawer(
        "live-capture",
        8,
        "公共直播",
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(div().text_xs().text_color(rgb(shell.palette.muted)).child(
                "输入公开 HTTPS 直播页地址或 OGS 游戏地址（例如 https://online-go.com/game/42）。Ryusei 只读取公开页面与 SGF，不会连接私有接口。",
            ))
            .child(
                div()
                    .track_focus(&shell.text_inputs.live_url_focus_handle)
                    .tab_index(4)
                    .key_context("LiveUrlInput")
                    .p_2p5()
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(
                        if shell.active_text_input == Some(crate::ActiveTextInput::LiveUrl) {
                            shell.palette.accent
                        } else {
                            shell.palette.border
                        },
                    ))
                    .bg(rgb(shell.palette.input))
                    .text_xs()
                    .text_color(rgb(shell.palette.text))
                    .child(if shell.text_inputs.live_url_input.text().is_empty() {
                        div()
                            .text_color(rgb(shell.palette.muted))
                            .child("https://example.com/live/...")
                    } else {
                        div()
                            .text_color(rgb(shell.palette.text))
                            .child(shell.text_inputs.live_url_input.text().to_owned())
                    })
                    .child(NativeInputBinding::new(
                        shell.text_inputs.live_url_focus_handle.clone(),
                        cx.entity().clone(),
                    ))
                    .on_mouse_down(MouseButton::Left, cx.listener(ShellApp::on_live_url_focus))
                    .on_key_down(cx.listener(ShellApp::on_live_url_key_down)),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        Button::new("live-capture-submit")
                            .small()
                            .primary()
                            .label("载入直播棋谱")
                            .on_click(cx.listener(|shell, _, _, cx| {
                                shell.capture_public_live_game(cx);
                            })),
                    )
                    .child(
                        Button::new("live-capture-clear")
                            .small()
                            .ghost()
                            .label("清空")
                            .on_click(cx.listener(|shell, _, _, cx| {
                                shell.text_inputs.live_url_input.set_text("");
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("live-capture-refresh")
                            .small()
                            .outline()
                            .label("刷新 OGS")
                            .disabled(
                                shell
                                    .live_source_url
                                    .as_deref()
                                    .and_then(ryusei_host::ogs_game_id_from_public_url)
                                    .is_none(),
                            )
                            .on_click(cx.listener(|shell, _, _, cx| {
                                if let Some(url) = shell.live_source_url.clone() {
                                    shell.text_inputs.live_url_input.set_text(url);
                                    shell.capture_public_live_game(cx);
                                }
                            })),
                    )
                    .child(
                        Button::new("live-capture-save-library")
                            .small()
                            .outline()
                            .label("保存快照到库")
                            .disabled(shell.live_source_url.is_none())
                            .on_click(cx.listener(|shell, _, _, cx| {
                                if let Some(url) = shell.live_source_url.clone() {
                                    shell.save_current_game_to_library(
                                        ryusei_domain_core::RecordSource::Live { page_url: url },
                                        vec!["直播快照".to_owned()],
                                        cx,
                                    );
                                }
                            })),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(shell.palette.muted))
                    .child(if let Some(state) = shell.live_ogs_state.as_ref() {
                        let move_desc = if state.move_number == 0 {
                            "开局".to_owned()
                        } else {
                            format!("第 {} 手", state.move_number)
                        };
                        format!(
                            "OGS #{} · {} vs {} · {} · {}",
                            state.game_id,
                            state.black_name,
                            state.white_name,
                            move_desc,
                            state.phase,
                        )
                    } else {
                        "载入后会切换到‘实时’只读模式，并启用连续分析。".to_owned()
                    }),
            ),
        shell,
        cx,
    )
}

/// OGS account workspace: app-internal login, account state, and game connect.
pub fn render_ogs_account_drawer(shell: &ShellApp, cx: &Context<ShellApp>) -> Stateful<Div> {
    let snapshot = shell.ogs_client.snapshot();
    let signed_in = snapshot.user.is_some();
    let credential_ok = shell.ogs_client.credential_storage_available();

    let text_input = |id: &'static str,
                      input: &crate::native_text_input::NativeTextInput,
                      focus: &FocusHandle,
                      active: crate::ActiveTextInput,
                      placeholder: &'static str,
                      secure: bool,
                      on_focus: fn(
        &mut ShellApp,
        &MouseDownEvent,
        &mut Window,
        &mut Context<ShellApp>,
    ),
                      on_key: fn(
        &mut ShellApp,
        &gpui::KeyDownEvent,
        &mut Window,
        &mut Context<ShellApp>,
    )| {
        div()
            .track_focus(focus)
            .key_context(id)
            .p_2p5()
            .rounded_md()
            .border_1()
            .border_color(rgb(if shell.active_text_input == Some(active) {
                shell.palette.accent
            } else {
                shell.palette.border
            }))
            .when(shell.active_text_input == Some(active), |this| {
                this.shadow(vec![focus_ring(shell.palette.accent)])
            })
            .bg(rgb(shell.palette.input))
            .text_xs()
            .text_color(rgb(shell.palette.text))
            .child(if input.text().is_empty() {
                div()
                    .text_color(rgb(shell.palette.muted))
                    .child(placeholder)
            } else if secure {
                // Mask the password so it never appears on screen, in a
                // screenshot, or in a screen share.
                div()
                    .text_color(rgb(shell.palette.text))
                    .child("•".repeat(input.text().chars().count()))
            } else {
                div()
                    .text_color(rgb(shell.palette.text))
                    .child(input.text().to_owned())
            })
            .child(NativeInputBinding::new(focus.clone(), cx.entity().clone()))
            .on_mouse_down(MouseButton::Left, cx.listener(on_focus))
            .on_key_down(cx.listener(on_key))
    };

    render_readonly_drawer(
        "ogs-account",
        9,
        "OGS 账户",
        div()
            .flex()
            .flex_col()
            .gap_3()
            .when(!signed_in, |this| {
                this.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(shell.palette.muted))
                                .child("使用 OGS 账号登录。密码仅用于本次登录，绝不保存；会话令牌经系统钥匙串加密保存。"),
                        )
                        .child(text_input(
                            "OgsUsernameInput",
                            &shell.text_inputs.ogs_username_input,
                            &shell.text_inputs.ogs_username_focus_handle,
                            crate::ActiveTextInput::OgsUsername,
                            "用户名",
                            false,
                            ShellApp::on_ogs_username_focus,
                            ShellApp::on_ogs_username_key_down,
                        ))
                        .child(text_input(
                            "OgsPasswordInput",
                            &shell.text_inputs.ogs_password_input,
                            &shell.text_inputs.ogs_password_focus_handle,
                            crate::ActiveTextInput::OgsPassword,
                            "密码",
                            true,
                            ShellApp::on_ogs_password_focus,
                            ShellApp::on_ogs_password_key_down,
                        ))
                        .child(
                            div()
                                .flex()
                                .gap_2()
                                .child(
                                    Button::new("ogs-login-submit")
                                        .small()
                                        .primary()
                                        .label("登录")
                                        .disabled(shell.ogs_login_in_progress)
                                        .on_click(cx.listener(|shell, _, _, cx| shell.ogs_login(cx))),
                                ),
                        ),
                )
            })
            .when(signed_in, |this| {
                let name = snapshot
                    .user
                    .as_ref()
                    .and_then(|u| u.get("username"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("OGS user");
                this.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_base()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(shell.palette.text))
                                .child(name.to_owned()),
                        )
                        .child(
                            Button::new("ogs-logout")
                                .small()
                                .warning()
                                .label("登出")
                                .on_click(cx.listener(|shell, _, _, cx| shell.ogs_logout(cx))),
                        ),
                )
            })
            .when(!credential_ok, |this| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(rgb(shell.palette.danger_text))
                        .child("未检测到可用的系统钥匙串，登录会话不会在重启后保留。"),
                )
            })
            .when(credential_ok && snapshot.last_error.is_some(), |this| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(rgb(shell.palette.danger_text))
                        .child(snapshot.last_error.clone().unwrap_or_default()),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(shell.palette.muted))
                            .child("连接 OGS 对局（登录后输入对局 ID）。连接后棋盘进入公平竞赛只读模式。"),
                    )
                    .child(text_input(
                        "OgsGameIdInput",
                        &shell.text_inputs.ogs_game_id_input,
                        &shell.text_inputs.ogs_game_id_focus_handle,
                        crate::ActiveTextInput::OgsGameId,
                        "对局 ID",
                        false,
                        ShellApp::on_ogs_game_id_focus,
                        ShellApp::on_ogs_game_id_key_down,
                    ))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                Button::new("ogs-connect-game")
                                    .small()
                                    .outline()
                                    .label("连接对局")
                                    .disabled(!signed_in)
                                    .on_click(cx.listener(|shell, _, _, cx| shell.connect_ogs_game(cx))),
                            )
                            .when(shell.ogs_client.competition_game_id().is_some(), |this| {
                                let game_id = shell.ogs_client.competition_game_id().unwrap();
                                this.child(
                                    Button::new("ogs-save-to-library")
                                        .small()
                                        .outline()
                                        .label("保存对局到库")
                                        .on_click(cx.listener(move |shell, _, _, cx| {
                                            shell.save_current_game_to_library(
                                                ryusei_domain_core::RecordSource::Ogs { game_id },
                                                vec!["OGS".to_owned()],
                                                cx,
                                            );
                                        })),
                                )
                            }),
                    ),
            )
            .when(signed_in, |this| {
                let searching = snapshot.matchmaking_status
                    == ryusei_host::OgsMatchmakingStatus::Searching;
                this.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(shell.palette.muted))
                                .child(match snapshot.matchmaking_status {
                                    ryusei_host::OgsMatchmakingStatus::Idle => "自动匹配：空闲",
                                    ryusei_host::OgsMatchmakingStatus::Searching => "自动匹配：寻找中…",
                                    ryusei_host::OgsMatchmakingStatus::Matched => "自动匹配：已匹配",
                                }),
                        )
                        .child(if searching {
                            Button::new("ogs-cancel-automatch")
                                .small()
                                .warning()
                                .label("取消自动匹配")
                                .on_click(cx.listener(|shell, _, _, cx| {
                                    shell.ogs_cancel_automatch(cx);
                                }))
                        } else {
                            Button::new("ogs-start-automatch")
                                .small()
                                .outline()
                                .label("开始自动匹配")
                                .on_click(cx.listener(|shell, _, _, cx| {
                                    // Route through the guarded remote-match entry:
                                    // it locks fair-play analysis before starting
                                    // matchmaking, so an analysis engine connected
                                    // earlier can never stream onto the OGS board.
                                    shell.enter_ogs_remote_match(cx);
                                }))
                        }),
                )
            })
            .child(if let Some(game) = snapshot.online_game.as_ref() {
                let clock_text = game.clock.as_ref().map(|clock| {
                    format!(
                        "黑 {:.0}s / 白 {:.0}s{}",
                        clock.black_main_remaining.as_secs_f64(),
                        clock.white_main_remaining.as_secs_f64(),
                        if clock.paused { " · 暂停" } else { "" }
                    )
                });
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(shell.palette.muted))
                            .child(format!(
                                "已连接 OGS #{} · {} vs {} · {} · 行棋方 {}",
                                game.game_id,
                                game.black_name,
                                game.white_name,
                                if game.move_number == 0 {
                                    "开局".to_owned()
                                } else {
                                    format!("第 {} 手", game.move_number)
                                },
                                match game.next_player {
                                    Some(ryusei_domain_core::Color::Black) => "黑",
                                    Some(ryusei_domain_core::Color::White) => "白",
                                    None => "?",
                                },
                            )),
                    )
                    .child(if let Some(clock_text) = clock_text {
                        div()
                            .text_xs()
                            .text_color(rgb(shell.palette.text))
                            .child(clock_text)
                    } else {
                        div()
                    })
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                Button::new("ogs-pass")
                                    .small()
                                    .outline()
                                    .label("停一手")
                                    .on_click(cx.listener(|shell, _, _, cx| shell.ogs_pass(cx))),
                            )
                            .child(
                                Button::new("ogs-resign")
                                    .small()
                                    .warning()
                                    .label("认输")
                                    .on_click(cx.listener(|shell, _, _, cx| shell.ogs_resign(cx))),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                Button::new("ogs-toggle-dead-marking")
                                    .small()
                                    .outline()
                                    .selected(shell.ogs_marking_dead)
                                    .label(if shell.ogs_marking_dead {
                                        "标记死子中…"
                                    } else {
                                        "标记死子"
                                    })
                                    .tooltip("开启后点击棋盘切换死子标记")
                                    .on_click(cx.listener(|shell, _, _, cx| {
                                        shell.ogs_toggle_dead_marking(cx);
                                    })),
                            )
                            .child(
                                Button::new("ogs-clear-stones")
                                    .small()
                                    .ghost()
                                    .label("清空标记")
                                    .disabled(shell.ogs_removed_stones.is_empty())
                                    .on_click(cx.listener(|shell, _, _, cx| {
                                        shell.ogs_clear_dead_marking(cx);
                                    })),
                            )
                            .child(
                                Button::new("ogs-accept-stones")
                                    .small()
                                    .primary()
                                    .label(format!(
                                        "确认死子{}",
                                        if shell.ogs_removed_stones.is_empty() {
                                            String::new()
                                        } else {
                                            format!("({})", shell.ogs_removed_stones.len())
                                        }
                                    ))
                                    .tooltip("向服务器确认当前死子标记")
                                    .on_click(cx.listener(|shell, _, _, cx| {
                                        shell.ogs_accept_removed_stones(cx);
                                    })),
                            ),
                    )
                    .child(
                        Button::new("ogs-send-chat")
                            .small()
                            .ghost()
                            .label("发送聊天")
                            .on_click(cx.listener(|shell, _, _, cx| shell.ogs_send_chat(cx))),
                    )
                    .child(
                        div()
                            .track_focus(&shell.text_inputs.ogs_chat_focus_handle)
                            .tab_index(5)
                            .key_context("OgsChatInput")
                            .p_2()
                            .rounded_md()
                            .border_1()
                            .border_color(rgb(if shell.active_text_input
                                == Some(crate::ActiveTextInput::OgsChat)
                            {
                                shell.palette.accent
                            } else {
                                shell.palette.border
                            }))
                            .bg(rgb(shell.palette.input))
                            .text_xs()
                            .text_color(rgb(shell.palette.text))
                            .child(if shell.text_inputs.ogs_chat_input.text().is_empty() {
                                div()
                                    .text_color(rgb(shell.palette.muted))
                                    .child("输入聊天…")
                            } else {
                                div()
                                    .text_color(rgb(shell.palette.text))
                                    .child(shell.text_inputs.ogs_chat_input.text().to_owned())
                            })
                            .child(NativeInputBinding::new(
                                shell.text_inputs.ogs_chat_focus_handle.clone(),
                                cx.entity().clone(),
                            ))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(ShellApp::on_ogs_chat_focus),
                            )
                            .on_key_down(cx.listener(ShellApp::on_ogs_chat_key_down)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .children(game.chat.iter().rev().take(20).map(|line| {
                                div()
                                    .text_xs()
                                    .text_color(rgb(shell.palette.text))
                                    .child(format!("{}: {}", line.username, line.body))
                            })),
                    )
            } else {
                div()
                    .text_xs()
                    .text_color(rgb(shell.palette.muted))
                    .child("尚未连接对局。")
            }),
        shell,
        cx,
    )
}

/// Shows the local profile identity used by the navigation rail.
#[allow(dead_code)]
pub fn render_profile_drawer(shell: &ShellApp, cx: &Context<ShellApp>) -> Stateful<Div> {
    let name = shell
        .settings
        .get_str("profile.display_name")
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("Player")
        .to_owned();
    render_readonly_drawer(
        "profile",
        5,
        "Profile",
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .text_base()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(shell.palette.text))
                    .child(name),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(shell.palette.muted))
                    .child(format!("{} 个本地会话", shell.workspace_tabs.tabs().len())),
            ),
        shell,
        cx,
    )
}

/// Opens the user's current objective and plan workspace.
pub fn render_goals_drawer(shell: &ShellApp, cx: &Context<ShellApp>) -> Stateful<Div> {
    let goal = shell
        .settings
        .get_str("profile.current_goal")
        .filter(|goal| !goal.trim().is_empty())
        .unwrap_or("尚未设置目标")
        .to_owned();
    let plan = shell
        .settings
        .get_str("profile.current_plan")
        .filter(|plan| !plan.trim().is_empty())
        .unwrap_or("从棋谱库或当前会话开始制定计划")
        .to_owned();
    render_readonly_drawer(
        "goals",
        7,
        "目标与计划",
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(shell.palette.muted))
                    .child("当前目标"),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(shell.palette.text))
                    .child(goal),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(shell.palette.muted))
                    .child("完成计划"),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(shell.palette.text))
                    .child(plan),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        Button::new("goal-set-current")
                            .small()
                            .outline()
                            .label("设为当前会话目标")
                            .on_click(cx.listener(|shell, _, _, cx| {
                                shell.set_goal_from_active_session(cx);
                            })),
                    )
                    .child(
                        Button::new("goal-complete-plan")
                            .small()
                            .primary()
                            .label("完成计划")
                            .on_click(cx.listener(|shell, _, _, cx| {
                                shell.complete_current_plan(cx);
                            })),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(shell.palette.muted))
                    .child("目标和计划会跟随设置持久化。"),
            ),
        shell,
        cx,
    )
}

/// Renders the validated host setting table as native, immediately persisted controls.
pub fn render_preferences_drawer(shell: &ShellApp, cx: &Context<ShellApp>) -> Stateful<Div> {
    let mut settings = div().flex().flex_col().gap_2();
    for row in panel_setting_rows(&shell.settings) {
        settings = settings.child(render_preference_row(shell, cx, row));
    }
    render_readonly_drawer(
        "preferences",
        6,
        "设置",
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(shell.palette.muted))
                    .child("更改会立即校验并保存。文本或数值编辑按 Enter 应用，Esc 取消。"),
            )
            .child(settings),
        shell,
        cx,
    )
}

fn render_preference_row(shell: &ShellApp, cx: &Context<ShellApp>, row: SettingRow) -> Div {
    let row_key = row.key.clone();
    let value = editable_setting_value(row.value.as_ref());
    let display_value = if value.chars().count() > 28 {
        format!("{}…", value.chars().take(27).collect::<String>())
    } else {
        value
    };
    let control = if row.kind == ryusei_host::SettingKind::Boolean {
        let checked = row
            .value
            .as_ref()
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let listener_row = row.clone();
        Checkbox::new(gpui::SharedString::from(format!(
            "preference-toggle-{row_key}"
        )))
        .checked(checked)
        .on_click(cx.listener(move |shell, _, _, cx| {
            shell.apply_settings_edit(crate::settings_form::toggle_boolean_edit(&listener_row));
            cx.notify();
        }))
        .into_any_element()
    } else if shell.settings_editing_key.as_deref() == Some(row_key.as_str()) {
        div()
            .id(gpui::SharedString::from(format!(
                "preference-input-{row_key}"
            )))
            .track_focus(&shell.text_inputs.settings_input_focus_handle)
            .tab_index(7)
            .key_context("SettingsInput")
            .w(px(150.0))
            .px_2()
            .py_1p5()
            .rounded_md()
            .border_1()
            .border_color(rgb(shell.palette.accent))
            .bg(rgb(shell.palette.input))
            .text_xs()
            .text_color(rgb(shell.palette.text))
            .child(shell.text_inputs.settings_draft.clone())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(ShellApp::on_settings_input_focus),
            )
            .on_key_down(cx.listener(ShellApp::on_settings_key_down))
            .into_any_element()
    } else {
        let listener_row = row.clone();
        Button::new(gpui::SharedString::from(format!(
            "preference-edit-{row_key}"
        )))
        .xsmall()
        .outline()
        .label(display_value)
        .on_click(cx.listener(move |shell, _, window, cx| {
            shell.settings_editing_key = Some(listener_row.key.clone());
            shell.text_inputs.settings_draft = listener_row
                .value
                .as_ref()
                .map(|value| editable_setting_value(Some(value)))
                .unwrap_or_default()
                .into();
            window.focus(&shell.text_inputs.settings_input_focus_handle);
            cx.notify();
        }))
        .into_any_element()
    };
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_3()
        .py_2()
        .border_b_1()
        .border_color(rgb(shell.palette.border))
        .child(
            div()
                .min_w_0()
                .flex_1()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(shell.palette.text))
                        .child(row.label),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(shell.palette.muted))
                        .child(row.key),
                ),
        )
        .child(control)
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

/// Drawer for whole-game AI review: pick a budget profile and watch progress.
pub fn render_review_drawer(shell: &ShellApp, cx: &Context<ShellApp>) -> Stateful<Div> {
    let running = shell
        .batch_review_progress
        .as_ref()
        .is_some_and(|progress| progress.is_running);
    let progress = shell.batch_review_progress.as_ref();
    let active_profile = shell.batch_review_profile;

    let mut profiles = div().flex().flex_col().gap_2();
    for profile in ryusei_domain_core::ReviewProfile::ALL {
        // The 80-visit ultra-fast background profile is not a user-selectable
        // whole-game review tier; keep it out of the picker.
        if profile == ryusei_domain_core::ReviewProfile::Quick80 {
            continue;
        }
        let selected = active_profile == Some(profile);
        let running_this = running && selected;
        profiles = profiles.child(
            div()
                .p_2p5()
                .rounded_md()
                .border_1()
                .border_color(rgb(if selected {
                    shell.palette.accent
                } else {
                    shell.palette.border
                }))
                .bg(rgb(if selected {
                    shell.palette.button_active
                } else {
                    shell.palette.input
                }))
                .cursor_pointer()
                .hover(|style| {
                    if selected {
                        style
                    } else {
                        style.bg(rgb(shell.palette.button_active))
                    }
                })
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
                                .text_sm()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(if selected {
                                    shell.palette.accent
                                } else {
                                    shell.palette.text
                                }))
                                .child(format!("{} {}", profile.english_label(), profile.label())),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(shell.palette.muted))
                                .child(format!("{} visits / move", profile.visits())),
                        ),
                )
                .child(if running_this {
                    Badge::new().small().child("分析中")
                } else {
                    Badge::new().small().child("选择")
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |shell, _, _, cx| {
                        shell.start_review_profile_action(profile, cx);
                    }),
                ),
        );
    }

    render_readonly_drawer(
        "review",
        9,
        "KataGo 全谱批量复盘",
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(shell.palette.muted))
                    .child("选择复盘算力档位后，引擎将沿当前主谱逐手推演，自动生成双方胜率曲线、目数差与恶手诊断报告。"),
            )
            .child(profiles)
            .children(progress.map(|progress| {
                let percent = if progress.total_moves == 0 {
                    0.0
                } else {
                    (progress.current_move as f32 / progress.total_moves as f32 * 100.0).clamp(0.0, 100.0)
                };
                div()
                    .p_3()
                    .rounded_md()
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
                                    .text_xs()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(shell.palette.text))
                                    .child(if progress.is_running { "KataGo 推演计算中…" } else { "复盘进度" }),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(rgb(shell.palette.accent))
                                    .child(format!("{}/{} 手 · {:.0}%", progress.current_move, progress.total_moves, percent)),
                            ),
                    )
                    .child(
                        div()
                            .w_full()
                            .h(px(8.0))
                            .rounded(px(4.0))
                            .bg(rgb(shell.palette.track))
                            .child(
                                div()
                                    .h_full()
                                    .rounded(px(4.0))
                                    .bg(rgb(shell.palette.accent))
                                    .w_full()
                                    .max_w(px(percent / 100.0 * 300.0)),
                            ),
                    )
            }))
            .child(
                Button::new("review-stop-btn")
                    .small()
                    .warning()
                    .label("停止复盘")
                    .disabled(!running)
                    .on_click(cx.listener(|shell, _, _, cx| {
                        shell.stop_whole_game_review(cx);
                    })),
            ),
        shell,
        cx,
    )
}

/// Match-setup drawer: houses the less-frequent session configuration that
/// used to crowd the full-width toolbar — time control, participant detail,
/// OGS remote/account, quick review and live capture. High-frequency controls
/// (participants pill + analysis) stay on the floating match capsule.
/// Comprehensive New Game & Match Setup Drawer.
pub fn render_match_setup_drawer(shell: &ShellApp, cx: &Context<ShellApp>) -> Stateful<Div> {
    let policy = shell.session_policy;
    let clock = shell.clock.state();
    let palette = shell.palette;
    let snapshot = shell.host.snapshot();
    let current_board_size = snapshot.board.width;

    let section = |title: &'static str, body: gpui::AnyElement| {
        div()
            .flex()
            .flex_col()
            .gap_1p5()
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(palette.text))
                    .child(title),
            )
            .child(body)
    };

    let time_button =
        |id: &'static str, label: &'static str, selected: bool, control: TimeControl| {
            Button::new(id)
                .small()
                .ghost()
                .selected(selected)
                .label(label)
                .on_click(cx.listener(move |shell, _, _, cx| shell.set_time_control(control, cx)))
        };

    let participant_button = |id: &'static str,
                              label: &'static str,
                              selected: bool,
                              participants: MatchParticipants,
                              mode: ryusei_domain_core::SessionMode| {
        Button::new(id)
            .small()
            .ghost()
            .selected(selected && policy.mode == mode)
            .label(label)
            .on_click(cx.listener(move |shell, _, _, cx| {
                shell.set_session_mode(mode, cx);
                shell.set_match_participants(participants, cx);
            }))
    };

    let size_button = |size: usize, label: &'static str| {
        Button::new(gpui::SharedString::from(format!("setup-size-{size}")))
            .small()
            .ghost()
            .selected(current_board_size == size)
            .label(label)
            .on_click(cx.listener(move |shell, _, _, cx| {
                shell.new_game_at(size, cx);
            }))
    };

    let rules_button = |ruleset: &'static str, label: &'static str| {
        let current_rule = snapshot
            .root_properties
            .get("RU")
            .and_then(|v| v.first())
            .map(|s| s.as_str())
            .unwrap_or("Chinese");
        let selected = current_rule.eq_ignore_ascii_case(ruleset)
            || (ruleset == "Ancient Chinese"
                && ryusei_host::GoRuleset::from_setting(Some(current_rule))
                    == ryusei_host::GoRuleset::AncientChinese);
        Button::new(gpui::SharedString::from(format!("setup-rule-{ruleset}")))
            .small()
            .ghost()
            .selected(selected)
            .label(label)
            .on_click(cx.listener(move |shell, _, _, cx| {
                shell.host.set_root_property("RU", vec![ruleset.to_owned()]);
                shell.apply_settings_edit(crate::SettingEdit::Set {
                    key: "game.default_ruleset".to_owned(),
                    value: serde_json::Value::String(ruleset.to_owned()),
                });
                if ruleset == "Ancient Chinese" {
                    shell.host.set_root_property("KM", vec!["0.0".to_owned()]);
                    shell.apply_settings_edit(crate::SettingEdit::Set {
                        key: "game.default_komi".to_owned(),
                        value: serde_json::json!(0.0),
                    });
                }
                cx.notify();
            }))
    };

    let remote_button = if policy.source == ryusei_domain_core::SessionSource::RemoteCompetition {
        Button::new("setup-leave-remote")
            .small()
            .warning()
            .label("退出远程对局")
            .on_click(cx.listener(|shell, _, _, cx| shell.leave_remote_match(cx)))
    } else {
        Button::new("setup-enter-remote")
            .small()
            .ghost()
            .label("连接 OGS 远程对局")
            .on_click(cx.listener(|shell, _, _, cx| shell.enter_ogs_remote_match(cx)))
    };

    let visits_button = |visits: u64, label: &'static str| {
        let current_visits = shell
            .settings
            .get("engines.analysis_max_visits")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(500);
        Button::new(gpui::SharedString::from(format!("setup-visits-{visits}")))
            .small()
            .ghost()
            .selected(current_visits == visits)
            .label(label)
            .on_click(cx.listener(move |shell, _, _, cx| {
                shell.apply_analysis_visits(visits, cx);
            }))
    };

    let content = div()
        .flex()
        .flex_col()
        .gap_3p5()
        .child(section(
            "对弈与打谱模式",
            div()
                .flex()
                .flex_wrap()
                .gap_1p5()
                .child(participant_button(
                    "setup-mode-study",
                    "打谱研讨 (Record)",
                    policy.mode == ryusei_domain_core::SessionMode::Record,
                    MatchParticipants::human_vs_human(),
                    ryusei_domain_core::SessionMode::Record,
                ))
                .child(participant_button(
                    "setup-players-human",
                    "双人对弈",
                    policy.participants == MatchParticipants::human_vs_human()
                        && policy.mode == ryusei_domain_core::SessionMode::Match,
                    MatchParticipants::human_vs_human(),
                    ryusei_domain_core::SessionMode::Match,
                ))
                .child(participant_button(
                    "setup-players-human-ai",
                    "人机对弈 (执黑)",
                    policy.participants == MatchParticipants::human_vs_ai()
                        && policy.mode == ryusei_domain_core::SessionMode::Match,
                    MatchParticipants::human_vs_ai(),
                    ryusei_domain_core::SessionMode::Match,
                ))
                .child(participant_button(
                    "setup-players-ai-human",
                    "机人对弈 (执白)",
                    policy.participants
                        == MatchParticipants {
                            black: PlayerKind::Ai,
                            white: PlayerKind::Human,
                        }
                        && policy.mode == ryusei_domain_core::SessionMode::Match,
                    MatchParticipants {
                        black: PlayerKind::Ai,
                        white: PlayerKind::Human,
                    },
                    ryusei_domain_core::SessionMode::Match,
                ))
                .child(participant_button(
                    "setup-players-ai-ai",
                    "AI×AI (自弈)",
                    policy.participants == MatchParticipants::ai_vs_ai()
                        && policy.mode == ryusei_domain_core::SessionMode::Match,
                    MatchParticipants::ai_vs_ai(),
                    ryusei_domain_core::SessionMode::Match,
                ))
                .into_any_element(),
        ))
        .child(section(
            "AI 棋力 / 算力档位",
            div()
                .flex()
                .flex_wrap()
                .gap_1p5()
                .child(visits_button(100, "100v (初级快速)"))
                .child(visits_button(500, "500v (业余中级)"))
                .child(visits_button(1500, "1500v (业余强豪)"))
                .child(visits_button(5000, "5000v (职业水平)"))
                .child(visits_button(0, "无限算力 (最强)"))
                .into_any_element(),
        ))
        .child(section(
            "棋盘规格",
            div()
                .flex()
                .gap_1p5()
                .child(size_button(19, "19 路 (标准)"))
                .child(size_button(13, "13 路 (中盘)"))
                .child(size_button(9, "9 路 (死活)"))
                .into_any_element(),
        ))
        .child(section(
            "规则体系",
            div()
                .flex()
                .flex_wrap()
                .gap_1p5()
                .child(rules_button("Chinese", "中国规则 (数子/贴3.75子)"))
                .child(rules_button("Japanese", "日本规则 (数目/贴6.5目)"))
                .child(rules_button("Ing", "应氏规则 (填满/贴8点)"))
                .child(rules_button("Ancient Chinese", "中国古棋 (还棋头/无贴目)"))
                .into_any_element(),
        ))
        .child(section(
            "时钟与读秒",
            div()
                .flex()
                .flex_wrap()
                .gap_1p5()
                .child(time_button(
                    "setup-time-none",
                    "无时钟 (自由)",
                    clock.control == TimeControl::None,
                    TimeControl::None,
                ))
                .child(time_button(
                    "setup-time-absolute",
                    "10 分钟包干",
                    clock.control
                        == TimeControl::Absolute {
                            main_time_secs: 600,
                        },
                    TimeControl::Absolute {
                        main_time_secs: 600,
                    },
                ))
                .child(time_button(
                    "setup-time-byo-yomi",
                    "20m + 3×30s 读秒",
                    clock.control
                        == TimeControl::ByoYomi {
                            main_time_secs: 1200,
                            period_time_secs: 30,
                            periods: 3,
                        },
                    TimeControl::ByoYomi {
                        main_time_secs: 1200,
                        period_time_secs: 30,
                        periods: 3,
                    },
                ))
                .child(time_button(
                    "setup-time-blitz",
                    "1m + 5×10s 快棋",
                    clock.control
                        == TimeControl::ByoYomi {
                            main_time_secs: 60,
                            period_time_secs: 10,
                            periods: 5,
                        },
                    TimeControl::ByoYomi {
                        main_time_secs: 60,
                        period_time_secs: 10,
                        periods: 5,
                    },
                ))
                .child(time_button(
                    "setup-time-fischer",
                    "10m + 10s 加秒",
                    clock.control
                        == TimeControl::Fischer {
                            main_time_secs: 600,
                            increment_secs: 10,
                        },
                    TimeControl::Fischer {
                        main_time_secs: 600,
                        increment_secs: 10,
                    },
                ))
                .into_any_element(),
        ))
        .child(section(
            "远程网络对局",
            div()
                .flex()
                .flex_wrap()
                .gap_1p5()
                .child(remote_button)
                .child(
                    Button::new("setup-ogs-account")
                        .small()
                        .ghost()
                        .selected(shell.ogs_auth_state == ryusei_host::OgsAuthState::Authenticated)
                        .label(match shell.ogs_auth_state {
                            ryusei_host::OgsAuthState::SignedOut => "OGS 账户",
                            ryusei_host::OgsAuthState::Authenticated => "OGS 已登录",
                        })
                        .on_click(cx.listener(|shell, _, _, cx| shell.open_ogs_account(cx))),
                )
                .child(
                    Button::new("setup-live-capture")
                        .small()
                        .ghost()
                        .label("导入直播")
                        .on_click(cx.listener(|shell, _, _, cx| shell.open_live_capture(cx))),
                )
                .into_any_element(),
        ))
        .child(
            div()
                .pt_2()
                .border_t_1()
                .border_color(rgb(palette.border))
                .flex()
                .items_center()
                .justify_end()
                .gap_2()
                .child(
                    Button::new("setup-start-game-btn")
                        .primary()
                        .disabled(
                            policy.source == ryusei_domain_core::SessionSource::RemoteCompetition,
                        )
                        .label(
                            if policy.source == ryusei_domain_core::SessionSource::RemoteCompetition
                            {
                                "远程对局由服务器自动开始"
                            } else {
                                "开始对局 ↵"
                            },
                        )
                        .on_click(cx.listener(|shell, _, window, cx| {
                            shell.start_new_match_from_setup(
                                &MouseDownEvent::default(),
                                window,
                                cx,
                            );
                        })),
                ),
        );

    render_readonly_drawer("match-setup", 11, "新建对局 / 对局设置", content, shell, cx)
}

/// Drawer for unified export: SGF file/clipboard, position PNG and animated GIF.
pub fn render_export_drawer(
    _snapshot: &GameSnapshot,
    shell: &ShellApp,
    cx: &Context<ShellApp>,
) -> Stateful<Div> {
    render_readonly_drawer(
        "export",
        10,
        "导出与分享棋谱",
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                Button::new("export-save-sgf-file")
                    .small()
                    .primary()
                    .child(icon_label(
                        ShellIcon::Save,
                        "导出标准 SGF 文件 (.sgf)",
                        shell.palette.muted,
                    ))
                    .on_click(cx.listener(|shell, _, _, cx| {
                        shell.save_game_as(cx);
                        cx.notify();
                    })),
            )
            .child(
                Button::new("export-clipboard-sgf")
                    .small()
                    .outline()
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
                Button::new("export-animated-gif")
                    .small()
                    .outline()
                    .child(icon_label(
                        ShellIcon::Film,
                        "导出动画 GIF 棋谱",
                        shell.palette.muted,
                    ))
                    .on_click(cx.listener(|shell, _, _, cx| {
                        shell.on_export_gif_action(cx);
                    })),
            )
            .child(
                Button::new("export-position-png")
                    .small()
                    .outline()
                    .child(icon_label(
                        ShellIcon::Image,
                        "导出当前局面高清图 (PNG)",
                        shell.palette.muted,
                    ))
                    .on_click(cx.listener(|shell, _, _, cx| {
                        shell.export_current_position_png(cx);
                    })),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(shell.palette.subtle))
                    .child("SGF 保留完整分支树与注释；PNG/GIF 导出到本地文件。"),
            ),
        shell,
        cx,
    )
}

#[cfg(test)]
mod tests {
    use super::{LibraryListRow, library_list_rows};
    use ryusei_domain_core::RecordMetadata;
    use ryusei_host::SgfLibraryEntry;
    use std::path::PathBuf;

    fn entry(source_id: &str, relative_path: &str, metadata: RecordMetadata) -> SgfLibraryEntry {
        SgfLibraryEntry {
            source_id: source_id.to_owned(),
            relative_path: relative_path.to_owned(),
            path: PathBuf::from("/checkout").join(relative_path),
            metadata,
        }
    }

    fn meta(black: Option<&str>, white: Option<&str>, result: Option<&str>) -> RecordMetadata {
        RecordMetadata {
            black: black.map(ToOwned::to_owned),
            white: white.map(ToOwned::to_owned),
            result: result.map(ToOwned::to_owned),
            ..RecordMetadata::default()
        }
    }

    #[test]
    fn list_rows_assign_rank_as_number_and_fill_columns() {
        let entries = vec![
            entry(
                "pro",
                "a.sgf",
                meta(Some("黑甲"), Some("白乙"), Some("B+R")),
            ),
            entry("pro", "b.sgf", meta(Some("丙"), Some("丁"), None)),
        ];
        let rows = library_list_rows(&entries, &std::collections::HashMap::new());
        assert_eq!(
            rows,
            vec![
                LibraryListRow {
                    number: "1".to_owned(),
                    black: "黑甲".to_owned(),
                    white: "白乙".to_owned(),
                    result: "B+R".to_owned(),
                    entry_id: "pro-a.sgf".to_owned(),
                    path: PathBuf::from("/checkout/a.sgf"),
                },
                LibraryListRow {
                    number: "2".to_owned(),
                    black: "丙".to_owned(),
                    white: "丁".to_owned(),
                    result: "—".to_owned(),
                    entry_id: "pro-b.sgf".to_owned(),
                    path: PathBuf::from("/checkout/b.sgf"),
                },
            ]
        );
    }

    #[test]
    fn list_rows_are_capped_at_two_hundred_and_numbering_is_by_rank() {
        let mut entries = Vec::new();
        for i in 0..220 {
            let name = format!("g{i:03}.sgf");
            entries.push(entry("pro", &name, meta(None, None, None)));
        }
        let rows = library_list_rows(&entries, &std::collections::HashMap::new());
        assert_eq!(rows.len(), 200);
        // First rendered row keeps its whole-library rank, not 1.
        assert_eq!(rows[0].number, "1");
        assert_eq!(rows[199].number, "200");
        // Rows beyond the render limit are excluded entirely.
        assert!(!rows.iter().any(|row| row.number == "201"));
    }

    #[test]
    fn list_rows_use_stable_record_numbers_when_index_has_them() {
        let entries = vec![
            entry("pro", "a.sgf", meta(Some("甲"), Some("乙"), Some("B+R"))),
            entry("pro", "b.sgf", meta(None, None, None)),
        ];
        // The index assigned b number 7 and a number 3 in an earlier run; the
        // rows must surface those stable numbers, not the list position.
        let numbers = std::collections::HashMap::from([
            ("pro-a.sgf".to_owned(), 3u64),
            ("pro-b.sgf".to_owned(), 7u64),
        ]);
        let rows = library_list_rows(&entries, &numbers);
        assert_eq!(rows[0].number, "3");
        assert_eq!(rows[1].number, "7");
    }
}
