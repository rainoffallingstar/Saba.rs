//! Built-in plugin command registry.
//!
//! The registry is the identity seam between manifest contributions and host
//! actions. It owns plugin/command IDs; GPUI code consumes semantic variants
//! instead of expanding string comparisons across `ShellApp`.

use crate::KataGoModelTier;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuiltinPluginCommand {
    KataGoSetup,
    KataGoDownload(KataGoModelTier),
    FoxFetchLatest,
    PositionCheck,
    SgfExport,
}

impl BuiltinPluginCommand {
    pub const fn is_katago(self) -> bool {
        matches!(self, Self::KataGoSetup | Self::KataGoDownload(_))
    }

    pub const fn is_fox(self) -> bool {
        matches!(self, Self::FoxFetchLatest)
    }
}

/// Stable registry for commands implemented by the host rather than by a
/// plugin runtime. Unknown IDs remain available to declarative/native/WASM
/// dispatch and are never silently claimed by this registry.
pub struct BuiltinPluginCommandRegistry;

impl BuiltinPluginCommandRegistry {
    pub fn resolve(plugin_id: &str, command_id: &str) -> Option<BuiltinPluginCommand> {
        match (plugin_id, command_id) {
            ("org.sabaki.katago-setup-hub", "org.sabaki.katago-setup-hub.setup") => {
                Some(BuiltinPluginCommand::KataGoSetup)
            }
            ("org.sabaki.katago-setup-hub", "org.sabaki.katago-setup-hub.download_balanced") => {
                Some(BuiltinPluginCommand::KataGoDownload(
                    KataGoModelTier::Balanced,
                ))
            }
            ("org.sabaki.katago-setup-hub", "org.sabaki.katago-setup-hub.download_lightweight") => {
                Some(BuiltinPluginCommand::KataGoDownload(
                    KataGoModelTier::Lightweight,
                ))
            }
            ("org.sabaki.katago-setup-hub", "org.sabaki.katago-setup-hub.download_strongest") => {
                Some(BuiltinPluginCommand::KataGoDownload(
                    KataGoModelTier::Strongest,
                ))
            }
            ("org.sabaki.fox-kifu-sync", "org.sabaki.fox-kifu-sync.fetch_latest")
            | ("org.sabaki.fox-kifu-sync", "org.sabaki.fox-kifu-sync.query_user") => {
                Some(BuiltinPluginCommand::FoxFetchLatest)
            }
            ("org.sabaki.position-checker", "org.sabaki.position-checker.check") => {
                Some(BuiltinPluginCommand::PositionCheck)
            }
            ("org.sabaki.sgf-exporter", "org.sabaki.sgf-exporter.export") => {
                Some(BuiltinPluginCommand::SgfExport)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BuiltinPluginCommand, BuiltinPluginCommandRegistry};
    use crate::KataGoModelTier;

    #[test]
    fn registry_resolves_every_builtin_command() {
        assert_eq!(
            BuiltinPluginCommandRegistry::resolve(
                "org.sabaki.katago-setup-hub",
                "org.sabaki.katago-setup-hub.setup"
            ),
            Some(BuiltinPluginCommand::KataGoSetup)
        );
        assert_eq!(
            BuiltinPluginCommandRegistry::resolve(
                "org.sabaki.katago-setup-hub",
                "org.sabaki.katago-setup-hub.download_balanced"
            ),
            Some(BuiltinPluginCommand::KataGoDownload(
                KataGoModelTier::Balanced
            ))
        );
        assert_eq!(
            BuiltinPluginCommandRegistry::resolve(
                "org.sabaki.katago-setup-hub",
                "org.sabaki.katago-setup-hub.download_lightweight"
            ),
            Some(BuiltinPluginCommand::KataGoDownload(
                KataGoModelTier::Lightweight
            ))
        );
        assert_eq!(
            BuiltinPluginCommandRegistry::resolve(
                "org.sabaki.katago-setup-hub",
                "org.sabaki.katago-setup-hub.download_strongest"
            ),
            Some(BuiltinPluginCommand::KataGoDownload(
                KataGoModelTier::Strongest
            ))
        );
        assert_eq!(
            BuiltinPluginCommandRegistry::resolve(
                "org.sabaki.fox-kifu-sync",
                "org.sabaki.fox-kifu-sync.query_user"
            ),
            Some(BuiltinPluginCommand::FoxFetchLatest)
        );
        assert_eq!(
            BuiltinPluginCommandRegistry::resolve(
                "org.sabaki.position-checker",
                "org.sabaki.position-checker.check"
            ),
            Some(BuiltinPluginCommand::PositionCheck)
        );
        assert_eq!(
            BuiltinPluginCommandRegistry::resolve(
                "org.sabaki.sgf-exporter",
                "org.sabaki.sgf-exporter.export"
            ),
            Some(BuiltinPluginCommand::SgfExport)
        );
    }

    #[test]
    fn registry_does_not_claim_unknown_or_cross_plugin_commands() {
        assert_eq!(
            BuiltinPluginCommandRegistry::resolve(
                "org.sabaki.position-checker",
                "org.sabaki.sgf-exporter.export"
            ),
            None
        );
        assert_eq!(
            BuiltinPluginCommandRegistry::resolve("org.example.plugin", "command"),
            None
        );
    }
}
