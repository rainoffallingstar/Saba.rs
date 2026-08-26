# Ryusei (流星)

A native Go / Baduk / Weiqi board and SGF editor, written in **Rust + GPUI**.

Ryusei is a Rust + GPUI port of [Sabaki](https://github.com/SabakiHQ/Sabaki) —
the elegant open-source Go board editor. It keeps Sabaki's document model and
editor philosophy while replacing the Electron/Node.js stack with a fully
native, GPU-rendered client.

## Features

- **Native GPUI client** — Metal (blade) rendering on macOS, no WebView/Node/Electron.
- **Full SGF editor** — open/save with strict `CA`-driven multi-encoding
  (UTF-8 / Shift_JIS / EUC-JP / GBK / Big5), atomic writes, crash recovery,
  recent files, external change detection, dirty-close confirmation.
- **KaTrain-style analysis UX**
  - Horizontal variation tree with AI move-quality colors and move numbers.
  - AI candidate list: click a candidate to preview its PV line on the board.
  - OGS-style winrate / score-lead graph with coordinate ticks and dual
    advantage shading.
  - Whole-game AI review with move grading (Best / Good / Inaccuracy /
    Mistake / Blunder) and per-player loss statistics.
- **KataGo integration** — engine discovery, one-click setup and model
  download, live analysis, GTP terminal in a bottom deck, analysis visits
  control, AI move generation.
- **Bottom deck workspace** — toolbar buttons reveal a second screen where the
  GTP terminal and every plugin card register as switchable tabs, without
  covering the board.
- **Plugin system** — sandboxed plugin runtime (WASM + native processes),
  permission model, install from ZIP, pinned toolbar shortcuts.
- **Markdown comments** — per-node comments render and edit inline.
- **Fox Go (野狐) sync** — query a player and pull recent games.

## Workspace layout

```
crates/domain-core       UI-independent domain core: GameDocument / SGF / board / GTP / DTOs
crates/plugin-runtime    Plugin manifests, permission model, JSON-RPC framing, native processes
crates/ryusei-host       UI-independent application workflows (open/save/recovery/settings/engines/plugins)
apps/ryusei-gpui         The native GPUI client
examples/                Fake GTP engine for subprocess smoke tests; example plugins
```

UI-independent logic lives in `crates/ryusei-host`, driven through typed ports
so every layer stays testable without a window.

## Building & testing

```bash
cargo test --workspace              # full workspace test and doctest gate
cargo run -p ryusei-gpui            # launch the native client (optional SGF path argument)
RYUSEI_CONFIG_DIR=/tmp/sg cargo run -p ryusei-gpui   # isolated config directory
```

On macOS the GPUI client uses the `macos-blade` rendering backend, so it builds
with only the Command Line Tools; on Linux/Windows the default backends apply.

## Packaging

- macOS: `./scripts/bundle-macos.sh` → `dist/macos/Ryusei.app`
- Linux: `./scripts/bundle-linux-appimage.sh`
- Windows: `scripts/installer.nsi` (NSIS)
- Flatpak: `flatpak/dev.ryusei.app.yml`

## License

MIT. This project is a Rust + GPUI port of
[Sabaki](https://github.com/SabakiHQ/Sabaki) and keeps its license and
copyright notice — see `LICENSE.md`.
