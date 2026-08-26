//! SGF Ruleset, Komi, and Handicap synchronization with GTP engines (KataGo).
//!
//! Recognizes Chinese, Japanese, Korean, AGA, and Tromp-Taylor rules from SGF `RU`
//! properties and matches them to official KataGo GTP `kata-set-rules` and `komi` commands.

use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GoRuleset {
    Chinese,
    Japanese,
    Korean,
    Aga,
    TrompTaylor,
    NewZealand,
}

impl GoRuleset {
    /// Canonical KataGo rules string used with `kata-set-rules`.
    pub fn katago_name(self) -> &'static str {
        match self {
            GoRuleset::Chinese => "chinese",
            GoRuleset::Japanese => "japanese",
            GoRuleset::Korean => "korean",
            GoRuleset::Aga => "aga",
            GoRuleset::TrompTaylor => "tromp-taylor",
            GoRuleset::NewZealand => "new-zealand",
        }
    }

    /// User-facing display label.
    pub fn label(self) -> &'static str {
        match self {
            GoRuleset::Chinese => "中国规则 (Chinese / 数子法)",
            GoRuleset::Japanese => "日本规则 (Japanese / 目数法)",
            GoRuleset::Korean => "韩国规则 (Korean)",
            GoRuleset::Aga => "AGA 规则 (American)",
            GoRuleset::TrompTaylor => "Tromp-Taylor 规则",
            GoRuleset::NewZealand => "新西兰规则 (New Zealand)",
        }
    }

    /// Default komi when SGF `KM` property is missing or unparseable.
    pub fn default_komi(self, handicap: usize) -> f64 {
        if handicap >= 2 {
            0.5
        } else {
            match self {
                GoRuleset::Chinese
                | GoRuleset::Aga
                | GoRuleset::TrompTaylor
                | GoRuleset::NewZealand => 7.5,
                GoRuleset::Japanese | GoRuleset::Korean => 6.5,
            }
        }
    }

    /// Parses an SGF `RU` property value into a standard Go ruleset.
    pub fn from_sgf_ru(ru: &str) -> Self {
        let trimmed = ru.trim().to_lowercase();
        if trimmed.contains("japan") || trimmed.contains("nihon") || trimmed.contains("territory") {
            GoRuleset::Japanese
        } else if trimmed.contains("korea") || trimmed.contains("hangul") {
            GoRuleset::Korean
        } else if trimmed.contains("aga") || trimmed.contains("american") {
            GoRuleset::Aga
        } else if trimmed.contains("tromp") || trimmed.contains("tt") {
            GoRuleset::TrompTaylor
        } else if trimmed.contains("nz") || trimmed.contains("zealand") {
            GoRuleset::NewZealand
        } else {
            // Default to Chinese (Area scoring) for modern AI engines
            GoRuleset::Chinese
        }
    }
}

/// Parsed game header parameters relevant to KataGo analysis configuration.
#[derive(Clone, Debug, PartialEq)]
pub struct GameRuleConfig {
    pub ruleset: GoRuleset,
    pub komi: f64,
    pub handicap: usize,
    pub board_size: usize,
}

impl GameRuleConfig {
    /// Extracts rule parameters from SGF root properties and board dimension.
    pub fn from_root_properties(
        root_properties: &BTreeMap<String, Vec<String>>,
        board_size: usize,
    ) -> Self {
        let handicap = root_properties
            .get("HA")
            .and_then(|vals| vals.first())
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(0);

        let ruleset = root_properties
            .get("RU")
            .and_then(|vals| vals.first())
            .map(|s| GoRuleset::from_sgf_ru(s))
            .unwrap_or(GoRuleset::Chinese);

        let komi = root_properties
            .get("KM")
            .and_then(|vals| vals.first())
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or_else(|| ruleset.default_komi(handicap));

        Self {
            ruleset,
            komi,
            handicap,
            board_size,
        }
    }

    /// Generates GTP configuration commands to set up KataGo for this game.
    pub fn to_gtp_setup_commands(&self) -> Vec<String> {
        vec![
            format!("boardsize {}", self.board_size),
            "clear_board".to_owned(),
            format!("kata-set-rules {}", self.ruleset.katago_name()),
            format!("komi {:.1}", self.komi),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_various_sgf_rules_strings() {
        assert_eq!(GoRuleset::from_sgf_ru("Chinese"), GoRuleset::Chinese);
        assert_eq!(GoRuleset::from_sgf_ru("Japanese"), GoRuleset::Japanese);
        assert_eq!(GoRuleset::from_sgf_ru("korean"), GoRuleset::Korean);
        assert_eq!(GoRuleset::from_sgf_ru("AGA"), GoRuleset::Aga);
        assert_eq!(
            GoRuleset::from_sgf_ru("Tromp-Taylor"),
            GoRuleset::TrompTaylor
        );
        assert_eq!(GoRuleset::from_sgf_ru("Unknown"), GoRuleset::Chinese);
    }

    #[test]
    fn extracts_config_and_generates_matching_gtp_commands() {
        let mut props = BTreeMap::new();
        props.insert("RU".to_owned(), vec!["Japanese".to_owned()]);
        props.insert("KM".to_owned(), vec!["6.5".to_owned()]);
        props.insert("HA".to_owned(), vec!["0".to_owned()]);

        let config = GameRuleConfig::from_root_properties(&props, 19);
        assert_eq!(config.ruleset, GoRuleset::Japanese);
        assert_eq!(config.komi, 6.5);
        assert_eq!(
            config.to_gtp_setup_commands(),
            vec![
                "boardsize 19".to_owned(),
                "clear_board".to_owned(),
                "kata-set-rules japanese".to_owned(),
                "komi 6.5".to_owned(),
            ]
        );
    }
}
