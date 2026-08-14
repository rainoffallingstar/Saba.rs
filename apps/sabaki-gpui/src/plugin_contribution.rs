use serde::{Deserialize, Serialize};

pub const PLUGIN_CONTRIBUTION_SCHEMA_VERSION: u32 = 1;

/// A host-validated, closed-set widget for a plugin panel. Plugins may never
/// embed arbitrary Rust/GPUI components or GPU contexts; they contribute data
/// that the host renders from this closed set.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PanelWidget {
    Label {
        text: String,
    },
    Value {
        label: String,
        value: String,
    },
    Button {
        id: String,
        title: String,
    },
    Select {
        id: String,
        options: Vec<String>,
        selected: Option<String>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginPanelContribution {
    pub schema_version: u32,
    pub plugin_id: String,
    pub panel_title: String,
    pub widgets: Vec<PanelWidget>,
}

impl PluginPanelContribution {
    pub fn parse(json: &str) -> Result<Self, String> {
        let contribution: Self = serde_json::from_str(json)
            .map_err(|error| format!("invalid plugin panel contribution: {error}"))?;
        contribution.validate()?;
        Ok(contribution)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != PLUGIN_CONTRIBUTION_SCHEMA_VERSION {
            return Err(format!(
                "unsupported plugin contribution schema version {}",
                self.schema_version
            ));
        }
        if self.plugin_id.trim().is_empty() || !self.plugin_id.contains('.') {
            return Err("plugin id must be a reverse-domain identifier".to_owned());
        }
        if self.panel_title.trim().is_empty() {
            return Err("panel title must not be empty".to_owned());
        }
        let mut button_ids = std::collections::BTreeSet::new();
        for widget in &self.widgets {
            match widget {
                PanelWidget::Label { text } if text.trim().is_empty() => {
                    return Err("label widget must not be empty".to_owned());
                }
                PanelWidget::Value { label, .. } if label.trim().is_empty() => {
                    return Err("value widget must have a label".to_owned());
                }
                PanelWidget::Button { id, title } => {
                    if title.trim().is_empty() {
                        return Err("button widget must have a title".to_owned());
                    }
                    if !button_ids.insert(id.as_str()) {
                        return Err(format!("duplicate button id {id:?}"));
                    }
                }
                PanelWidget::Select { id, options, .. } if options.is_empty() => {
                    return Err(format!("select widget {id:?} needs at least one option"));
                }
                _ => {}
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{PanelWidget, PluginPanelContribution};

    #[test]
    fn parses_and_validates_a_declarative_panel() {
        let contribution = PluginPanelContribution::parse(
            r#"{
                "schemaVersion": 1,
                "pluginId": "org.example.opening-trainer",
                "panelTitle": "Opening Trainer",
                "widgets": [
                    {"type": "label", "text": "Play three moves"},
                    {"type": "value", "label": "Accuracy", "value": "87%"},
                    {"type": "button", "id": "start", "title": "Start"},
                    {"type": "select", "id": "level", "options": ["easy", "hard"], "selected": "easy"}
                ]
            }"#,
        )
        .unwrap();

        assert_eq!(contribution.plugin_id, "org.example.opening-trainer");
        assert_eq!(contribution.widgets.len(), 4);
        assert!(
            matches!(contribution.widgets[2], PanelWidget::Button { ref id, .. } if id == "start")
        );
    }

    #[test]
    fn rejects_non_reverse_domain_plugin_ids() {
        let contribution = PluginPanelContribution::parse(
            r#"{
                "schemaVersion": 1,
                "pluginId": "opening-trainer",
                "panelTitle": "Opening Trainer",
                "widgets": []
            }"#,
        );
        assert!(contribution.is_err());
    }

    #[test]
    fn rejects_duplicate_button_ids() {
        let contribution = PluginPanelContribution::parse(
            r#"{
                "schemaVersion": 1,
                "pluginId": "org.example.opening-trainer",
                "panelTitle": "Opening Trainer",
                "widgets": [
                    {"type": "button", "id": "start", "title": "Start"},
                    {"type": "button", "id": "start", "title": "Start Again"}
                ]
            }"#,
        );
        assert!(contribution.is_err());
    }

    #[test]
    fn rejects_empty_select_option_sets() {
        let contribution = PluginPanelContribution::parse(
            r#"{
                "schemaVersion": 1,
                "pluginId": "org.example.opening-trainer",
                "panelTitle": "Opening Trainer",
                "widgets": [
                    {"type": "select", "id": "level", "options": [], "selected": null}
                ]
            }"#,
        );
        assert!(contribution.is_err());
    }

    #[test]
    fn rejects_unknown_schema_versions() {
        let contribution = PluginPanelContribution::parse(
            r#"{
                "schemaVersion": 2,
                "pluginId": "org.example.opening-trainer",
                "panelTitle": "Opening Trainer",
                "widgets": []
            }"#,
        );
        assert!(contribution.is_err());
    }
}
