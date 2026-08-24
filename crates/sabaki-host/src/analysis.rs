//! GTP analysis response parsing.
//!
//! Two transcript formats are supported:
//! - `lz-analyze`-style lines (`info move D4 visits 320 winrate 0.55 ...`),
//!   also used by plain `analyze` in Leela-family engines;
//! - `kata-analyze` JSON lines (`{"id":1,"move":"D4","visits":10,...}`).
//!
//! Parsing is pure so the transcripts are pinned by unit tests without a real
//! engine. Streaming (throttled events while the engine searches) is a later
//! supervision concern; the session method returns the entries the engine
//! emitted for one request.

/// One analysis candidate emitted by an engine.
#[derive(Clone, Debug, PartialEq)]
pub struct AnalysisEntry {
    /// Request identifier, when the engine echoes one.
    pub id: Option<u64>,
    /// GTP vertex of the candidate move (`pass` or `resign` possible).
    pub vertex: Option<String>,
    pub visits: u64,
    /// Winrate in `[0, 1]` for the side the engine evaluates.
    pub winrate: f64,
    /// Estimated score lead in points, when the engine reports one.
    pub score_lead: Option<f64>,
    /// Principal variation as GTP vertices.
    pub pv: Vec<String>,
    /// `true` while the engine is still searching (kata-analyze only).
    pub is_during_search: bool,
    /// Optional board territory ownership probabilities (-1.0 to +1.0) for each intersection.
    pub ownership: Option<Vec<f64>>,
}

/// Parses one Leela-family analysis line:
/// `info move D4 visits 320 winrate 0.55 scoreLead 2.1 pv D4 Q16`.
/// Unknown fields are skipped; lines without any core field yield `None`.
pub fn parse_lz_analysis_line(line: &str) -> Option<AnalysisEntry> {
    let mut tokens = line.split_whitespace();
    if tokens.next()? != "info" {
        return None;
    }
    let mut entry = AnalysisEntry {
        id: None,
        vertex: None,
        visits: 0,
        winrate: 0.0,
        score_lead: None,
        pv: Vec::new(),
        is_during_search: false,
        ownership: None,
    };
    let mut saw_core_field = false;
    while let Some(field) = tokens.next() {
        match field {
            "move" => {
                entry.vertex = tokens.next().map(ToOwned::to_owned);
                saw_core_field = true;
            }
            "visits" => {
                entry.visits = tokens.next()?.parse().ok()?;
                saw_core_field = true;
            }
            "winrate" => {
                entry.winrate = tokens.next()?.parse().ok()?;
                saw_core_field = true;
            }
            "scoreLead" => {
                entry.score_lead = tokens.next()?.parse().ok();
                saw_core_field = true;
            }
            "ownership" => {
                let mut ownership_vals = Vec::new();
                while let Some(next_tok) = tokens.clone().next() {
                    if let Ok(val) = next_tok.parse::<f64>() {
                        tokens.next();
                        ownership_vals.push(val);
                    } else {
                        break;
                    }
                }
                if !ownership_vals.is_empty() {
                    entry.ownership = Some(ownership_vals);
                    saw_core_field = true;
                }
            }
            "pv" => {
                entry.pv = tokens.map(ToOwned::to_owned).collect();
                saw_core_field = true;
                break;
            }
            // "order", "lcb", "prior", "utility", "scoreSelfplay", ... are
            // engine-specific extras we intentionally skip.
            _ => {
                tokens.next()?;
            }
        }
    }
    saw_core_field.then_some(entry)
}

/// Parses one KataGo analysis JSON line. Lines without a `move` field or
/// with structurally invalid JSON yield `None`.
pub fn parse_kata_analysis_line(line: &str) -> Option<AnalysisEntry> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    let vertex = value
        .get("move")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)?;
    Some(AnalysisEntry {
        id: value.get("id").and_then(serde_json::Value::as_u64),
        vertex: Some(vertex),
        visits: value
            .get("visits")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        winrate: value
            .get("winrate")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(0.0),
        score_lead: value.get("scoreLead").and_then(serde_json::Value::as_f64),
        pv: value
            .get("pv")
            .and_then(serde_json::Value::as_array)
            .map(|array| {
                array
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(ToOwned::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
        is_during_search: value
            .get("isDuringSearch")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        ownership: value
            .get("ownership")
            .and_then(serde_json::Value::as_array)
            .map(|array| array.iter().filter_map(serde_json::Value::as_f64).collect()),
    })
}

/// Parses an engine's analysis response content. `kata-analyze` transcripts
/// are JSON lines; everything else uses the Leela-family format. Lines that
/// do not parse are skipped.
pub fn parse_analysis_response(command: &str, content: &str) -> Vec<AnalysisEntry> {
    let kata_style = command == "kata-analyze" || command == "kata-analyze_interval";
    content
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            if kata_style {
                parse_kata_analysis_line(line)
            } else {
                parse_lz_analysis_line(line)
            }
        })
        .collect()
}

/// A sink that accepts GTP commands line-by-line, for streaming analysis
/// processes that do not use request/response framing.
pub trait AnalysisCommandSink {
    fn send_command(&mut self, command: &str) -> std::io::Result<()>;
}

impl AnalysisCommandSink for sabaki_domain_core::gtp::AnalysisStream {
    fn send_command(&mut self, command: &str) -> std::io::Result<()> {
        sabaki_domain_core::gtp::AnalysisStream::send_command(self, command)
    }
}

/// Formats a zero-based vertex as a GTP vertex (`D4`, skipping the `I`
/// column), for analysis-process board replay.
fn gtp_vertex(board_size: usize, column: usize, row: usize) -> String {
    let letter_offset = if column >= 8 { column + 1 } else { column };
    let column_char = char::from_u32((b'A' + letter_offset as u8) as u32).unwrap_or('A');
    format!("{column_char}{}", board_size - row)
}

/// Replays a position into a streaming analysis process: board size, clear,
/// then every move as `play <color> <vertex>` (passes as `play <color> pass`).
pub fn replay_position_stream(
    sink: &mut impl AnalysisCommandSink,
    board_size: usize,
    moves: &[sabaki_domain_core::MoveDto],
) -> std::io::Result<()> {
    sink.send_command(&format!("boardsize {board_size}"))?;
    sink.send_command("clear_board")?;
    for move_dto in moves {
        let color = match move_dto.color {
            sabaki_domain_core::Color::Black => "B",
            sabaki_domain_core::Color::White => "W",
        };
        let vertex = match move_dto.vertex {
            Some(vertex) => gtp_vertex(board_size, vertex.column, vertex.row),
            None => "pass".to_owned(),
        };
        sink.send_command(&format!("play {color} {vertex}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{parse_analysis_response, parse_kata_analysis_line, parse_lz_analysis_line};

    #[test]
    fn parses_lz_style_analysis_lines() {
        let entry = parse_lz_analysis_line(
            "info move D4 visits 320 winrate 0.55 scoreLead 2.1 pv D4 Q16 C4",
        )
        .expect("a well-formed line parses");

        assert_eq!(entry.vertex.as_deref(), Some("D4"));
        assert_eq!(entry.visits, 320);
        assert!((entry.winrate - 0.55).abs() < 1e-9);
        assert_eq!(entry.score_lead, Some(2.1));
        assert_eq!(entry.pv, vec!["D4", "Q16", "C4"]);
        assert!(!entry.is_during_search);
    }

    #[test]
    fn lz_lines_without_score_lead_or_extra_fields_parse() {
        let entry = parse_lz_analysis_line("info move pass visits 10 winrate 0.5")
            .expect("a minimal line parses");
        assert_eq!(entry.vertex.as_deref(), Some("pass"));
        assert_eq!(entry.score_lead, None);
        assert!(entry.pv.is_empty());
    }

    #[test]
    fn lz_lines_skip_unknown_extra_fields() {
        let entry = parse_lz_analysis_line(
            "info move D4 visits 100 winrate 0.5 order 1 lcb 0.1 prior 0.2 pv D4",
        )
        .expect("extra fields are skipped");
        assert_eq!(entry.visits, 100);
        assert_eq!(entry.pv, vec!["D4"]);
    }

    #[test]
    fn malformed_lines_yield_none() {
        assert!(parse_lz_analysis_line("not an analysis line").is_none());
        assert!(parse_lz_analysis_line("info").is_none());
        assert!(parse_lz_analysis_line("info move D4 visits nope winrate 0.5").is_none());
    }

    #[test]
    fn parses_kata_json_lines() {
        let entry = parse_kata_analysis_line(
            r#"{"id":1,"move":"Q16","visits":10,"winrate":0.6,"scoreLead":3.5,"pv":["Q16","D4"],"isDuringSearch":true}"#,
        )
        .expect("a kata line parses");

        assert_eq!(entry.id, Some(1));
        assert_eq!(entry.vertex.as_deref(), Some("Q16"));
        assert_eq!(entry.visits, 10);
        assert!((entry.winrate - 0.6).abs() < 1e-9);
        assert_eq!(entry.score_lead, Some(3.5));
        assert_eq!(entry.pv, vec!["Q16", "D4"]);
        assert!(entry.is_during_search);
    }

    #[test]
    fn invalid_kata_lines_yield_none() {
        assert!(parse_kata_analysis_line("not json").is_none());
        assert!(parse_kata_analysis_line(r#"{"visits":"x"}"#).is_none());
    }

    #[test]
    fn response_parsing_dispatches_by_command_name() {
        let lz_transcript =
            "info move D4 visits 10 winrate 0.5\ninfo move Q16 visits 5 winrate 0.4";
        let kata_transcript = concat!(
            r#"{"id":1,"move":"D4","visits":10,"winrate":0.5,"isDuringSearch":true}"#,
            "\n",
            r#"{"id":1,"move":"Q16","visits":20,"winrate":0.6,"isDuringSearch":false}"#,
        );

        let lz_entries = parse_analysis_response("lz-analyze", lz_transcript);
        assert_eq!(lz_entries.len(), 2);
        assert_eq!(lz_entries[0].visits, 10);

        let kata_entries = parse_analysis_response("kata-analyze", kata_transcript);
        assert_eq!(kata_entries.len(), 2);
        assert_eq!(kata_entries[1].id, Some(1));
        assert!(!kata_entries[1].is_during_search);

        let garbage =
            parse_analysis_response("lz-analyze", "garbage\ninfo move D4 visits 1 winrate 0.5");
        assert_eq!(garbage.len(), 1);
    }
    use super::{AnalysisCommandSink, replay_position_stream};
    use sabaki_domain_core::{Color, MoveDto, Vertex};

    #[derive(Default)]
    struct RecordingSink {
        commands: Vec<String>,
    }

    impl AnalysisCommandSink for RecordingSink {
        fn send_command(&mut self, command: &str) -> std::io::Result<()> {
            self.commands.push(command.to_owned());
            Ok(())
        }
    }

    #[test]
    fn replay_position_stream_emits_board_setup_and_moves() {
        let moves = vec![
            MoveDto {
                color: Color::Black,
                vertex: Some(Vertex { column: 3, row: 15 }),
            },
            MoveDto {
                color: Color::White,
                vertex: None,
            },
            MoveDto {
                color: Color::Black,
                vertex: Some(Vertex { column: 8, row: 16 }),
            },
        ];
        let mut sink = RecordingSink::default();

        replay_position_stream(&mut sink, 19, &moves).expect("replay succeeds");

        assert_eq!(
            sink.commands,
            vec![
                "boardsize 19".to_owned(),
                "clear_board".to_owned(),
                "play B D4".to_owned(),
                "play W pass".to_owned(),
                "play B J3".to_owned(),
            ]
        );
    }

    #[test]
    fn replay_position_stream_handles_empty_positions() {
        let mut sink = RecordingSink::default();
        replay_position_stream(&mut sink, 9, &[]).expect("empty replay succeeds");
        assert_eq!(
            sink.commands,
            vec!["boardsize 9".to_owned(), "clear_board".to_owned()]
        );
    }
}
