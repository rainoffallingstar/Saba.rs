#!/usr/bin/env bash
# Builds a macOS release of the GPUI client and bundles it into a .app
# directory, then packages a .dmg.
#
# Requirements: a working Rust toolchain (release build), and `hdiutil` for
# the dmg step. No Xcode needed — the GPUI client uses the macos-blade
# backend, which builds with only the Command Line Tools.
#
# Usage: scripts/bundle-macos.sh [output-dir]
#   output-dir defaults to "dist/macos".

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUTPUT_DIR="${1:-$ROOT_DIR/dist/macos}"
APP_NAME="Saba.rs"
BUNDLE_DIR="$OUTPUT_DIR/$APP_NAME.app"

echo "==> Building release binary"
cargo build --release -p sabaki-gpui

BINARY="$(find "$ROOT_DIR/target/release" -maxdepth 1 -type f -perm -111 -name 'sabaki-gpui' | head -1)"
if [[ -z "$BINARY" ]]; then
  echo "error: release binary not found" >&2
  exit 1
fi

echo "==> Assembling $BUNDLE_DIR"
rm -rf "$BUNDLE_DIR"
mkdir -p "$BUNDLE_DIR/Contents/MacOS" "$BUNDLE_DIR/Contents/Resources"

cp "$BINARY" "$BUNDLE_DIR/Contents/MacOS/$APP_NAME"

# A minimal Info.plist; the icon is intentionally omitted until real artwork
# exists (the app still launches without one).
cat > "$BUNDLE_DIR/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>$APP_NAME</string>
    <key>CFBundleDisplayName</key>
    <string>$APP_NAME</string>
    <key>CFBundleIdentifier</key>
    <string>dev.saba-rs.app</string>
    <key>CFBundleVersion</key>
    <string>0.1.0</string>
    <key>CFBundleShortVersionString</key>
    <string>0.1.0</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleExecutable</key>
    <string>$APP_NAME</string>
    <key>LSMinimumSystemVersion</key>
    <string>12.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSPrincipalClass</key>
    <string>NSApplication</string>
</dict>
</plist>
PLIST

# Ad-hoc signature so the bundle runs locally without a Developer ID.
echo "==> Ad-hoc signing"
codesign --force --deep -s - "$BUNDLE_DIR"

echo "==> Packaging dmg"
DMG_PATH="$OUTPUT_DIR/$APP_NAME-0.1.0.dmg"
rm -f "$DMG_PATH"
# Some environments (containers, restricted CI sandboxes) cannot create disk
# images; the .app is still complete and valid there, so a dmg failure is
# reported but not fatal.
if ! hdiutil create -volname "$APP_NAME" -srcfolder "$BUNDLE_DIR" -ov -format UDZO "$DMG_PATH" >/dev/null 2>&1; then
  echo "warning: hdiutil could not create the dmg in this environment; the .app bundle is unaffected"
fi

echo "==> Done"
echo "  app: $BUNDLE_DIR"
if [[ -f "$DMG_PATH" ]]; then
  echo "  dmg: $DMG_PATH"
fi
