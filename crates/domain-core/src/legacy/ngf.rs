//! wBaduk NGF importer.
//!
//! Mirrors the Electron reference (`src/modules/fileformats/ngf.js`): the
//! header lines carry board size, players, handicap, komi, date and result;
//! `PM` lines carry the moves with `B` as the lowest coordinate letter.

use crate::legacy::{LegacyImportError, escape_sgf_value, format_sgf_vertex};

const FORMAT: &str = "ngf";

/// Mimics JavaScript `parseFloat`: parses the leading numeric prefix of a
/// token ("0.5!" -> 0.5, "B+4.5" -> None since it starts with a letter).
fn parse_float_prefix(token: &str) -> Option<f64> {
    let prefix: String = token
        .chars()
        .take_while(|character| character.is_ascii_digit() || matches!(character, '.' | '-' | '+'))
        .collect();
    if prefix.is_empty() || prefix == "-" || prefix == "+" || prefix == "." {
        return None;
    }
    prefix.parse().ok()
}

/// Parses NGF text into normalized SGF text.
pub fn parse(content: &str) -> Result<String, LegacyImportError> {
    let lines: Vec<&str> = content.split('\n').collect();
    if lines.len() < 8 {
        return Err(LegacyImportError::NoMoves(FORMAT));
    }

    let boardsize = lines
        .get(1)
        .and_then(|line| line.trim().parse::<f64>().ok())
        .map(|value| value.floor() as usize)
        .filter(|&size| (2..=25).contains(&size))
        .unwrap_or(19);
    let handicap = lines
        .get(5)
        .and_then(|line| line.trim().parse::<f64>().ok())
        .map(|value| value.floor() as usize)
        .unwrap_or(0);
    let komi = lines
        .get(7)
        .and_then(|line| line.trim().parse::<f64>().ok())
        .map(|value| value.floor())
        .unwrap_or(0.0);
    let pw = lines
        .get(2)
        .map(|line| line.trim().split(' ').next().unwrap_or("").to_owned())
        .unwrap_or_default();
    let pb = lines
        .get(3)
        .map(|line| line.trim().split(' ').next().unwrap_or("").to_owned())
        .unwrap_or_default();

    let mut komi = komi;
    if handicap == 0 && komi == komi.floor() {
        komi += 0.5;
    }

    let mut root: Vec<(String, String)> = vec![
        ("CA".to_owned(), "UTF-8".to_owned()),
        ("FF".to_owned(), "4".to_owned()),
        ("GM".to_owned(), "1".to_owned()),
        ("SZ".to_owned(), boardsize.to_string()),
    ];

    // Player ranks follow the name on lines 2/3, marked with 'K'/'D'/'DP'.
    if let Some(line) = lines.get(2) {
        let parts: Vec<&str> = line.trim().split(' ').collect();
        if parts.len() > 1 {
            let rank = parts[parts.len() - 1]
                .replace("DP", "p")
                .replace('K', "k")
                .replace('D', "d");
            root.push(("WR".to_owned(), rank));
        }
    }
    if let Some(line) = lines.get(3) {
        let parts: Vec<&str> = line.trim().split(' ').collect();
        if parts.len() > 1 {
            let rank = parts[parts.len() - 1]
                .replace("DP", "p")
                .replace('K', "k")
                .replace('D', "d");
            root.push(("BR".to_owned(), rank));
        }
    }

    if handicap >= 2 {
        root.push(("HA".to_owned(), handicap.to_string()));
        for (column, row) in crate::legacy::tygem_handicap_placement(boardsize, handicap) {
            root.push(("AB".to_owned(), format_sgf_vertex(column, row)));
        }
    }
    if komi != 0.0 {
        root.push(("KM".to_owned(), komi.to_string()));
    }

    // Result line: margin markers and winner detection.
    if let Some(line) = lines.get(10) {
        let mut margin = String::new();
        if line.contains("resign") {
            margin = "R".to_owned();
        }
        if line.contains("time") {
            margin = "T".to_owned();
        }
        let winner = if line.contains("hite win") || line.contains("lack lose") {
            "W"
        } else if line.contains("lack win") || line.contains("hite lose") {
            "B"
        } else {
            ""
        };
        if margin.is_empty() {
            if let Some(score) = line.split(' ').filter_map(parse_float_prefix).last() {
                margin = score.to_string();
            }
        }
        if !winner.is_empty() {
            root.push(("RE".to_owned(), format!("{winner}+{margin}")));
        }
    }

    // Date: YYYYMMDD on line 8.
    if let Some(line) = lines.get(8) {
        let raw = &line[..line.len().min(8)];
        if raw.len() == 8 && raw.chars().all(|character| character.is_ascii_digit()) {
            root.push((
                "DT".to_owned(),
                format!("{}-{}-{}", &raw[0..4], &raw[4..6], &raw[6..8]),
            ));
        }
    }

    root.push(("PW".to_owned(), pw));
    root.push(("PB".to_owned(), pb));

    // Moves: `PM` lines with the color at index 4 and coordinate letters at
    // 5/6, where 'B' is the lowest coordinate.
    let mut moves = Vec::new();
    for line in &lines {
        let line = line.trim();
        if line.len() >= 7 && line.starts_with("PM") {
            let key = line.chars().nth(4).unwrap_or(' ');
            if key == 'B' || key == 'W' {
                let x = line.chars().nth(5).map(|c| c as usize - 66);
                let y = line.chars().nth(6).map(|c| c as usize - 66);
                if let (Some(x), Some(y)) = (x, y) {
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

#[cfg(test)]
mod tests {
    use super::parse;

    const EVEN: &str = "\
Even Game
19
Player White 5d
Player Black 4d
www.cyberoro.com
0
0
7
20080501
5
Black wins by 0.5!
3
PM1BBCC
PM2AWDD
PM3BBEE
";

    const HANDICAP: &str = "\
Handicap Game
19
Player White 1k
Player Black 1d
www.cyberoro.com
2
0
0
20090101
5
White wins by resign!
3
PM1BBCC
PM2AWDD
PM3BBEE
";

    #[test]
    fn parses_an_even_game_into_sgf() {
        let sgf = parse(EVEN).expect("even NGF parses");
        assert!(sgf.starts_with("(;CA[UTF-8]FF[4]GM[1]SZ[19]"));
        assert!(sgf.contains("WR[5d]"), "white rank extracts: {sgf}");
        assert!(sgf.contains("BR[4d]"));
        assert!(
            sgf.contains("KM[7.5]"),
            "komi 7.5 floors to 7 then +0.5: {sgf}"
        );
        assert!(sgf.contains("DT[2008-05-01]"));
        assert!(sgf.contains("RE[B+0.5]"));
        assert!(sgf.contains("PW[Player]"));
        assert!(sgf.contains("PB[Player]"));
        assert!(sgf.contains(";B[bb];W[cc];B[dd]"), "PM lines: {sgf}");
        assert!(sgf.ends_with(")"));
    }

    #[test]
    fn parses_a_handicap_game_with_tygem_placements() {
        let sgf = parse(HANDICAP).expect("handicap NGF parses");
        assert!(sgf.contains("HA[2]"));
        assert!(
            sgf.contains("AB[dp]AB[pd]"),
            "2-stone tygem placement: {sgf}"
        );
        assert!(sgf.contains("RE[W+R]"));
        assert!(
            !sgf.contains("KM"),
            "handicap games with komi 0 omit KM: {sgf}"
        );
    }

    #[test]
    fn missing_move_lines_are_rejected() {
        assert!(parse("foo\nbar\n").is_err());
    }
}
