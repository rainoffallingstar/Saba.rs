//! Panel rendering for the shell, split out of `main.rs` so the shell keeps
//! only state, actions and assembly. Every function is a pure view over
//! `ShellApp` state plus precomputed values; listeners are built with
//! `cx.listener` against `ShellApp` handlers.

mod drawers;
mod engine_panels;
mod plugin_dialogs;

pub use drawers::{
    render_about_drawer, render_export_drawer, render_game_info_drawer, render_goals_drawer,
    render_library_drawer, render_live_capture_drawer, render_match_setup_drawer,
    render_ogs_account_drawer, render_preferences_drawer, render_profile_drawer,
    render_review_drawer, render_score_drawer,
};
pub(crate) use engine_panels::render_gtp_terminal_body;
pub use engine_panels::{
    render_analysis_preview_panel, render_left_engine_sidebar, render_node_inspector_panel,
};
pub(crate) use plugin_dialogs::{
    plugin_icon, render_fox_sync_dialog, render_generic_plugin_dialog, render_katago_dialog,
    render_pinned_plugins_manager, render_position_to_sgf_dialog,
};

use std::rc::Rc;

use gpui::{
    App, Context, Div, FontWeight, InteractiveElement, MouseButton, MouseDownEvent, PathBuilder,
    Stateful, StatefulInteractiveElement, Window, canvas, div, hsla, linear_color_stop,
    linear_gradient, point, prelude::*, px, rgb,
};

use ryusei_domain_core::{AnalysisPolicy, GameMode, GameSnapshot, SessionMode};

use crate::icons::{self, ShellIcon};

/// An icon + label row for ghost buttons/tabs, tinted to the muted shell
/// foreground so it reads consistently across themes.
pub(crate) fn icon_label(icon: ShellIcon, label: &str, color: u32) -> Div {
    div()
        .flex()
        .items_center()
        .gap_1p5()
        .child(icons::icon(icon, 13.0, color))
        .child(div().child(label.to_owned()))
}

/// Height of the bottom analysis deck. Following the design, the deck is an
/// attachment of the center board column (inside `center-canvas`), not a
/// window-wide dock; the extra height over the design's 180px keeps the
/// winrate curve clearly readable in the narrower column.
pub const BOTTOM_DECK_HEIGHT: f32 = 240.0;

/// The design's focus ring (`--focus-ring`: 3px accent at 65% transparency),
/// applied to focused text inputs on top of their accent border.
pub(crate) fn focus_ring(accent: u32) -> gpui::BoxShadow {
    gpui::BoxShadow {
        color: hsla_from_accent(accent, 0.35),
        offset: gpui::point(gpui::px(0.0), gpui::px(0.0)),
        blur_radius: gpui::px(0.0),
        spread_radius: gpui::px(3.0),
    }
}

/// Builds the translucent ring color from the theme accent.
pub(crate) fn hsla_from_accent(accent: u32, alpha: f32) -> gpui::Hsla {
    let r = ((accent >> 16) & 0xff) as f32 / 255.0;
    let g = ((accent >> 8) & 0xff) as f32 / 255.0;
    let b = (accent & 0xff) as f32 / 255.0;
    gpui::Rgba { r, g, b, a: alpha }.into()
}

use gpui_component::badge::Badge;
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::{Disableable, Selectable, Sizable};

use crate::ShellApp;
use crate::engine_console::{best_analysis_winrate, parse_gtp_vertex};
use crate::goban_view::{pv_preview_points, render_goban, render_goban_click_layer};
use crate::layout::SplitPane;
use crate::settings::ThemeChoice;
use crate::theme::UiPalette;
use crate::variation_tree::VariationTreeLayout;
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
    snapshot: &GameSnapshot,
    window_width: f32,
    is_fullscreen: bool,
    shell: &ShellApp,
    cx: &Context<ShellApp>,
) -> Stateful<Div> {
    let palette = shell.palette;
    let title = shell.workspace_tabs.active_tab().title.clone();
    let session_mode = shell.session_policy.mode;
    let icon_color = palette.muted;
    let active_color = palette.accent;
    let fill_color = palette.accent;

    // Player metadata from SGF root properties.
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
    let rules = property("RU");
    let komi = property("KM");
    let rules_label = if !rules.is_empty() {
        rules
    } else if !komi.is_empty() {
        format!("贴{komi}目")
    } else {
        "中国规则".to_owned()
    };

    // Header clocks (server/local prediction via the shared clock controller).
    let clock_state = shell.clock.state();
    // Responsive breakpoint (design: ≤840px hides the VS-pill clock chips).
    let clocks_visible = window_width > 840.0
        && !matches!(clock_state.control, ryusei_domain_core::TimeControl::None);
    let black_clock = crate::ui_format::format_clock(clock_state.black);
    let white_clock = crate::ui_format::format_clock(clock_state.white);
    let is_black_turn = snapshot.board.next_player == ryusei_domain_core::Color::Black;

    div()
        .id("titlebar")
        .debug_selector(|| "titlebar".to_owned())
        .flex_none()
        .h(px(44.0))
        .flex()
        .items_center()
        .justify_between()
        .px_3()
        .border_b_1()
        .border_color(rgb(palette.border))
        .bg(rgb(palette.panel))
        // The in-app bar sits inside the native transparent titlebar area:
        // double-click behaves like the native chrome (zoom), per macOS HIG.
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|_, event: &MouseDownEvent, window, _| {
                if event.click_count == 2 {
                    window.zoom_window();
                }
            }),
        )
        // Left: traffic lights spacer + sidebar toggles + history arrows
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                // In native fullscreen the traffic lights only slide in on
                // hover over the menu-bar reveal area; keeping the spacer
                // would let the controls overlap them once they appear.
                .children((!is_fullscreen).then(|| div().w(px(72.0))))
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
                    Button::new("titlebar-bottom-deck-toggle")
                        .small()
                        .ghost()
                        .selected(shell.is_bottom_deck_open())
                        .label("▤")
                        .tooltip("切换底部分析面板 (Cmd+J)")
                        .on_click(cx.listener(|shell, _, _, cx| {
                            if shell.is_bottom_deck_open() {
                                shell.close_bottom_deck(cx);
                            } else {
                                shell.switch_bottom_tab(crate::BottomDeckTab::WinrateGraph, cx);
                            }
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
        // Center: game title + player VS pill + rules badge
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .flex_none()
                        .max_w(px(280.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_sm()
                        .text_color(rgb(palette.text))
                        .truncate()
                        .child(title.clone()),
                )
                .child(
                    div()
                        .id("player-vs-pill")
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_2p5()
                        .py_0p5()
                        .rounded(px(980.0))
                        .bg(rgb(palette.input))
                        .border_1()
                        .border_color(rgb(palette.border))
                        .cursor_pointer()
                        .hover(|style| style.border_color(rgb(palette.accent)))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|shell, _, _, cx| shell.open_match_setup(cx)),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_1p5()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(if is_black_turn {
                                            rgb(palette.accent)
                                        } else {
                                            rgb(palette.text)
                                        })
                                        .child("●"),
                                )
                                .child(
                                    div()
                                        .flex_none()
                                        .max_w(px(140.0))
                                        .truncate()
                                        .text_xs()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(black_name),
                                )
                                .children((!black_rank.is_empty()).then(|| {
                                    div()
                                        .text_xs()
                                        .text_color(rgb(palette.muted))
                                        .child(format!("({black_rank})"))
                                }))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(palette.subtle))
                                        .child(format!("({}提)", snapshot.black_captures)),
                                )
                                .children(clocks_visible.then(|| {
                                    // Clock chip (design: mono text on a small
                                    // rounded surface tile inside the VS pill).
                                    div()
                                        .px_1p5()
                                        .py_0p5()
                                        .rounded(px(crate::theme::radius::SM))
                                        .bg(rgb(palette.panel))
                                        .text_xs()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(rgb(if is_black_turn {
                                            palette.accent
                                        } else {
                                            palette.muted
                                        }))
                                        .child(black_clock)
                                })),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(palette.subtle))
                                .font_weight(FontWeight::BOLD)
                                .child("VS"),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_1p5()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(if !is_black_turn {
                                            rgb(palette.accent)
                                        } else {
                                            rgb(palette.text)
                                        })
                                        .child("○"),
                                )
                                .child(
                                    div()
                                        .flex_none()
                                        .max_w(px(140.0))
                                        .truncate()
                                        .text_xs()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(white_name),
                                )
                                .children((!white_rank.is_empty()).then(|| {
                                    div()
                                        .text_xs()
                                        .text_color(rgb(palette.muted))
                                        .child(format!("({white_rank})"))
                                }))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(palette.subtle))
                                        .child(format!("({}提)", snapshot.white_captures)),
                                )
                                .children(clocks_visible.then(|| {
                                    div()
                                        .px_1p5()
                                        .py_0p5()
                                        .rounded(px(crate::theme::radius::SM))
                                        .bg(rgb(palette.panel))
                                        .text_xs()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(rgb(if !is_black_turn {
                                            palette.accent
                                        } else {
                                            palette.muted
                                        }))
                                        .child(white_clock)
                                })),
                        ),
                )
                .child(Badge::new().small().child(format!(
                    "{} · {}",
                    match session_mode {
                        SessionMode::Match => "对弈",
                        SessionMode::Record => "打谱",
                        SessionMode::Live => "实时",
                    },
                    rules_label
                ))),
        )
        // Right: Batch review + Theme selector + Export GIF + Info icon + Right sidebar toggle icon
        .child(
            div()
                .flex()
                .items_center()
                .gap_1p5()
                .child(
                    Button::new("titlebar-batch-review")
                        .small()
                        .ghost()
                        .child(icon_label(ShellIcon::Sparkles, "全谱复盘", palette.muted))
                        .tooltip("全谱 AI 深度复盘：选择档位并逐手推演")
                        .on_click(cx.listener(|shell, _, _, cx| {
                            shell.open_review(cx);
                        })),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .p_0p5()
                        .rounded_md()
                        .bg(rgb(palette.input))
                        .border_1()
                        .border_color(rgb(palette.border))
                        .child(
                            Button::new("theme-btn-classic")
                                .small()
                                .ghost()
                                .label("榧木")
                                .tooltip("新榧木纹材质")
                                .on_click(cx.listener(|shell, _, _, cx| {
                                    shell.on_theme_selected(ThemeChoice::Classic, cx);
                                })),
                        )
                        .child(
                            Button::new("theme-btn-mist")
                                .small()
                                .ghost()
                                .label("白墨")
                                .tooltip("极简白墨材质")
                                .on_click(cx.listener(|shell, _, _, cx| {
                                    shell.on_theme_selected(ThemeChoice::Mist, cx);
                                })),
                        )
                        .child(
                            Button::new("theme-btn-dark")
                                .small()
                                .ghost()
                                .label("曜黑")
                                .tooltip("深空曜黑材质")
                                .on_click(cx.listener(|shell, _, _, cx| {
                                    shell.on_theme_selected(ThemeChoice::Dark, cx);
                                })),
                        ),
                )
                .child(
                    Button::new("titlebar-export-gif")
                        .small()
                        .ghost()
                        .child(icon_label(ShellIcon::Share, "导出", palette.muted))
                        .tooltip("导出 SGF / 局面 PNG / 动画 GIF")
                        .on_click(cx.listener(|shell, _, _, cx| {
                            shell.open_export(cx);
                        })),
                )
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

/// Floating match-control capsule above the goban.
///
/// After merging the top chrome into a single 44px titlebar, the session
/// controls no longer live in a full-width toolbar. Only the two high-frequency
/// groups stay floating here: the participant segmented pill (双人/人机/…) and
/// the analysis pill. Mode switching lives in the navigation rail, board tools
/// in the floating markup bar, and clock / OGS / review / live-capture settings
/// in the match-setup drawer opened from the player VS pill.
pub fn render_session_toolbar(shell: &ShellApp, cx: &Context<ShellApp>) -> Stateful<Div> {
    let policy = shell.session_policy;
    let mode = shell.mode;
    let active_tool = shell.active_tool;
    let show_analysis = shell
        .settings
        .get_bool("board.show_analysis")
        .unwrap_or(true);
    let snapshot = shell.host.snapshot();
    let has_markups = snapshot
        .nodes
        .iter()
        .find(|node| node.id == snapshot.current_node_id)
        .map(|node| {
            ["CR", "SQ", "TR", "MA", "LB", "LN", "AR"]
                .iter()
                .any(|property| node.properties.contains_key(*property))
        })
        .unwrap_or(false);

    let analysis_running = shell.analysis_task.is_some();
    let analysis_button = if analysis_running {
        Button::new("session-stop-analysis")
            .small()
            .warning()
            .label("停止分析")
            .on_click(cx.listener(|shell, _, _, cx| shell.stop_analysis(cx)))
    } else {
        Button::new("session-start-analysis")
            .small()
            .outline()
            .label("开始分析")
            .disabled(policy.analysis == AnalysisPolicy::FairPlayLockedOff)
            .on_click(cx.listener(|shell, _, _, cx| shell.start_analysis(cx)))
    };

    let palette = shell.palette;
    div()
        .id("session-toolbar")
        .debug_selector(|| "session-toolbar".to_owned())
        .flex_none()
        .overflow_x_scroll()
        .flex()
        .items_center()
        .gap_1p5()
        .px_3()
        .py_1()
        // Single unified floating capsule above the goban
        .rounded(px(crate::theme::radius::PILL))
        .border_1()
        .border_color(rgb(palette.border))
        .bg(rgb(palette.panel))
        .shadow_md()
        // 1. New Game affordance
        .child(
            Button::new("session-new-game-btn")
                .small()
                .ghost()
                .label("新建对局")
                .tooltip("新建对局与参数配置 (Cmd+N)")
                .on_click(cx.listener(|shell, _, _, cx| shell.open_match_setup(cx))),
        )
        .child(div().w(px(1.0)).h(px(18.0)).bg(rgb(palette.border)))
        // 2. Board Markup & Interaction Tools
        .child(
            Button::new("tool-play")
                .small()
                .ghost()
                .selected(mode == GameMode::Play && active_tool == crate::markup::MarkupTool::Play)
                .label("落子")
                .tooltip("落子对弈 (Play)")
                .on_click(cx.listener(|shell, _, _, cx| {
                    shell.active_tool = crate::markup::MarkupTool::Play;
                    shell.set_mode(GameMode::Play, cx);
                })),
        )
        .child(
            Button::new("tool-triangle")
                .small()
                .ghost()
                .selected(
                    mode == GameMode::Edit && active_tool == crate::markup::MarkupTool::Triangle,
                )
                .label("▲")
                .tooltip("标注三角 (▲)")
                .on_click(cx.listener(|shell, _, _, cx| {
                    shell.active_tool = crate::markup::MarkupTool::Triangle;
                    shell.set_mode(GameMode::Edit, cx);
                })),
        )
        .child(
            Button::new("tool-square")
                .small()
                .ghost()
                .selected(
                    mode == GameMode::Edit && active_tool == crate::markup::MarkupTool::Square,
                )
                .label("■")
                .tooltip("标注方块 (■)")
                .on_click(cx.listener(|shell, _, _, cx| {
                    shell.active_tool = crate::markup::MarkupTool::Square;
                    shell.set_mode(GameMode::Edit, cx);
                })),
        )
        .child(
            Button::new("tool-circle")
                .small()
                .ghost()
                .selected(
                    mode == GameMode::Edit && active_tool == crate::markup::MarkupTool::Circle,
                )
                .label("●")
                .tooltip("标注圆圈 (●)")
                .on_click(cx.listener(|shell, _, _, cx| {
                    shell.active_tool = crate::markup::MarkupTool::Circle;
                    shell.set_mode(GameMode::Edit, cx);
                })),
        )
        .child(
            Button::new("tool-cross")
                .small()
                .ghost()
                .selected(mode == GameMode::Edit && active_tool == crate::markup::MarkupTool::Cross)
                .label("✖")
                .tooltip("标注叉号 (✖)")
                .on_click(cx.listener(|shell, _, _, cx| {
                    shell.active_tool = crate::markup::MarkupTool::Cross;
                    shell.set_mode(GameMode::Edit, cx);
                })),
        )
        .child(
            Button::new("tool-estimate")
                .small()
                .ghost()
                .selected(mode == GameMode::Estimator)
                .label("估目")
                .tooltip("形势判断与估目；再次点击退出 (Territory)")
                .on_click(cx.listener(|shell, _, _, cx| {
                    if shell.mode == GameMode::Estimator {
                        shell.set_mode(GameMode::Play, cx);
                    } else {
                        shell.set_mode(GameMode::Estimator, cx);
                    }
                })),
        )
        .children(has_markups.then(|| {
            Button::new("tool-clear-markups")
                .small()
                .ghost()
                .danger()
                .label("清空标记")
                .tooltip("清空当前节点全部标记与线条")
                .on_click(cx.listener(|shell, _, _, cx| {
                    shell.clear_current_node_markups(cx);
                }))
        }))
        .child(div().w(px(1.0)).h(px(18.0)).bg(rgb(palette.border)))
        // 3. Analysis & View quick actions
        // 手数 toggle lives only in the bottom player bar; keeping it here
        // duplicates the control across two toolbars.
        .child(analysis_button)
        .child(
            Button::new("tool-toggle-ai")
                .small()
                .ghost()
                .selected(show_analysis)
                .label("AI选点")
                .tooltip("显示/隐藏 KataGo 候选着法圆点")
                .on_click(cx.listener(|shell, _, _, cx| {
                    shell.toggle_view_setting("board.show_analysis", "analysis overlay", cx);
                })),
        )
        .child(
            Button::new("session-match-setup")
                .small()
                .ghost()
                .child(icon_label(ShellIcon::Settings, "对局设置", palette.muted))
                .tooltip("参与方 / 时钟读秒 / OGS 远程 / 复盘档位 / 导入直播")
                .on_click(cx.listener(|shell, _, _, cx| shell.open_match_setup(cx))),
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
    palette: UiPalette,
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
    let clock_state = shell.clock.state();
    let black_clock = crate::ui_format::format_clock(clock_state.black);
    let white_clock = crate::ui_format::format_clock(clock_state.white);
    let clocks_visible = !matches!(clock_state.control, ryusei_domain_core::TimeControl::None);
    let show_coordinates = shell
        .settings
        .get_bool("view.show_coordinates")
        .unwrap_or(true);
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
        .bg(rgb(palette.panel))
        .border_t_1()
        .border_color(rgb(palette.border))
        .text_xs()
        .text_color(rgb(palette.text))
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
                .child(
                    div()
                        .flex_none()
                        .max_w(px(140.0))
                        .truncate()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(black_name),
                )
                .children((!black_rank.is_empty()).then(|| {
                    div()
                        .text_color(rgb(palette.muted))
                        .child(format!("({black_rank})"))
                }))
                .children(clocks_visible.then(|| {
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(if is_black_turn {
                            palette.warn
                        } else {
                            palette.muted
                        }))
                        .child(black_clock)
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
                                    rgb(palette.text)
                                } else {
                                    rgb(palette.panel)
                                })
                                .border_1()
                                .border_color(rgb(palette.border))
                                .child(if is_black_turn {
                                    div().text_color(rgb(palette.panel)).child("B")
                                } else {
                                    div().text_color(rgb(palette.text)).child("W")
                                }),
                        )
                        .child(
                            div()
                                .text_color(rgb(palette.muted))
                                .child(if status.is_empty() {
                                    if is_black_turn {
                                        "Black to move".to_owned()
                                    } else {
                                        "White to move".to_owned()
                                    }
                                } else {
                                    status.to_owned()
                                }),
                        ),
                )
                .children((shell.session_policy.mode != SessionMode::Live).then(|| {
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
                        )
                }))
                .children(
                    (shell.mode == GameMode::Scoring || shell.mode == GameMode::Estimator).then(
                        || {
                            Button::new("exit-scoring-button")
                                .small()
                                .warning()
                                .child(icon_label(
                                    ShellIcon::Flag,
                                    "退出点目 (返回落子)",
                                    shell.palette.muted,
                                ))
                                .on_click(
                                    cx.listener(|shell, _, _, cx| {
                                        shell.set_mode(GameMode::Play, cx)
                                    }),
                                )
                        },
                    ),
                ),
        )
        // Right: White player + rank + pinned plugin buttons + 🧩 plugin menu + hamburger menu
        .child(
            div()
                .flex()
                .items_center()
                .gap_1p5()
                .children((!white_rank.is_empty()).then(|| {
                    div()
                        .text_color(rgb(palette.muted))
                        .child(format!("({white_rank})"))
                }))
                .children(clocks_visible.then(|| {
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(if !is_black_turn {
                            palette.warn
                        } else {
                            palette.muted
                        }))
                        .child(white_clock)
                }))
                .child(
                    div()
                        .flex_none()
                        .max_w(px(140.0))
                        .truncate()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(white_name),
                )
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
                // Tool buttons were consolidated: plugin/engine panels live in
                // the left sidebar + main menu, export lives in the titlebar.
                // The player bar keeps only the GTP console shortcut and the
                // main-menu affordance alongside the clock/game controls.
                .child(
                    Button::new("gtp-terminal-button")
                        .small()
                        .ghost()
                        .selected(shell.gtp_terminal_open)
                        .child(icon_label(
                            ShellIcon::Terminal,
                            "GTP 终端",
                            shell.palette.muted,
                        ))
                        .tooltip("切换 KataGo / GTP 控制台 (Cmd+T)")
                        .on_click(cx.listener(|shell, _, window, cx| {
                            shell.toggle_gtp_terminal(&MouseDownEvent::default(), window, cx);
                        })),
                )
                .child(
                    Button::new("drawer-menu-button")
                        .small()
                        .ghost()
                        .child(icons::icon(ShellIcon::Settings, 14.0, shell.palette.muted))
                        .tooltip("主菜单 (Cmd+, 首选项)")
                        .on_click(cx.listener(|shell, _, window, cx| {
                            shell.open_side_menu(&MouseDownEvent::default(), window, cx);
                        })),
                ),
        )
}

/// X-axis normalization for the winrate graph. The axis is pre-loaded for 50
/// moves: while the game is shorter, move `i` renders at `i/49` and the curve
/// grows into the reserved space as each new result arrives; beyond 50 moves
/// the curve re-normalizes to fill the plot width (natural compression).
fn winrate_axis_denominator(point_count: usize) -> f32 {
    point_count.saturating_sub(1).max(49) as f32
}

/// Builds the floating readout tooltip for the winrate graph (design
/// `.graph-tooltip`): an inverted pill floating above the hovered point that
/// shows the move number, the metric value and the move-quality flag.
/// Returns `None` when no point is hovered or the point has no data.
fn winrate_hover_tooltip(
    points: &[GraphPlotPoint],
    metric: WinrateGraphMetric,
    palette: UiPalette,
    hover_index: Option<usize>,
) -> Option<Stateful<Div>> {
    let index = hover_index?;
    let point = points.get(index)?;
    let y = point.y?;
    // Position the tooltip above the hovered x, clamped inside the plot.
    let last_index = winrate_axis_denominator(points.len());
    let x_ratio = (index as f32 / last_index).clamp(0.0, 1.0);
    let value_label = match metric {
        WinrateGraphMetric::Winrate => format!("胜率 {:.1}%", y * 100.0),
        WinrateGraphMetric::ScoreLead => format!("目差 {:+.1}", (y * 100.0) - 50.0),
    };
    let quality = if point.is_blunder { " · 失误" } else { "" };
    let move_text = if point.move_number == 0 {
        "开局".to_owned()
    } else {
        format!("第 {} 手", point.move_number)
    };
    let text = format!("{} · {}{}", move_text, value_label, quality);
    Some(
        div()
            .id("winrate-tooltip")
            .absolute()
            .top(px(2.0))
            // Anchor by ratio; the plot owns the pixel mapping, so use a
            // percentage left offset minus half the tooltip width for centering.
            .left(gpui::relative(x_ratio))
            .px_2p5()
            .py_1()
            .rounded_sm()
            .bg(rgb(palette.text))
            .shadow_md()
            .text_color(rgb(palette.panel))
            .child(div().text_xs().font_weight(FontWeight::MEDIUM).child(text)),
    )
}

/// Renders an OGS-style Winrate & Score Lead Graph with coordinate ticks,
/// reference baselines, split advantage shading, and move number markers.
pub fn render_winrate_graph_panel(
    points: &[GraphPlotPoint],
    metric: WinrateGraphMetric,
    height: f32,
    palette: UiPalette,
    shell: &ShellApp,
    current_label: Option<String>,
    on_node_clicked: impl Fn(&ryusei_domain_core::NodeId, &mut Window, &mut App) + 'static,
    cx: &Context<ShellApp>,
) -> Stateful<Div> {
    let on_node_clicked = Rc::new(on_node_clicked);
    let has_values = points.iter().any(|point| point.y.is_some());
    // Pre-loaded 50-move axis: early games render into the reserved span
    // instead of stretching to the full width.
    let last_index = winrate_axis_denominator(points.len());
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
    // The tick labels follow the pre-loaded 50-move axis: while the game is
    // short they read 0/10/20/30/40/50; beyond 50 moves the axis compresses
    // and the right edge shows the real move count.
    let axis_moves = total_moves.max(50);
    let axis_step = if total_moves <= 50 { 10 } else { x_step };

    div()
        .id("winrate-graph-panel")
        .debug_selector(|| "winrate-graph-panel".to_owned())
        .flex_1()
        .min_h(px(height))
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
                        .gap_2()
                        .children(current_label.map(|label| {
                            div()
                                .text_base()
                                .font_weight(FontWeight::BOLD)
                                .text_color(rgb(palette.info))
                                .child(label)
                        }))
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(rgb(palette.muted))
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
            let graph_accent = palette.info; // OGS / KaTrain sky blue (theme info)
            let graph_danger = palette.danger_text; // Blunder red
            // Translucent overlays read differently on light vs dark plot
            // surfaces: use dark strokes on light themes and white strokes on
            // dark themes so the grid/baseline/crosshair stay visible.
            let dark_plot = palette.is_dark();
            let overlay = move |alpha: f32| {
                if dark_plot {
                    hsla(0.0, 0.0, 1.0, alpha)
                } else {
                    hsla(0.0, 0.0, 0.0, alpha)
                }
            };

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
                                .w(px(42.0))
                                .h_full()
                                .flex()
                                .flex_col()
                                .justify_between()
                                .text_sm()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(palette.muted))
                                .child(div().text_color(rgb(palette.info)).child(y_labels[0]))
                                .child(div().child(y_labels[1]))
                                .child(
                                    div()
                                        .text_color(rgb(palette.text_secondary))
                                        .child(y_labels[2]),
                                )
                                .child(div().child(y_labels[3]))
                                .child(div().text_color(rgb(palette.warn)).child(y_labels[4])),
                        )
                        .child(
                            // Graph Plot Canvas & Scrub Area
                            div()
                                .id("winrate-graph-plot")
                                .relative()
                                .flex_1()
                                .h_full()
                                .rounded_md()
                                .bg(rgb(palette.panel))
                                .border_1()
                                .border_color(rgb(palette.border))
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
                                                window.paint_path(path, overlay(0.07));
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
                                                window.paint_path(path, overlay(0.07));
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
                                                window.paint_path(path, overlay(0.22));
                                            }

                                            let valid: Vec<(f32, f32, u32, bool, bool)> =
                                                graph_points
                                                    .iter()
                                                    .enumerate()
                                                    .filter_map(|(index, point)| {
                                                        let y = point.y? as f32;
                                                        // The pre-loaded 50-move
                                                        // axis starts at the left
                                                        // edge: move 0 sits at x=0.
                                                        let x = index as f32 / last_index;
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
                                                        window.paint_path(path, overlay(0.55));
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
                                                    rgb(palette.panel),
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
                                                .on_mouse_move(cx.listener(
                                                    move |shell, _, _, cx| {
                                                        shell.set_winrate_hover(Some(index), cx);
                                                    },
                                                ))
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    move |_: &MouseDownEvent,
                                                          window: &mut Window,
                                                          cx: &mut App| {
                                                        handler(&node_id, window, cx);
                                                    },
                                                )
                                        })),
                                )
                                // Floating readout tooltip (design `.graph-tooltip`):
                                // inverted pill above the hovered point.
                                .child(
                                    if let Some(tooltip) = winrate_hover_tooltip(
                                        points,
                                        metric,
                                        palette,
                                        shell.winrate_hover_index,
                                    ) {
                                        tooltip
                                    } else {
                                        div().id("winrate-tooltip-hidden")
                                    },
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
                        .text_color(rgb(palette.subtle))
                        .child("0")
                        .children((1..=4).filter_map(|i| {
                            let move_val = i * axis_step;
                            (move_val < axis_moves).then(|| div().child(move_val.to_string()))
                        }))
                        .child(format!("{axis_moves}")),
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

#[allow(clippy::too_many_arguments)]
pub fn render_variation_tree_panel(
    panel_id: &'static str,
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
        .id(panel_id)
        .debug_selector(move || panel_id.to_owned())
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
        .child(crate::variation_tree::render_variation_tree_with_prefix(
            panel_id,
            layout,
            grid_size,
            node_size,
            palette,
            on_node_clicked,
            on_node_context_requested,
        ))
        .child(
            if let Some(node_id) = shell.game_graph_context_node.as_ref() {
                render_game_graph_context_menu(panel_id, node_id, shell, cx)
            } else {
                div().id(gpui::SharedString::from(format!(
                    "{panel_id}-context-menu-hidden"
                )))
            },
        )
}

/// Renders a node-specific GameGraph context menu. It deliberately has a small
/// command surface while native popup support remains outside GPUI 0.2.2.
fn render_game_graph_context_menu(
    prefix: &'static str,
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
        .id(gpui::SharedString::from(format!(
            "{prefix}-game-graph-context-menu"
        )))
        .debug_selector(move || format!("{prefix}-game-graph-context-menu"))
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
                .id(gpui::SharedString::from(format!(
                    "{prefix}-game-graph-context-navigate"
                )))
                .debug_selector(move || format!("{prefix}-game-graph-context-navigate"))
                .px_2()
                .py_1()
                .rounded_sm()
                .cursor_pointer()
                .hover(|style| style.bg(rgb(shell.palette.button_active)))
                .child("jump to node")
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(ShellApp::navigate_game_graph_context_node),
                ),
        )
        .child(
            div()
                .id(gpui::SharedString::from(format!(
                    "{prefix}-game-graph-context-hotspot"
                )))
                .debug_selector(move || format!("{prefix}-game-graph-context-hotspot"))
                .px_2()
                .py_1()
                .rounded_sm()
                .cursor_pointer()
                .hover(|style| style.bg(rgb(shell.palette.button_active)))
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
        // Variation structure actions (PRD §4.2: 设为主干 / 删除分支).
        .child(
            div()
                .id(gpui::SharedString::from(format!(
                    "{prefix}-game-graph-context-promote"
                )))
                .debug_selector(move || format!("{prefix}-game-graph-context-promote"))
                .px_2()
                .py_1()
                .rounded_sm()
                .cursor_pointer()
                .hover(|style| style.bg(rgb(shell.palette.button_active)))
                .child("设为主干 (promote)")
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(ShellApp::promote_game_graph_context_variation),
                ),
        )
        .child(
            div()
                .id(gpui::SharedString::from(format!(
                    "{prefix}-game-graph-context-delete"
                )))
                .debug_selector(move || format!("{prefix}-game-graph-context-delete"))
                .px_2()
                .py_1()
                .rounded_sm()
                .cursor_pointer()
                .text_color(rgb(shell.palette.danger_text))
                .hover(|style| style.bg(rgb(shell.palette.button_active)))
                .child("删除分支 (delete)")
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(ShellApp::delete_game_graph_context_variation),
                ),
        )
        .child(
            div()
                .id(gpui::SharedString::from(format!(
                    "{prefix}-game-graph-context-close"
                )))
                .px_2()
                .py_1()
                .rounded_sm()
                .cursor_pointer()
                .hover(|style| style.bg(rgb(shell.palette.button_active)))
                .child("close")
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(ShellApp::close_game_graph_context_menu),
                ),
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

/// Appends a tab button for every installed third-party plugin that declares a
/// UI panel contribution. Built-in panels (KataGo / fox / sgf / engines /
/// plugin manager) no longer get deck tabs — they live in the left engine
/// sidebar or the main menu — so only genuine `Generic(id)` plugin tabs appear.
fn generic_plugin_tabs(
    shell: &ShellApp,
    active_tab: &crate::BottomDeckTab,
    cx: &Context<ShellApp>,
) -> Vec<gpui::AnyElement> {
    shell
        .installed_plugins
        .iter()
        .filter(|plugin| plugin.enabled && plugin.ui_panel_granted && !plugin.panels.is_empty())
        .map(|plugin| {
            let id = plugin.plugin_id.clone();
            let selected =
                matches!(active_tab, crate::BottomDeckTab::Generic(active) if *active == id);
            let label: gpui::SharedString = plugin.name.clone().into();
            Button::new(gpui::SharedString::from(format!("deck-tab-plugin-{id}")))
                .small()
                .ghost()
                .selected(selected)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .child(icons::icon(plugin_icon(&id), 13.0, shell.palette.muted))
                        .child(div().child(label)),
                )
                .on_click(cx.listener(move |shell, _, _, cx| {
                    shell.switch_bottom_tab(crate::BottomDeckTab::Generic(id.clone()), cx);
                }))
                .into_any_element()
        })
        .collect()
}

/// The integrated bottom deck panel (second screen) revealed by clicking toolbar buttons.
pub fn render_bottom_deck_panel(
    snapshot: &GameSnapshot,
    shell: &ShellApp,
    cx: &Context<ShellApp>,
) -> Stateful<Div> {
    let active_tab = shell.active_bottom_tab();

    div()
        .id("bottom-deck-panel")
        .debug_selector(|| "bottom-deck-panel".to_owned())
        .h(px(BOTTOM_DECK_HEIGHT))
        .w_full()
        .flex_none()
        .flex()
        .flex_col()
        .border_t_1()
        .border_color(rgb(shell.palette.border))
        .bg(rgb(shell.palette.panel))
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
                .border_color(rgb(shell.palette.border))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(
                            Button::new("deck-tab-winrate")
                                .small()
                                .ghost()
                                .selected(active_tab == crate::BottomDeckTab::WinrateGraph)
                                .child(icon_label(
                                    ShellIcon::TrendingUp,
                                    "胜率走势",
                                    shell.palette.muted,
                                ))
                                .on_click(cx.listener(|shell, _, _, cx| {
                                    shell.switch_bottom_tab(crate::BottomDeckTab::WinrateGraph, cx);
                                })),
                        )
                        .child(
                            Button::new("deck-tab-tree")
                                .small()
                                .ghost()
                                .selected(active_tab == crate::BottomDeckTab::VariationTree)
                                .child(icon_label(
                                    ShellIcon::GitBranch,
                                    "变着树",
                                    shell.palette.muted,
                                ))
                                .on_click(cx.listener(|shell, _, _, cx| {
                                    shell
                                        .switch_bottom_tab(crate::BottomDeckTab::VariationTree, cx);
                                })),
                        )
                        .child(
                            Button::new("deck-tab-gtp")
                                .small()
                                .ghost()
                                .selected(active_tab == crate::BottomDeckTab::GtpTerminal)
                                .child(icon_label(
                                    ShellIcon::Terminal,
                                    "GTP 终端",
                                    shell.palette.muted,
                                ))
                                .on_click(cx.listener(|shell, _, _, cx| {
                                    shell.switch_bottom_tab(crate::BottomDeckTab::GtpTerminal, cx);
                                })),
                        )
                        // Third-party plugin tabs append after the three core
                        // analysis tabs (design keeps the deck to three; engine
                        // config / fox / sgf moved to the left engine sidebar).
                        .children(generic_plugin_tabs(shell, &active_tab, cx)),
                )
                .child(
                    div()
                        .id("bottom-deck-close-container")
                        .debug_selector(|| "bottom-deck-close-container".to_owned())
                        .child(
                            Button::new("bottom-deck-close-btn")
                                .small()
                                .ghost()
                                .child(icon_label(ShellIcon::Close, "收起", shell.palette.muted))
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
                .p_2()
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .child(match active_tab {
                    crate::BottomDeckTab::WinrateGraph => {
                        let live_player_winrate = if shell.analysis.is_empty() {
                            None
                        } else {
                            Some(best_analysis_winrate(
                                &shell.analysis,
                                snapshot.board.next_player,
                            ))
                        };
                        let live_score_lead =
                            crate::engine_console::best_analysis_entry(&shell.analysis)
                                .and_then(|entry| entry.score_lead)
                                .filter(|lead| lead.is_finite());
                        let winrate_points = crate::winrate_graph::winrate_history(
                            snapshot,
                            live_player_winrate,
                            live_score_lead,
                            snapshot.board.next_player,
                        );
                        let winrate_metric = crate::winrate_graph::WinrateGraphMetric::from_setting(
                            shell.settings.get_str("board.analysis_type"),
                        );
                        let points = crate::winrate_graph::graph_plot_points(
                            &winrate_points,
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
                            shell
                                .settings
                                .get("view.winrategraph_blunderthreshold_scorelead")
                                .and_then(serde_json::Value::as_f64)
                                .unwrap_or(2.0),
                        );
                        // Bold current-value readout: the graph's headline so
                        // the live winrate / score lead is clear at a glance.
                        let current_label = winrate_points
                            .iter()
                            .find(|point| point.is_current)
                            .and_then(|point| match winrate_metric {
                                WinrateGraphMetric::Winrate => point.black_winrate.map(|w| {
                                    format!("黑 {:.1}% · 白 {:.1}%", w * 100.0, (1.0 - w) * 100.0)
                                }),
                                WinrateGraphMetric::ScoreLead => point.black_score_lead.map(|s| {
                                    if s >= 0.0 {
                                        format!("黑领先 +{s:.1} 目")
                                    } else {
                                        format!("白领先 +{:.1} 目", -s)
                                    }
                                }),
                            });
                        let weak_shell = cx.entity().downgrade();
                        let on_node_clicked =
                            move |node_id: &ryusei_domain_core::NodeId,
                                  _window: &mut Window,
                                  cx: &mut App| {
                                weak_shell
                                    .update(cx, |shell, cx| {
                                        shell.navigate_to_node(node_id.clone(), cx);
                                    })
                                    .ok();
                            };
                        div()
                            .w_full()
                            .h_full()
                            .flex()
                            .child(render_winrate_graph_panel(
                                &points,
                                winrate_metric,
                                160.0,
                                shell.palette,
                                shell,
                                current_label,
                                on_node_clicked,
                                cx,
                            ))
                    }
                    crate::BottomDeckTab::VariationTree => {
                        let variation_layout =
                            crate::variation_tree::build_variation_tree_layout(snapshot);
                        let weak_shell = cx.entity().downgrade();
                        let on_node_clicked =
                            move |node_id: &ryusei_domain_core::NodeId,
                                  _window: &mut Window,
                                  cx: &mut App| {
                                weak_shell
                                    .update(cx, |shell, cx| {
                                        shell.navigate_to_node(node_id.clone(), cx);
                                    })
                                    .ok();
                            };
                        let weak_shell_for_context = cx.entity().downgrade();
                        let on_node_context_requested =
                            move |node_id: &ryusei_domain_core::NodeId,
                                  _window: &mut Window,
                                  cx: &mut App| {
                                weak_shell_for_context
                                    .update(cx, |shell, cx| {
                                        shell.open_game_graph_context_menu(node_id.clone(), cx);
                                    })
                                    .ok();
                            };
                        div().child(render_variation_tree_panel(
                            "deck-variation-tree",
                            &variation_layout,
                            26.0,
                            4.0,
                            shell.palette,
                            shell,
                            cx,
                            on_node_clicked,
                            on_node_context_requested,
                        ))
                    }
                    crate::BottomDeckTab::GtpTerminal => render_gtp_terminal_body(shell, cx),
                    crate::BottomDeckTab::PluginManager => render_pinned_plugins_manager(shell, cx),
                    // Built-in engine/tool panels render only in the left engine
                    // sidebar now; these deck variants are unreachable because
                    // `active_bottom_tab` reroutes their ids to the winrate tab.
                    crate::BottomDeckTab::KataGo
                    | crate::BottomDeckTab::FoxSync
                    | crate::BottomDeckTab::PositionSgf
                    | crate::BottomDeckTab::Engines => div()
                        .p_3()
                        .text_xs()
                        .text_color(rgb(shell.palette.subtle))
                        .child("该面板已移至左侧引擎侧栏「引擎与工具」区。"),
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
        .pv_animation
        .as_ref()
        .and_then(|(vertex, step)| {
            if *step == 0 {
                Some(Vec::new())
            } else {
                shell
                    .analysis
                    .iter()
                    .find(|entry| entry.vertex.as_deref() == Some(vertex.as_str()))
                    .map(|entry| {
                        pv_preview_points(board.width, board.next_player, &entry.pv, *step)
                    })
            }
        })
        .unwrap_or_else(|| {
            shell
                .hovered_candidate_vertex
                .as_deref()
                .and_then(|vertex| {
                    shell
                        .analysis
                        .iter()
                        .find(|entry| entry.vertex.as_deref() == Some(vertex))
                        .map(|entry| {
                            pv_preview_points(board.width, board.next_player, &entry.pv, 6)
                        })
                })
                .unwrap_or_default()
        });
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
            .unwrap_or(true),
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
            .and_then(|entry| entry.ownership.clone())
            .or_else(|| {
                (shell.mode == GameMode::Estimator)
                    .then(|| crate::goban_view::estimate_ownership_from_board(board))
                    .flatten()
            }),
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
    // Frame the goban in a rounded container whose surface follows the active
    // board skin (kaya gradient / studio / midnight), matching the design
    // prototype's `.board-container-*` rules.
    let skin = crate::theme::BoardSkin::from_theme(theme);
    let mut container = div()
        .id("goban-area")
        .relative()
        .size(px(board_pixel_size))
        .rounded(px(crate::theme::BOARD_RADIUS))
        .overflow_hidden();
    if let Some((from, to)) = skin.gradient {
        // Linear stand-in for the design's radial kaya highlight: bright at the
        // top-left, deepening toward the bottom-right (≈135°).
        container = container.bg(linear_gradient(
            135.0,
            linear_color_stop(rgb(from), 0.0),
            linear_color_stop(rgb(to), 1.0),
        ));
    }
    if let Some(border) = skin.border {
        container = container.border_1().border_color(rgb(border));
    }
    if skin.shadow {
        container = container.shadow_lg();
    }
    container
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

/// Floating markup toolbar on top of the goban.
#[allow(dead_code)]
pub fn render_floating_markup_bar(shell: &ShellApp, cx: &Context<ShellApp>) -> Stateful<Div> {
    let active_tool = shell.active_tool;
    let mode = shell.mode;
    let show_numbers = shell
        .settings
        .get_bool("view.show_move_numbers")
        .unwrap_or(false);
    let show_analysis = shell
        .settings
        .get_bool("board.show_analysis")
        .unwrap_or(true);
    let snapshot = shell.host.snapshot();
    let has_markups = snapshot
        .nodes
        .iter()
        .find(|node| node.id == snapshot.current_node_id)
        .map(|node| {
            ["CR", "SQ", "TR", "MA", "LB", "LN", "AR"]
                .iter()
                .any(|property| node.properties.contains_key(*property))
        })
        .unwrap_or(false);

    div()
        .id("floating-markup-bar")
        .debug_selector(|| "floating-markup-bar".to_owned())
        .flex()
        .items_center()
        .gap_1()
        .px_3()
        .py_1()
        .rounded(px(980.0))
        .bg(rgb(shell.palette.panel))
        .border_1()
        .border_color(rgb(shell.palette.border))
        .shadow_md()
        .child(
            Button::new("tool-play")
                .small()
                .ghost()
                .selected(mode == GameMode::Play && active_tool == crate::markup::MarkupTool::Play)
                .label("落子")
                .tooltip("落子对弈 (Play)")
                .on_click(cx.listener(|shell, _, _, cx| {
                    shell.active_tool = crate::markup::MarkupTool::Play;
                    shell.set_mode(GameMode::Play, cx);
                })),
        )
        .child(
            Button::new("tool-triangle")
                .small()
                .ghost()
                .selected(
                    mode == GameMode::Edit && active_tool == crate::markup::MarkupTool::Triangle,
                )
                .label("▲")
                .tooltip("标注三角 (▲)")
                .on_click(cx.listener(|shell, _, _, cx| {
                    shell.active_tool = crate::markup::MarkupTool::Triangle;
                    shell.set_mode(GameMode::Edit, cx);
                })),
        )
        .child(
            Button::new("tool-square")
                .small()
                .ghost()
                .selected(
                    mode == GameMode::Edit && active_tool == crate::markup::MarkupTool::Square,
                )
                .label("■")
                .tooltip("标注方块 (■)")
                .on_click(cx.listener(|shell, _, _, cx| {
                    shell.active_tool = crate::markup::MarkupTool::Square;
                    shell.set_mode(GameMode::Edit, cx);
                })),
        )
        .child(
            Button::new("tool-circle")
                .small()
                .ghost()
                .selected(
                    mode == GameMode::Edit && active_tool == crate::markup::MarkupTool::Circle,
                )
                .label("●")
                .tooltip("标注圆圈 (●)")
                .on_click(cx.listener(|shell, _, _, cx| {
                    shell.active_tool = crate::markup::MarkupTool::Circle;
                    shell.set_mode(GameMode::Edit, cx);
                })),
        )
        .child(
            Button::new("tool-cross")
                .small()
                .ghost()
                .selected(mode == GameMode::Edit && active_tool == crate::markup::MarkupTool::Cross)
                .label("✖")
                .tooltip("标注叉号 (✖)")
                .on_click(cx.listener(|shell, _, _, cx| {
                    shell.active_tool = crate::markup::MarkupTool::Cross;
                    shell.set_mode(GameMode::Edit, cx);
                })),
        )
        .child(
            Button::new("tool-estimate")
                .small()
                .ghost()
                .selected(mode == GameMode::Estimator)
                .label("估目")
                .tooltip("形势判断与估目；再次点击退出 (Territory)")
                .on_click(cx.listener(|shell, _, _, cx| {
                    if shell.mode == GameMode::Estimator {
                        shell.set_mode(GameMode::Play, cx);
                    } else {
                        shell.set_mode(GameMode::Estimator, cx);
                    }
                })),
        )
        .children(has_markups.then(|| {
            Button::new("tool-clear-markups")
                .small()
                .ghost()
                .danger()
                .label("清空标记")
                .tooltip("清空当前节点全部标记与线条")
                .on_click(cx.listener(|shell, _, _, cx| {
                    shell.clear_current_node_markups(cx);
                }))
        }))
        .child(div().w(px(1.0)).h(px(16.0)).bg(rgb(shell.palette.border)))
        .child(
            Button::new("tool-toggle-numbers")
                .small()
                .ghost()
                .selected(show_numbers)
                .label("手数")
                .tooltip("显示/隐藏手数序号 (Cmd+Shift+M)")
                .on_click(cx.listener(|shell, _, _, cx| {
                    shell.toggle_view_setting("view.show_move_numbers", "move numbers", cx);
                })),
        )
        .child(
            Button::new("tool-toggle-ai")
                .small()
                .ghost()
                .selected(show_analysis)
                .label("AI选点")
                .tooltip("显示/隐藏 KataGo 候选着法圆点")
                .on_click(cx.listener(|shell, _, _, cx| {
                    shell.toggle_view_setting("board.show_analysis", "analysis overlay", cx);
                })),
        )
}

/// Floating playback bar at the bottom of the goban.
pub fn render_floating_playback_bar(
    snapshot: &GameSnapshot,
    shell: &ShellApp,
    cx: &Context<ShellApp>,
) -> Stateful<Div> {
    let total_moves = snapshot.moves.len();
    let current_step = snapshot
        .moves
        .iter()
        .position(|m| m.vertex == snapshot.board.current_vertex)
        .map(|idx| idx + 1)
        .unwrap_or(0);

    div()
        .id("floating-playback-bar")
        .debug_selector(|| "floating-playback-bar".to_owned())
        .flex()
        .items_center()
        .gap_1p5()
        .px_3()
        .py_1()
        .rounded(px(980.0))
        .bg(rgb(shell.palette.panel))
        .border_1()
        .border_color(rgb(shell.palette.border))
        .shadow_md()
        .child(
            Button::new("playback-first")
                .small()
                .ghost()
                .child(icons::icon(
                    ShellIcon::ChevronsLeft,
                    14.0,
                    shell.palette.muted,
                ))
                .tooltip("开局 (Home)")
                .on_click(cx.listener(|shell, _, window, cx| {
                    shell.on_navigate_first(&MouseDownEvent::default(), window, cx);
                })),
        )
        .child(
            Button::new("playback-prev")
                .small()
                .ghost()
                .child(icons::icon(
                    ShellIcon::ChevronLeft,
                    14.0,
                    shell.palette.muted,
                ))
                .tooltip("上一手 (Left)")
                .on_click(cx.listener(|shell, _, window, cx| {
                    shell.on_navigate_previous(&MouseDownEvent::default(), window, cx);
                })),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap_0p5()
                .child(
                    div()
                        .px_2()
                        .text_xs()
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(shell.palette.text))
                        .child(format!("{current_step} / {total_moves}")),
                )
                .child(
                    div()
                        .w(px(96.0))
                        .h(px(4.0))
                        .rounded(px(2.0))
                        .bg(rgb(shell.palette.track))
                        .child(
                            div()
                                .h_full()
                                .rounded(px(2.0))
                                .bg(rgb(shell.palette.accent))
                                .w(px({
                                    let progress = if total_moves == 0 {
                                        0.0
                                    } else {
                                        (current_step as f32 / total_moves as f32).clamp(0.0, 1.0)
                                    };
                                    progress * 96.0
                                })),
                        ),
                ),
        )
        .child(
            Button::new("playback-next")
                .small()
                .ghost()
                .child(icons::icon(
                    ShellIcon::ChevronRight,
                    14.0,
                    shell.palette.muted,
                ))
                .tooltip("下一手 (Right)")
                .on_click(cx.listener(|shell, _, window, cx| {
                    shell.on_navigate_next(&MouseDownEvent::default(), window, cx);
                })),
        )
        .child(
            Button::new("playback-last")
                .small()
                .ghost()
                .child(icons::icon(
                    ShellIcon::ChevronsRight,
                    14.0,
                    shell.palette.muted,
                ))
                .tooltip("末手 (End)")
                .on_click(cx.listener(|shell, _, window, cx| {
                    shell.on_navigate_last(&MouseDownEvent::default(), window, cx);
                })),
        )
        .child(div().w(px(1.0)).h(px(16.0)).bg(rgb(shell.palette.border)))
        .child(
            Button::new("playback-autoplay")
                .small()
                .ghost()
                .selected(shell.autoplay_task.is_some())
                .child(if shell.autoplay_task.is_some() {
                    icon_label(ShellIcon::Pause, "暂停", shell.palette.muted)
                } else {
                    icon_label(ShellIcon::Play, "自动", shell.palette.muted)
                })
                .tooltip("自动播放 / 暂停 (Space)")
                .on_click(cx.listener(|shell, _, _, cx| {
                    shell.toggle_autoplay(cx);
                })),
        )
        .child(
            Button::new("playback-pass")
                .small()
                .ghost()
                .label("Pass")
                .tooltip("停一手 (Pass)")
                .on_click(cx.listener(|shell, _, window, cx| {
                    shell.on_pass(&MouseDownEvent::default(), window, cx);
                })),
        )
}
