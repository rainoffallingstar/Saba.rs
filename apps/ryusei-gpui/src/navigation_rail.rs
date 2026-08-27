//! Compact global navigation rail, separate from the resizable engine sidebar.

use gpui::{Context, Stateful, div, prelude::*, px, rgb};
use gpui_component::Sizable;
use gpui_component::button::{Button, ButtonVariants};

use crate::{NAVIGATION_RAIL_WIDTH, ShellApp};

pub fn render_navigation_rail(shell: &ShellApp, cx: &Context<ShellApp>) -> Stateful<gpui::Div> {
    let tabs = shell.workspace_tabs.tabs().to_vec();
    let active_tab_id = shell.workspace_tabs.active_tab_id().to_owned();
    let profile_name = shell
        .settings
        .get_str("profile.display_name")
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("Player");
    let profile_initial = profile_name.chars().next().unwrap_or('P').to_string();

    div()
        .id("navigation-rail")
        .debug_selector(|| "navigation-rail".to_owned())
        .flex_none()
        .w(px(NAVIGATION_RAIL_WIDTH))
        .h_full()
        .min_h_0()
        .flex()
        .flex_col()
        .items_center()
        .justify_between()
        .py_2()
        .border_r_1()
        .border_color(rgb(shell.palette.border))
        .bg(rgb(shell.palette.panel))
        .child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .w(px(32.0))
                        .h(px(32.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(6.0))
                        .bg(rgb(shell.palette.accent))
                        .text_color(rgb(0xffffff))
                        .font_weight(gpui::FontWeight::BOLD)
                        .child("R"),
                )
                .child(
                    Button::new("nav-library")
                        .small()
                        .ghost()
                        .label("谱")
                        .tooltip("棋谱库")
                        .on_click(cx.listener(|shell, _, _, cx| shell.open_library(cx))),
                )
                .child(div().h(px(1.0)).w(px(28.0)).bg(rgb(shell.palette.border)))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap_1()
                        .children(tabs.iter().enumerate().map(|(index, tab)| {
                            let tab_id = tab.id.clone();
                            let tab_title = tab.title.clone();
                            let active = tab.id == active_tab_id;
                            let mode = match tab.policy.mode {
                                ryusei_domain_core::SessionMode::Match => "对弈",
                                ryusei_domain_core::SessionMode::Record => "打谱",
                                ryusei_domain_core::SessionMode::Live => "实时",
                            };
                            let button = Button::new(gpui::SharedString::from(format!(
                                "nav-session-{tab_id}"
                            )))
                            .small()
                            .label(format!(
                                "{}{}",
                                index + 1,
                                if tab.is_dirty { "•" } else { "" }
                            ))
                            .tooltip(format!("{mode} · {tab_title}"))
                            .on_click(cx.listener(
                                move |shell, _, _, cx| {
                                    shell.activate_workspace_tab(&tab_id, cx);
                                },
                            ));
                            if active {
                                button.primary()
                            } else {
                                button.ghost()
                            }
                        }))
                        .child(
                            Button::new("nav-new-session")
                                .small()
                                .ghost()
                                .label("+")
                                .tooltip("新建会话")
                                .on_click(cx.listener(|shell, _, _, cx| {
                                    shell.create_workspace_session(cx);
                                })),
                        ),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap_1()
                .child(
                    Button::new("nav-goals")
                        .small()
                        .ghost()
                        .label("GOAL")
                        .tooltip("目标与计划")
                        .on_click(cx.listener(|shell, _, _, cx| shell.open_goals(cx))),
                )
                .child(
                    Button::new("nav-profile")
                        .small()
                        .outline()
                        .label(profile_initial)
                        .tooltip(format!("{} 的 Profile", profile_name))
                        .on_click(cx.listener(|shell, _, _, cx| shell.open_profile(cx))),
                )
                .child(
                    Button::new("nav-settings")
                        .small()
                        .ghost()
                        .label("SET")
                        .tooltip("设置")
                        .on_click(cx.listener(|shell, _, _, cx| shell.open_preferences(cx))),
                ),
        )
}
