use ryusei_domain_core::{
    CURRENT_TRANSACTION_SCHEMA_VERSION, GameTransaction, GameTransactionType, MarkerSnapshot,
    NodeId, Vertex,
};

/// The markup tools exposed by the shell toolbar. `Play` falls through to the
/// normal board interaction; the markup tools attach a marker to the clicked
/// vertex via the `AddMarkup` transaction; the setup tools write `AB`/`AW`/
/// `AE`-style setup properties via `SetNodeProperty`.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MarkupTool {
    Play,
    Circle,
    Square,
    Triangle,
    Cross,
    Label,
    Line,
    Arrow,
    SetupBlack,
    SetupWhite,
    SetupClear,
}

impl MarkupTool {
    pub fn label(self) -> &'static str {
        match self {
            MarkupTool::Play => "Play move",
            MarkupTool::Circle => "Circle",
            MarkupTool::Square => "Square",
            MarkupTool::Triangle => "Triangle",
            MarkupTool::Cross => "Cross",
            MarkupTool::Label => "Label",
            MarkupTool::Line => "Line",
            MarkupTool::Arrow => "Arrow",
            MarkupTool::SetupBlack => "Setup black",
            MarkupTool::SetupWhite => "Setup white",
            MarkupTool::SetupClear => "Setup clear",
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

    /// The SGF line property for drag-drawn board annotations.
    pub fn line_property(self) -> Option<&'static str> {
        match self {
            MarkupTool::Line => Some("LN"),
            MarkupTool::Arrow => Some("AR"),
            _ => None,
        }
    }

    /// Whether this tool drives line/arrow drawing.
    pub fn is_line_tool(self) -> bool {
        self.line_property().is_some()
    }

    /// Whether this tool drives setup-stone editing instead of markup.
    pub fn is_setup_tool(self) -> bool {
        self.setup_property().is_some()
    }
}

/// Builds the `AddMarkup` transaction for the current node, or `None` when the
/// tool is `Play` (which is handled as a normal move instead).
///
/// `MarkerSnapshot.marker_type` uses the domain-core semantic names
/// (`circle`/`square`/`triangle`/`cross`/`label`) — the same vocabulary the
/// board renderer and the document normalizer expect. Writing SGF property
/// names here makes `add_markup` reject the transaction.
pub fn create_markup_transaction(
    node_id: &NodeId,
    vertex: Vertex,
    tool: MarkupTool,
    label: &str,
) -> Option<GameTransaction> {
    let marker_type = match tool {
        MarkupTool::Circle => "circle",
        MarkupTool::Square => "square",
        MarkupTool::Triangle => "triangle",
        MarkupTool::Cross => "cross",
        MarkupTool::Label => "label",
        _ => return None,
    };
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

/// Builds a `SetNodeProperty` transaction that appends one `start:end` point
/// pair to the current node's `LN` (line) or `AR` (arrow) property.
pub fn create_line_transaction(
    node_id: &NodeId,
    start: Vertex,
    end: Vertex,
    tool: MarkupTool,
    node_properties: &ryusei_domain_core::Properties,
) -> Option<GameTransaction> {
    let property = tool.line_property()?;
    let point = format!(
        "{}:{}",
        crate::goban_view::format_sgf_vertex(start),
        crate::goban_view::format_sgf_vertex(end)
    );
    let mut values = node_properties.get(property).cloned().unwrap_or_default();
    if !values.contains(&point) {
        values.push(point);
    }
    Some(set_property_transaction(node_id, property, values))
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
    node_properties: &ryusei_domain_core::Properties,
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

/// Builds transactions that clear every markup/label/line property from the
/// current node. Each property is reset to an empty value list, which the
/// domain-core document normalizer removes entirely. Pure and additive so the
/// board-markup toolbar keeps one-click clearing independent of tool state.
pub fn create_clear_markup_transactions(node_id: &NodeId) -> Vec<GameTransaction> {
    ["CR", "SQ", "TR", "MA", "LB", "LN", "AR"]
        .into_iter()
        .map(|property| set_property_transaction(node_id, property, Vec::new()))
        .collect()
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
/// ryusei UI. Label markers render their text directly.
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

/// Computes the scoring summary line for the status area: territory,
/// stones, captures, komi and margin. Pure function so the panel logic is
/// unit-testable without a view. `estimator_iterations` is the
/// `score.estimator_iterations` setting: `0` keeps the deterministic
/// zero-liberty heuristic, `>0` uses Monte-Carlo playout for life-and-death.
pub fn scoring_summary(
    snapshot: &ryusei_domain_core::GameSnapshot,
    estimator_iterations: usize,
) -> String {
    let komi = snapshot
        .root_properties
        .get("KM")
        .and_then(|values| values.first())
        .and_then(|value| value.parse::<f64>().ok())
        .or(Some(ryusei_domain_core::DEFAULT_KOMI));
    let rule = ryusei_domain_core::ScoringRule::from_sgf_ru(
        snapshot
            .root_properties
            .get("RU")
            .and_then(|values| values.first())
            .map(String::as_str),
    );
    let result = ryusei_domain_core::score_board_with_estimation(
        &snapshot.board,
        komi,
        &snapshot.score_overrides,
        rule,
        estimator_iterations,
    );
    let winner = match result.winner {
        Some(ryusei_domain_core::Color::Black) => "Black",
        Some(ryusei_domain_core::Color::White) => "White",
        None => "Draw",
    };
    let tax = (result.rule == ryusei_domain_core::ScoringRule::ChineseAncient).then(|| {
        format!(
            " - {} group tax ({} groups)",
            result.black_group_tax, result.black_groups
        )
    });
    let white_tax = (result.rule == ryusei_domain_core::ScoringRule::ChineseAncient).then(|| {
        format!(
            " - {} group tax ({} groups)",
            result.white_group_tax, result.white_groups
        )
    });
    format!(
        "B {:.1} ({} ter + {} stones + {} cap{}) - W {:.1} ({} ter + {} stones + {} cap + {:.1} komi{}) → {} by {:.1}",
        result.black_total,
        result.black_territory,
        result.black_stones,
        result.black_captured,
        tax.unwrap_or_default(),
        result.white_total,
        result.white_territory,
        result.white_stones,
        result.white_captured,
        result.komi,
        white_tax.unwrap_or_default(),
        winner,
        result.margin
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn scoring_summary_reports_black_win_by_margin() {
        use ryusei_domain_core::{Color, GameDocument, Vertex};

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
        let summary = super::scoring_summary(&snapshot, 0);
        assert!(
            summary.contains("B 9.0"),
            "black total must be territory 5 + stones 4: {summary}"
        );
        assert!(summary.contains("→ Black by 9.0"), "summary: {summary}");
    }

    #[test]
    fn scoring_summary_reports_ancient_chinese_group_tax() {
        use ryusei_domain_core::{Color, GameDocument, Vertex};

        let mut game = GameDocument::new(3, 3).unwrap();
        for vertex in [Vertex { column: 0, row: 0 }, Vertex { column: 2, row: 0 }] {
            game.play_move(Color::Black, Some(vertex))
                .expect("setup moves are legal");
        }
        let mut snapshot = game.snapshot();
        snapshot
            .root_properties
            .insert("RU".to_owned(), vec!["Chinese-ancient".to_owned()]);
        snapshot
            .root_properties
            .insert("KM".to_owned(), vec!["0".to_owned()]);
        let summary = super::scoring_summary(&snapshot, 0);
        assert!(
            summary.contains("group tax (2 groups)"),
            "summary: {summary}"
        );
    }

    #[test]
    fn scoring_summary_reports_white_win_with_komi() {
        use ryusei_domain_core::{Color, GameDocument, Vertex};

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
        let summary = super::scoring_summary(&snapshot, 0);
        assert!(
            summary.contains("+ 10.0 komi") && summary.contains("→ White"),
            "komi must flip the winner: {summary}"
        );
    }

    use super::{MarkupTool, create_line_transaction, create_markup_transaction, markup_symbol};
    use ryusei_domain_core::{GameDocument, GameTransactionType, Vertex};

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
        assert_eq!(marker.marker_type, "circle");
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
        assert_eq!(marker.marker_type, "label");
        assert_eq!(marker.label.as_deref(), Some("Q"));
    }

    #[test]
    fn each_tool_maps_to_its_marker_type() {
        let cases = [
            (MarkupTool::Circle, "circle"),
            (MarkupTool::Square, "square"),
            (MarkupTool::Triangle, "triangle"),
            (MarkupTool::Cross, "cross"),
            (MarkupTool::Label, "label"),
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
                "{tool:?} must use the domain-core marker type {expected}"
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
    fn line_transactions_use_sgf_pairs_and_do_not_duplicate_them() {
        let node_id = current_node_id();
        let mut properties = ryusei_domain_core::Properties::new();
        properties.insert("LN".to_owned(), vec!["dd:ee".to_owned()]);

        let duplicate = create_line_transaction(
            &node_id,
            Vertex { column: 3, row: 3 },
            Vertex { column: 4, row: 4 },
            MarkupTool::Line,
            &properties,
        )
        .expect("line tool creates a transaction");
        assert_eq!(duplicate.property.as_deref(), Some("LN"));
        assert_eq!(duplicate.values, vec!["dd:ee"]);

        let arrow = create_line_transaction(
            &node_id,
            Vertex { column: 3, row: 3 },
            Vertex { column: 5, row: 5 },
            MarkupTool::Arrow,
            &properties,
        )
        .expect("arrow tool creates a transaction");
        assert_eq!(arrow.property.as_deref(), Some("AR"));
        assert_eq!(arrow.values, vec!["dd:ff"]);
    }

    #[test]
    fn setup_placement_appends_and_deduplicates_vertices() {
        use ryusei_domain_core::Properties;

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
            &ryusei_domain_core::Properties::new(),
        );

        assert_eq!(transactions.len(), 1);
        assert_eq!(transactions[0].property.as_deref(), Some("AW"));
        assert_eq!(transactions[0].values, vec!["pd".to_owned()]);
    }

    #[test]
    fn setup_clear_removes_the_vertex_from_both_properties() {
        use ryusei_domain_core::Properties;

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
            &ryusei_domain_core::Properties::new(),
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
