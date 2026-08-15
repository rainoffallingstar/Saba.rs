use sabaki_domain_core::gtp::{GtpError, GtpResponse, parse_response};

/// A minimal GTP engine port. The real process transport already lives in
/// `sabaki_domain_core::GtpProcessSupervisor`; the shell console uses an
/// in-memory mock so it can be exercised without a bundled engine binary.
pub trait GtpEngine {
    fn send(&mut self, name: &str, arguments: Vec<String>) -> Result<GtpResponse, GtpError>;
}

/// A deterministic in-memory GTP engine that answers the protocol handshake,
/// board setup and a fake move generator. Command semantics mirror the
/// reference engines closely enough for the console and its tests.
#[derive(Clone, Debug)]
pub struct MockGtpEngine {
    board_size: usize,
    occupied: Vec<(usize, usize)>,
    generated_moves: usize,
}

impl Default for MockGtpEngine {
    fn default() -> Self {
        Self {
            board_size: 19,
            occupied: Vec::new(),
            generated_moves: 0,
        }
    }
}

impl MockGtpEngine {
    /// Exposes the simulated board size for tests and diagnostics.
    #[allow(dead_code)]
    pub fn board_size(&self) -> usize {
        self.board_size
    }

    /// Exposes the simulated occupied vertices for tests and diagnostics.
    #[allow(dead_code)]
    pub fn occupied_vertices(&self) -> &[(usize, usize)] {
        &self.occupied
    }
}

/// Parses a GTP vertex like `D4` into zero-based `(column, row)`, or `pass`.
/// GTP column letters skip `I` (A..H, J..Z), so `J3` is column 8.
pub fn parse_gtp_vertex(board_size: usize, text: &str) -> Option<(usize, usize)> {
    let text = text.trim();
    if text.eq_ignore_ascii_case("pass") {
        return None;
    }
    let column_char = text.chars().next()?;
    let row_text = text.get(1..)?;
    if !column_char.is_ascii_alphabetic() {
        return None;
    }
    let column = gtp_column_index(column_char)?;
    let row = row_text.parse::<usize>().ok()?;
    if column >= board_size || row < 1 || row > board_size {
        return None;
    }
    Some((column, board_size - row))
}

/// Maps a GTP column letter (skipping `I`) to a zero-based column index.
fn gtp_column_index(column_char: char) -> Option<usize> {
    let upper = column_char.to_ascii_uppercase();
    if !('A'..='Z').contains(&upper) || upper == 'I' {
        return None;
    }
    let index = upper as usize - 'A' as usize;
    Some(if index >= 8 { index - 1 } else { index })
}

/// Formats a zero-based `(column, row)` vertex as a GTP vertex like `D4`,
/// skipping the `I` column letter as the protocol requires.
pub fn format_gtp_vertex(board_size: usize, column: usize, row: usize) -> String {
    let letter_offset = if column >= 8 { column + 1 } else { column };
    let column_char = char::from_u32((b'A' + letter_offset as u8) as u32).unwrap_or('A');
    let row_number = board_size - row;
    format!("{column_char}{row_number}")
}

impl GtpEngine for MockGtpEngine {
    fn send(&mut self, name: &str, arguments: Vec<String>) -> Result<GtpResponse, GtpError> {
        let response = match name {
            "protocol_version" => ok("2"),
            "name" => ok("MockGTP 1.0"),
            "version" => ok("0.1.0"),
            "boardsize" => {
                let Some(size) = arguments.first().and_then(|value| value.parse().ok()) else {
                    return Err(GtpError::EmptyCommandName);
                };
                self.board_size = size;
                self.occupied.clear();
                ok("")
            }
            "clear_board" => {
                self.occupied.clear();
                ok("")
            }
            "play" => match arguments.as_slice() {
                [_, vertex] => match parse_gtp_vertex(self.board_size, vertex) {
                    Some(vertex) => {
                        if self.occupied.contains(&vertex) {
                            err("illegal move: vertex already occupied")
                        } else {
                            self.occupied.push(vertex);
                            ok("")
                        }
                    }
                    None => ok(""),
                },
                _ => err("play expects a color and a vertex"),
            },
            "genmove" => {
                self.generated_moves += 1;
                let move_index = self.generated_moves;
                let column = (move_index * 3) % self.board_size;
                let row = (move_index * 7) % self.board_size;
                let vertex = (column, row);
                if self.occupied.contains(&vertex) {
                    ok("pass")
                } else {
                    self.occupied.push(vertex);
                    ok(&format_gtp_vertex(self.board_size, column, row))
                }
            }
            "lz-analyze" => ok(
                "info move D4 visits 320 winrate 0.55 scoreLead 2.1 pv D4 Q16\n\
                 info move Q16 visits 210 winrate 0.51 scoreLead 1.4 pv Q16 D4\n\
                 info move C4 visits 90 winrate 0.44 scoreLead -1.2 pv C4",
            ),
            "kata-analyze" => ok(
                r#"{"id":1,"move":"D4","visits":320,"winrate":0.55,"scoreLead":2.1,"pv":["D4","Q16"],"isDuringSearch":false}"#,
            ),
            "stop" => ok(""),
            "known_command" => {
                let is_known = matches!(
                    arguments.first().map(String::as_str),
                    Some(
                        "protocol_version"
                            | "name"
                            | "version"
                            | "boardsize"
                            | "clear_board"
                            | "play"
                            | "genmove"
                            | "lz-analyze"
                            | "kata-analyze"
                            | "stop"
                            | "known_command"
                            | "list_commands"
                    )
                );
                ok(if is_known { "true" } else { "false" })
            }
            "list_commands" => ok(
                "protocol_version\nname\nversion\nboardsize\nclear_board\nplay\ngenmove\nlz-analyze\nkata-analyze\nstop\nknown_command\nlist_commands",
            ),
            _ => err(&format!("unknown command: {name}")),
        };
        Ok(response)
    }
}

impl sabaki_host::GtpTransport for MockGtpEngine {
    fn send(&mut self, name: &str, arguments: Vec<String>) -> Result<GtpResponse, GtpError> {
        GtpEngine::send(self, name, arguments)
    }

    fn stop(&mut self) -> Result<(), std::io::Error> {
        Ok(())
    }
}

fn ok(content: &str) -> GtpResponse {
    GtpResponse {
        identifier: None,
        success: true,
        content: content.to_owned(),
    }
}

fn err(content: &str) -> GtpResponse {
    GtpResponse {
        identifier: None,
        success: false,
        content: content.to_owned(),
    }
}

/// Parses an engine-management spec line of the form
/// `Name | /path/to/engine | args | startup commands` (name and path
/// required; `args` and `commands` optional) into a validated `EngineRecord`.
pub fn parse_engine_spec(spec: &str) -> Result<sabaki_host::EngineRecord, String> {
    let mut parts = spec.splitn(4, '|').map(str::trim);
    let name = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or_default();
    let args = parts.next().unwrap_or_default();
    let commands = parts.next().unwrap_or_default();
    if name.is_empty() || path.is_empty() {
        return Err("engine spec needs at least 'name | path'".to_owned());
    }
    let mut record = sabaki_host::EngineRecord::new(name, path, args);
    if !commands.is_empty() {
        record.commands = Some(commands.to_owned());
    }
    let mut value = serde_json::json!({
        "name": record.name,
        "path": record.path,
        "args": record.args,
    });
    if let Some(commands) = &record.commands {
        value["commands"] = serde_json::json!(commands);
    }
    sabaki_host::validate_engine_record(&value).map_err(|error| error.to_string())?;
    Ok(record)
}

/// Formats a command as it should appear in the console transcript.
pub fn format_console_command(name: &str, arguments: &[String]) -> String {
    if arguments.is_empty() {
        name.to_owned()
    } else {
        format!("{} {}", name, arguments.join(" "))
    }
}

/// A single console transcript entry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EngineLogEntry {
    pub command: String,
    pub success: bool,
    pub response: String,
}

/// Captures a formatted command and its parsed response as a log entry.
pub fn entry_for_response(command: String, response: &GtpResponse) -> EngineLogEntry {
    EngineLogEntry {
        command,
        success: response.success,
        response: response.content.clone(),
    }
}

/// Renders a raw GTP line (already normalized) via the domain parser so the
/// console uses the same protocol interpretation as the engine service.
#[allow(dead_code)]
pub fn parse_console_response(lines: Vec<String>) -> Result<GtpResponse, GtpError> {
    parse_response(lines)
}

/// Selects the best analysis candidate (most visits) and converts its GTP
/// vertex to a board vertex. `pass`/`resign`/unparsable vertices yield `None`.
pub fn best_analysis_move(
    entries: &[sabaki_host::AnalysisEntry],
    board_size: usize,
) -> Option<(usize, usize)> {
    entries
        .iter()
        .max_by_key(|entry| entry.visits)
        .and_then(|entry| entry.vertex.as_deref())
        .and_then(|vertex| parse_gtp_vertex(board_size, vertex))
}

/// The black winrate fraction of the best candidate, for the winrate bar.
///
/// Engines evaluate from the perspective of the player to move; when it is
/// White to play, the reported winrate is converted to the black-perspective
/// fraction (`1 - winrate`) so the bar always reads "black vs white".
pub fn best_analysis_winrate(
    entries: &[sabaki_host::AnalysisEntry],
    next_player: sabaki_domain_core::Color,
) -> f64 {
    let raw = entries
        .iter()
        .max_by_key(|entry| entry.visits)
        .map(|entry| entry.winrate.clamp(0.0, 1.0))
        .unwrap_or(0.0);
    match next_player {
        sabaki_domain_core::Color::Black => raw,
        sabaki_domain_core::Color::White => 1.0 - raw,
    }
}

/// Merges a batch of streamed analysis entries into the current set, keyed by
/// vertex: later batches replace earlier entries for the same move (KataGo
/// re-emits candidates as the search progresses).
pub fn merge_analysis_entries(
    existing: &[sabaki_host::AnalysisEntry],
    new: Vec<sabaki_host::AnalysisEntry>,
) -> Vec<sabaki_host::AnalysisEntry> {
    use std::collections::BTreeMap;
    let mut by_vertex: BTreeMap<String, sabaki_host::AnalysisEntry> = existing
        .iter()
        .filter_map(|entry| entry.vertex.clone().map(|vertex| (vertex, entry.clone())))
        .collect();
    for entry in new {
        if let Some(vertex) = entry.vertex.clone() {
            by_vertex.insert(vertex, entry);
        }
    }
    by_vertex.into_values().collect()
}

/// The analysis command to use for the streaming analyze button, from the
/// `engines.analyze_commands` setting (first entry wins); defaults to
/// `lz-analyze` when the setting is absent or empty.
pub fn analysis_command_from_settings(settings: &sabaki_host::SettingsStore) -> String {
    settings
        .get("engines.analyze_commands")
        .and_then(serde_json::Value::as_array)
        .and_then(|entries| entries.first())
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
        .filter(|command| !command.is_empty())
        .unwrap_or_else(|| "lz-analyze".to_owned())
}

/// Parses one streamed analysis line for the given command: JSON for
/// `kata-analyze`, Leela-family info lines otherwise.
pub fn parse_stream_line(command: &str, line: &str) -> Option<sabaki_host::AnalysisEntry> {
    if command == "kata-analyze" {
        sabaki_host::parse_kata_analysis_line(line)
    } else {
        sabaki_host::parse_lz_analysis_line(line)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn streamed_batches_merge_by_vertex() {
        use sabaki_host::AnalysisEntry;
        let first = vec![AnalysisEntry {
            id: None,
            vertex: Some("D4".to_owned()),
            visits: 10,
            winrate: 0.5,
            score_lead: None,
            pv: vec![],
            is_during_search: true,
        }];
        let second = vec![
            AnalysisEntry {
                id: None,
                vertex: Some("D4".to_owned()),
                visits: 500,
                winrate: 0.8,
                score_lead: None,
                pv: vec![],
                is_during_search: false,
            },
            AnalysisEntry {
                id: None,
                vertex: Some("Q16".to_owned()),
                visits: 300,
                winrate: 0.7,
                score_lead: None,
                pv: vec![],
                is_during_search: false,
            },
        ];

        let merged = super::merge_analysis_entries(&first, second);
        assert_eq!(merged.len(), 2);
        let d4 = merged
            .iter()
            .find(|entry| entry.vertex.as_deref() == Some("D4"))
            .expect("D4 survives");
        assert_eq!(d4.visits, 500, "later batches replace earlier entries");
        assert!(!d4.is_during_search);
    }

    #[test]
    fn analysis_command_reads_the_configured_setting() {
        let mut settings = sabaki_host::SettingsStore::default();
        assert_eq!(
            super::analysis_command_from_settings(&settings),
            "lz-analyze"
        );

        settings
            .set(
                "engines.analyze_commands",
                serde_json::json!(["kata-analyze"]),
            )
            .expect("the setting accepts a string array");
        assert_eq!(
            super::analysis_command_from_settings(&settings),
            "kata-analyze"
        );

        settings
            .set("engines.analyze_commands", serde_json::json!([]))
            .expect("an empty array is valid");
        assert_eq!(
            super::analysis_command_from_settings(&settings),
            "lz-analyze"
        );
    }

    use super::{
        GtpEngine, MockGtpEngine, best_analysis_move, best_analysis_winrate, entry_for_response,
        format_console_command, format_gtp_vertex, parse_console_response, parse_gtp_vertex,
    };

    #[test]
    fn answers_the_protocol_handshake() {
        let mut engine = MockGtpEngine::default();
        let version = engine.send("protocol_version", Vec::new()).unwrap();
        assert!(version.success);
        assert_eq!(version.content, "2");

        let name = engine.send("name", Vec::new()).unwrap();
        assert_eq!(name.content, "MockGTP 1.0");
    }

    #[test]
    fn tracks_occupied_vertices_through_play() {
        let mut engine = MockGtpEngine::default();
        engine
            .send("play", vec!["B".to_owned(), "D4".to_owned()])
            .unwrap();
        engine
            .send("play", vec!["W".to_owned(), "D4".to_owned()])
            .unwrap();
        assert_eq!(engine.occupied_vertices(), &[(3, 15)]);

        let duplicate = engine
            .send("play", vec!["B".to_owned(), "D4".to_owned()])
            .unwrap();
        assert!(!duplicate.success);
    }

    #[test]
    fn generates_moves_on_empty_vertices() {
        let mut engine = MockGtpEngine::default();
        let first = engine.send("genmove", vec!["B".to_owned()]).unwrap();
        let second = engine.send("genmove", vec!["W".to_owned()]).unwrap();
        assert!(first.success);
        assert!(second.success);
        assert_ne!(first.content, second.content);
    }

    #[test]
    fn parses_and_formats_gtp_vertices() {
        assert_eq!(parse_gtp_vertex(19, "D4"), Some((3, 15)));
        assert_eq!(parse_gtp_vertex(19, "A1"), Some((0, 18)));
        assert_eq!(parse_gtp_vertex(19, "pass"), None);
        assert_eq!(parse_gtp_vertex(19, "invalid"), None);
        assert_eq!(format_gtp_vertex(19, 3, 15), "D4");
    }

    #[test]
    fn gtp_vertices_skip_the_i_column_letter() {
        // GTP columns run A..H, J..Z: J is column 8, not 9.
        assert_eq!(parse_gtp_vertex(19, "J3"), Some((8, 16)));
        assert_eq!(parse_gtp_vertex(19, "I3"), None);
        assert_eq!(format_gtp_vertex(19, 8, 16), "J3");
        assert_eq!(format_gtp_vertex(19, 7, 16), "H3");
        assert_eq!(parse_gtp_vertex(19, "H3"), Some((7, 16)));
        // Round trip across the whole board.
        for column in 0..19 {
            for row in 0..19 {
                let vertex = format_gtp_vertex(19, column, row);
                assert_eq!(parse_gtp_vertex(19, &vertex), Some((column, row)));
            }
        }
    }

    #[test]
    fn parses_engine_specs_into_validated_records() {
        let record =
            super::parse_engine_spec("KataGo | /engines/katago | -config a.cfg | level 10")
                .expect("a full spec parses");
        assert_eq!(record.name, "KataGo");
        assert_eq!(record.path, "/engines/katago");
        assert_eq!(record.args, "-config a.cfg");
        assert_eq!(record.commands.as_deref(), Some("level 10"));

        let minimal =
            super::parse_engine_spec("GNU Go | /usr/bin/gnugo").expect("a minimal spec parses");
        assert_eq!(minimal.args, "");
        assert_eq!(minimal.commands, None);

        assert!(super::parse_engine_spec("missing path").is_err());
        assert!(super::parse_engine_spec(" | /usr/bin/gnugo").is_err());
    }

    #[test]
    fn console_entry_reflects_success_and_content() {
        let response = parse_console_response(vec!["= D4".to_owned()]).unwrap();
        let entry = entry_for_response("genmove B".to_owned(), &response);
        assert!(entry.success);
        assert_eq!(entry.response, "D4");
        assert_eq!(
            format_console_command("genmove", &["B".to_owned()]),
            "genmove B"
        );
    }

    #[test]
    fn host_session_drives_the_mock_engine_through_the_lifecycle() {
        use sabaki_host::{EngineRecord, EngineSession, EngineSessionState};

        let record = EngineRecord::new("MockGTP", "mock", "");
        let mut session =
            EngineSession::start(MockGtpEngine::default(), &record, 9).expect("session starts");

        assert_eq!(
            session.state(),
            &EngineSessionState::Ready {
                name: "MockGTP 1.0".to_owned(),
                version: "0.1.0".to_owned(),
            }
        );
        assert_eq!(session.board_size(), 9);

        let generated = session.generate_move("B").expect("genmove succeeds");
        assert!(generated.success);
        assert!(!generated.content.is_empty());

        session.stop().expect("session stops");
        assert_eq!(session.state(), &EngineSessionState::Stopped);
    }

    #[test]
    fn mock_engine_answers_analysis_requests() {
        let mut engine = MockGtpEngine::default();
        let response = engine.send("lz-analyze", Vec::new()).unwrap();
        assert!(response.success);
        let entries = sabaki_host::parse_analysis_response("lz-analyze", &response.content);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].visits, 320);
        assert_eq!(entries[0].vertex.as_deref(), Some("D4"));
    }

    #[test]
    fn best_analysis_move_prefers_the_most_visited_candidate() {
        let entries = sabaki_host::parse_analysis_response(
            "lz-analyze",
            "info move D4 visits 90 winrate 0.44\ninfo move Q16 visits 320 winrate 0.55",
        );
        assert_eq!(best_analysis_move(&entries, 19), Some((15, 3)));
        assert!(
            (best_analysis_winrate(&entries, sabaki_domain_core::Color::Black) - 0.55).abs() < 1e-9
        );
        // With White to play the bar must flip to the black perspective.
        assert!(
            (best_analysis_winrate(&entries, sabaki_domain_core::Color::White) - 0.45).abs() < 1e-9
        );

        let empty: Vec<sabaki_host::AnalysisEntry> = Vec::new();
        assert_eq!(best_analysis_move(&empty, 19), None);
        assert_eq!(
            best_analysis_winrate(&empty, sabaki_domain_core::Color::Black),
            0.0
        );
    }

    #[test]
    fn best_analysis_move_yields_none_for_pass_candidates() {
        let entries = sabaki_host::parse_analysis_response(
            "lz-analyze",
            "info move pass visits 500 winrate 0.5\ninfo move D4 visits 10 winrate 0.4",
        );
        // The most-visited candidate is a pass: there is no move to mark.
        assert_eq!(best_analysis_move(&entries, 19), None);
    }
}
