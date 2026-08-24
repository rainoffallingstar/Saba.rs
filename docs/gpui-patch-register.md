# GPUI Local Patch Register

This register governs the `gpui` crates.io override in the root
[`Cargo.toml`](../Cargo.toml). It makes the vendor dependency auditable during
release work and upgrades; it does **not** claim that a source-level check can
replace native macOS regression testing.

## Scope and baseline

| Field | Value |
|---|---|
| Dependency | `gpui` |
| Locked version | `0.2.2` |
| Vendor baseline | `vendor/gpui` git `2ceed723` |
| Cargo seam | `[patch.crates-io] gpui = { path = "vendor/gpui" }` |
| License retained | Apache-2.0 — `vendor/gpui/LICENSE-APACHE` |
| Structural guard | `scripts/verify-gpui-patch.sh` |

Run this before each framework upgrade and in the release candidate evidence
set:

```bash
scripts/verify-gpui-patch.sh
```

It checks that Cargo still routes through the vendor copy, that the intended
base version remains `0.2.2`, that the Apache license is retained, and that the
specific patch seams exist. When Cargo is available, it also runs locked Cargo
metadata; the pre-toolchain Flatpak job intentionally performs the structural
checks without Cargo. It is a guard against silently dropping or broadening a
patch, not a behavioral test.

## Patch inventory

| Patch / touched path | Problem and constrained behavior | Automated evidence | Native evidence required | Upstream / removal condition |
|---|---|---|---|---|
| `src/platform/mac/window.rs` — fullscreen drawable resize deferral | Between `windowWillExitFullScreen:` and `windowDidExitFullScreen:`, suppress Blade `update_drawable_size` calls from frame/backing resize; update once at final content size. Prevents macOS 26 AppKit style-mask transition re-entering drawable destruction. | Structural guard; workspace frontend smokes compile against the patched GPUI. | Each RC: ≥5 native fullscreen enter/exit cycles, resize and multi-display path when available; see [`release-qa.md`](release-qa.md). | No upstream issue/PR URL has been recorded yet. Remove only after an upstream GPUI release is tested on macOS 26 for this exact Blade transition. Details: [`gpui-macos-fullscreen-workaround.md`](gpui-macos-fullscreen-workaround.md). |
| `src/app.rs`, `src/app/test_context.rs`, `src/platform/test/platform.rs` — keyboard-layout reentrancy | Replace notification observer's unconditional `App::borrow_mut()` with `try_borrow_mut()` and expose a test-platform injection seam. A nested native Open/Save panel notification skips one refresh instead of aborting on an active action borrow. | `frontend_smoke::keyboard_layout_notification_does_not_abort_during_an_app_action`; structural guard. | Each macOS RC: Open/Save As while changing input source, then actual open/save; see [`release-qa.md`](release-qa.md). | No upstream issue/PR URL has been recorded yet. Remove only after upstream uses a safe reentrant borrow strategy and the RC scenario passes without this patch. Details: [`gpui-macos-file-dialog-workaround.md`](gpui-macos-file-dialog-workaround.md). |
| `src/taffy.rs` — explicit grid float type | Pins the grid literal type so the vendored GPUI compiles cleanly under strict local Rust linting/future-incompat checks. | `cargo clippy --workspace --all-targets -- -D warnings`. | None beyond ordinary CI compile. | Remove when an upgraded upstream source contains equivalent explicit typing. |
| `src/platform/test/platform.rs` — test-platform hooks | Keeps upstream-style test-only hooks required to inject the notification regression path. It must not affect native platform behavior. | Frontend smoke named above. | N/A; native evidence belongs to the dialog patch. | Remove when upstream exposes an equivalent supported test seam. |

## Upgrade and release procedure

1. Record the proposed upstream GPUI revision and compare its license files.
2. Run `scripts/verify-gpui-patch.sh` before changing the baseline; record its
   output in the candidate evidence.
3. Rebase each inventory row independently. Do not carry forward unrelated
   vendor edits merely to make a merge easy.
4. Run the serial locked quality gate with an isolated `CARGO_TARGET_DIR` when
   the workspace volume lacks artifact space:

   ```bash
   CARGO_TARGET_DIR=/tmp/sabaki-cargo-target \
     cargo fmt --all -- --check && \
   CARGO_TARGET_DIR=/tmp/sabaki-cargo-target \
     cargo test --workspace --locked && \
   CARGO_TARGET_DIR=/tmp/sabaki-cargo-target \
     cargo clippy --workspace --all-targets -- -D warnings && \
   CARGO_TARGET_DIR=/tmp/sabaki-cargo-target \
     cargo build --release --locked -p sabaki-gpui --bin saba-rs
   ```

5. Run the test-platform smokes and the two native macOS manual scenarios.
6. Update this register with upstream issue/PR URLs as soon as they are opened;
   none are known in the current worktree, so do not imply upstream acceptance.
7. Delete a patch instead of retaining a no-op compatibility layer once the
   upstream release has passed the corresponding native and automated evidence.

## Ownership rule

The vendor directory is a critical release dependency, not application source.
It must be split into its own reviewable commit before publication (P0.2), and
the patch-only commit must contain this register, the guard script, and no
unrelated product behavior.
