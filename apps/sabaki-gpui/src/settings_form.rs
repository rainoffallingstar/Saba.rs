//! Key-table driven settings form for the GPUI client.
//!
//! The host key table (`sabaki_host::setting_kind`) is the single source of
//! truth for which keys exist and what values they accept; this module turns
//! that schema into UI rows and applies user edits through the validated
//! `SettingsStore`. All persistence happens through the shared
//! `persist_settings_store` workflow.

use sabaki_host::{SettingKind, SettingsStore, setting_kind};
use serde_json::Value;

/// The setting keys surfaced in the shell settings panel. This is the
/// high-frequency subset of the full Electron key table; unknown keys are
/// never rendered and never written.
pub const PANEL_SETTING_KEYS: &[&str] = &[
    "board.show_analysis",
    "view.show_coordinates",
    "view.show_move_numbers",
    "view.show_comments",
    "view.show_graph",
    "gtp.console_log_enabled",
    "game.default_board_size",
    "game.default_komi",
    "game.default_handicap",
    "engines.analyze_commands",
];

/// A single rendered settings row: the key, a human label, the value kind from
/// the host key table, and the current value in the store.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingRow {
    pub key: String,
    pub label: &'static str,
    pub kind: SettingKind,
    pub value: Option<Value>,
}

/// Builds the panel rows from the current store. Keys missing from the host
/// key table are skipped, so a stale panel list can never render an
/// unvalidatable control.
pub fn panel_setting_rows(store: &SettingsStore) -> Vec<SettingRow> {
    PANEL_SETTING_KEYS
        .iter()
        .filter_map(|key| {
            let kind = setting_kind(key)?;
            Some(SettingRow {
                key: (*key).to_owned(),
                label: setting_label(key),
                kind,
                value: store.get(key).cloned(),
            })
        })
        .collect()
}

/// A user-requested settings change, produced by the UI controls.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SettingEdit {
    Set { key: String, value: Value },
    Clear { key: String },
}

impl SettingEdit {
    pub fn key(&self) -> &str {
        match self {
            SettingEdit::Set { key, .. } | SettingEdit::Clear { key } => key,
        }
    }
}

/// Builds the toggle edit for a boolean row from its current value.
pub fn toggle_boolean_edit(row: &SettingRow) -> SettingEdit {
    let current = row.value.as_ref().and_then(Value::as_bool).unwrap_or(false);
    SettingEdit::Set {
        key: row.key.clone(),
        value: Value::Bool(!current),
    }
}

/// Parses a number row's text input into a number edit. Empty or
/// non-numeric text is rejected before it reaches the store.
pub fn number_edit(key: &str, text: &str) -> Result<SettingEdit, String> {
    let text = text.trim();
    let parsed = text
        .parse::<f64>()
        .map_err(|_| format!("{text:?} is not a valid number"))?;
    Ok(SettingEdit::Set {
        key: key.to_owned(),
        value: serde_json::json!(parsed),
    })
}

/// Splits a string-array row's text input on commas, trimming entries and
/// dropping empties, so "a, b, ,c" becomes ["a", "b", "c"].
pub fn string_array_edit(key: &str, text: &str) -> SettingEdit {
    let values: Vec<String> = text
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(ToOwned::to_owned)
        .collect();
    SettingEdit::Set {
        key: key.to_owned(),
        value: serde_json::json!(values),
    }
}

/// Applies an edit to the store. The host validates the value against the key
/// table, so wrong types are rejected instead of silently stored.
pub fn apply_setting_edit(store: &mut SettingsStore, edit: SettingEdit) -> Result<(), String> {
    match edit {
        SettingEdit::Set { key, value } => store
            .set(&key, value)
            .map(|_| ())
            .map_err(|error| error.to_string()),
        SettingEdit::Clear { key } => {
            store.remove(&key);
            Ok(())
        }
    }
}

/// Formats a setting value for display; missing values render as "—".
pub fn display_setting_value(value: Option<&Value>) -> String {
    match value {
        Some(value) => value.to_string(),
        None => "—".to_owned(),
    }
}

/// Human-readable label for a panel key; unknown keys (never rendered) fall
/// back to the key itself.
pub fn setting_label(key: &str) -> &'static str {
    match key {
        "board.show_analysis" => "Show analysis",
        "view.show_coordinates" => "Show coordinates",
        "view.show_move_numbers" => "Show move numbers",
        "view.show_comments" => "Show comments",
        "view.show_graph" => "Show game graph",
        "gtp.console_log_enabled" => "Log GTP console",
        "game.default_board_size" => "Default board size",
        "game.default_komi" => "Default komi",
        "game.default_handicap" => "Default handicap",
        "engines.analyze_commands" => "Analyze commands",
        _ => "Setting",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        PANEL_SETTING_KEYS, SettingEdit, SettingRow, apply_setting_edit, display_setting_value,
        number_edit, panel_setting_rows, setting_label, string_array_edit, toggle_boolean_edit,
    };
    use sabaki_host::{SettingKind, SettingsStore, setting_kind};
    use serde_json::json;

    #[test]
    fn panel_rows_cover_the_high_frequency_keys_with_host_kinds() {
        let store = SettingsStore::default();
        let rows = panel_setting_rows(&store);

        assert_eq!(rows.len(), PANEL_SETTING_KEYS.len());
        for row in rows {
            assert_eq!(setting_kind(&row.key), Some(row.kind));
            assert!(row.label.len() > 3, "labels are human-readable");
            assert_eq!(row.value, None);
        }
    }

    #[test]
    fn panel_rows_reflect_stored_values() {
        let mut store = SettingsStore::default();
        store
            .set("view.show_graph", json!(false))
            .expect("boolean value is valid");
        store
            .set("game.default_komi", json!(6.5))
            .expect("number value is valid");

        let rows = panel_setting_rows(&store);
        let graph = rows
            .iter()
            .find(|row| row.key == "view.show_graph")
            .expect("graph row exists");
        let komi = rows
            .iter()
            .find(|row| row.key == "game.default_komi")
            .expect("komi row exists");

        assert_eq!(graph.value, Some(json!(false)));
        assert_eq!(komi.value, Some(json!(6.5)));
    }

    #[test]
    fn boolean_toggle_flips_the_current_value() {
        let row = SettingRow {
            key: "view.show_graph".to_owned(),
            label: "Show game graph",
            kind: SettingKind::Boolean,
            value: Some(json!(false)),
        };
        assert_eq!(
            toggle_boolean_edit(&row),
            SettingEdit::Set {
                key: "view.show_graph".to_owned(),
                value: json!(true),
            }
        );

        let mut store = SettingsStore::default();
        apply_setting_edit(&mut store, toggle_boolean_edit(&row)).expect("toggle applies");
        assert_eq!(store.get_bool("view.show_graph"), Some(true));
    }

    #[test]
    fn number_input_parses_and_rejects_garbage() {
        assert_eq!(
            number_edit("game.default_komi", "6.5").unwrap().key(),
            "game.default_komi"
        );
        assert!(number_edit("game.default_komi", "abc").is_err());
        assert!(number_edit("game.default_komi", "").is_err());
        assert!(number_edit("game.default_komi", "  ").is_err());
    }

    #[test]
    fn string_array_input_splits_and_trims_entries() {
        let edit = string_array_edit("engines.analyze_commands", "a, b, ,c");
        assert_eq!(
            edit,
            SettingEdit::Set {
                key: "engines.analyze_commands".to_owned(),
                value: json!(["a", "b", "c"]),
            }
        );
    }

    #[test]
    fn wrong_types_are_rejected_by_the_store() {
        let mut store = SettingsStore::default();
        let error = apply_setting_edit(
            &mut store,
            SettingEdit::Set {
                key: "sound.enable".to_owned(),
                value: json!("yes"),
            },
        )
        .expect_err("a string is not a boolean");

        assert!(error.contains("sound.enable"));
        assert_eq!(store.get("sound.enable"), None);
    }

    #[test]
    fn clearing_a_nullable_string_removes_the_key() {
        let mut store = SettingsStore::default();
        store
            .set("theme.custom_board", json!("/themes/board.png"))
            .expect("nullable string value is valid");

        apply_setting_edit(
            &mut store,
            SettingEdit::Clear {
                key: "theme.custom_board".to_owned(),
            },
        )
        .expect("clearing succeeds");

        assert_eq!(store.get("theme.custom_board"), None);
    }

    #[test]
    fn display_values_cover_missing_and_present_states() {
        assert_eq!(display_setting_value(None), "—");
        assert_eq!(display_setting_value(Some(&json!(true))), "true");
        assert_eq!(display_setting_value(Some(&json!(6.5))), "6.5");
        assert_eq!(
            display_setting_value(Some(&json!("classic"))),
            "\"classic\""
        );
    }

    #[test]
    fn labels_exist_for_every_panel_key() {
        for key in PANEL_SETTING_KEYS {
            assert_ne!(setting_label(key), "Setting");
        }
    }
}
