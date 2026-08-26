#!/usr/bin/env bash
# Rasterizes the SVG logo into a complete macOS .icns file.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SOURCE="${1:-$ROOT_DIR/fig/ryusei-logo.svg}"
OUTPUT="${2:-$ROOT_DIR/dist/Ryusei.icns}"

command -v sips >/dev/null || { echo "error: sips is required" >&2; exit 1; }
command -v iconutil >/dev/null || { echo "error: iconutil is required" >&2; exit 1; }
test -f "$SOURCE" || { echo "error: logo source not found: $SOURCE" >&2; exit 1; }

# iconutil on some macOS versions rejects iconsets under the per-process
# TMPDIR; use the system /tmp root for a stable iconset path.
WORK_DIR="$(mktemp -d /tmp/ryusei-icon.XXXXXX)"
trap 'rm -rf "$WORK_DIR"' EXIT
ICONSET="$WORK_DIR/Ryusei.iconset"
mkdir -p "$ICONSET" "$(dirname "$OUTPUT")"

# The base sizes are accepted by iconutil across older macOS versions;
# omitting 1024px @2x entries also avoids a Ventura-era iconutil failure.
for size in 16 32 128 256 512; do
  sips -s format png -z "$size" "$size" "$SOURCE" \
    --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
done

# Generate in /tmp first; iconutil can fail when its output target is on
# a mounted workspace volume, even though the final copy is valid.
ICNS_TEMP="$WORK_DIR/Ryusei.icns"
iconutil -c icns -o "$ICNS_TEMP" "$ICONSET"
rm -f "$OUTPUT"
cp "$ICNS_TEMP" "$OUTPUT"
printf 'icon: %s\n' "$OUTPUT"
