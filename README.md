# Saba.rs

A native rewrite of [Sabaki](https://github.com/SabakiHQ/Sabaki) — an elegant
Go/Baduk/Weiqi board and SGF editor — built with **Rust + GPUI**, with a
sandboxed plugin system.

The Electron/Node.js version of Sabaki remains the stable release and the
behavioral reference; Saba.rs is the active native migration line
(`apps/sabaki-gpui`). UI-independent logic lives in `crates/sabaki-host`,
driven through typed ports so every layer stays testable without a window.

## Workspace layout

```
crates/domain-core      UI-independent domain core: GameDocument / SGF / board / GTP / DTOs
crates/plugin-runtime   Plugin manifests, permission model, JSON-RPC framing, native processes
crates/sabaki-host      UI-independent application workflows (open/save/recovery/settings/engines/plugins)
apps/sabaki-gpui        The GPUI client (active development target)
examples/               Fake GTP engine for subprocess smoke tests; example plugins
```

## Building & testing

```bash
cargo test --workspace   # all tests (201 across the workspace)
cargo run -p sabaki-gpui # launch the GPUI client (optional SGF path argument)
SABAKI_CONFIG_DIR=/tmp/sg cargo run -p sabaki-gpui   # isolated config directory
```

On macOS the GPUI client uses the `macos-blade` rendering backend so it builds
with only the Command Line Tools; on Linux/Windows the default backends apply.

## Status

The client covers: SGF open/save with strict `CA`-driven multi-encoding
(UTF-8/Shift_JIS/EUC-JP/GBK/Big5), atomic writes, crash recovery, recent
files, external change detection, dirty-close confirmation, window state
(restored bounds and maximized), themes, a key-table-driven settings panel,
real GTP engine sessions (handshake, board sync, genmove, analysis with
winrate display), and a plugin panel (install-root scan, permission grants,
native authorization, command dispatch).

See `docs/handoff.md` for the detailed snapshot and roadmap.

## License

MIT. This project derives from
[Sabaki](https://github.com/SabakiHQ/Sabaki) and keeps its license and
copyright notice — see `LICENSE.md`.
