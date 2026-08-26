use ryusei_host::{
    GameFileAccess, HostApplication, HostError, HostEvent, HostEventSink, SourceEncoding,
};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    path::{Path, PathBuf},
};

struct MemoryFileAccess {
    files: BTreeMap<PathBuf, String>,
}

impl GameFileAccess for MemoryFileAccess {
    fn read_game_file(
        &self,
        path: &Path,
    ) -> Result<ryusei_host::DecodedGameFile, ryusei_host::HostError> {
        self.files
            .get(path)
            .cloned()
            .map(|content| ryusei_host::DecodedGameFile {
                content,
                encoding: SourceEncoding::Utf8,
            })
            .ok_or_else(|| HostError::FileRead("the file does not exist".to_owned()))
    }

    fn write_game_file(
        &mut self,
        path: &Path,
        content: &str,
        _encoding: SourceEncoding,
    ) -> Result<(), ryusei_host::HostError> {
        self.files.insert(path.to_owned(), content.to_owned());
        Ok(())
    }
}

#[derive(Default)]
struct RecordedEvents {
    events: RefCell<Vec<HostEvent>>,
}

impl HostEventSink for RecordedEvents {
    fn emit(&mut self, event: HostEvent) {
        self.events.borrow_mut().push(event);
    }
}

const NGF_SAMPLE: &str = r#"Rated game
19
LQC         9DP
CYY         9DP
www.cyberoro.com
0
0
7
20170316 [09:37]
5
Black wins by 0.5!
4
PMABBREER
PMACWEEEE
PMADBQRRQ
PMAEWEQQE
"#;

#[test]
fn open_dispatches_ngf_through_the_legacy_importer() {
    let game_path = PathBuf::from("/games/even.ngf");
    let file_access = MemoryFileAccess {
        files: BTreeMap::from([(game_path.clone(), NGF_SAMPLE.to_owned())]),
    };
    let mut host = HostApplication::default();
    let mut events = RecordedEvents::default();

    let snapshot = host
        .open(game_path.clone(), &file_access, &mut events)
        .expect("a well-formed NGF file must import");

    assert_eq!(snapshot.moves.len(), 4);
    assert_eq!(snapshot.file_state.path.as_deref(), Some("/games/even.ngf"));
    let sgf = host.to_sgf();
    assert!(
        sgf.starts_with("(;") && sgf.contains("SZ[19]") && sgf.contains("GM[1]"),
        "imported NGF must become SGF, got: {sgf}"
    );
    assert!(
        sgf.contains(";B[qd]") && sgf.contains(";W[dd]"),
        "moves AB EE must be imported: {sgf}"
    );
}

#[test]
fn open_rejects_unsupported_legacy_extensions() {
    let game_path = PathBuf::from("/games/game.xyz");
    let file_access = MemoryFileAccess {
        files: BTreeMap::from([(game_path.clone(), "whatever".to_owned())]),
    };
    let mut host = HostApplication::default();
    let mut events = RecordedEvents::default();

    let error = host
        .open(game_path.clone(), &file_access, &mut events)
        .expect_err("an unknown extension must be rejected as an SGF parse error");
    assert!(
        matches!(error, HostError::Domain(_)),
        "unexpected error: {error:?}"
    );
}
