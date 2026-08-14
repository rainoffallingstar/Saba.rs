//! Legacy game format importers (wBaduk NGF, Tygem GIB, PandaNET UGF).
//!
//! Each importer parses its text format into a normalized SGF document (a
//! single `GameDocument` line), so the domain keeps one canonical tree
//! representation and the importers stay pure text → text functions. The
//! resulting SGF is fed through the existing `GameDocument::from_sgf` for
//! full validation.
//!
//! Behavior mirrors the Electron reference (`src/modules/fileformats/*.js`),
//! including the Tygem handicap placement order.

use std::path::Path;

use thiserror::Error;

pub mod gib;
pub mod ngf;
pub mod ugf;

/// Formats a zero-based vertex as an SGF point like `dd` (lowercase).
pub fn format_sgf_vertex(column: usize, row: usize) -> String {
    let column_char = char::from_u32((b'a' + column as u8) as u32).unwrap_or('a');
    let row_char = char::from_u32((b'a' + row as u8) as u32).unwrap_or('a');
    format!("{column_char}{row_char}")
}

/// Escapes an SGF property value: `]` becomes `\]` and `\` becomes `\\`.
pub fn escape_sgf_value(value: &str) -> String {
    value.replace('\\', "\\\\").replace(']', "\\]")
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum LegacyImportError {
    #[error("unsupported legacy format extension {0:?}")]
    UnsupportedExtension(String),
    #[error("the {0} file has no usable move data")]
    NoMoves(&'static str),
}

/// Imports a legacy-format text by its file extension (`ngf`, `gib`, `ugf`)
/// into normalized SGF text. Unknown extensions are rejected; callers decide
/// when a file should be routed here.
pub fn import_by_extension(extension: &str, content: &str) -> Result<String, LegacyImportError> {
    match extension.to_ascii_lowercase().as_str() {
        "ngf" => ngf::parse(content),
        "gib" => gib::parse(content),
        "ugf" => ugf::parse(content),
        other => Err(LegacyImportError::UnsupportedExtension(other.to_owned())),
    }
}

/// The extension of `path` without the dot, lowercased.
pub fn file_extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
}

/// Tygem handicap placement points for a square board, mirroring
/// `@sabaki/go-board`'s `getHandicapPlacement(count, {tygem: true})`.
/// Returns at most `count` points as `(column, row)`.
pub fn tygem_handicap_placement(size: usize, count: usize) -> Vec<(usize, usize)> {
    if size <= 6 || count < 2 {
        return Vec::new();
    }
    let near = if size >= 13 { 3 } else { 2 };
    let far = size - near - 1;
    let middle = (size - 1) / 2;

    let mut result = vec![(near, far), (far, near), (near, near), (far, far)];

    // For square boards, the reference's width-only / height-only branches
    // reduce to this: odd sizes (except 7) add the middle line/column points.
    if size % 2 != 0 && size != 7 {
        if count == 5 {
            result.push((middle, middle));
        }
        result.extend([(near, middle), (far, middle)]);

        if count == 7 {
            result.push((middle, middle));
        }
        result.extend([(middle, near), (middle, far), (middle, middle)]);
    }

    result.truncate(count);
    result
}

#[cfg(test)]
mod tests {
    use super::tygem_handicap_placement;

    #[test]
    fn tygem_handicap_placements_match_the_reference_order() {
        assert_eq!(tygem_handicap_placement(19, 2), vec![(3, 15), (15, 3)]);
        assert_eq!(
            tygem_handicap_placement(19, 4),
            vec![(3, 15), (15, 3), (3, 3), (15, 15)]
        );
        assert_eq!(
            tygem_handicap_placement(19, 5),
            vec![(3, 15), (15, 3), (3, 3), (15, 15), (9, 9)]
        );
        assert_eq!(
            tygem_handicap_placement(19, 7),
            vec![(3, 15), (15, 3), (3, 3), (15, 15), (3, 9), (15, 9), (9, 9)]
        );
        assert_eq!(
            tygem_handicap_placement(19, 9),
            vec![
                (3, 15),
                (15, 3),
                (3, 3),
                (15, 15),
                (3, 9),
                (15, 9),
                (9, 3),
                (9, 15),
                (9, 9)
            ]
        );
        assert_eq!(tygem_handicap_placement(7, 2), vec![(2, 4), (4, 2)]);
        assert!(tygem_handicap_placement(6, 2).is_empty());
        assert!(tygem_handicap_placement(19, 1).is_empty());
    }
}
