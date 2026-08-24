# macOS Native File Dialog Keyboard-Layout Reentrancy Workaround

## Symptom

On macOS 26.6.1, choosing **File → Open** could abort Sabaki while the native
Open panel was visible. The release crash report shows this exact path:

```text
OpenGame action → ShellApp::open → rfd::FileDialog::pick_file
  → NSOpenPanel runModal (nested AppKit event loop)
  → NSSelectedKeyboardInputSourcesChangedNotification
  → GPUI on_keyboard_layout_change
  → App borrow_mut → RefCell already borrowed → SIGABRT
```

This is unrelated to SGF contents or file decoding. The native modal dialog
allows AppKit notifications to run while GPUI still holds the mutable `App`
borrow used to dispatch the menu action.

## Patch

The local GPUI patch now registers its keyboard-layout observer using
`AppCell::try_borrow_mut()` rather than an unconditional `borrow_mut()`. If a
native modal loop delivers the notification reentrantly, the observer skips
that individual refresh instead of panicking and aborting the process. A later
keyboard-layout notification refreshes the cached layout normally.

This patch is deliberately confined to the framework notification boundary; it
does not alter Sabaki's `DialogService`, game/file workflow, GTP sessions, or
settings state.

## Regression coverage

`frontend_smoke::keyboard_layout_notification_does_not_abort_during_an_app_action`
uses GPUI's test platform to hold the same mutable `App` borrow as a menu
action and then inject a keyboard-layout notification. With the original
unconditional borrow, the test deterministically fails with `RefCell already
borrowed`. With the patch it passes.

## Manual verification

The native `NSOpenPanel` nested run loop cannot be created by GPUI's test
platform. The initial macOS release-binary retest was completed after the
patch: Open and Save As remained alive while the keyboard input source changed,
and actual open/save operations succeeded. Retain this check for every macOS
release candidate:

1. Choose **File → Open** and leave the native panel visible briefly.
2. Change keyboard input source while it is open when practical.
3. Cancel the panel, then repeat and open an SGF.
4. Choose **File → Save As**, repeat the input-source change when practical,
   and save a file.
5. Confirm no abort occurs and the opened/saved file workflow still works.
