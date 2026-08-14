#!/usr/bin/env bash
# Builds a Linux AppImage of the GPUI client.
#
# Two tools are used:
#   1. linuxdeploy assembles the AppDir and bundles the dynamically linked
#      dependencies (fontconfig, freetype, xkbcommon, xcb) of the binary.
#   2. appimagetool turns the AppDir into a self-mounting AppImage.
#
# Usage: scripts/bundle-linux-appimage.sh [output-dir]
#   output-dir defaults to "dist/linux".

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUTPUT_DIR="${1:-$ROOT_DIR/dist/linux}"
APP_NAME="saba-rs"
TOOLS_DIR="$ROOT_DIR/target/appimage-tools"
LINUXDEPLOY_URL="https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage"
APPIMAGETOOL_URL="https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage"

echo "==> Building release binary"
cargo build --release -p sabaki-gpui

BINARY="$ROOT_DIR/target/release/sabaki-gpui"
if [[ ! -x "$BINARY" ]]; then
  echo "error: release binary not found" >&2
  exit 1
fi

echo "==> Downloading AppImage tooling"
mkdir -p "$TOOLS_DIR"
LINUXDEPLOY="$TOOLS_DIR/linuxdeploy-x86_64.AppImage"
APPIMAGETOOL="$TOOLS_DIR/appimagetool-x86_64.AppImage"
if [[ ! -x "$LINUXDEPLOY" ]]; then
  curl -L --fail --silent --show-error -o "$LINUXDEPLOY" "$LINUXDEPLOY_URL"
  chmod +x "$LINUXDEPLOY"
fi
if [[ ! -x "$APPIMAGETOOL" ]]; then
  curl -L --fail --silent --show-error -o "$APPIMAGETOOL" "$APPIMAGETOOL_URL"
  chmod +x "$APPIMAGETOOL"
fi

echo "==> Assembling AppDir"
APPDIR="$ROOT_DIR/target/AppDir"
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/licenses/saba-rs"
cp "$BINARY" "$APPDIR/usr/bin/$APP_NAME"
cp "$ROOT_DIR/LICENSE.md" "$APPDIR/usr/share/licenses/saba-rs/LICENSE.md"

# A minimal desktop entry so the AppImage integrates with launchers.
cat > "$APPDIR/$APP_NAME.desktop" <<DESKTOP
[Desktop Entry]
Name=Saba.rs
Comment=Modern Go board editor
Exec=$APP_NAME
Icon=$APP_NAME
Terminal=false
Type=Application
Categories=Game;BoardGame;
DESKTOP

# SVG icon placeholder (kept minimal; real artwork arrives later).
mkdir -p "$APPDIR/usr/share/icons/hicolor/scalable/apps"
cat > "$APPDIR/usr/share/icons/hicolor/scalable/apps/$APP_NAME.svg" <<SVG
<svg xmlns="http://www.w3.org/2000/svg" width="256" height="256"><rect width="256" height="256" rx="32" fill="#d9a866"/><circle cx="96" cy="96" r="22" fill="#1a1a1a"/><circle cx="160" cy="160" r="22" fill="#1a1a1a"/></svg>
SVG

echo "==> Bundling dependencies into the AppDir"
# APPIMAGE_EXTRACT_AND_RUN avoids FUSE requirements on CI runners.
APPIMAGE_EXTRACT_AND_RUN=1 "$LINUXDEPLOY" \
  --appdir "$APPDIR" \
  --desktop-file "$APPDIR/$APP_NAME.desktop" \
  --icon-file "$APPDIR/usr/share/icons/hicolor/scalable/apps/$APP_NAME.svg" \
  --executable "$APPDIR/usr/bin/$APP_NAME"

echo "==> Producing the AppImage"
mkdir -p "$OUTPUT_DIR"
APPIMAGE_EXTRACT_AND_RUN=1 "$APPIMAGETOOL" \
  "$APPDIR" "$OUTPUT_DIR/saba-rs-linux-x86_64.AppImage"

echo "AppImage written to $OUTPUT_DIR/saba-rs-linux-x86_64.AppImage"
