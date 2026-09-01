# Ryusei GPUI Release Readiness

This document is the release-candidate source of truth for the Rust/GPUI
mainline. Detailed findings and the execution plan are in
[`architecture-release-audit-2026-08-21.md`](architecture-release-audit-2026-08-21.md)
and [`release-remediation-plan.md`](release-remediation-plan.md).

The repository root is the active Ryusei mainline. The legacy Electron/Tauri
Sabaki checkout lives in the Git-ignored `refer-repo/` directory and is used
only for behavior and UI comparison.

## Current automated evidence

Run from the repository root:

```text
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --release --locked -p ryusei-gpui --bin ryusei
```

The full locked gate is **locally green**. The four commands above were run
serially as one gate twice consecutively, covering 34 domain unit tests, 8
shared fixtures, 5 legacy fixtures, 5 property tests, 141 `ryusei-gpui` tests,
125 `ryusei-host` tests, 26 plugin-runtime tests, integration tests, real
subprocess smoke tests, and all three doctest crates. The latest gate also
covers `PluginController` lifecycle tests, the built-in command registry,
`EngineController` lifecycle/role tests, `AnalysisRunController` ticket-state
tests, and the KataGo model resource seam. The workspace volume ran out of
space during an initial link (`errno=28`); the same serialized gate passed
using the isolated, regenerable `CARGO_TARGET_DIR=/tmp/ryusei-cargo-target`.

### 2026-08-31 worktree gate (after UI-alignment + ShellApp refactor)

After the Apple-design UI alignment series (theme tokens, goban skins, icons,
animations, top-bar merge, bottom-deck slimming, focus navigation, responsive
breakpoints), the panels split (`panels/{mod,drawers,engine_panels,plugin_dialogs}.rs`),
the `ui_format`/`markdown`/`text_inputs` extraction, and the backend rule work
(positional superko + Monte-Carlo scoring), the four gates were re-run on the
current worktree:

- `cargo fmt --all -- --check` — clean (reformatted icons/goban after edits);
- `cargo clippy --workspace --all-targets -- -D warnings` — clean (fixed 7
  warnings: 2 redundant `usize` casts, 1 `while let`→`for`, 3 identical `if`
  branches, 1 unneeded `mut`, 1 `clone`-to-slice in the superko test);
- `cargo test --workspace` — green: domain-core 77 unit + 8 diff + 5 legacy +
  5 proptest, `ryusei-gpui` 173 (up from 141), `ryusei-host` 236 (up from 125),
  plugin-runtime 27, plus integration/real-subprocess smoke;
- `cargo build --release --locked -p ryusei-gpui --bin ryusei` — clean.

The only remaining warnings are the two **transitive** future-incompatible
packages (`block 0.1.6`, `proc-macro-error2 2.0.1`), not project source.

The latest candidate commit `1cc6404c44e99f4d1e2e9c4d9adbc3291244c4b6` passed the
three-platform CI matrix in run
[`33466668494`](https://github.com/rainoffallingstar/ryusei/actions/runs/33466668494).
The release workflow in run
[`33467089572`](https://github.com/rainoffallingstar/ryusei/actions/runs/33467089572)
passed macOS (`.app` + `.dmg`), Windows (`.zip` + NSIS `setup.exe`), Ubuntu (`.tar.gz` + `.AppImage`)
and Flatpak packaging for this same commit.

The earlier candidate commit `8880412e54bef87ddf3091e0f9cc830696c067b0` passed historical runs
[`32693487577`](https://github.com/rainoffallingstar/Ryusei/actions/runs/32693487577) and
[`32693823374`](https://github.com/rainoffallingstar/Ryusei/actions/runs/32693823374).

The earlier missing-crate doctest failure was not reproduced serially: it was
caused by overlapping Cargo processes competing for the same `target/` build
artifacts. CI and local release validation must therefore keep Cargo gates
serial.

Cargo still reports two **transitive** future-incompatible packages:
`block 0.1.6` (uninhabited static) and `proc-macro-error2 2.0.1` (private
`proc_macro` re-export). Neither is project source; track upstream upgrades or
a scoped dependency patch before a Rust toolchain makes either warning fatal.

### Final candidate artifact evidence

The final release workflow produced and locally validated these artifacts for
commit `8880412e`:

| Artifact | SHA-256 |
|---|---|
| `ryusei-v0.1.0-macos.dmg` | `98ee3e8f94a592553693b725aca3938f10bf2b245a635dca68d4d1717408af25` |
| `ryusei-v0.1.0-linux-x86_64.tar.gz` | `e9639f9014c8bf7c23f2804d4e2a3fa76370913ef2d90ec964f4f282064bd245` |
| `ryusei-v0.1.0-linux-x86_64.AppImage` | `a3261f451777744c912f3f28ec46ec5574b738783dc4a545b30756b78580f693` |
| `ryusei-v0.1.0-linux-x86_64.flatpak` | `9c3a109705b2b48a06a0ddce1516760e512851a914a0821c97110228fd9a7f08` |
| `ryusei-v0.1.0-windows-x86_64.zip` | `e9d87a962625f1e6b5e4c9831c0ba5326eb7ca37593f4485e91d6b1232b613bd` |
| `ryusei-v0.1.0-windows-x86_64-setup.exe` | `c45b8280d2e703013507c8acd4a1b495a89aba59315924e9c6acf7677340cb48` |

The ZIP passed `unzip -t`; the Linux tarball passed `tar -tzf`; `file`
identified the expected ELF, PE, Flatpak data and DMG payload types. This is
packaging evidence, not clean-machine install, upgrade, uninstall, signing,
notarization or end-user QA evidence.

## CI and packaging evidence

Root workflows:

- `.github/workflows/ci.yml`: Ubuntu, macOS and Windows GPUI patch-contract guard, formatting, strict lint, and workspace tests;
- `.github/workflows/release.yml`: runs the same GPUI patch-contract guard before
  macOS `.app/.dmg`, Linux tarball/AppImage, Windows zip/NSIS and Flatpak.

Historical workflow runs have successfully generated all packaging formats.
This proves that the pipeline shape works, but not that the current uncommitted
candidate builds on every runner. There is no published GitHub Release yet.

## Release gates

| Gate | Status | Required next evidence |
|---|---|---|
| Repository authority | Ready | Root is Ryusei mainline; `refer-repo/` is ignored. Keep CI/Tag/Release here. |
| Formatting, tests, clippy, locked release build | Locally ready | All four commands passed serially; keep CI gates serial. CI now also runs the vendored GPUI patch-contract guard on all three runners. |
| Current commit three-platform CI | Passed | Commit `1cc6404c44e99f4d1e2e9c4d9adbc3291244c4b6` passed Ubuntu/macOS/Windows in run [`33466668494`](https://github.com/rainoffallingstar/ryusei/actions/runs/33466668494). |
| Current commit packaging | Passed | Same commit passed macOS/Windows/Ubuntu/Flatpak release packaging in run [`33467089572`](https://github.com/rainoffallingstar/ryusei/actions/runs/33467089572). |
| macOS fullscreen and file-dialog regressions | Passed once, candidate retest required | Re-run `release-qa.md` for every candidate and GPUI upgrade. |
| Application packages | Structure verified; unsigned | Final workflow artifacts passed ZIP and tar listings, executable/file-type checks, plus DMG attach. Mounted `.app` passed `plutil` and `codesign --verify --deep --strict`. AppImage and Flatpak cannot execute/import on this ARM macOS host; CI built them successfully. |
| macOS signing, hardened runtime, notarization | Not started | The DMG app is ad-hoc-valid but `spctl --assess` rejects it, as expected for an unsigned candidate. Provide Developer ID credentials; notarize, staple and test on a clean Mac. |
| Windows signing | Not started | Provide Authenticode identity and verify SmartScreen behavior. |
| Version and application identity | Implemented and artifact-checked | Workspace version source, `ryusei` binary, platform IDs, configuration migration and versioned artifact names are aligned; downloaded candidate artifacts use `v0.1.0` consistently. |
| File associations | Implemented, package declarations checked | macOS/Windows/Linux declarations cover SGF/NGF/GIB/UGF. Real double-click behavior remains an installed-platform test. |
| Install, upgrade, rollback and uninstall | Not tested | Test candidate artifacts on clean macOS, Windows and Linux systems. |
| Native screenshot / GPU CI | Partial | Keep full-window test-platform smoke plus manual visual QA; track upstream snapshot support. |
| Electron parity comparison | Automated fixture provenance verified | The Rust legacy suite uses byte-identical copies of the Electron reference fixtures (`even.ngf`, `handicap2.ngf`, `utf8.gib`, `amateur.ugf`) and asserts reference move/metadata expectations. GUI visual and interactive comparison still requires manual runs in both applications. |
| Real GTP engine smoke | KataGo streaming and second-engine baseline passed | On macOS 26 / Apple M4, Homebrew KataGo 1.17.2 loaded the downloaded official lightweight model (SHA-256 `0ba27e…c1229`), replayed 9×9 `play B D4`, streamed documented `kata-analyze B 10` GTP `info move` records, accepted `stop`, and answered a later `protocol_version`. Homebrew GNU Go 3.8 independently completed handshake/replay/genmove (`F7`) but does not support `lz-analyze`. Still test GUI analysis persistence, abnormal exit recovery, and installed-package interaction. |
| Local GPUI patches | Governed locally; upstream tracking absent | `gpui-patch-register.md` + `scripts/verify-gpui-patch.sh` record baseline, license, seams and removal criteria. Guard runs in CI plus release/Flatpak jobs (structural-only if their toolchain is not installed); still obtain upstream issue/PR URLs and repeat native macOS scenarios each RC. |

## Explicit non-claims

- Passing ordinary tests does not mean `cargo test --workspace` is green when
  doctests fail.
- A historical CI or release run does not validate the current candidate.
- An unsigned package is not a notarized/signed end-user release.
- GPUI test-platform rendering does not prove native AppKit, GPU, sound,
  file-dialog, real-engine or visual behavior.

## Suggested release sequence

1. Complete P0 in `release-remediation-plan.md`: split the worktree and restore
   all four quality gates.
2. Push the candidate and make the root three-platform CI matrix green.
3. Run the root release workflow and retain checksums for all artifacts.
4. Unify version/application identity and complete file associations.
5. Test installation, upgrade, rollback and uninstall on clean systems.
6. Run `release-qa.md`, real-engine smoke and Electron reference comparison.
7. Publish `v0.1.0-beta.1` only after every blocking row has dated evidence.
