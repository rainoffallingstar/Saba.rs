use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::Vertex;

/// Opening convention controls setup before the first move. It is separate
/// from scoring rules: ancient Chinese scoring may be used without seat stones,
/// and seat stones may be represented explicitly in SGF as AB setup points.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OpeningConvention {
    #[default]
    Free,
    ChineseAncientSeatStones,
}

impl OpeningConvention {
    pub const fn setting_value(self) -> &'static str {
        match self {
            Self::Free => "free",
            Self::ChineseAncientSeatStones => "chineseAncientSeatStones",
        }
    }

    pub fn from_setting(value: Option<&str>) -> Self {
        match value.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
            Some("chineseancientseatstones")
            | Some("ancientchinese")
            | Some("seatstones")
            | Some("座子")
            | Some("座子制") => Self::ChineseAncientSeatStones,
            _ => Self::Free,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Free => "自由布局",
            Self::ChineseAncientSeatStones => "中国古谱座子制",
        }
    }

    /// Returns the four traditional corner setup points for a square board.
    /// 19x19/13x13 use the 4-4 points; 9x9 uses the 3-3 points.
    pub fn seat_stones(self, board_size: usize) -> Vec<Vertex> {
        if !matches!(self, Self::ChineseAncientSeatStones) || board_size < 3 {
            return Vec::new();
        }
        let distance = if board_size >= 13 { 3 } else { 2 };
        let far = board_size - 1 - distance;
        [
            Vertex {
                column: distance,
                row: distance,
            },
            Vertex {
                column: far,
                row: distance,
            },
            Vertex {
                column: distance,
                row: far,
            },
            Vertex {
                column: far,
                row: far,
            },
        ]
        .into_iter()
        .collect()
    }

    /// Adds the convention's setup stones to a fresh SGF root without
    /// overwriting unrelated root properties. Only the `AB` (add black stones)
    /// list is touched: existing `AB` values are retained so an explicit
    /// handicap setup remains user-visible and serializable, and other root
    /// properties such as `RU` / `KM` are left untouched. Scoring and komi
    /// are separate concerns handled by the caller.
    pub fn apply_to_root_properties(
        self,
        board_size: usize,
        properties: &mut BTreeMap<String, Vec<String>>,
        mut format_vertex: impl FnMut(Vertex) -> String,
    ) {
        let stones = self.seat_stones(board_size);
        if stones.is_empty() {
            return;
        }
        let entries = properties.entry("AB".to_owned()).or_default();
        for stone in stones {
            let value = format_vertex(stone);
            if !entries.contains(&value) {
                entries.push(value);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_four_four_points_on_standard_boards() {
        let points = OpeningConvention::ChineseAncientSeatStones.seat_stones(19);
        assert_eq!(points[0], Vertex { column: 3, row: 3 });
        assert_eq!(
            points[3],
            Vertex {
                column: 15,
                row: 15
            }
        );
    }

    #[test]
    fn uses_three_three_points_on_nine_by_nine() {
        let points = OpeningConvention::ChineseAncientSeatStones.seat_stones(9);
        assert_eq!(
            points,
            vec![
                Vertex { column: 2, row: 2 },
                Vertex { column: 6, row: 2 },
                Vertex { column: 2, row: 6 },
                Vertex { column: 6, row: 6 },
            ]
        );
    }

    #[test]
    fn root_setup_is_idempotent_and_only_sets_ab() {
        let mut properties = BTreeMap::new();
        OpeningConvention::ChineseAncientSeatStones.apply_to_root_properties(
            19,
            &mut properties,
            |vertex| format!("{}{}", vertex.column, vertex.row),
        );
        OpeningConvention::ChineseAncientSeatStones.apply_to_root_properties(
            19,
            &mut properties,
            |vertex| format!("{}{}", vertex.column, vertex.row),
        );
        assert_eq!(properties["AB"].len(), 4);
        // Seat stones must not inject scoring metadata; only AB is written.
        assert!(!properties.contains_key("RU"));
        assert!(!properties.contains_key("KM"));
    }

    #[test]
    fn root_setup_does_not_overwrite_existing_ru_and_km() {
        let mut properties = BTreeMap::new();
        properties.insert("RU".to_owned(), vec!["japanese".to_owned()]);
        properties.insert("KM".to_owned(), vec!["6.5".to_owned()]);
        properties.insert("AB".to_owned(), vec!["cc".to_owned()]);
        OpeningConvention::ChineseAncientSeatStones.apply_to_root_properties(
            19,
            &mut properties,
            |vertex| format!("{}{}", vertex.column, vertex.row),
        );
        // Pre-existing RU / KM survive untouched.
        assert_eq!(properties["RU"], vec!["japanese"]);
        assert_eq!(properties["KM"], vec!["6.5"]);
        // Existing AB is retained and the four corner stones are appended.
        assert_eq!(properties["AB"], vec!["cc", "33", "153", "315", "1515"]);
    }
}
