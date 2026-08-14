//! PandaNET UGF importer.
//!
//! Mirrors the Electron reference (`src/modules/fileformats/ugf.js`): a
//! `[Header]` section with key=value lines, a `[Data]` section with
//! `coords,color,nodeNum,?` lines (nodeNum 0 = handicap), and an optional
//! `[ReviewComment]` section with `.Comment,nodeNum,...` lines.

use crate::legacy::{LegacyImportError, escape_sgf_value, format_sgf_vertex};

const FORMAT: &str = "ugf";

/// Converts a UGF vertex like `Dq` (where the row letter counts down from
/// the board size) into an SGF point like `dq`.
fn convert_vertex(ugf_vertex: &str, board_size: usize) -> Option<String> {
    let mut characters = ugf_vertex.chars();
    let column_char = characters.next()?;
    let row_char = characters.next()?;
    if characters.next().is_some() {
        return None;
    }
    if !column_char.is_ascii_uppercase() || !row_char.is_ascii_uppercase() {
        return None;
    }
    // The reference maps the row letter through
    // `fromCharCode(boardSize - code + 129)` (an uppercase char) and
    // lowercases the result: row = 64 + boardSize - code.
    let row = 64 + board_size as i32 - row_char as i32;
    if row < 0 || row >= board_size as i32 {
        return None;
    }
    Some(format_sgf_vertex(column_char as usize - 65, row as usize))
}

/// Parses UGF text into normalized SGF text.
pub fn parse(content: &str) -> Result<String, LegacyImportError> {
    let mut root: Vec<(String, String)> = vec![
        ("CA".to_owned(), "UTF-8".to_owned()),
        ("FF".to_owned(), "4".to_owned()),
        ("GM".to_owned(), "1".to_owned()),
        ("SZ".to_owned(), "19".to_owned()),
    ];
    let mut moves: Vec<String> = Vec::new();
    let mut comments: Vec<(usize, String)> = Vec::new();
    let mut current_mode: Option<String> = None;

    let lines: Vec<&str> = content.split('\n').map(str::trim).collect();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        if line.is_empty() {
            index += 1;
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            current_mode = Some(line[1..line.len() - 1].to_owned());
            index += 1;
            continue;
        }

        match current_mode.as_deref() {
            Some("Header") => {
                if let Some((key, value)) = line.split_once('=') {
                    match key {
                        "PlayerB" => {
                            let mut parts = value.split(',');
                            root.push((
                                "PB".to_owned(),
                                parts.next().unwrap_or_default().to_owned(),
                            ));
                            if let Some(rank) = parts.next() {
                                root.push(("BR".to_owned(), rank.to_owned()));
                            }
                        }
                        "PlayerW" => {
                            let mut parts = value.split(',');
                            root.push((
                                "PW".to_owned(),
                                parts.next().unwrap_or_default().to_owned(),
                            ));
                            if let Some(rank) = parts.next() {
                                root.push(("WR".to_owned(), rank.to_owned()));
                            }
                        }
                        "Size" => {
                            if let Some((_, current)) = root.iter_mut().find(|(key, _)| key == "SZ")
                            {
                                *current = value.to_owned();
                            } else {
                                root.push(("SZ".to_owned(), value.to_owned()));
                            }
                        }
                        "Hdcp" => {
                            let mut parts = value.split(',');
                            let handicap = parts.next().unwrap_or_default();
                            if handicap != "0" {
                                root.push(("HA".to_owned(), handicap.to_owned()));
                            }
                            if let Some(komi) = parts.next() {
                                root.push(("KM".to_owned(), komi.to_owned()));
                            }
                        }
                        "Rules" => root.push(("RU".to_owned(), value.to_owned())),
                        "Date" => root.push((
                            "DT".to_owned(),
                            value
                                .split(',')
                                .next()
                                .unwrap_or_default()
                                .replace('/', "-"),
                        )),
                        "Copyright" => root.push(("CP".to_owned(), value.to_owned())),
                        "Winner" => {
                            let mut parts = value.split(',');
                            let winner = parts.next().unwrap_or_default();
                            let margin = parts.next().unwrap_or_default();
                            root.push(("RE".to_owned(), format!("{winner}+{margin}")));
                        }
                        _ => {}
                    }
                }
            }
            Some("Data") => {
                let parts: Vec<&str> = line.split(',').collect();
                if parts.len() < 3 {
                    index += 1;
                    continue;
                }
                let coords = parts[0];
                let color = parts[1].chars().next().unwrap_or(' ');
                let node_num = parts[2].parse::<usize>().unwrap_or(0);
                let board_size = root
                    .iter()
                    .find(|(key, _)| key == "SZ")
                    .map(|(_, value)| value.parse::<usize>().unwrap_or(19))
                    .unwrap_or(19);
                let Some(vertex) = convert_vertex(coords, board_size) else {
                    index += 1;
                    continue;
                };
                if node_num > 0 {
                    moves.push(format!(";{color}[{vertex}]"));
                } else {
                    root.push(("AB".to_owned(), vertex));
                }
            }
            Some("ReviewComment") => {
                if line.starts_with(".Comment") {
                    let parts: Vec<&str> = line.split(',').collect();
                    if let Ok(node_num) = parts[1].parse::<usize>() {
                        let mut comment = String::new();
                        while index + 1 < lines.len() && !lines[index + 1].starts_with(".Comment") {
                            index += 1;
                            comment.push_str(lines[index].trim());
                            comment.push('\n');
                        }
                        comments.push((node_num.saturating_sub(1), comment));
                    }
                }
            }
            _ => {}
        }
        index += 1;
    }

    if moves.is_empty() {
        return Err(LegacyImportError::NoMoves(FORMAT));
    }

    let mut body = moves.join("");
    // Comments attach to the node of the referenced move (1-based nodeNum).
    for (node_num, comment) in comments {
        if node_num == 0 {
            root.push(("C".to_owned(), comment));
        } else if let Some((start, end)) = find_node_range(&body, node_num) {
            body.insert_str(
                end,
                &format!("C[{}]", escape_sgf_value(&comment.trim_end())),
            );
            let _ = start;
        }
    }

    let root_properties = root
        .into_iter()
        .map(|(key, value)| format!("{key}[{}]", escape_sgf_value(&value)))
        .collect::<Vec<_>>()
        .join("");
    Ok(format!("(;{root_properties}{body})"))
}

/// Finds the byte range of the `node_num`-th move node (1-based) in the SGF
/// body. Returns the range of the `;` prefix plus the node text.
fn find_node_range(body: &str, node_num: usize) -> Option<(usize, usize)> {
    let mut count = 0;
    let mut start = None;
    for (index, character) in body.char_indices() {
        if character == ';' {
            count += 1;
            if count == node_num {
                start = Some(index);
            } else if count > node_num {
                return start.map(|start| (start, index));
            }
        }
    }
    start.map(|start| (start, body.len()))
}

#[cfg(test)]
mod tests {
    use super::{convert_vertex, parse};

    const SAMPLE: &str = "\
[Header]
PlayerB=Alpha,5d
PlayerW=Beta,1k
Size=19
Hdcp=2,0.5
Rules=Chinese
Date=2008/05/01
Winner=B,4.5
[Data]
DP,B,0,0
EP,B,1,0
FP,W,2,0
GP,B,3,0
[ReviewComment]
.Comment,2,0,0,0
A nice move.
";

    #[test]
    fn parses_header_data_and_comments_into_sgf() {
        let sgf = parse(SAMPLE).expect("sample UGF parses");
        assert!(sgf.starts_with("(;CA[UTF-8]FF[4]GM[1]SZ[19]"));
        assert!(sgf.contains("PB[Alpha]BR[5d]"));
        assert!(sgf.contains("PW[Beta]WR[1k]"));
        assert!(sgf.contains("HA[2]"));
        assert!(sgf.contains("KM[0.5]"));
        assert!(sgf.contains("RU[Chinese]"));
        assert!(sgf.contains("DT[2008-05-01]"));
        assert!(sgf.contains("RE[B+4.5]"));
        assert!(
            sgf.contains("AB[dd]"),
            "nodeNum 0 becomes a handicap stone: {sgf}"
        );
        assert!(sgf.contains(";B[ed]C[A nice move.];W[fd];B[gd]"));
        // The comment attaches right after the 2nd move node (1-based
        // nodeNum 2), before the 3rd move.
        let comment_start = sgf.find("C[A nice move.]").expect("comment is present");
        let first_move = sgf.find(";B[ed]").expect("first move is present");
        let third_move = sgf.find(";B[gd]").expect("third move is present");
        assert!(comment_start > first_move);
        assert!(comment_start < third_move);
    }

    #[test]
    fn converts_ugf_vertices() {
        assert_eq!(convert_vertex("DP", 19), Some("dd".to_owned()));
        assert_eq!(convert_vertex("QP", 19), Some("qd".to_owned()));
        assert_eq!(convert_vertex("DA", 19), Some("ds".to_owned()));
        assert_eq!(convert_vertex("Dq", 19), None);
        assert_eq!(convert_vertex("D?", 19), None);
    }

    #[test]
    fn missing_move_lines_are_rejected() {
        assert!(parse("[Header]\nPlayerB=A\n").is_err());
    }
}
