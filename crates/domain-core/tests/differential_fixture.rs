use std::{fs, path::PathBuf};

use sabaki_domain_core::{
    CURRENT_TRANSACTION_SCHEMA_VERSION, Color, GameDocument, GameTransaction, GameTransactionType,
    Vertex,
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct LinearFixture {
    sgf: String,
    expected: ExpectedLinearSnapshot,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExpectedLinearSnapshot {
    root_properties: std::collections::BTreeMap<String, Vec<String>>,
    moves: Vec<ExpectedMove>,
    board: ExpectedBoard,
    #[serde(default)]
    markup: Option<ExpectedMarkup>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VariationFixture {
    sgf: String,
    expected: ExpectedVariationSnapshot,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExpectedVariationSnapshot {
    root_properties: std::collections::BTreeMap<String, Vec<String>>,
    node_count: usize,
    main_line_moves: Vec<ExpectedMove>,
    first_move_variation_count: usize,
    board: ExpectedBoard,
    markup: ExpectedMarkup,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExpectedMove {
    color: Color,
    vertex: Option<Vertex>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExpectedBoard {
    width: usize,
    height: usize,
    sign_map: Vec<Vec<i8>>,
    current_vertex: Option<Vertex>,
    next_player: Color,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExpectedMarkup {
    markers: Vec<ExpectedMarker>,
    lines: Vec<ExpectedLine>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExpectedMarker {
    vertex: Vertex,
    #[serde(rename = "type")]
    marker_type: String,
    label: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExpectedLine {
    start: Vertex,
    end: Vertex,
    #[serde(rename = "type")]
    line_type: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VariationHistoryFixture {
    schema_version: u32,
    sgf: String,
    ops: Vec<HistoryOperation>,
    checkpoints: Vec<ExpectedHistoryCheckpoint>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryOperation {
    op: String,
    target: Option<ExpectedMove>,
    property: Option<String>,
    values: Option<Vec<String>>,
    vertex: Option<Vertex>,
    marker: Option<sabaki_domain_core::MarkerSnapshot>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExpectedHistoryCheckpoint {
    after_op: usize,
    main_line_moves: Vec<ExpectedMove>,
    main_line_child_counts: Vec<usize>,
    node_count: usize,
    #[serde(default)]
    board: Option<ExpectedBoard>,
    #[serde(default)]
    markup: Option<ExpectedMarkup>,
    #[serde(default)]
    node_properties: Option<Vec<std::collections::BTreeMap<String, Vec<String>>>>,
    history: ExpectedHistory,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExpectedHistory {
    can_undo: bool,
    can_redo: bool,
    undo_depth: usize,
    redo_depth: usize,
}

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/differential")
        .join(name)
}

fn assert_moves_match(
    actual_moves: &[sabaki_domain_core::MoveDto],
    expected_moves: &[ExpectedMove],
) {
    assert_eq!(
        actual_moves.len(),
        expected_moves.len(),
        "move count must match"
    );
    for (actual_move, expected_move) in actual_moves.iter().zip(expected_moves) {
        assert_eq!(actual_move.color, expected_move.color);
        assert_eq!(actual_move.vertex, expected_move.vertex);
    }
}

fn assert_board_matches(
    actual_board: &sabaki_domain_core::BoardSnapshot,
    expected_board: &ExpectedBoard,
) {
    assert_eq!(actual_board.width, expected_board.width);
    assert_eq!(actual_board.height, expected_board.height);
    assert_eq!(actual_board.sign_map, expected_board.sign_map);
    assert_eq!(actual_board.current_vertex, expected_board.current_vertex);
    assert_eq!(actual_board.next_player, expected_board.next_player);
}

fn assert_markup_matches(
    actual_board: &sabaki_domain_core::BoardSnapshot,
    expected_markup: &ExpectedMarkup,
) {
    for expected_marker in &expected_markup.markers {
        let actual_marker = actual_board.markers[expected_marker.vertex.row]
            [expected_marker.vertex.column]
            .as_ref()
            .expect("expected marker must be present");
        assert_eq!(actual_marker.marker_type, expected_marker.marker_type);
        assert_eq!(actual_marker.label, expected_marker.label);
    }

    assert_eq!(actual_board.lines.len(), expected_markup.lines.len());
    for (actual_line, expected_line) in actual_board.lines.iter().zip(&expected_markup.lines) {
        assert_eq!(actual_line.start, expected_line.start);
        assert_eq!(actual_line.end, expected_line.end);
        assert_eq!(actual_line.line_type, expected_line.line_type);
    }
}

fn format_sgf_vertex(vertex: Vertex) -> String {
    let column = (b'a' + vertex.column as u8) as char;
    let row = (b'a' + vertex.row as u8) as char;
    format!("{column}{row}")
}

fn node_matches_move(node: &sabaki_domain_core::NodeSnapshot, target: &ExpectedMove) -> bool {
    let property = match target.color {
        Color::Black => "B",
        Color::White => "W",
    };
    let expected_vertex = target.vertex.map(format_sgf_vertex).unwrap_or_default();
    node.properties
        .get(property)
        .and_then(|values| values.first())
        .is_some_and(|vertex| vertex == &expected_vertex)
}

fn find_node_id_by_move(game: &GameDocument, target: &ExpectedMove) -> String {
    game.snapshot()
        .nodes
        .into_iter()
        .find(|node| node_matches_move(node, target))
        .map(|node| node.id)
        .expect("fixture move target must identify a node")
}

fn create_node_transaction(
    transaction_type: GameTransactionType,
    node_id: String,
) -> GameTransaction {
    GameTransaction {
        schema_version: CURRENT_TRANSACTION_SCHEMA_VERSION,
        transaction_type,
        color: None,
        vertex: None,
        node_id: Some(node_id),
        property: None,
        values: Vec::new(),
        marker: None,
        nodes: Vec::new(),
        score_override: None,
    }
}

fn collect_main_line_nodes(
    snapshot: &sabaki_domain_core::GameSnapshot,
) -> Vec<sabaki_domain_core::NodeSnapshot> {
    let nodes_by_id = snapshot
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut node_id = snapshot.root_node_id.as_str();
    let mut main_line = Vec::new();

    while let Some(node) = nodes_by_id.get(node_id) {
        main_line.push((*node).clone());
        let Some(child_id) = node.child_ids.first() else {
            break;
        };
        node_id = child_id;
    }

    main_line
}

fn node_snapshot_move(
    node: &sabaki_domain_core::NodeSnapshot,
) -> Option<sabaki_domain_core::MoveDto> {
    for (property, color) in [("B", Color::Black), ("W", Color::White)] {
        let Some(value) = node
            .properties
            .get(property)
            .and_then(|values| values.first())
        else {
            continue;
        };
        let vertex = if value.is_empty() {
            None
        } else {
            let bytes = value.as_bytes();
            if bytes.len() != 2 {
                return None;
            }
            Some(Vertex {
                column: usize::from(bytes[0] - b'a'),
                row: usize::from(bytes[1] - b'a'),
            })
        };
        return Some(sabaki_domain_core::MoveDto { color, vertex });
    }
    None
}

fn assert_history_checkpoint_matches(
    snapshot: &sabaki_domain_core::GameSnapshot,
    expected: &ExpectedHistoryCheckpoint,
) {
    let main_line_nodes = collect_main_line_nodes(snapshot);
    let actual_main_line_moves = main_line_nodes
        .iter()
        .skip(1)
        .filter_map(node_snapshot_move)
        .collect::<Vec<_>>();
    let actual_child_counts = main_line_nodes
        .iter()
        .map(|node| node.child_ids.len())
        .collect::<Vec<_>>();

    assert_moves_match(&actual_main_line_moves, &expected.main_line_moves);
    assert_eq!(actual_child_counts, expected.main_line_child_counts);
    assert_eq!(snapshot.nodes.len(), expected.node_count);
    assert_eq!(snapshot.history.can_undo, expected.history.can_undo);
    assert_eq!(snapshot.history.can_redo, expected.history.can_redo);
    assert_eq!(snapshot.history.undo_depth, expected.history.undo_depth);
    assert_eq!(snapshot.history.redo_depth, expected.history.redo_depth);
    if let Some(expected_board) = &expected.board {
        assert_board_matches(&snapshot.board, expected_board);
    }
    if let Some(expected_markup) = &expected.markup {
        assert_markup_matches(&snapshot.board, expected_markup);
    }
    if let Some(expected_properties) = &expected.node_properties {
        let actual_properties = main_line_nodes
            .iter()
            .map(|node| node.properties.clone())
            .collect::<Vec<_>>();
        assert_eq!(actual_properties, *expected_properties);
    }
}

fn assert_linear_fixture_matches(name: &str) {
    let fixture: LinearFixture = serde_json::from_str(
        &fs::read_to_string(fixture_path(name)).expect("fixture must be readable"),
    )
    .expect("fixture must be valid JSON");
    let snapshot = GameDocument::from_sgf(&fixture.sgf)
        .expect("fixture SGF must be valid")
        .snapshot();

    assert_eq!(snapshot.root_properties, fixture.expected.root_properties);
    assert_moves_match(&snapshot.moves, &fixture.expected.moves);
    assert_board_matches(&snapshot.board, &fixture.expected.board);
    if let Some(expected_markup) = &fixture.expected.markup {
        assert_markup_matches(&snapshot.board, expected_markup);
    }
}

#[test]
fn matches_the_shared_linear_mainline_fixture() {
    assert_linear_fixture_matches("linear-mainline-smoke.json");
}

#[test]
fn matches_the_shared_capture_fixture() {
    assert_linear_fixture_matches("capture-single-stone.json");
}

#[test]
fn matches_the_shared_rectangular_board_and_pass_fixture() {
    assert_linear_fixture_matches("rectangular-board-pass.json");
}

#[test]
fn matches_the_shared_escaped_unicode_properties_fixture() {
    assert_linear_fixture_matches("escaped-unicode-properties.json");
}

#[test]
fn matches_the_shared_setup_stones_and_markup_fixture() {
    assert_linear_fixture_matches("setup-stones-and-markup.json");
}

fn apply_history_fixture_operation(game: &mut GameDocument, operation: &HistoryOperation) {
    match operation.op.as_str() {
        "promoteVariation" => {
            let target = operation
                .target
                .as_ref()
                .expect("promotion must specify a target move");
            let node_id = find_node_id_by_move(game, target);
            game.apply_transaction(create_node_transaction(
                GameTransactionType::PromoteVariation,
                node_id,
            ))
            .expect("promotion transaction must succeed");
        }
        "removeVariation" => {
            let target = operation
                .target
                .as_ref()
                .expect("removal must specify a target move");
            let node_id = find_node_id_by_move(game, target);
            game.apply_transaction(create_node_transaction(
                GameTransactionType::RemoveVariation,
                node_id,
            ))
            .expect("removal transaction must succeed");
        }
        "setNodeProperty" | "removeNodeProperty" => {
            let target = operation
                .target
                .as_ref()
                .expect("property mutation must specify a target move");
            let node_id = find_node_id_by_move(game, target);
            let property = operation
                .property
                .clone()
                .expect("property mutation must specify a property");
            let transaction_type = if operation.op == "setNodeProperty" {
                GameTransactionType::SetNodeProperty
            } else {
                GameTransactionType::RemoveNodeProperty
            };
            let mut transaction = create_node_transaction(transaction_type, node_id);
            transaction.property = Some(property);
            transaction.values = operation.values.clone().unwrap_or_default();
            game.apply_transaction(transaction)
                .expect("property transaction must succeed");
        }
        "addMarkup" => {
            let target = operation
                .target
                .as_ref()
                .expect("markup mutation must specify a target move");
            let node_id = find_node_id_by_move(game, target);
            let mut transaction = create_node_transaction(GameTransactionType::AddMarkup, node_id);
            transaction.vertex = Some(operation.vertex.expect("markup must specify a vertex"));
            transaction.marker = Some(
                operation
                    .marker
                    .clone()
                    .expect("markup must specify a marker"),
            );
            game.apply_transaction(transaction)
                .expect("markup transaction must succeed");
        }
        "undo" => assert!(game.undo(), "undo operation must succeed"),
        "redo" => assert!(game.redo(), "redo operation must succeed"),
        unsupported_operation => {
            panic!("unsupported differential operation: {unsupported_operation}")
        }
    }
}

fn assert_history_fixture_matches(name: &str) {
    let fixture: VariationHistoryFixture = serde_json::from_str(
        &fs::read_to_string(fixture_path(name)).expect("fixture must be readable"),
    )
    .expect("fixture must be valid JSON");
    assert_eq!(fixture.schema_version, 1);
    assert_eq!(fixture.checkpoints.len(), fixture.ops.len() + 1);

    let mut game = GameDocument::from_sgf(&fixture.sgf).expect("fixture SGF must be valid");
    assert_eq!(fixture.checkpoints[0].after_op, 0);
    assert_history_checkpoint_matches(&game.snapshot(), &fixture.checkpoints[0]);

    for (operation_index, operation) in fixture.ops.iter().enumerate() {
        apply_history_fixture_operation(&mut game, operation);

        let expected_checkpoint = &fixture.checkpoints[operation_index + 1];
        assert_eq!(expected_checkpoint.after_op, operation_index + 1);
        assert_history_checkpoint_matches(&game.snapshot(), expected_checkpoint);
    }
}

#[test]
fn matches_the_shared_variation_promotion_and_history_fixture() {
    assert_history_fixture_matches("variation-promote-remove-history.json");
}

#[test]
fn matches_the_shared_property_and_markup_history_fixture() {
    assert_history_fixture_matches("node-property-markup-history.json");
}

#[test]
fn matches_the_shared_variation_and_markup_fixture() {
    let fixture: VariationFixture = serde_json::from_str(
        &fs::read_to_string(fixture_path("variations-and-markup.json"))
            .expect("fixture must be readable"),
    )
    .expect("fixture must be valid JSON");
    let snapshot = GameDocument::from_sgf(&fixture.sgf)
        .expect("fixture SGF must be valid")
        .snapshot();

    assert_eq!(snapshot.root_properties, fixture.expected.root_properties);
    assert_eq!(snapshot.nodes.len(), fixture.expected.node_count);
    assert_moves_match(&snapshot.moves, &fixture.expected.main_line_moves);
    assert_eq!(
        snapshot.nodes[1].child_ids.len(),
        fixture.expected.first_move_variation_count
    );
    assert_board_matches(&snapshot.board, &fixture.expected.board);
    assert_markup_matches(&snapshot.board, &fixture.expected.markup);
}
