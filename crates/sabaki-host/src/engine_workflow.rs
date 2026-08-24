use crate::settings::{SettingValidationError, SettingsStore};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// A configured GTP engine, matching one entry of the `engines.list` setting.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineRecord {
    pub name: String,
    pub path: String,
    #[serde(default)]
    pub args: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commands: Option<String>,
}

impl EngineRecord {
    pub fn new(name: impl Into<String>, path: impl Into<String>, args: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            path: path.into(),
            args: args.into(),
            commands: None,
        }
    }

    pub fn with_commands(mut self, commands: impl Into<String>) -> Self {
        self.commands = Some(commands.into());
        self
    }
}

/// The expected shape of a single `engines.list` entry.
pub fn validate_engine_record(value: &Value) -> Result<(), SettingValidationError> {
    let Some(record) = value.as_object() else {
        return Err(engine_list_error(value));
    };
    let has_required_strings = ["name", "path", "args"]
        .into_iter()
        .all(|field| record.get(field).is_some_and(Value::is_string));
    let commands_is_valid = record.get("commands").is_none_or(Value::is_string);
    if has_required_strings && commands_is_valid {
        Ok(())
    } else {
        Err(engine_list_error(value))
    }
}

/// Validates an `engines.list` value: an array of engine objects with string
/// name/path/args and an optional string commands field.
pub fn validate_engine_list_value(value: &Value) -> Result<(), SettingValidationError> {
    let Some(entries) = value.as_array() else {
        return Err(engine_list_error(value));
    };
    for entry in entries {
        validate_engine_record(entry)?;
    }
    Ok(())
}

fn engine_list_error(value: &Value) -> SettingValidationError {
    SettingValidationError {
        key: "engines.list".to_owned(),
        expected: "an array of engine objects with string name, path, args, and optional commands"
            .to_owned(),
        found: value_kind(value).to_owned(),
    }
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

/// Parses the `engines.list` setting value into engine records. Invalid values
/// are rejected rather than silently truncated.
pub fn engine_list_from_value(value: &Value) -> Result<Vec<EngineRecord>, SettingValidationError> {
    validate_engine_list_value(value)?;
    let entries = value.as_array().expect("validated engine lists are arrays");
    Ok(entries
        .iter()
        .filter_map(|value| serde_json::from_value::<EngineRecord>(value.clone()).ok())
        .collect())
}

/// Serializes engine records back into the `engines.list` setting shape.
pub fn engine_list_to_value(engines: &[EngineRecord]) -> Value {
    json!(engines)
}

/// UI-independent registry of configured engines, bound to the `engines.list`
/// setting key. Engines are pure data here; the process transport lives in
/// `sabaki-domain-core::gtp`.
#[derive(Clone, Debug, Default)]
pub struct EngineStore {
    engines: Vec<EngineRecord>,
}

impl EngineStore {
    /// Builds the store from the current settings value. A missing key yields
    /// an empty store; an invalid value surfaces the reason.
    pub fn from_settings(settings: &SettingsStore) -> Result<Self, SettingValidationError> {
        match settings.get("engines.list") {
            None => Ok(Self::default()),
            Some(value) => {
                let engines = engine_list_from_value(value)?;
                Ok(Self { engines })
            }
        }
    }

    /// Writes the current engine list back into the settings store.
    pub fn save(&self, settings: &mut SettingsStore) -> Result<(), SettingValidationError> {
        settings.set("engines.list", engine_list_to_value(&self.engines))?;
        Ok(())
    }

    pub fn list(&self) -> &[EngineRecord] {
        &self.engines
    }

    pub fn add(&mut self, engine: EngineRecord) -> Result<(), SettingValidationError> {
        if self
            .engines
            .iter()
            .any(|existing| existing.name == engine.name)
        {
            return Err(SettingValidationError {
                key: "engines.list".to_owned(),
                expected: "engine names to be unique".to_owned(),
                found: format!("duplicate name '{}'", engine.name),
            });
        }
        self.engines.push(engine);
        Ok(())
    }

    /// Adds or updates an engine record by name.
    pub fn upsert(&mut self, engine: EngineRecord) {
        if let Some(existing) = self.engines.iter_mut().find(|e| e.name == engine.name) {
            *existing = engine;
        } else {
            self.engines.push(engine);
        }
    }

    pub fn remove(&mut self, name: &str) -> bool {
        let previous_len = self.engines.len();
        self.engines.retain(|engine| engine.name != name);
        self.engines.len() != previous_len
    }
}

#[cfg(test)]
mod tests {
    use super::{
        EngineRecord, EngineStore, engine_list_from_value, engine_list_to_value,
        validate_engine_list_value,
    };
    use crate::settings::SettingsStore;
    use serde_json::json;

    fn sample_value() -> serde_json::Value {
        json!([
            {"name": "KataGo", "path": "/engines/katago", "args": "-config config.cfg"},
            {"name": "GNU Go", "path": "/engines/gnugo", "args": "", "commands": "level 10"}
        ])
    }

    #[test]
    fn parses_engine_objects_from_the_settings_shape() {
        let engines = engine_list_from_value(&sample_value()).expect("sample engines parse");
        assert_eq!(engines.len(), 2);
        assert_eq!(engines[0].name, "KataGo");
        assert_eq!(engines[0].path, "/engines/katago");
        assert_eq!(engines[0].commands, None);
        assert_eq!(engines[1].commands.as_deref(), Some("level 10"));
    }

    #[test]
    fn rejects_non_object_arrays_and_missing_fields() {
        assert!(validate_engine_list_value(&json!(["kata"])).is_err());
        assert!(validate_engine_list_value(&json!([{"name": "KataGo"}])).is_err());
        assert!(
            validate_engine_list_value(&json!([{"name": "KataGo", "path": "p", "args": 5}]))
                .is_err()
        );
        assert!(validate_engine_list_value(&json!("not an array")).is_err());
    }

    #[test]
    fn round_trips_records_through_the_setting_value() {
        let engines = engine_list_from_value(&sample_value()).expect("sample engines parse");
        let value = engine_list_to_value(&engines);
        let reparsed = engine_list_from_value(&value).expect("round trip parses");
        assert_eq!(reparsed, engines);
    }

    #[test]
    fn store_binds_to_the_engines_list_setting() {
        let mut settings = SettingsStore::default();
        settings
            .set("engines.list", sample_value())
            .expect("sample engines are valid settings");

        let mut store = EngineStore::from_settings(&settings).expect("store builds");
        assert_eq!(store.list().len(), 2);

        assert!(store.add(EngineRecord::new("KataGo", "/x", "")).is_err());
        store
            .add(EngineRecord::new("Leela", "/engines/leela", "--noponder"))
            .expect("a new engine is added");
        assert!(store.remove("GNU Go"));
        assert!(!store.remove("GNU Go"));

        store.save(&mut settings).expect("store saves");
        let reloaded = EngineStore::from_settings(&settings).expect("reloads");
        assert_eq!(reloaded.list().len(), 2);
        assert_eq!(reloaded.list()[0].name, "KataGo");
        assert_eq!(reloaded.list()[1].name, "Leela");
    }

    #[test]
    fn a_missing_engine_list_yields_an_empty_store() {
        let store = EngineStore::from_settings(&SettingsStore::default()).expect("empty store");
        assert!(store.list().is_empty());
    }
}
