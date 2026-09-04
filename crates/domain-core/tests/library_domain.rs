//! Integration tests for the unified library domain vocabulary
//! (`ryusei_domain_core::library`).
//!
//! These tests exercise only public seams: metadata normalization from SGF
//! root properties, record identity/dedupe across sources, and the library
//! index's stable numbering plus query behaviour.

use std::path::PathBuf;

use ryusei_domain_core::library::{
    GameRecord, LibraryIndex, LibraryQuery, LibrarySort, RecordId, RecordMetadata, RecordNumber,
    RecordSource,
};

fn properties_of(sgf: &str) -> ryusei_domain_core::Properties {
    ryusei_domain_core::extract_root_properties(sgf)
}

fn record(
    id: RecordId,
    source: RecordSource,
    title: &str,
    metadata: RecordMetadata,
    updated_at: u64,
) -> GameRecord {
    GameRecord {
        id,
        number: RecordNumber(0),
        title: title.to_owned(),
        source,
        metadata,
        tags: Vec::new(),
        content_fingerprint: None,
        revisions: Vec::new(),
        created_at: 1_700_000_000_000,
        updated_at,
    }
}

fn git_record(source_id: &str, relative_path: &str, title: &str, updated_at: u64) -> GameRecord {
    let source = RecordSource::Git {
        source_id: source_id.to_owned(),
        relative_path: relative_path.to_owned(),
    };
    let id = RecordId::for_source(&source);
    record(id, source, title, RecordMetadata::default(), updated_at)
}

#[test]
fn metadata_normalizes_the_full_sgf_header() {
    let properties = properties_of(
        "(;GM[1]FF[4]SZ[19]PB[柯洁]PW[申真谞]RE[B+R]DT[2024-01-02]\
         EV[某赛事]RO[决赛第3局]KM[6.5]RU[Chinese]HA[2])",
    );
    let metadata = RecordMetadata::from_root_properties(&properties);

    assert_eq!(metadata.black.as_deref(), Some("柯洁"));
    assert_eq!(metadata.white.as_deref(), Some("申真谞"));
    assert_eq!(metadata.result.as_deref(), Some("B+R"));
    assert_eq!(metadata.date.as_deref(), Some("2024-01-02"));
    assert_eq!(metadata.event.as_deref(), Some("某赛事"));
    assert_eq!(metadata.round.as_deref(), Some("决赛第3局"));
    assert_eq!(metadata.komi.as_deref(), Some("6.5"));
    assert_eq!(metadata.rules.as_deref(), Some("Chinese"));
    assert_eq!(metadata.handicap, Some(2));
    assert_eq!(metadata.board_size, Some(19));
}

#[test]
fn metadata_handles_partial_headers_and_empty_values() {
    // A bare game with no header fields.
    let empty = RecordMetadata::from_root_properties(&properties_of("(;GM[1])"));
    assert_eq!(empty, RecordMetadata::default());
    assert_eq!(empty.display_name("fallback.sgf"), "fallback.sgf");

    // Empty PB/PW strings are treated as absent.
    let blank = RecordMetadata::from_root_properties(&properties_of("(;PB[]PW[]GN[仅名字])"));
    assert_eq!(blank.black, None);
    assert_eq!(blank.display_name("x"), "仅名字");
}

#[test]
fn metadata_display_name_prefers_game_name_then_players() {
    let game_name = RecordMetadata::from_root_properties(&properties_of("(;GN[名人战]PB[A]PW[B])"));
    assert_eq!(game_name.display_name("f"), "名人战");

    let players = RecordMetadata::from_root_properties(&properties_of("(;PB[黑方甲]PW[白方乙])"));
    assert_eq!(players.display_name("f"), "黑方甲 vs 白方乙");
    assert_eq!(players.game_name(), None);
}

#[test]
fn record_source_identity_is_stable_per_external_game() {
    let ogs_a = RecordSource::Ogs { game_id: 42 };
    let ogs_b = RecordSource::Ogs { game_id: 42 };
    assert_eq!(RecordId::for_source(&ogs_a), RecordId::for_source(&ogs_b));

    let fox = RecordSource::Fox {
        chess_id: "abc123".to_owned(),
    };
    assert_eq!(RecordId::for_source(&fox), RecordId::for_source(&fox));
    assert_ne!(RecordId::for_source(&ogs_a), RecordId::for_source(&fox));

    let local_a = RecordSource::Local {
        path: PathBuf::from("/games/a.sgf"),
    };
    let local_b = RecordSource::Local {
        path: PathBuf::from("/games/b.sgf"),
    };
    assert_ne!(
        RecordId::for_source(&local_a),
        RecordId::for_source(&local_b)
    );

    let git_a = RecordSource::Git {
        source_id: "pro".to_owned(),
        relative_path: "x.sgf".to_owned(),
    };
    let git_b = RecordSource::Git {
        source_id: "pro".to_owned(),
        relative_path: "x.sgf".to_owned(),
    };
    assert_eq!(RecordId::for_source(&git_a), RecordId::for_source(&git_b));
}

#[test]
fn record_source_round_trips_through_json_with_kind_tags() {
    let sources = vec![
        RecordSource::Local {
            path: PathBuf::from("/g.sgf"),
        },
        RecordSource::Git {
            source_id: "s".to_owned(),
            relative_path: "r.sgf".to_owned(),
        },
        RecordSource::Ogs { game_id: 7 },
        RecordSource::Fox {
            chess_id: "c".to_owned(),
        },
        RecordSource::Live {
            page_url: "https://online-go.com/game/7".to_owned(),
        },
    ];
    for source in sources {
        let json = serde_json::to_string(&source).expect("serialize source");
        assert!(
            json.contains("\"kind\""),
            "internal kind tag expected: {json}"
        );
        let restored: RecordSource = serde_json::from_str(&json).expect("deserialize source");
        assert_eq!(restored, source);
    }
}

#[test]
fn index_insert_assigns_stable_numbers_and_dedupes_by_identity() {
    let mut index = LibraryIndex::default();

    let first = git_record("pro", "a.sgf", "Game A", 100);
    let (number, outcome) = index.insert(first);
    assert_eq!(number, RecordNumber(1));
    assert_eq!(outcome, ryusei_domain_core::library::InsertOutcome::Added);
    assert_eq!(index.len(), 1);

    let second = git_record("pro", "b.sgf", "Game B", 200);
    let (number, outcome) = index.insert(second);
    assert_eq!(number, RecordNumber(2));
    assert_eq!(outcome, ryusei_domain_core::library::InsertOutcome::Added);
    assert_eq!(index.len(), 2);

    // Re-inserting the same source identity updates content but keeps its
    // original number, and never grows the index.
    let mut updated_a = git_record("pro", "a.sgf", "Game A (renamed)", 300);
    updated_a.metadata = RecordMetadata::from_root_properties(&properties_of("(;PB[新黑])"));
    let (number, outcome) = index.insert(updated_a);
    assert_eq!(number, RecordNumber(1), "number must survive updates");
    assert_eq!(outcome, ryusei_domain_core::library::InsertOutcome::Updated);
    assert_eq!(index.len(), 2);
    let found = index.get(&RecordId::for_source(&RecordSource::Git {
        source_id: "pro".to_owned(),
        relative_path: "a.sgf".to_owned(),
    }));
    assert_eq!(found.map(|r| r.title.as_str()), Some("Game A (renamed)"));
    assert_eq!(found.unwrap().metadata.black.as_deref(), Some("新黑"));

    // A brand-new record gets the next free number (no gaps from updates).
    let third = git_record("pro", "c.sgf", "Game C", 400);
    let (number, _) = index.insert(third);
    assert_eq!(number, RecordNumber(3));
}

#[test]
fn index_query_filters_by_text_source_and_sort() {
    let mut index = LibraryIndex::default();

    let mut ke_jie = git_record("tour", "kj.sgf", "应氏杯决赛", 500);
    ke_jie.metadata =
        RecordMetadata::from_root_properties(&properties_of("(;PB[柯洁]PW[申真谞]EV[应氏杯])"));
    ke_jie.source = RecordSource::Fox {
        chess_id: "kj1".to_owned(),
    };
    ke_jie.id = RecordId::for_source(&ke_jie.source);
    index.insert(ke_jie);

    let mut local_study = git_record("study", "lesson.sgf", "布局课", 100);
    local_study.metadata =
        RecordMetadata::from_root_properties(&properties_of("(;PB[老师]PW[学生]GN[布局课])"));
    index.insert(local_study);

    let text_hits = index.query(&LibraryQuery {
        text: Some("申真谞".to_owned()),
        ..LibraryQuery::default()
    });
    assert_eq!(text_hits.len(), 1);
    assert_eq!(text_hits[0].title, "应氏杯决赛");

    let fox_hits = index.query(&LibraryQuery {
        source_kind: Some("fox".to_owned()),
        ..LibraryQuery::default()
    });
    assert_eq!(fox_hits.len(), 1);
    assert_eq!(fox_hits[0].metadata.black.as_deref(), Some("柯洁"));

    // Default ordering: newest-updated first.
    let newest_first = index.query(&LibraryQuery::default());
    assert_eq!(newest_first.len(), 2);
    assert_eq!(newest_first[0].title, "应氏杯决赛");

    // Explicit number ordering is stable regardless of update time.
    let by_number = index.query(&LibraryQuery {
        sort: LibrarySort::NumberAscending,
        ..LibraryQuery::default()
    });
    assert_eq!(by_number[0].title, "应氏杯决赛");
    assert_eq!(by_number[1].title, "布局课");
}

#[test]
fn push_revision_is_monotonic_bounded_and_serializable() {
    use ryusei_domain_core::library::{RecordRevisionRef, RevisionTrigger};

    let mut index = LibraryIndex::default();
    let record = git_record("pro", "a.sgf", "A", 100);
    let id = record.id.clone();
    index.insert(record);

    let revision_of = |n: u64| RecordRevisionRef {
        revision: 0, // sequence is assigned by push_revision
        saved_at_unix_milliseconds: n,
        trigger: RevisionTrigger::ManualSave,
        content_fingerprint: Some(format!("fp-{n}")),
    };

    // Sequence numbers are assigned monotonically per record.
    for n in 1..=5u64 {
        let seq = index
            .push_revision(&id, revision_of(1000 + n), 3)
            .expect("record exists");
        assert_eq!(seq, n);
    }
    let record = index.get(&id).expect("record exists");
    // Bounded to 3, newest first: revisions 5, 4, 3.
    let seqs: Vec<u64> = record.revisions.iter().map(|r| r.revision).collect();
    assert_eq!(seqs, vec![5, 4, 3]);

    // Revisions survive serialization.
    let json = index.to_json().expect("serialize");
    let restored = LibraryIndex::from_json(&json).expect("deserialize");
    let restored_record = restored.get(&id).expect("record present");
    assert_eq!(restored_record.revisions, record.revisions);
    // A record with no revisions (old index files) still parses.
    let bare = LibraryIndex::from_json(r#"{"schemaVersion":1,"nextRecordNumber":2,"records":{}}"#)
        .expect("bare index parses");
    assert_eq!(bare.len(), 0);
}

#[test]
fn index_serializes_and_restores_without_losing_numbers() {
    let mut index = LibraryIndex::default();
    index.insert(git_record("pro", "a.sgf", "A", 100));
    index.insert(git_record("pro", "b.sgf", "B", 200));

    let json = index.to_json().expect("index serializes");
    let mut restored = LibraryIndex::from_json(&json).expect("index deserializes");
    assert_eq!(restored, index);
    assert_eq!(restored.len(), 2);
    // The next number survived, so a new insert does not reuse old numbers.
    let (number, _) = restored.insert(git_record("pro", "c.sgf", "C", 300));
    assert_eq!(number, RecordNumber(3));
}
