use serde::{Deserialize, Serialize};

/// Named whole-game review budgets. These are deliberately separate from the
/// live analysis max-visits setting: a review is a reproducible batch job,
/// while live analysis is an interactive search.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReviewProfile {
    Quick,
    #[default]
    Preliminary,
    Intermediate,
    Advanced,
}

impl ReviewProfile {
    pub const ALL: [Self; 4] = [
        Self::Quick,
        Self::Preliminary,
        Self::Intermediate,
        Self::Advanced,
    ];

    pub const fn visits(self) -> u64 {
        match self {
            Self::Quick => 50,
            Self::Preliminary => 800,
            Self::Intermediate => 2_500,
            Self::Advanced => 10_000,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Quick => "快速复盘",
            Self::Preliminary => "初步复盘",
            Self::Intermediate => "中级复盘",
            Self::Advanced => "高级复盘",
        }
    }

    pub const fn english_label(self) -> &'static str {
        match self {
            Self::Quick => "Quick",
            Self::Preliminary => "Preliminary",
            Self::Intermediate => "Intermediate",
            Self::Advanced => "Advanced",
        }
    }

    pub const fn from_visits(visits: u64) -> Option<Self> {
        match visits {
            50 => Some(Self::Quick),
            800 => Some(Self::Preliminary),
            2_500 => Some(Self::Intermediate),
            10_000 => Some(Self::Advanced),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ReviewProfile;

    #[test]
    fn exposes_the_four_product_review_budgets() {
        assert_eq!(
            ReviewProfile::ALL.map(ReviewProfile::visits),
            [50, 800, 2_500, 10_000]
        );
        assert_eq!(
            ReviewProfile::from_visits(2_500),
            Some(ReviewProfile::Intermediate)
        );
        assert_eq!(ReviewProfile::from_visits(500), None);
    }
}
