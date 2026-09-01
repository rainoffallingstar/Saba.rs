use crate::theme::ThemeTokens;
use ryusei_host::setting_kind;

/// The board sizes offered by the settings panel presets.
#[allow(dead_code)]
pub const BOARD_SIZE_OPTIONS: &[usize] = &[9, 13, 19];

/// A named theme the settings panel can switch between. Each theme maps to a
/// validated `ThemeTokens` set. The label of a choice is the value written to
/// the persisted `theme.current` setting, so switching themes round-trips
/// through the shared settings schema.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThemeChoice {
    Classic,
    Dark,
    Mist,
}

#[allow(dead_code)]
pub const THEME_CHOICES: &[ThemeChoice] =
    &[ThemeChoice::Classic, ThemeChoice::Dark, ThemeChoice::Mist];

impl ThemeChoice {
    /// The value stored under the `theme.current` setting key.
    pub fn setting_value(self) -> &'static str {
        match self {
            ThemeChoice::Classic => "classic",
            ThemeChoice::Dark => "dark",
            ThemeChoice::Mist => "mist",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ThemeChoice::Classic => "Classic",
            ThemeChoice::Dark => "Dark",
            ThemeChoice::Mist => "Mist",
        }
    }

    pub fn tokens(self) -> ThemeTokens {
        match self {
            ThemeChoice::Classic => ThemeTokens::parse(
                r##"{"schemaVersion":2,"boardWood":"#e2b177","boardLine":"#3d2814","starPoint":"#2a1a0b","stoneBlack":"#1a1a1a","stoneWhite":"#ffffff","background":"#f5f5f7","shell":{"text":"#1d1d1f","muted":"#6e6e73","subtle":"#86868b","panel":"#ffffff","input":"#f0f0f3","border":"#e5e5ea","button":"#ebebef","buttonActive":"#dedee4","accent":"#0071e3","danger":"#fee2e2","dangerText":"#dc2626","success":"#16a34a","track":"#e5e5ea","textSecondary":"#424245","borderSoft":"#e8e8ed","accentHover":"#0077ed","accentActive":"#0066cc","warn":"#dd9d06","mistake":"#f97316","elevated":"#ffffff","info":"#0ea5e9"}}"##,
            )
            .expect("the classic theme tokens are valid"),
            ThemeChoice::Dark => ThemeTokens::parse(
                r##"{"schemaVersion":2,"boardWood":"#2e2a24","boardLine":"#8a7a5a","starPoint":"#a89264","stoneBlack":"#0f0f0f","stoneWhite":"#ececec","background":"#1c1c1e","shell":{"text":"#f5f5f7","muted":"#98989d","subtle":"#636366","panel":"#252528","input":"#1e1e20","border":"#38383a","button":"#2c2c2e","buttonActive":"#3a3a3c","accent":"#0a84ff","danger":"#3d1c1c","dangerText":"#ff453a","success":"#30d158","track":"#38383a","textSecondary":"#d2d2d7","borderSoft":"#2a2a2f","accentHover":"#409cff","accentActive":"#0a84ff","warn":"#ffd60a","mistake":"#ff9f0a","elevated":"#2c2c2e","info":"#64d2ff"}}"##,
            )
            .expect("the dark theme tokens are valid"),
            ThemeChoice::Mist => ThemeTokens::parse(
                r##"{"schemaVersion":2,"boardWood":"#cad5c7","boardLine":"#334a33","starPoint":"#243824","stoneBlack":"#141c14","stoneWhite":"#f4faf4","background":"#f2f5f2","shell":{"text":"#18221a","muted":"#4d5e50","subtle":"#788a7b","panel":"#ffffff","input":"#ebf0eb","border":"#d2ded2","button":"#e4ede4","buttonActive":"#d4e3d4","accent":"#248a3d","danger":"#fee2e2","dangerText":"#dc2626","success":"#28753a","track":"#d2ded2","textSecondary":"#3c4a3e","borderSoft":"#dfe9df","accentHover":"#2ea04a","accentActive":"#1e7a33","warn":"#a97c04","mistake":"#c2571a","elevated":"#ffffff","info":"#1f7a8c"}}"##,
            )
            .expect("the mist theme tokens are valid"),
        }
    }
}

/// Resolves the persisted `theme.current` value back to a theme choice. Unknown
/// or absent names fall back to the default (classic) theme so old or foreign
/// settings files still render.
pub fn theme_from_setting(value: Option<&str>) -> ThemeChoice {
    match value {
        Some("dark") => ThemeChoice::Dark,
        Some("mist") => ThemeChoice::Mist,
        _ => ThemeChoice::Classic,
    }
}

/// Reads the persisted `window.width` / `window.height` numbers as the initial
/// window size. Non-numeric, non-positive or missing values yield `None` so the
/// caller falls back to its own default.
pub fn window_bounds_from_settings(settings: &ryusei_host::SettingsStore) -> Option<(f64, f64)> {
    let width = settings
        .get("window.width")
        .and_then(serde_json::Value::as_f64)?;
    let height = settings
        .get("window.height")
        .and_then(serde_json::Value::as_f64)?;
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    Some((width, height))
}

/// Reads the persisted `window.maximized` boolean; missing or non-boolean
/// values fall back to `false` so the window opens windowed by default.
pub fn window_maximized_from_settings(settings: &ryusei_host::SettingsStore) -> bool {
    settings
        .get("window.maximized")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

/// Validates that a persisted setting key is a known Sabaki setting, using the
/// shared key table owned by `ryusei-host` (mirroring the Electron schema).
#[allow(dead_code)]
pub fn is_supported_setting(key: &str) -> bool {
    setting_kind(key).is_some()
}

#[cfg(test)]
mod tests {
    use super::{
        BOARD_SIZE_OPTIONS, THEME_CHOICES, ThemeChoice, is_supported_setting, theme_from_setting,
        window_bounds_from_settings, window_maximized_from_settings,
    };
    use ryusei_host::SettingsStore;
    use serde_json::json;

    #[test]
    fn every_theme_choice_exposes_valid_tokens() {
        for choice in THEME_CHOICES {
            assert!(!choice.label().is_empty());
            assert_eq!(ThemeChoice::tokens(*choice).schema_version, 2);
            assert!(ThemeChoice::tokens(*choice).shell.is_some());
        }
    }

    #[test]
    fn themes_produce_distinct_board_colors() {
        let classic = ThemeChoice::Classic.tokens();
        let dark = ThemeChoice::Dark.tokens();
        assert_ne!(classic.board_wood, dark.board_wood);
    }

    #[test]
    fn setting_values_round_trip_through_the_shared_schema() {
        for choice in THEME_CHOICES {
            assert_eq!(theme_from_setting(Some(choice.setting_value())), *choice);
        }
        assert_eq!(theme_from_setting(None), ThemeChoice::Classic);
        assert_eq!(theme_from_setting(Some("unknown")), ThemeChoice::Classic);
    }

    #[test]
    fn recognizes_supported_setting_keys_from_the_shared_table() {
        assert!(is_supported_setting("theme.current"));
        assert!(is_supported_setting("window.width"));
        assert!(is_supported_setting("sound.enable"));
        assert!(!is_supported_setting("unknown.setting"));
    }

    #[test]
    fn offers_sane_board_sizes() {
        assert_eq!(BOARD_SIZE_OPTIONS, &[9, 13, 19]);
    }

    #[test]
    fn window_bounds_restore_from_persisted_numbers() {
        let mut settings = SettingsStore::default();
        assert_eq!(window_bounds_from_settings(&settings), None);

        settings
            .set("window.width", json!(1280))
            .expect("window width is a valid number");
        settings
            .set("window.height", json!(720))
            .expect("window height is a valid number");
        assert_eq!(
            window_bounds_from_settings(&settings),
            Some((1280.0, 720.0))
        );
    }

    #[test]
    fn window_bounds_reject_missing_or_non_positive_values() {
        let mut settings = SettingsStore::default();
        settings
            .set("window.width", json!(800))
            .expect("window width is valid");
        assert_eq!(window_bounds_from_settings(&settings), None);

        settings
            .set("window.height", json!(-10))
            .expect("negative height is still a number");
        assert_eq!(window_bounds_from_settings(&settings), None);
    }

    #[test]
    fn window_maximized_restores_from_the_persisted_boolean() {
        let mut settings = SettingsStore::default();
        assert!(!window_maximized_from_settings(&settings));

        settings
            .set("window.maximized", json!(true))
            .expect("maximized is a valid boolean");
        assert!(window_maximized_from_settings(&settings));
    }

    #[test]
    fn window_maximized_falls_back_when_the_load_drops_invalid_values() {
        #[derive(Default)]
        struct RawSettingsPersistence(String);
        impl ryusei_host::SettingsPersistence for RawSettingsPersistence {
            fn load_settings(&self) -> Result<Option<String>, String> {
                Ok(Some(self.0.clone()))
            }

            fn load_styles(&self) -> String {
                String::new()
            }

            fn persist_settings(
                &mut self,
                _settings_json: &str,
                _styles_css: &str,
            ) -> Result<(), String> {
                Ok(())
            }
        }

        let persistence = RawSettingsPersistence(r#"{"window.maximized": "yes"}"#.to_owned());
        let loaded = ryusei_host::load_settings_store(&persistence).expect("raw settings load");
        assert_eq!(loaded.validation.invalid_values.len(), 1);
        assert!(!window_maximized_from_settings(&loaded.store));
    }
}
