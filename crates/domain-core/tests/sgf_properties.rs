//! Property tests for SGF data fidelity.
//!
//! These run with proptest and pin the core invariants that guard against
//! silent data loss (Beta gate: "SGF 不静默丢失数据"):
//! - serialize(parse(sgf)) is a fixed point: parsing a generated SGF and
//!   re-serializing it must yield the same text again (canonical round-trip);
//! - parse(serialize(game)) must reproduce the exact same move sequence and
//!   board, for any game reachable through random legal moves.

use proptest::prelude::*;
use sabaki_domain_core::{Color, GameDocument, GameTransaction, GameTransactionType, Vertex};

fn legal_play(color: Color, vertex: Vertex) -> GameTransaction {
    GameTransaction {
        schema_version: sabaki_domain_core::CURRENT_TRANSACTION_SCHEMA_VERSION,
        transaction_type: GameTransactionType::PlayMove,
        color: Some(color),
        vertex: Some(vertex),
        node_id: None,
        property: None,
        values: Vec::new(),
        marker: None,
        nodes: Vec::new(),
        score_override: None,
    }
}

fn pass(color: Color) -> GameTransaction {
    GameTransaction {
        schema_version: sabaki_domain_core::CURRENT_TRANSACTION_SCHEMA_VERSION,
        transaction_type: GameTransactionType::Pass,
        color: Some(color),
        vertex: None,
        node_id: None,
        property: None,
        values: Vec::new(),
        marker: None,
        nodes: Vec::new(),
        score_override: None,
    }
}

/// A random sequence of legal-looking moves; games are played on small boards
/// so captures and kos actually occur, and each move is replayed against the
/// document so only legal moves survive.
fn move_sequence(size: usize) -> impl Strategy<Value = Vec<GameTransaction>> {
    let vertex_strategy = (0..size * size).prop_map(move |index| Vertex {
        column: index % size,
        row: index / size,
    });
    proptest::collection::vec(
        proptest::prop_oneof![
            5 => vertex_strategy
                .clone()
                .prop_map(move |vertex| legal_play(Color::Black, vertex)),
            5 => vertex_strategy
                .clone()
                .prop_map(move |vertex| legal_play(Color::White, vertex)),
            1 => Just(pass(Color::Black)),
            1 => Just(pass(Color::White)),
        ],
        0..120,
    )
}

proptest! {
    /// Canonical round-trip: re-serializing an already-serialized document
    /// must be idempotent, so toggling save/reload never rewrites the file.
    #[test]
    fn reserialization_is_idempotent(moves in move_sequence(9)) {
        let mut game = GameDocument::new(9, 9).unwrap();
        for transaction in moves {
            let _ = game.apply_transaction(transaction);
        }
        let first = game.to_sgf();
        let reparsed = GameDocument::from_sgf(&first).expect("serialized SGF must reparse");
        prop_assert_eq!(reparsed.to_sgf(), first);
    }

    /// The move sequence survives the serialize -> parse round trip exactly.
    #[test]
    fn moves_survive_round_trip(moves in move_sequence(7)) {
        let mut game = GameDocument::new(7, 7).unwrap();
        let mut expected = Vec::new();
        for transaction in moves {
            if game.apply_transaction(transaction).is_ok() {
                expected.push(game.snapshot().moves.last().cloned().unwrap());
            }
        }
        let reparsed = GameDocument::from_sgf(&game.to_sgf()).unwrap();
        let actual: Vec<_> = reparsed.snapshot().moves;
        prop_assert_eq!(actual, expected, "move sequence must round-trip exactly");
    }

    /// The board state after replay must match the parsed document's board.
    #[test]
    fn board_survives_round_trip(moves in move_sequence(7)) {
        let mut game = GameDocument::new(7, 7).unwrap();
        for transaction in moves {
            let _ = game.apply_transaction(transaction);
        }
        let before = game.snapshot().board.clone();
        let reparsed = GameDocument::from_sgf(&game.to_sgf()).unwrap();
        prop_assert_eq!(reparsed.snapshot().board, before);
    }

    /// Root properties such as SZ/GM/FF survive the round trip.
    #[test]
    fn root_properties_survive_round_trip(moves in move_sequence(9)) {
        let mut game = GameDocument::new(9, 9).unwrap();
        for transaction in moves {
            let _ = game.apply_transaction(transaction);
        }
        let before = game.snapshot().root_properties.clone();
        let reparsed = GameDocument::from_sgf(&game.to_sgf()).unwrap();
        prop_assert_eq!(reparsed.snapshot().root_properties, before);
    }

    /// A legal game on a real board never crashes the serializers.
    #[test]
    fn arbitrary_legal_game_never_panics(moves in move_sequence(19)) {
        let mut game = GameDocument::new(19, 19).unwrap();
        let mut serialized = game.to_sgf();
        for transaction in moves {
            let _ = game.apply_transaction(transaction);
            serialized = game.to_sgf();
            let _ = GameDocument::from_sgf(&serialized);
        }
    }
}
