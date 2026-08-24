# macOS 26 Fullscreen Exit Workaround

## Symptom

The first M6 manual build crashed while exiting native macOS fullscreen on an
Apple M4 running macOS 26.6.1. The crash report's triggering path was:

```text
AppKit NSWindow setStyleMask (fullscreen exit)
  → GPUI mac window set_frame_size
  → BladeRenderer::update_drawable_size
  → objc_release / EXC_BAD_ACCESS
```

The report contains no Sabaki action, game transaction or sound frame. It is a
GPUI 0.2.2 macOS Blade renderer lifecycle failure during the AppKit transition.

## Patch

The workspace now patches crates.io GPUI 0.2.2 through `vendor/gpui` (declared
in the root `Cargo.toml`). The first patch stopped GPUI itself from restoring
`titlebarAppearsTransparent` during fullscreen exit, but a later crash report
proved that macOS's own `_NSExitFullScreenTransitionController` still changes
the window style mask and resizes the content view.

The effective patch marks `windowWillExitFullScreen:` through
`windowDidExitFullScreen:` as an unsafe Blade transition. GPUI defers all
`update_drawable_size` calls from `setFrameSize:` and backing-property changes
while that flag is set, then recreates the drawable once using the final
content size in `windowDidExitFullScreen:`. This moves Blade's Metal resource
destruction/reconfiguration outside the AppKit style-mask transition.

This is deliberately a narrow, macOS-only renderer lifecycle change. It does
not affect Sabaki document state, host transactions, GTP sessions, sound
feedback, or normal window resizing.

## Rejected fallback

Disabling `macos-blade` would select GPUI's legacy native Metal renderer, but
GPUI 0.2.2 compiles Metal shaders using the `metal` tool. The manual machine
has Command Line Tools only, which lacks that tool; the fallback therefore
fails at GPUI build time and is not a viable release mitigation.

## Verification

Automated coverage verifies the patched dependency compiles and full-window
frontend smokes pass. The local patch also types GPUI's two grid float literals
explicitly, avoiding a Rust future-incompat warning. AppKit fullscreen
transitions cannot run under GPUI's test platform.

**Important:** an earlier manual retest did not reproduce the crash, but a
later macOS 26 release-binary crash proved that the titlebar-only patch was
insufficient. The drawable-resize deferral was subsequently retested with the
new release binary: repeated native fullscreen enter/exit did not reproduce
the crash. Keep the test in `docs/release-qa.md` for every release candidate,
including resizing/display changes when available.

Remove this patch only after validating an upstream GPUI version on macOS 26
that fixes the Blade drawable-resize lifecycle.
