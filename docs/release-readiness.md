# Saba.rs GPUI Release Readiness

This document is the release-candidate source of truth for the Rust/GPUI
mainline. Detailed findings and the execution plan are in
[`architecture-release-audit-2026-08-21.md`](architecture-release-audit-2026-08-21.md)
and [`release-remediation-plan.md`](release-remediation-plan.md).

The repository root is the active Saba.rs mainline. The legacy Electron/Tauri
Sabaki checkout lives in the Git-ignored `refer-repo/` directory and is used
only for behavior and UI comparison.

## Current automated evidence

Run from the repository root:

```text
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --release --locked -p sabaki-gpui --bin saba-rs
```

The full locked gate is **locally green**. The four commands above were run
serially as one gate twice consecutively, covering 34 domain unit tests, 8
shared fixtures, 5 legacy fixtures, 5 property tests, 141 `sabaki-gpui` tests,
125 `sabaki-host` tests, 26 plugin-runtime tests, integration tests, real
subprocess smoke tests, and all three doctest crates. The latest gate also
covers `PluginController` lifecycle tests, the built-in command registry,
`EngineController` lifecycle/role tests, `AnalysisRunController` ticket-state
tests, and the KataGo model resource seam. The workspace volume ran out of
space during an initial link (`errno=28`); the same serialized gate passed
using the isolated, regenerable `CARGO_TARGET_DIR=/tmp/sabaki-cargo-target`.

The earlier missing-crate doctest failure was not reproduced serially: it was
caused by overlapping Cargo processes competing for the same `target/` build
artifacts. CI and local release validation must therefore keep Cargo gates
serial.

Cargo still reports two **transitive** future-incompatible packages:
`block 0.1.6` (uninhabited static) and `proc-macro-error2 2.0.1` (private
`proc_macro` re-export). Neither is project source; track upstream upgrades or
a scoped dependency patch before a Rust toolchain makes either warning fatal.
The current worktree is not pushed, so historical CI still does not validate it.

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
| Repository authority | Ready | Root is Saba.rs mainline; `refer-repo/` is ignored. Keep CI/Tag/Release here. |
| Formatting, tests, clippy, locked release build | Locally ready | All four commands passed serially; keep CI gates serial. CI now also runs the vendored GPUI patch-contract guard on all three runners. |
| Current commit three-platform CI | Not run | Split/commit/push current work and record the green run URL. |
| Current commit packaging | Not run | Trigger root release workflow; it now generates `SHA256SUMS.txt`. | |
| macOS fullscreen and file-dialog regressions | Passed once, candidate retest required | Re-run `release-qa.md` for every candidate and GPUI upgrade. |
| Application packages | macOS `.app` locally verified; unsigned | Current `.app` passed plist and ad-hoc signature verification; the restricted local environment could not create a dmg. Validate CI `.dmg`, AppImage/tarball, NSIS/zip and Flatpak. |
| macOS signing, hardened runtime, notarization | Not started | Provide Developer ID credentials; notarize, staple and test on a clean Mac. |
| Windows signing | Not started | Provide Authenticode identity and verify SmartScreen behavior. |
| Version and application identity | Implemented locally | Workspace version source, `saba-rs` binary, platform IDs, configuration migration and versioned artifact names are aligned; verify CI artifacts. |
| File associations | Implemented, untested | macOS/Windows/Linux declarations cover SGF/NGF/GIB/UGF; test double-click/install lifecycle on supported platforms. |
| Install, upgrade, rollback and uninstall | Not tested | Test candidate artifacts on clean macOS, Windows and Linux systems. |
| Native screenshot / GPU CI | Partial | Keep full-window test-platform smoke plus manual visual QA; track upstream snapshot support. |
| Electron parity comparison | Not run for candidate | Run identical fixtures against `refer-repo/` and classify every delta. |
| Real GTP engine smoke | KataGo partial evidence; second engine absent | On macOS 26 / Apple M4, Homebrew KataGo 1.17.2 with bundled `kata1-b18c384nbt-s9996604416-d4316597426.bin.gz` completed GTP handshake, 9×9 setup, `play B D4`, and `genmove W` (`F6`) through Metal. `generate_optimized_gtp_config` was corrected for its required logging keys. Still test streaming analysis, stop/detach/reconnect, SGF analysis persistence, abnormal exit, and a second engine. |
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
