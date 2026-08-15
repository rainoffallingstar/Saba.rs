use std::rc::Rc;

use gpui::{
    App, Div, InteractiveElement, MouseButton, MouseDownEvent, ParentElement, Styled, Window, div,
    px, rgb,
};
use sabaki_domain_core::{
    CURRENT_TRANSACTION_SCHEMA_VERSION, GameTransaction, GameTransactionType, MarkerSnapshot,
    NodeId, Vertex,
};

use crate::theme::UiPalette;

/// The markup tools exposed by the shell toolbar. `Play` falls through to the
/// normal board interaction; the markup tools attach a marker to the clicked
/// vertex via the `AddMarkup` transaction; the setup tools write `AB`/`AW`/
/// `AE`-style setup properties via `SetNodeProperty`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MarkupTool {
    Play,
    Circle,
    Square,
    Triangle,
    Cross,
    Label,
    SetupBlack,
    SetupWhite,
    SetupClear,
}

pub const MARKUP_TOOLS: &[MarkupTool] = &[
    MarkupTool::Play,
    MarkupTool::Circle,
    MarkupTool::Square,
    MarkupTool::Triangle,
    MarkupTool::Cross,
    MarkupTool::Label,
    MarkupTool::SetupBlack,
    MarkupTool::SetupWhite,
    MarkupTool::SetupClear,
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
            MarkupTool::SetupBlack => "Setup black",
            MarkupTool::SetupWhite => "Setup white",
            MarkupTool::SetupClear => "Setup clear",
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
            MarkupTool::Play
            | MarkupTool::SetupBlack
            | MarkupTool::SetupWhite
            | MarkupTool::SetupClear => None,
        }
    }

    /// The SGF setup property for this tool (`AB`, `AW`, `AE`).
    pub fn setup_property(self) -> Option<&'static str> {
        match self {
            MarkupTool::SetupBlack => Some("AB"),
            MarkupTool::SetupWhite => Some("AW"),
            MarkupTool::SetupClear => Some("AE"),
            _ => None,
        }
    }

    /// Whether this tool drives setup-stone editing instead of markup.
    pub fn is_setup_tool(self) -> bool {
        self.setup_property().is_some()
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

fn set_property_transaction(
    node_id: &NodeId,
    property: &str,
    values: Vec<String>,
) -> GameTransaction {
    GameTransaction {
        schema_version: CURRENT_TRANSACTION_SCHEMA_VERSION,
        transaction_type: GameTransactionType::SetNodeProperty,
        color: None,
        vertex: None,
        node_id: Some(node_id.clone()),
        property: Some(property.to_owned()),
        values,
        marker: None,
        nodes: Vec::new(),
        score_override: None,
    }
}

/// Builds the setup-stone transactions for the current node. Placing a black
/// or white setup stone appends the vertex to the node's `AB`/`AW` property
/// (deduplicated); clearing removes the vertex from both properties. A
/// property that ends up empty is removed entirely (empty `values`), which
/// also covers the initial placement when no setup exists yet.
pub fn create_setup_transactions(
    node_id: &NodeId,
    vertex: Vertex,
    tool: MarkupTool,
    node_properties: &sabaki_domain_core::Properties,
) -> Vec<GameTransaction> {
    let point = crate::goban_view::format_sgf_vertex(vertex);
    match tool.setup_property() {
        Some("AB") | Some("AW") => {
            let property = tool
                .setup_property()
                .expect("matched setup properties exist");
            let mut values = node_properties.get(property).cloned().unwrap_or_default();
            if !values.contains(&point) {
                values.push(point);
            }
            vec![set_property_transaction(node_id, property, values)]
        }
        Some("AE") => {
            let mut transactions = Vec::new();
            for property in ["AB", "AW"] {
                if !node_properties.contains_key(property) {
                    continue;
                }
                let values: Vec<String> = node_properties
                    .get(property)
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|value| value != &point)
                    .collect();
                transactions.push(set_property_transaction(node_id, property, values));
            }
            transactions
        }
        _ => Vec::new(),
    }
}

/// Cycles the scoring override for a vertex: none → alive black (1) →
/// alive white (-1) → clear (0). Pure so the cycling is unit-testable.
pub fn next_scoring_override(current: Option<i8>) -> i8 {
    match current {
        None | Some(0) => 1,
        Some(1) => -1,
        Some(-1) => 0,
        Some(_) => 1,
    }
}

/// Builds the `ApplyScoringOverride` transaction for the current position.
pub fn create_scoring_transaction(vertex: Vertex, override_value: i8) -> GameTransaction {
    GameTransaction {
        schema_version: CURRENT_TRANSACTION_SCHEMA_VERSION,
        transaction_type: GameTransactionType::ApplyScoringOverride,
        color: None,
        vertex: Some(vertex),
        node_id: None,
        property: None,
        values: Vec::new(),
        marker: None,
        nodes: Vec::new(),
        score_override: Some(override_value),
    }
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
pub fn render_markup_toolbar<F>(
    active_tool: MarkupTool,
    palette: UiPalette,
    on_tool_clicked: F,
) -> Div
where
    F: Fn(&MarkupTool, &mut Window, &mut App) + 'static,
{
    let on_tool_clicked = Rc::new(on_tool_clicked);

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
                .border_color(rgb(palette.accent))
                .rounded(px(4.0))
                .bg(if is_active {
                    rgb(palette.button)
                } else {
                    rgb(palette.button_active)
                })
                .text_color(rgb(palette.text))
                .text_sm()
                .child(match tool {
                    MarkupTool::Play => "●".to_owned(),
                    MarkupTool::Circle => "◯".to_owned(),
                    MarkupTool::Square => "□".to_owned(),
                    MarkupTool::Triangle => "△".to_owned(),
                    MarkupTool::Cross => "✕".to_owned(),
                    MarkupTool::Label => "A".to_owned(),
                    MarkupTool::SetupBlack => "B".to_owned(),
                    MarkupTool::SetupWhite => "W".to_owned(),
                    MarkupTool::SetupClear => "✖".to_owned(),
                })
                .on_mouse_down(
                    MouseButton::Left,
                    move |_: &MouseDownEvent, window: &mut Window, cx: &mut App| {
                        handler(&tool, window, cx);
                    },
                )
        }))
}

/// Computes the scoring summary line for the status area: territory,
/// stones, captures, komi and margin. Pure function so the panel logic is
/// unit-testable without a view.
pub fn scoring_summary(snapshot: &sabaki_domain_core::GameSnapshot) -> String {
    let komi = snapshot
        .root_properties
        .get("KM")
        .and_then(|values| values.first())
        .and_then(|value| value.parse::<f64>().ok())
        .or(Some(sabaki_domain_core::DEFAULT_KOMI));
    let result = sabaki_domain_core::score_board(&snapshot.board, komi, &snapshot.score_overrides);
    let winner = match result.winner {
        Some(sabaki_domain_core::Color::Black) => "Black",
        Some(sabaki_domain_core::Color::White) => "White",
        None => "Draw",
    };
    format!(
        "B {:.1} ({} ter + {} stones + {} cap) — W {:.1} ({} ter + {} stones + {} cap + {:.1} komi) → {} by {:.1}",
        result.black_total,
        result.black_territory,
        result.black_stones,
        result.black_captured,
        result.white_total,
        result.white_territory,
        result.white_stones,
        result.white_captured,
        result.komi,
        winner,
        result.margin
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn scoring_summary_reports_black_win_by_margin() {
        use sabaki_domain_core::{Color, GameDocument, Vertex};

        // Build a tiny game: black owns a 2x2 block; score with komi 0.
        let mut game = GameDocument::new(3, 3).unwrap();
        for vertex in [
            Vertex { column: 0, row: 0 },
            Vertex { column: 0, row: 1 },
            Vertex { column: 1, row: 0 },
            Vertex { column: 1, row: 1 },
        ] {
            game.play_move(Color::Black, Some(vertex))
                .expect("setup moves are legal");
        }
        let mut snapshot = game.snapshot();
        snapshot
            .root_properties
            .insert("KM".to_owned(), vec!["0".to_owned()]);
        let summary = super::scoring_summary(&snapshot);
        assert!(
            summary.contains("B 9.0"),
            "black total must be territory 5 + stones 4: {summary}"
        );
        assert!(summary.contains("→ Black by 9.0"), "summary: {summary}");
    }

    #[test]
    fn scoring_summary_reports_white_win_with_komi() {
        use sabaki_domain_core::{Color, GameDocument, Vertex};

        let mut game = GameDocument::new(3, 3).unwrap();
        for vertex in [
            Vertex { column: 0, row: 0 },
            Vertex { column: 0, row: 1 },
            Vertex { column: 1, row: 0 },
            Vertex { column: 1, row: 1 },
        ] {
            game.play_move(Color::Black, Some(vertex))
                .expect("setup moves are legal");
        }
        let mut snapshot = game.snapshot();
        snapshot
            .root_properties
            .insert("KM".to_owned(), vec!["10".to_owned()]);
        let summary = super::scoring_summary(&snapshot);
        assert!(
            summary.contains("+ 10.0 komi") && summary.contains("→ White"),
            "komi must flip the winner: {summary}"
        );
    }

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
    #[test]
    fn setup_placement_appends_and_deduplicates_vertices() {
        use sabaki_domain_core::Properties;

        let node_id = "root".to_owned();
        let mut properties = Properties::new();
        properties.insert("AB".to_owned(), vec!["dd".to_owned()]);

        let transactions = super::create_setup_transactions(
            &node_id,
            Vertex { column: 3, row: 3 },
            MarkupTool::SetupBlack,
            &properties,
        );

        assert_eq!(transactions.len(), 1);
        assert_eq!(
            transactions[0].transaction_type,
            GameTransactionType::SetNodeProperty
        );
        assert_eq!(transactions[0].property.as_deref(), Some("AB"));
        assert_eq!(transactions[0].values, vec!["dd".to_owned()]);
    }

    #[test]
    fn setup_placement_writes_a_new_property_when_none_exists() {
        let transactions = super::create_setup_transactions(
            &"root".to_owned(),
            Vertex { column: 15, row: 3 },
            MarkupTool::SetupWhite,
            &sabaki_domain_core::Properties::new(),
        );

        assert_eq!(transactions.len(), 1);
        assert_eq!(transactions[0].property.as_deref(), Some("AW"));
        assert_eq!(transactions[0].values, vec!["pd".to_owned()]);
    }

    #[test]
    fn setup_clear_removes_the_vertex_from_both_properties() {
        use sabaki_domain_core::Properties;

        let mut properties = Properties::new();
        properties.insert("AB".to_owned(), vec!["dd".to_owned(), "pp".to_owned()]);
        properties.insert("AW".to_owned(), vec!["dd".to_owned()]);

        let transactions = super::create_setup_transactions(
            &"root".to_owned(),
            Vertex { column: 3, row: 3 },
            MarkupTool::SetupClear,
            &properties,
        );

        assert_eq!(transactions.len(), 2);
        let ab = transactions
            .iter()
            .find(|transaction| transaction.property.as_deref() == Some("AB"))
            .expect("an AB transaction exists");
        assert_eq!(ab.values, vec!["pp".to_owned()]);
        let aw = transactions
            .iter()
            .find(|transaction| transaction.property.as_deref() == Some("AW"))
            .expect("an AW transaction exists");
        assert!(aw.values.is_empty(), "emptied properties are removed");
    }

    #[test]
    fn setup_clear_skips_absent_properties() {
        let transactions = super::create_setup_transactions(
            &"root".to_owned(),
            Vertex { column: 3, row: 3 },
            MarkupTool::SetupClear,
            &sabaki_domain_core::Properties::new(),
        );
        assert!(transactions.is_empty());
    }

    #[test]
    fn scoring_override_cycles_through_states() {
        assert_eq!(super::next_scoring_override(None), 1);
        assert_eq!(super::next_scoring_override(Some(0)), 1);
        assert_eq!(super::next_scoring_override(Some(1)), -1);
        assert_eq!(super::next_scoring_override(Some(-1)), 0);

        let transaction = super::create_scoring_transaction(Vertex { column: 3, row: 3 }, 1);
        assert_eq!(
            transaction.transaction_type,
            GameTransactionType::ApplyScoringOverride
        );
        assert_eq!(transaction.score_override, Some(1));
    }
}
