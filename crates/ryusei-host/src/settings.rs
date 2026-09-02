use std::collections::BTreeMap;

use serde_json::Value;

/// The value type a supported setting key must hold, mirroring the Electron
/// settings schema. This lives in the host so both the Tauri adapter and the
/// GPUI client validate persisted values with the same rules.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingKind {
    Boolean,
    Number,
    String,
    NullableString,
    StringArray,
}

/// A rejected setting value with a stable, machine-readable description.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingValidationError {
    pub key: String,
    pub expected: String,
    pub found: String,
}

impl std::fmt::Display for SettingValidationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "setting '{}' expects {}, found {}",
            self.key, self.expected, self.found
        )
    }
}

/// Returns the kind for a supported setting key, or `None` for unknown keys.
/// The key table is the single source of truth mirroring the Electron
/// `settings.json` schema; the Tauri adapter delegates to it.
pub fn setting_kind(key: &str) -> Option<SettingKind> {
    match key {
        "engines.list"
        | "edit.copy_variation_strip_props"
        | "edit.flatten_inherit_root_props"
        | "engines.analyze_commands"
        | "engines.gemove_analyze_commands"
        | "plugins.pinned"
        | "sgf.comment_properties" => Some(SettingKind::StringArray),
        "app.lang"
        | "board.analysis_type"
        | "board.analysis_value_type"
        | "board.variation_replay_mode"
        | "scoring.method"
        | "view.coordinates_type"
        | "view.move_numbers_type"
        | "game.opening_convention"
        | "game.default_ruleset"
        | "katago.human_sl_profile"
        | "profile.display_name"
        | "profile.current_goal"
        | "profile.current_plan"
        | "workspace.tabs"
        | "library.sources" => Some(SettingKind::String),
        "engines.analysis"
        | "engines.black"
        | "engines.white"
        | "gtp.console_log_path"
        | "theme.current"
        | "theme.custom_blackstones"
        | "theme.custom_whitestones"
        | "theme.custom_board"
        | "theme.custom_background" => Some(SettingKind::NullableString),
        "app.always_show_result"
        | "app.enable_hardware_acceleration"
        | "app.startup_check_updates"
        | "board.show_analysis"
        | "cleanmarkup.annotations"
        | "cleanmarkup.arrow"
        | "cleanmarkup.circle"
        | "cleanmarkup.comments"
        | "cleanmarkup.cross"
        | "cleanmarkup.hotspots"
        | "cleanmarkup.label"
        | "cleanmarkup.line"
        | "cleanmarkup.square"
        | "cleanmarkup.triangle"
        | "cleanmarkup.winrate"
        | "comments.show_move_interpretation"
        | "debug.dev_tools"
        | "edit.click_currentvertex_to_remove"
        | "edit.show_removenode_warning"
        | "edit.show_removeothervariations_warning"
        | "file.show_reload_warning"
        | "game.goto_end_after_loading"
        | "game.show_ko_warning"
        | "game.show_suicide_warning"
        | "gtp.console_log_enabled"
        | "sgf.format_code"
        | "sound.enable"
        | "view.animated_stone_placement"
        | "view.fuzzy_stone_placement"
        | "view.show_menubar"
        | "view.show_leftsidebar"
        | "view.show_analysis_preview"
        | "view.show_comments"
        | "view.show_coordinates"
        | "view.show_graph"
        | "view.show_move_colorization"
        | "view.show_move_numbers"
        | "view.show_next_moves"
        | "view.show_siblings"
        | "view.show_winrategraph"
        | "view.winrategraph_invert"
        | "katago.human_sl_enabled"
        | "window.maximized"
        | "review.analyze_during_game"
        | "library.redistribution_allowed" => Some(SettingKind::Boolean),
        "app.hide_busy_delay"
        | "app.loadgame_delay"
        | "app.startup_check_updates_delay"
        | "app.zoom_factor"
        | "autoplay.max_sec_per_move"
        | "autoplay.sec_per_move"
        | "autoscroll.delay"
        | "autoscroll.diff"
        | "autoscroll.max_interval"
        | "autoscroll.min_interval"
        | "board.analysis_interval"
        | "board.variation_replay_interval"
        | "engines.analysis_max_visits"
        | "comments.commit_delay"
        | "console.max_history_count"
        | "edit.history_batch_interval"
        | "edit.max_history_count"
        | "find.delay"
        | "game.default_board_size"
        | "game.default_komi"
        | "game.default_handicap"
        | "game.navigation_analysis_delay"
        | "game.navigation_sensitivity"
        | "gamechooser.show_delay"
        | "gamechooser.thumbnail_size"
        | "graph.delay"
        | "graph.grid_size"
        | "graph.node_size"
        | "gtp.engine_quit_timeout"
        | "gtp.move_delay"
        | "score.estimator_iterations"
        | "sound.capture_delay_max"
        | "sound.capture_delay_min"
        | "view.leftsidebar_width"
        | "view.leftsidebar_minwidth"
        | "view.peerlist_height"
        | "view.peerlist_minheight"
        | "view.properties_height"
        | "view.properties_minheight"
        | "view.sidebar_width"
        | "view.sidebar_minwidth"
        | "view.winrategraph_blunderthreshold"
        | "view.winrategraph_blunderthreshold_scorelead"
        | "view.winrategraph_height"
        | "view.winrategraph_minheight"
        | "view.winrategraph_maxheight"
        | "infooverlay.duration"
        | "window.height"
        | "window.minheight"
        | "window.minwidth"
        | "window.width" => Some(SettingKind::Number),
        _ => None,
    }
}

/// True for `setting.overwrite.*` keys written by old Electron versions. These
/// are valid but never migrate: they only record which settings were already
/// force-overwritten during previous upgrades.
pub fn is_legacy_overwrite_marker(key: &str) -> bool {
    matches!(
        key,
        "setting.overwrite.v0.19.1"
            | "setting.overwrite.v0.19.3"
            | "setting.overwrite.v0.30.0-beta"
            | "setting.overwrite.v0.33.0"
            | "setting.overwrite.v0.33.4"
            | "setting.overwrite.v0.41.0"
            | "setting.overwrite.v0.43.3_4"
            | "setting.overwrite.v0.50.1"
    )
}

fn value_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Validates a single setting value against its expected kind. Returns the
/// machine-readable reason when the value does not fit the schema.
pub fn validate_setting_value(key: &str, value: &Value) -> Result<(), SettingValidationError> {
    if key == "engines.list" {
        return crate::engine_workflow::validate_engine_list_value(value);
    }
    if key == "katago.human_sl_profile"
        && !value
            .as_str()
            .is_some_and(crate::katago_setup::is_valid_human_sl_profile)
    {
        return Err(SettingValidationError {
            key: key.to_owned(),
            expected: "a HumanSL rank_*/preaz_* profile from 20K through 9D".to_owned(),
            found: value_kind(value).to_owned(),
        });
    }
    if key == "game.opening_convention"
        && !matches!(value.as_str(), Some("free" | "chineseAncientSeatStones"))
    {
        return Err(SettingValidationError {
            key: key.to_owned(),
            expected: "free or chineseAncientSeatStones".to_owned(),
            found: value_kind(value).to_owned(),
        });
    }
    let Some(kind) = setting_kind(key) else {
        return Err(SettingValidationError {
            key: key.to_owned(),
            expected: "a supported Sabaki setting".to_owned(),
            found: value_kind(value).to_owned(),
        });
    };

    let is_valid = match kind {
        SettingKind::Boolean => value.is_boolean(),
        SettingKind::Number => value.is_number(),
        SettingKind::String => value.is_string(),
        SettingKind::NullableString => value.is_null() || value.is_string(),
        SettingKind::StringArray => value
            .as_array()
            .is_some_and(|items| items.iter().all(Value::is_string)),
    };

    if is_valid {
        Ok(())
    } else {
        Err(SettingValidationError {
            key: key.to_owned(),
            expected: format!("{kind:?}").to_lowercase(),
            found: value_kind(value).to_owned(),
        })
    }
}

/// Validates a full settings map, partitioning it into accepted, unknown and
/// invalid entries. This is the shared rule set for settings migration and for
/// the GPUI settings panel.
#[derive(Clone, Debug, Default)]
pub struct SettingsValidation {
    pub accepted: Vec<(String, Value)>,
    pub unknown_keys: Vec<String>,
    pub invalid_values: Vec<SettingValidationError>,
}

pub fn validate_settings(values: impl IntoIterator<Item = (String, Value)>) -> SettingsValidation {
    let mut result = SettingsValidation::default();
    for (key, value) in values {
        if is_legacy_overwrite_marker(&key) {
            continue;
        }
        match setting_kind(&key) {
            None => result.unknown_keys.push(key),
            Some(_) => match validate_setting_value(&key, &value) {
                Ok(()) => result.accepted.push((key, value)),
                Err(error) => result.invalid_values.push(error),
            },
        }
    }
    result
}

/// An in-memory settings map that validates every write against the shared
/// schema. Views read and write settings through this store instead of
/// inventing their own key vocabulary.
#[derive(Clone, Debug, Default)]
pub struct SettingsStore {
    values: BTreeMap<String, Value>,
    user_styles: String,
}

impl SettingsStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds a store that also carries raw user CSS, mirroring the reference
    /// `styles.css` file that accompanies `settings.json`.
    pub fn with_styles(user_styles: String) -> Self {
        Self {
            values: BTreeMap::new(),
            user_styles,
        }
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.values.get(key)
    }

    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Value::as_str)
    }

    pub fn get_bool(&self, key: &str) -> Option<bool> {
        self.get(key).and_then(Value::as_bool)
    }

    /// Writes a validated setting. Unknown keys and type mismatches are
    /// rejected so the store never carries values the schema cannot migrate.
    pub fn set(
        &mut self,
        key: &str,
        value: Value,
    ) -> Result<Option<Value>, SettingValidationError> {
        validate_setting_value(key, &value)?;
        Ok(self.values.insert(key.to_owned(), value))
    }

    pub fn remove(&mut self, key: &str) -> Option<Value> {
        self.values.remove(key)
    }

    pub fn user_styles(&self) -> &str {
        &self.user_styles
    }

    pub fn set_user_styles(&mut self, styles: String) {
        self.user_styles = styles;
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.values.iter()
    }

    /// Exports the settings as a plain map for DTO boundaries that keep their
    /// own serialized shape.
    pub fn to_values(&self) -> BTreeMap<String, Value> {
        self.values.clone()
    }
}

/// Persistence boundary for the settings store, mirroring the reference app
/// config directory layout: `settings.json` plus an optional `styles.css`.
/// The implementation owns the paths; the host workflows stay file-system
/// agnostic.
pub trait SettingsPersistence {
    /// Returns the persisted settings JSON, or `None` when no settings file
    /// has been written yet.
    fn load_settings(&self) -> Result<Option<String>, String>;

    /// Returns the persisted user CSS, or an empty string when absent.
    fn load_styles(&self) -> String;

    /// Persists both files; on partial failure the boundary must leave the
    /// prior state recoverable.
    fn persist_settings(&mut self, settings_json: &str, styles_css: &str) -> Result<(), String>;
}

/// The result of loading a settings store: the store itself plus the
/// validation report for anything that had to be dropped.
#[derive(Clone, Debug, Default)]
pub struct LoadedSettings {
    pub store: SettingsStore,
    pub validation: SettingsValidation,
}

/// Loads and validates a persisted settings store. Unknown keys and invalid
/// values are reported but never written into the store, so a corrupt legacy
/// file cannot poison the running configuration.
pub fn load_settings_store(
    persistence: &impl SettingsPersistence,
) -> Result<LoadedSettings, String> {
    let persisted_values = match persistence.load_settings()? {
        Some(content) => serde_json::from_str(&content)
            .map_err(|error| format!("could not parse persisted settings: {error}"))?,
        None => BTreeMap::new(),
    };
    let validation = validate_settings(persisted_values);
    let mut store = SettingsStore::with_styles(persistence.load_styles());
    for (key, value) in &validation.accepted {
        store
            .set(key, value.clone())
            .expect("values already passed the shared schema");
    }
    Ok(LoadedSettings { store, validation })
}

/// Persists the whole store. The serialized settings are validated again so a
/// buggy caller cannot write a value the schema rejects.
pub fn persist_settings_store(
    store: &SettingsStore,
    persistence: &mut impl SettingsPersistence,
) -> Result<(), String> {
    let mut values = BTreeMap::new();
    for (key, value) in store.iter() {
        validate_setting_value(key, value)
            .map_err(|error| format!("refusing to persist: {error}"))?;
        values.insert(key.clone(), value.clone());
    }
    let settings_json = serde_json::to_string_pretty(&values)
        .map_err(|error| format!("could not serialize settings: {error}"))?;
    persistence.persist_settings(&settings_json, store.user_styles())
}

#[cfg(test)]
mod tests {
    use super::{
        SettingKind, SettingValidationError, SettingsPersistence, SettingsStore,
        is_legacy_overwrite_marker, load_settings_store, persist_settings_store, setting_kind,
        validate_setting_value, validate_settings,
    };
    use serde_json::json;

    #[test]
    fn recognizes_each_setting_kind() {
        assert_eq!(setting_kind("sound.enable"), Some(SettingKind::Boolean));
        assert_eq!(setting_kind("window.width"), Some(SettingKind::Number));
        assert_eq!(setting_kind("app.lang"), Some(SettingKind::String));
        assert_eq!(
            setting_kind("theme.current"),
            Some(SettingKind::NullableString)
        );
        for key in ["engines.analysis", "engines.black", "engines.white"] {
            assert_eq!(setting_kind(key), Some(SettingKind::NullableString));
        }
        assert_eq!(setting_kind("engines.list"), Some(SettingKind::StringArray));
        assert_eq!(
            setting_kind("plugins.pinned"),
            Some(SettingKind::StringArray)
        );
        assert_eq!(setting_kind("unknown.key"), None);
        assert!(is_legacy_overwrite_marker("setting.overwrite.v0.50.1"));
        assert!(!is_legacy_overwrite_marker("setting.overwrite.future"));
    }

    #[test]
    fn validates_values_against_their_kind() {
        assert!(validate_setting_value("sound.enable", &json!(true)).is_ok());
        assert!(validate_setting_value("game.opening_convention", &json!("free")).is_ok());
        assert!(
            validate_setting_value(
                "game.opening_convention",
                &json!("chineseAncientSeatStones")
            )
            .is_ok()
        );
        assert!(validate_setting_value("katago.human_sl_profile", &json!("rank_9d")).is_ok());
        assert!(validate_setting_value("katago.human_sl_profile", &json!("rank_21k")).is_err());
        assert!(validate_setting_value("katago.human_sl_enabled", &json!(true)).is_ok());
        assert!(validate_setting_value("katago.human_sl_enabled", &json!("yes")).is_err());
        assert!(validate_setting_value("sound.enable", &json!("yes")).is_err());
        assert!(validate_setting_value("theme.current", &json!(null)).is_ok());
        assert!(validate_setting_value("theme.current", &json!("mist")).is_ok());
        assert!(validate_setting_value("engines.analysis", &json!("KataGo")).is_ok());
        assert!(validate_setting_value("engines.black", &json!(null)).is_ok());
        assert!(validate_setting_value("engines.white", &json!(3)).is_err());
        assert!(validate_setting_value("window.width", &json!(1200)).is_ok());
        assert!(validate_setting_value("window.width", &json!("wide")).is_err());
        assert!(
            validate_setting_value(
                "engines.list",
                &json!([{"name": "KataGo", "path": "/engines/katago", "args": ""}]),
            )
            .is_ok()
        );
        assert!(validate_setting_value("engines.list", &json!(["kata"])).is_err());
    }

    #[test]
    fn partitions_a_settings_map_into_accepted_unknown_invalid() {
        let validation = validate_settings(vec![
            ("sound.enable".to_owned(), json!(true)),
            ("theme.current".to_owned(), json!("classic")),
            ("mystery.key".to_owned(), json!(1)),
            ("window.height".to_owned(), json!("tall")),
        ]);

        assert_eq!(validation.accepted.len(), 2);
        assert_eq!(validation.unknown_keys, vec!["mystery.key".to_owned()]);
        assert_eq!(validation.invalid_values.len(), 1);
        assert!(matches!(
            &validation.invalid_values[0],
            SettingValidationError { key, .. } if key == "window.height"
        ));
    }

    #[test]
    fn overwrite_markers_are_accepted_but_never_migrated() {
        let validation = validate_settings(vec![
            ("sound.enable".to_owned(), json!(true)),
            (
                "setting.overwrite.v0.41.0".to_owned(),
                json!(["sound.enable"]),
            ),
        ]);

        assert_eq!(validation.accepted.len(), 1);
        assert!(validation.unknown_keys.is_empty());
        assert!(validation.invalid_values.is_empty());
    }

    #[test]
    fn store_keeps_validated_settings_and_rejects_bad_writes() {
        let mut store = SettingsStore::new();
        assert!(
            store
                .set("theme.current", json!("dark"))
                .expect("theme names are valid nullable strings")
                .is_none()
        );
        assert!(
            store
                .set("sound.enable", json!(true))
                .expect("booleans are valid")
                .is_none()
        );

        let rejected = store
            .set("sound.enable", json!("loud"))
            .expect_err("a string is not a valid boolean");
        assert_eq!(rejected.key, "sound.enable");
        assert_eq!(store.len(), 2);
        assert_eq!(store.get_str("theme.current"), Some("dark"));
        assert_eq!(store.get_bool("sound.enable"), Some(true));
        assert!(store.get("unknown.key").is_none());
    }

    #[test]
    fn store_round_trips_replace_and_remove() {
        let mut store = SettingsStore::new();
        store.set("theme.current", json!("classic")).unwrap();
        let replaced = store.set("theme.current", json!("mist")).unwrap();
        assert_eq!(replaced, Some(json!("classic")));
        assert_eq!(store.get_str("theme.current"), Some("mist"));
        assert_eq!(store.remove("theme.current"), Some(json!("mist")));
        assert!(store.is_empty());
    }

    #[test]
    fn store_carries_user_styles_alongside_values() {
        let store = SettingsStore::with_styles("body { color: red; }".to_owned());
        assert_eq!(store.user_styles(), "body { color: red; }");
    }

    #[test]
    fn loading_rejects_corrupt_or_unknown_values_but_keeps_valid_ones() {
        let persistence = MemorySettingsPersistence {
            settings_json: Some(
                json!({
                    "sound.enable": true,
                    "window.width": "wide",
                    "mystery.key": 1,
                    "theme.current": "dark",
                })
                .to_string(),
            ),
            styles_css: "body { color: blue; }".to_owned(),
        };

        let loaded = load_settings_store(&persistence).expect("valid settings load");

        assert_eq!(loaded.store.get_bool("sound.enable"), Some(true));
        assert_eq!(loaded.store.get_str("theme.current"), Some("dark"));
        assert!(loaded.store.get("window.width").is_none());
        assert!(loaded.store.get("mystery.key").is_none());
        assert_eq!(loaded.store.user_styles(), "body { color: blue; }");
        assert_eq!(
            loaded.validation.unknown_keys,
            vec!["mystery.key".to_owned()]
        );
        assert_eq!(loaded.validation.invalid_values.len(), 1);
    }

    #[test]
    fn persisting_round_trips_through_the_boundary() {
        let mut store = SettingsStore::with_styles("body { color: green; }".to_owned());
        store.set("theme.current", json!("mist")).unwrap();
        let mut persistence = MemorySettingsPersistence::default();

        persist_settings_store(&store, &mut persistence).expect("valid stores persist");

        let reloaded = load_settings_store(&persistence).expect("persisted settings load");
        assert_eq!(reloaded.store.get_str("theme.current"), Some("mist"));
        assert_eq!(reloaded.store.user_styles(), "body { color: green; }");
    }

    #[test]
    fn a_missing_settings_file_loads_an_empty_store() {
        let persistence = MemorySettingsPersistence::default();
        let loaded = load_settings_store(&persistence).expect("absence is not an error");
        assert!(loaded.store.is_empty());
        assert!(loaded.validation.accepted.is_empty());
        assert_eq!(loaded.store.user_styles(), "");
    }

    #[derive(Default)]
    struct MemorySettingsPersistence {
        settings_json: Option<String>,
        styles_css: String,
    }

    impl SettingsPersistence for MemorySettingsPersistence {
        fn load_settings(&self) -> Result<Option<String>, String> {
            Ok(self.settings_json.clone())
        }

        fn load_styles(&self) -> String {
            self.styles_css.clone()
        }

        fn persist_settings(
            &mut self,
            settings_json: &str,
            styles_css: &str,
        ) -> Result<(), String> {
            self.settings_json = Some(settings_json.to_owned());
            self.styles_css = styles_css.to_owned();
            Ok(())
        }
    }
}
