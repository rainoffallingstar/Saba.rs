//! Plugin panel contribution types are owned by `ryusei-plugin-runtime`
//! so the manifest schema and the host render layer share one closed set.

#[allow(unused_imports)]
pub use ryusei_plugin_runtime::{
    PLUGIN_CONTRIBUTION_SCHEMA_VERSION, PanelWidget, PluginPanelContribution,
};
