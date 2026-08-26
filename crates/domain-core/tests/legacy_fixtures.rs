//! Integration tests driving the real legacy fixtures (copied from the
//! Electron reference test suite) through the importers into the canonical
//! `GameDocument`, asserting the same metadata and scale the reference
//! produces.

use ryusei_domain_core::{GameDocument, legacy};
use std::path::PathBuf;

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/legacy")
        .join(name);
    std::fs::read_to_string(path).expect("fixture must be readable")
}

fn imported(
    name: &str,
    importer: fn(&str) -> Result<String, legacy::LegacyImportError>,
) -> GameDocument {
    let sgf = importer(&fixture(name)).expect("fixture imports");
    GameDocument::from_sgf(&sgf).expect("imported SGF is valid")
}

#[test]
fn imports_the_real_even_ngf() {
    let game = imported("even.ngf", legacy::ngf::parse);
    let snapshot = game.snapshot();

    assert_eq!(snapshot.moves.len(), 333, "the reference records 333 moves");
    let root = &snapshot.root_properties;
    assert_eq!(root.get("SZ"), Some(&vec!["19".to_owned()]));
    assert_eq!(root.get("KM"), Some(&vec!["7.5".to_owned()]));
    assert_eq!(root.get("RE"), Some(&vec!["B+0.5".to_owned()]));
    assert_eq!(root.get("DT"), Some(&vec!["2017-03-16".to_owned()]));
    assert_eq!(root.get("WR"), Some(&vec!["9p".to_owned()]));
    assert_eq!(root.get("BR"), Some(&vec!["9p".to_owned()]));
    assert_eq!(root.get("PW"), Some(&vec!["LQC".to_owned()]));
    assert_eq!(root.get("PB"), Some(&vec!["CYY".to_owned()]));
}

#[test]
fn imports_the_real_handicap_ngf() {
    let game = imported("handicap2.ngf", legacy::ngf::parse);
    let snapshot = game.snapshot();

    assert_eq!(snapshot.moves.len(), 189);
    let root = &snapshot.root_properties;
    assert_eq!(root.get("HA"), Some(&vec!["2".to_owned()]));
    assert_eq!(
        root.get("AB"),
        Some(&vec!["dp".to_owned(), "pd".to_owned()]),
        "2-stone tygem placement"
    );
    assert_eq!(root.get("RE"), Some(&vec!["W+R".to_owned()]));
    assert_eq!(root.get("DT"), Some(&vec!["2017-03-16".to_owned()]));
    assert_eq!(root.get("BR"), Some(&vec!["5d*".to_owned()]));
    assert_eq!(root.get("WR"), Some(&vec!["7d*".to_owned()]));
    assert_eq!(root.get("PB"), Some(&vec!["p81587".to_owned()]));
    assert_eq!(root.get("PW"), Some(&vec!["ace550".to_owned()]));
}

#[test]
fn imports_the_real_utf8_gib() {
    let game = imported("utf8.gib", legacy::gib::parse);
    let snapshot = game.snapshot();

    assert_eq!(
        snapshot.moves.len(),
        118,
        "the reference records 118 STO moves"
    );
    let root = &snapshot.root_properties;
    assert_eq!(root.get("SZ"), Some(&vec!["19".to_owned()]));
    assert_eq!(root.get("HA"), Some(&vec!["3".to_owned()]), "INI handicap");
    assert_eq!(
        root.get("AB").map(|values| values.len()),
        Some(3),
        "three handicap stones"
    );
    assert_eq!(root.get("PW"), Some(&vec!["leejw977".to_owned()]));
    assert_eq!(root.get("WR"), Some(&vec!["10K".to_owned()]));
    assert_eq!(root.get("PB"), Some(&vec!["jy512".to_owned()]));
    assert_eq!(root.get("BR"), Some(&vec!["15K".to_owned()]));
}

#[test]
fn imports_the_real_amateur_ugf() {
    let game = imported("amateur.ugf", legacy::ugf::parse);
    let snapshot = game.snapshot();

    assert_eq!(snapshot.moves.len(), 254, "254 data rows with nodeNum > 0");
    let root = &snapshot.root_properties;
    assert_eq!(root.get("SZ"), Some(&vec!["19".to_owned()]));
    assert_eq!(root.get("KM"), Some(&vec!["-5.50".to_owned()]));
    assert_eq!(root.get("RE"), Some(&vec!["B+7.50".to_owned()]));
    assert_eq!(root.get("DT"), Some(&vec!["2019-03-08".to_owned()]));
    assert_eq!(root.get("PB"), Some(&vec!["kaziwami".to_owned()]));
    assert_eq!(root.get("BR"), Some(&vec!["7d".to_owned()]));
    assert_eq!(root.get("PW"), Some(&vec!["YINNI".to_owned()]));
    assert_eq!(root.get("WR"), Some(&vec!["8d".to_owned()]));
}

#[test]
fn import_by_extension_dispatches_and_rejects_unknown_formats() {
    let sgf = legacy::import_by_extension("ngf", "(fake)").unwrap_err();
    let _ = sgf;
    assert!(matches!(
        legacy::import_by_extension("xyz", "content"),
        Err(legacy::LegacyImportError::UnsupportedExtension(_))
    ));
}
