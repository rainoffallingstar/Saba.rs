#!/usr/bin/env bash
# Verifies the local GPUI patch contract before a framework upgrade or release.
# This is intentionally structural: native AppKit behavior still requires the
# manual macOS scenarios recorded in docs/release-qa.md.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

require() {
  local file="$1"
  local pattern="$2"
  if ! grep -Fq -- "$pattern" "$file"; then
    printf 'GPUI patch guard failed: %s is missing %q\n' "$file" "$pattern" >&2
    exit 1
  fi
}

require Cargo.toml 'gpui = { path = "vendor/gpui" }'
require vendor/gpui/Cargo.toml 'version = "0.2.2"'
test -f vendor/gpui/LICENSE-APACHE
require vendor/gpui/src/platform/mac/window.rs 'fullscreen_exit_in_progress'
require vendor/gpui/src/platform/mac/window.rs 'window_will_exit_fullscreen'
require vendor/gpui/src/platform/mac/window.rs 'update_drawable_size(drawable_size)'
require vendor/gpui/src/app.rs 'try_borrow_mut()'
require vendor/gpui/src/app/test_context.rs 'keyboard_layout'
require vendor/gpui/src/platform/test/platform.rs 'keyboard_layout'

# When Cargo is available, also ensure the locked vendored dependency graph
# resolves. Flatpak's pre-toolchain job intentionally runs this structural guard
# before Rust is installed, so absence of Cargo is not a guard failure there.
if command -v cargo >/dev/null 2>&1; then
  cargo metadata --locked --format-version 1 --no-deps >/dev/null
else
  printf 'Cargo unavailable; skipped locked metadata check.\n'
fi
printf 'GPUI 0.2.2 local patch contract verified.\n'
