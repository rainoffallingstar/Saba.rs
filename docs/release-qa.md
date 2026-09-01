# GPUI Beta Release QA

This checklist separates reproducible automated evidence from the final manual
comparison against the retained Electron reference. Do not mark a manual item
complete merely because an equivalent Rust unit test passes.

## Automated gate

Run from the repository root before packaging:

```text
cargo fmt --all -- --check
cargo test -p ryusei-gpui --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --release --locked -p ryusei-gpui --bin ryusei
```

The root workflows `.github/workflows/ci.yml` and
`.github/workflows/release.yml` run the Rust/GPUI mainline directly. CI tests
Ubuntu, macOS and Windows; the release workflow builds macOS `.app/.dmg`, Linux
tarball/AppImage, Windows zip/NSIS and Flatpak artifacts. These artifacts remain
unsigned until the signing gates below are completed.

The GPUI package suite includes these release-relevant paths:

- shared domain differential fixtures: linear games, captures, rectangular
  boards/pass, setup/markup, variation promotion/history and escaped Unicode
  properties;
- legacy NGF/GIB/UGF imports and strict SGF encoding round-trips;
- `headless_release_fixture_workflow_opens_edits_saves_and_reopens`, which
  drives `ShellApp` through native file access: open SGF → legal move → comment
  transaction → atomic save → reopen and assert both move and comment;
- `frontend_smoke`, which renders the full window on GPUI's test platform and
  exercises board hit testing, navigation, panes, drawers, graph/context
  interactions and persisted splitter sizing;
- `sound_feedback`: `sound.enable` gates cues, and the platform sink initializes
  without making an audio device a test prerequisite.

## macOS manual smoke

Run a release bundle on a machine with normal system audio. Confirm:

1. Open the same fixture set used by the automated gates.
2. Make a legal stone move and pass with `sound.enable` on: hear the two
   distinct system sounds. Turn the setting off and confirm both are silent.
3. Open/save/reopen a UTF-8 and a declared Shift_JIS SGF; verify comments,
   variations, markup and result are retained.
4. Exercise CommentBox and node title with a system IME, selection, undo/redo,
   Enter submit and Escape cancel.
5. Configure KataGo through Setup Hub, select it as **Analysis**, then connect
   it. A fresh profile now automatically keeps the engine sidebar and board
   analysis markers visible, and connecting the Analysis role automatically
   starts the live stream. Confirm candidates appear in all three places:
   board recommendation markers, the **LIVE KATAGO ANALYSIS** list, and the
   win-rate graph. Stop analysis, make a move, and verify that a fresh analysis
   run supersedes the old candidates; then save/reopen the generated SGF
   properties. KataGo's official GTP syntax is `kata-analyze [player]
   [interval-centiseconds] [key value ...]`, for example `kata-analyze B 10
   rootInfo true`; it streams `info move ...` records until an explicit `stop`.
   Do not configure unrecognized flags such as `-visits 100` as `kata-analyze`
   arguments.
6. In the Fox plugin card, enter a real Fox username or numeric ID, then press
   Enter or click **查询并导入最新对局**. Verify the returned latest game imports
   into the board. An empty query must show a prompt and must never download an
   unrelated hard-coded game.
7. Resize sidebars, use GameGraph right-click hotspot mutation, and switch
   Classic/Dark/Mist before and after an application restart.
8. On macOS 26, enter and exit native fullscreen at least five times, including
   after resizing the window and switching displays when possible. The
   application must remain alive on each exit; this directly covers the GPUI
   Blade drawable-resize crash recorded in the release candidate. A later
   crash proved the titlebar-only workaround insufficient; the current
   drawable-resize deferral passed a fresh manual release-binary retest.
   Retain the step for every new GPUI/versioned release candidate.
9. On macOS 26, choose **File → Open** and **File → Save As**. While each
   native panel is open, change keyboard input source when practical; cancel
   once and complete the open/save workflow once. The application must not
   abort, which covers the GPUI keyboard-layout notification reentrancy patch.
   This passed its initial manual release-binary retest; retain it for every
   macOS release candidate.
10. **Audio cues**: In a game with Japanese byo-yomi time control, let the clock
    enter byo-yomi and run down to <= 10s. Confirm a per-second countdown tick
    sounds (`Morse.aiff`), and a distinct timeout chime sounds (`Sosumi.aiff`)
    when the final period expires.
11. **Fischer time control**: Open **对局设置** from the match capsule, choose
    **10m + 10s 加秒**, make several moves, and verify that the active clock
    receives a +10s increment after each committed move. Save the SGF and
    verify `TM` and `OT[fischer +10s]` round-trip.
12. **Markdown commentary preview**: In the comment inspector, type markdown
    content (`# Title`, `**bold**`, `` `code` ``, fenced code blocks, `- lists`).
    Click the **预览 / 编辑** toggle (Eye icon) and verify the rendered view
    matches the formatting.
13. **Variation tree structure actions**: Right-click a branch node in the
    horizontal variation tree. Click **设为主干 (promote)** to promote it to
    the primary line, and click **删除分支 (delete)** to prune a branch.
14. **Keyboard Tab navigation**: Press `Tab` and `Shift-Tab` to cycle focus
    through registered inputs (comment box, GTP input, search fields). Confirm
    focused inputs show the 3px accent focus ring.
15. **Responsive breakpoints**: Resize the window below 1024px width; verify
    the sidebars automatically cap to 210px (left) and 260px (right). Resize
    below 840px width and verify the VS-pill clock chips hide gracefully.
16. **Monte-Carlo scoring**: Enter Scoring mode with `score.estimator_iterations`
    configured; confirm dead stones are marked according to playout survival
    rates and manual overrides remain authoritative.

## Electron reference comparison

Electron remains the stable reference until this checklist is completed on a
release candidate. Perform the manual smoke above in both applications using
identical fixture files. Record each difference as one of:

- **functional defect** — different document, command result or persistence;
- **intentional scope difference** — documented missing host workflow, such as
  multi-document GameChooser, bulk Clean Markup or constrained Advanced
  Properties;
- **visual/polish difference** — GPUI appearance differs without changing the
  document or user action result.

Attach the fixture name, exact steps, OS version, GPUI build identifier and a
screenshot or screen recording to the release issue for every non-intentional
difference.

## Known boundary

GPUI 0.2.2 has no stable offscreen screenshot API. The automated evidence is a
full-window render/input smoke on the GPUI test platform, not pixel-golden
screenshots. Keep Electron available and require the manual visual comparison
above until GPUI offers a stable snapshot interface or an independently
maintained offscreen renderer is adopted.
