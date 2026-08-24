//! Tygem GIB importer.
//!
//! Mirrors the Electron reference (`src/modules/fileformats/gib.js`): `\[`
//! escaped tag lines carry players, result and komi; `INI` lines carry the
//! handicap; `STO` lines carry the moves.

use crate::legacy::{LegacyImportError, escape_sgf_value, format_sgf_vertex};

const FORMAT: &str = "gib";

/// Maps a GIB game-result type and score (multiplied by 10) to an SGF `RE`.
fn make_result(grlt: i64, zipsu: i64) -> String {
    match grlt {
        3 => "B+R".to_owned(),
        4 => "W+R".to_owned(),
        7 => "B+T".to_owned(),
        8 => "W+T".to_owned(),
        0 | 1 => {
            let winner = if grlt == 0 { "B" } else { "W" };
            format!("{winner}+{}", zipsu as f64 / 10.0)
        }
        _ => String::new(),
    }
}

/// Splits a raw player entry like `Name (rank)` into name and rank.
fn parse_player_name(raw: &str) -> (String, String) {
    if let Some(open) = raw.find('(')
        && raw.ends_with(')')
        && raw[open..].chars().filter(|c| *c == ')').count() == 1
    {
        let name = raw[..open].trim().to_owned();
        let rank = raw[open + 1..raw.len() - 1].to_owned();
        if !name.is_empty() {
            return (name, rank);
        }
    }
    (raw.to_owned(), String::new())
}

/// Parses GIB text into normalized SGF text.
pub fn parse(content: &str) -> Result<String, LegacyImportError> {
    let mut root: Vec<(String, String)> = vec![
        ("CA".to_owned(), "UTF-8".to_owned()),
        ("FF".to_owned(), "4".to_owned()),
        ("GM".to_owned(), "1".to_owned()),
        ("SZ".to_owned(), "19".to_owned()),
    ];
    let mut moves: Vec<String> = Vec::new();

    for line in content.split('\n') {
        let line = line.trim();

        if line.starts_with("\\[GAMEBLACKNAME=") && line.ends_with("\\]") {
            let (name, rank) = parse_player_name(&line[16..line.len() - 2]);
            if !name.is_empty() {
                root.push(("PB".to_owned(), name));
            }
            if !rank.is_empty() {
                root.push(("BR".to_owned(), rank));
            }
        } else if line.starts_with("\\[GAMEWHITENAME=") && line.ends_with("\\]") {
            let (name, rank) = parse_player_name(&line[16..line.len() - 2]);
            if !name.is_empty() {
                root.push(("PW".to_owned(), name));
            }
            if !rank.is_empty() {
                root.push(("WR".to_owned(), rank));
            }
        } else if line.starts_with("\\[GAMEINFOMAIN=") {
            if !root.iter().any(|(key, _)| key == "RE")
                && let Some(result) = find_result(line, "GRLT:", ",", "ZIPSU:", ",")
            {
                root.push(("RE".to_owned(), result));
            }
            if !root.iter().any(|(key, _)| key == "KM")
                && let Some(value) = find_numeric(line, "GONGJE:", ",")
            {
                root.push(("KM".to_owned(), (value as f64 / 10.0).to_string()));
            }
        } else if line.starts_with("\\[GAMETAG=") {
            if !root.iter().any(|(key, _)| key == "DT") {
                // C(YYYY):(MM):(DD)
                if let Some(rest) = line.split_once('C')
                    && let Some(date) = rest.1.get(..10)
                    && date.len() == 10
                    && date.as_bytes()[4] == b':'
                    && date.as_bytes()[7] == b':'
                {
                    root.push((
                        "DT".to_owned(),
                        format!("{}-{}-{}", &date[..4], &date[5..7], &date[8..10]),
                    ));
                }
            }
            if !root.iter().any(|(key, _)| key == "RE")
                && let Some(result) = find_result(line, ",W", ",", ",Z", ",")
            {
                root.push(("RE".to_owned(), result));
            }
            if !root.iter().any(|(key, _)| key == "KM")
                && let Some(value) = find_numeric(line, ",G", ",")
            {
                root.push(("KM".to_owned(), (value as f64 / 10.0).to_string()));
            }
        } else if line.starts_with("INI") {
            let setup: Vec<&str> = line.split(' ').collect();
            let handicap = setup
                .get(3)
                .and_then(|value| value.parse::<f64>().ok())
                .map(|value| value.floor() as usize)
                .unwrap_or(0);
            if (2..=9).contains(&handicap) {
                root.push(("HA".to_owned(), handicap.to_string()));
                for (column, row) in crate::legacy::tygem_handicap_placement(19, handicap) {
                    root.push(("AB".to_owned(), format_sgf_vertex(column, row)));
                }
            }
        } else if line.starts_with("STO") {
            let elements: Vec<&str> = line.split(' ').collect();
            if elements.len() >= 6 {
                let key = if elements[3] == "1" { "B" } else { "W" };
                if let (Ok(x), Ok(y)) = (
                    elements[4].parse::<f64>().map(|v| v.floor() as usize),
                    elements[5].parse::<f64>().map(|v| v.floor() as usize),
                ) {
                    moves.push(format!(";{key}[{}]", format_sgf_vertex(x, y)));
                }
            }
        }
    }

    if moves.is_empty() {
        return Err(LegacyImportError::NoMoves(FORMAT));
    }

    let root_properties = root
        .into_iter()
        .map(|(key, value)| format!("{key}[{}]", escape_sgf_value(&value)))
        .collect::<Vec<_>>()
        .join("");
    Ok(format!("(;{root_properties}{})", moves.join("")))
}

/// Finds `prefix<number>,` in the line and returns the parsed number.
fn find_numeric(line: &str, prefix: &str, suffix: &str) -> Option<i64> {
    let rest = line.split_once(prefix)?.1;
    let value = rest.split_once(suffix)?.0;
    value.parse().ok()
}

/// Extracts a result from `grltPrefix<grlt>,` and `zipsuPrefix<zipsu>,`.
fn find_result(
    line: &str,
    grlt_prefix: &str,
    grlt_suffix: &str,
    zipsu_prefix: &str,
    zipsu_suffix: &str,
) -> Option<String> {
    let grlt = find_numeric(line, grlt_prefix, grlt_suffix)?;
    let zipsu = find_numeric(line, zipsu_prefix, zipsu_suffix)?;
    let result = make_result(grlt, zipsu);
    (!result.is_empty()).then_some(result)
}

#[cfg(test)]
mod tests {
    use super::{find_result, make_result, parse, parse_player_name};

    const SAMPLE: &str = "\
\\[GAMEBLACKNAME=Alpha (5d)\\]
\\[GAMEWHITENAME=Beta (1k)\\]
\\[GAMEINFOMAIN=GRLT:0,ZIPSU:45,GONGJE:65,NDRN:1,\\]
\\[GAMETAG=C2008:05:01,NDRN:1,W7,G7,\\]
INI 0 1 2 0 0 1
STO 0 2 1 3 3
STO 0 3 2 15 3
STO 0 4 1 3 15
";

    #[test]
    fn parses_players_result_komi_date_and_moves() {
        let sgf = parse(SAMPLE).expect("sample GIB parses");
        assert!(sgf.starts_with("(;CA[UTF-8]FF[4]GM[1]SZ[19]"));
        assert!(sgf.contains("PB[Alpha]BR[5d]"));
        assert!(sgf.contains("PW[Beta]WR[1k]"));
        assert!(sgf.contains("RE[B+4.5]"), "GRLT 0 is B + zipsu/10: {sgf}");
        assert!(sgf.contains("KM[6.5]"), "GONGJE 65 / 10: {sgf}");
        assert!(sgf.contains("DT[2008-05-01]"));
        assert!(
            sgf.contains("HA[2]AB[dp]AB[pd]"),
            "2-stone tygem placement: {sgf}"
        );
        assert!(
            sgf.contains(";B[dd];W[pd];B[dp]"),
            "STO moves (1=black, 2=white): {sgf}"
        );
    }

    #[test]
    fn result_mapping_covers_resign_and_time() {
        assert_eq!(make_result(3, 0), "B+R");
        assert_eq!(make_result(4, 0), "W+R");
        assert_eq!(make_result(7, 0), "B+T");
        assert_eq!(make_result(8, 0), "W+T");
        assert_eq!(make_result(1, 45), "W+4.5");
        assert_eq!(make_result(2, 0), "");
        assert_eq!(
            find_result(",W3,Z45,", ",W", ",", ",Z", ","),
            Some("B+R".to_owned())
        );
    }

    #[test]
    fn player_names_without_ranks_are_kept_whole() {
        assert_eq!(
            parse_player_name("Alpha"),
            ("Alpha".to_owned(), String::new())
        );
        assert_eq!(
            parse_player_name("Alpha (5d)"),
            ("Alpha".to_owned(), "5d".to_owned())
        );
    }
}
