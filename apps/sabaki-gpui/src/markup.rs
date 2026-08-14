use std::rc::Rc;

use gpui::{
    App, Div, InteractiveElement, MouseButton, MouseDownEvent, ParentElement, Styled, Window, div,
    px, rgb,
};
use sabaki_domain_core::{
    CURRENT_TRANSACTION_SCHEMA_VERSION, GameTransaction, GameTransactionType, MarkerSnapshot,
    NodeId, Vertex,
};

/// The markup tools exposed by the shell toolbar. `Play` falls through to the
/// normal board interaction; the remaining tools attach a markup marker to the
/// clicked vertex via the `AddMarkup` transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MarkupTool {
    Play,
    Circle,
    Square,
    Triangle,
    Cross,
    Label,
}

pub const MARKUP_TOOLS: &[MarkupTool] = &[
    MarkupTool::Play,
    MarkupTool::Circle,
    MarkupTool::Square,
    MarkupTool::Triangle,
    MarkupTool::Cross,
    MarkupTool::Label,
];

impl MarkupTool {
    pub fn label(self) -> &'static str {
        match self {
            MarkupTool::Play => "Play move",
            MarkupTool::Circle => "Circle",
            MarkupTool::Square => "Square",
            MarkupTool::Triangle => "Triangle",
            MarkupTool::Cross => "Cross",
            MarkupTool::Label => "Label",
        }
    }

    /// The SGF property used by this tool (`CR`, `MA`, `SQ`, `TR`, `LB`).
    fn sgf_property(self) -> Option<&'static str> {
        match self {
            MarkupTool::Circle => Some("CR"),
            MarkupTool::Square => Some("SQ"),
            MarkupTool::Triangle => Some("TR"),
            MarkupTool::Cross => Some("MA"),
            MarkupTool::Label => Some("LB"),
            MarkupTool::Play => None,
        }
    }
}

/// Builds the `AddMarkup` transaction for the current node, or `None` when the
/// tool is `Play` (which is handled as a normal move instead).
pub fn create_markup_transaction(
    node_id: &NodeId,
    vertex: Vertex,
    tool: MarkupTool,
    label: &str,
) -> Option<GameTransaction> {
    let marker_type = tool.sgf_property()?;
    Some(GameTransaction {
        schema_version: CURRENT_TRANSACTION_SCHEMA_VERSION,
        transaction_type: GameTransactionType::AddMarkup,
        color: None,
        vertex: Some(vertex),
        node_id: Some(node_id.clone()),
        property: None,
        values: Vec::new(),
        marker: Some(MarkerSnapshot {
            marker_type: marker_type.to_owned(),
            label: (tool == MarkupTool::Label).then(|| label.to_owned()),
        }),
        nodes: Vec::new(),
        score_override: None,
    })
}

/// Renders a compact single-character symbol for a marker type, matching the
/// sabaki UI. Label markers render their text directly.
pub fn markup_symbol(marker_type: &str, label: Option<&str>) -> String {
    match marker_type {
        "point" => "●".to_owned(),
        "circle" => "◯".to_owned(),
        "square" => "□".to_owned(),
        "triangle" => "△".to_owned(),
        "cross" => "✕".to_owned(),
        "label" => label
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| "?".to_owned()),
        _ => "•".to_owned(),
    }
}

/// Renders the tool picker row. Clicking a tool invokes `on_tool_clicked` with
/// that tool; the active tool is highlighted.
pub fn render_markup_toolbar<F>(active_tool: MarkupTool, on_tool_clicked: F) -> Div
where
    F: Fn(&MarkupTool, &mut Window, &mut App) + 'static,
{
    let on_tool_clicked = Rc::new(on_tool_clicked);
    let active_background = 0xf7ecd8;
    let inactive_background = 0xe8e0d4;

    div()
        .flex()
        .items_center()
        .gap_1()
        .children(MARKUP_TOOLS.iter().map(|tool| {
            let handler = on_tool_clicked.clone();
            let tool = *tool;
            let is_active = tool == active_tool;
            div()
                .px_2()
                .py_1()
                .border_1()
                .border_color(rgb(0x8a6d3b))
                .rounded(px(4.0))
                .bg(if is_active {
                    rgb(active_background)
                } else {
                    rgb(inactive_background)
                })
                .text_color(rgb(0x3a2410))
                .text_sm()
                .child(match tool {
                    MarkupTool::Play => "●".to_owned(),
                    MarkupTool::Circle => "◯".to_owned(),
                    MarkupTool::Square => "□".to_owned(),
                    MarkupTool::Triangle => "△".to_owned(),
                    MarkupTool::Cross => "✕".to_owned(),
                    MarkupTool::Label => "A".to_owned(),
                })
                .on_mouse_down(
                    MouseButton::Left,
                    move |_: &MouseDownEvent, window: &mut Window, cx: &mut App| {
                        handler(&tool, window, cx);
                    },
                )
        }))
}

#[cfg(test)]
mod tests {
    use super::{MarkupTool, create_markup_transaction, markup_symbol};
    use sabaki_domain_core::{GameDocument, GameTransactionType, Vertex};

    fn current_node_id() -> String {
        GameDocument::from_sgf("(;SZ[5])")
            .unwrap()
            .snapshot()
            .current_node_id
    }

    #[test]
    fn play_tool_builds_no_markup_transaction() {
        let transaction = create_markup_transaction(
            &current_node_id(),
            Vertex { column: 3, row: 3 },
            MarkupTool::Play,
            "",
        );
        assert!(transaction.is_none());
    }

    #[test]
    fn circle_builds_an_add_markup_transaction_without_label() {
        let transaction = create_markup_transaction(
            &current_node_id(),
            Vertex { column: 3, row: 3 },
            MarkupTool::Circle,
            "",
        )
        .expect("a circle tool must build a transaction");

        assert_eq!(transaction.transaction_type, GameTransactionType::AddMarkup);
        assert_eq!(transaction.vertex, Some(Vertex { column: 3, row: 3 }));
        let marker = transaction.marker.expect("markup needs a marker");
        assert_eq!(marker.marker_type, "CR");
        assert_eq!(marker.label, None);
    }

    #[test]
    fn label_builds_a_marker_with_the_given_text() {
        let transaction = create_markup_transaction(
            &current_node_id(),
            Vertex { column: 4, row: 4 },
            MarkupTool::Label,
            "Q",
        )
        .expect("a label tool must build a transaction");

        let marker = transaction.marker.expect("markup needs a marker");
        assert_eq!(marker.marker_type, "LB");
        assert_eq!(marker.label.as_deref(), Some("Q"));
    }

    #[test]
    fn each_tool_maps_to_its_sgf_property() {
        let cases = [
            (MarkupTool::Circle, "CR"),
            (MarkupTool::Square, "SQ"),
            (MarkupTool::Triangle, "TR"),
            (MarkupTool::Cross, "MA"),
            (MarkupTool::Label, "LB"),
        ];
        for (tool, expected) in cases {
            let transaction = create_markup_transaction(
                &current_node_id(),
                Vertex { column: 0, row: 0 },
                tool,
                "x",
            )
            .expect("markup tool must build a transaction");
            assert_eq!(
                transaction.marker.unwrap().marker_type,
                expected,
                "{tool:?} must use SGF property {expected}"
            );
        }
    }

    #[test]
    fn symbols_render_expected_glyphs() {
        assert_eq!(markup_symbol("point", None), "●");
        assert_eq!(markup_symbol("circle", None), "◯");
        assert_eq!(markup_symbol("square", None), "□");
        assert_eq!(markup_symbol("triangle", None), "△");
        assert_eq!(markup_symbol("cross", None), "✕");
        assert_eq!(markup_symbol("label", Some("A")), "A");
        assert_eq!(markup_symbol("label", None), "?");
    }
}
