# Tauri migration contract

This document defines the compatibility boundary while Sabaki transitions from
Electron/Node to the Rust host with a final **Rust + GPUI** client. The current
Electron implementation remains the behavioral source of truth until an
equivalent Rust command is covered by differential tests.

The Tauri/Preact layer is a **transitional adapter** only: it is the UI behavior
reference and fallback slice until GPUI reaches Beta quality, and it will be
removed once the GPUI client achieves parity. The final contract is between the
UI-independent `ryusei-host` crate and its GPUI adapter.

## User-data compatibility

The host must import the JSON object in the Electron `settings.json` file. These
settings are release-blocking compatibility data:

- engine entries from `engines.list`, including `name`, `path`, `args`, and
  `commands`;
- board, game, scoring, view, sound, locale, window, and GTP settings;
- the user stylesheet stored as `styles.css`.

The importer must create a backup before it writes migrated data and must leave
the original file unchanged when migration fails. Old `.asar` themes are
intentionally unsupported. Native themes use a versioned theme-token + asset
manifest (`theme.json` + `tokens.json` + assets) in an application-managed theme
directory. Runtime/binary compatibility with `styles.css` is **not** guaranteed;
CSS values expressible as theme-tokens are listed in the migration report and
can be imported into the new token format.

## Frontend boundary

The transitional Preact UI must interact only with the asynchronous bridge in
`src/tauri/bridge.js` and `src/tauri/store.js`. It must not depend on Rust
types, filesystem APIs, child-process APIs, or WebView globals.

The host exposes snapshots and named operations rather than mutable game-tree
references. During the transition these are exposed as Tauri commands,
including:

- `game_create_new`, `game_snapshot`, `game_save`, `game_open_dialog`, and
  `game_save_dialog`;
- `game_play_move`, `game_apply_transaction`, `game_undo`, and `game_redo`;
- `settings_snapshot`, `settings_set`, `settings_import_legacy`;
- `themes_list`;
- `plugins_list`, `plugins_install`, `plugins_enable`, and
  `plugins_authorize_native_execution`.

The final GPUI UI calls `ryusei-host` typed methods directly and receives typed
`HostEvent`s (e.g. `GameChanged { snapshot }`); the Tauri command/event layer is
an adapter projection of the same host API and will be removed with Tauri.

A `game-state-changed` event carries a complete serializable game snapshot after
each mutation. `game_open_dialog` and `game_save_dialog` return `null` when the
user cancels. The migration adapter owns the local UI cache; it serializes file
commands with edits and leaves its snapshot unchanged on cancellation.

## Domain DTOs

The stable data transfer objects are `GameSnapshot`, `BoardSnapshot`, `MoveDto`,
`PluginManifest`, and `PluginRecord`. They use `camelCase` JSON field names and
a semantic `schemaVersion` where applicable. Internal Rust game-tree, board,
process, and plugin-host types are never sent to the frontend.

## Plugin boundary

Plugins have a manifest with an API version and explicit permissions. There are
two execution tiers:

1. WASM and declarative plugins have no filesystem, network, shell, or process
   capability by default.
2. Native Go/Rust plugin executables are independent JSON-RPC processes. They
   are disabled by default and only start after the user explicitly grants the
   requested permissions.

Neither tier may import host internals or insert arbitrary DOM/Preact
components. The public extension surface is limited to commands, menu
contributions, settings schema, private storage, read-only snapshots, host
events, and named game transactions. Plugin UI contributions are declarative,
host-validated, closed-set widgets; arbitrary Web UI / Web Panel rendering is
not part of the final contract (in GPUI, plugin code never embeds arbitrary
Rust/GPUI components or GPU contexts).

## Compatibility test sources

- `test/gametreeTests.js` characterizes board reconstruction, markup, variation
  metadata, and game-info normalization.
- `test/gobantransformerTests.js` characterizes pure transformation behavior.
- `test/analysisTests.js` contains GTP analysis golden transcripts.
- `test/gibTests.js`, `test/ngfTests.js`, and `test/ugfTests.js` contain legacy
  format fixtures.
- `test/differentialTests.js` and `test/fixtures/differential/` define shared
  game-tree and board fixtures that must pass in both JavaScript and Rust via
  `crates/domain-core/tests/differential_fixture.rs`.
- `test/tauri*Tests.js` validates the frontend DTO, store, board adapter,
  navigation, variation-tree, markup, and node-metadata boundaries.
- `crates/ryusei-host` tests validate the UI-independent host workflow through
  typed ports (open/edit/save/reopen, failed open, save-location enforcement,
  recovery restore, source-location discard).
- `e2e/` remains the user-visible Electron regression suite during the
  migration; a separate native (Tauri-then-GPUI) e2e suite is still required
  before Beta.
