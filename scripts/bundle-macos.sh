#!/usr/bin/env bash
# Builds the native Ryusei client into a macOS .app and .dmg.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUTPUT_DIR="${1:-$ROOT_DIR/dist/macos}"
APP_NAME="Ryusei"
BUNDLE_ID="dev.ryusei.app"
EXECUTABLE="ryusei"
VERSION="$("$ROOT_DIR/scripts/release-version.sh")"
BUNDLE_DIR="$OUTPUT_DIR/$APP_NAME.app"

cd "$ROOT_DIR"
echo "==> Building Ryusei $VERSION"
cargo build --release --locked -p ryusei-gpui --bin "$EXECUTABLE"
# Honor Cargo's target override so release packaging works in constrained
# workspaces where builds intentionally use a volume with more free space.
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT_DIR/target}"
BINARY="$TARGET_DIR/release/$EXECUTABLE"
test -x "$BINARY" || { echo "error: release binary not found: $BINARY" >&2; exit 1; }

rm -rf "$BUNDLE_DIR"
mkdir -p "$BUNDLE_DIR/Contents/MacOS" "$BUNDLE_DIR/Contents/Resources"
cp "$BINARY" "$BUNDLE_DIR/Contents/MacOS/$EXECUTABLE"

cat > "$BUNDLE_DIR/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>$APP_NAME</string>
  <key>CFBundleDisplayName</key><string>$APP_NAME</string>
  <key>CFBundleIdentifier</key><string>$BUNDLE_ID</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleExecutable</key><string>$EXECUTABLE</string>
  <key>LSMinimumSystemVersion</key><string>12.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSPrincipalClass</key><string>NSApplication</string>
  <key>CFBundleDocumentTypes</key><array>
    <dict><key>CFBundleTypeName</key><string>Smart Game Format</string><key>CFBundleTypeRole</key><string>Editor</string><key>LSHandlerRank</key><string>Owner</string><key>LSItemContentTypes</key><array><string>dev.ryusei.sgf</string></array><key>CFBundleTypeExtensions</key><array><string>sgf</string></array></dict>
    <dict><key>CFBundleTypeName</key><string>CyberOro Go File</string><key>CFBundleTypeRole</key><string>Viewer</string><key>CFBundleTypeExtensions</key><array><string>ngf</string></array></dict>
    <dict><key>CFBundleTypeName</key><string>Tygem Go File</string><key>CFBundleTypeRole</key><string>Viewer</string><key>CFBundleTypeExtensions</key><array><string>gib</string></array></dict>
    <dict><key>CFBundleTypeName</key><string>PandaNet UGF File</string><key>CFBundleTypeRole</key><string>Viewer</string><key>CFBundleTypeExtensions</key><array><string>ugf</string></array></dict>
  </array>
  <key>UTExportedTypeDeclarations</key><array><dict>
    <key>UTTypeIdentifier</key><string>dev.ryusei.sgf</string>
    <key>UTTypeDescription</key><string>Smart Game Format</string>
    <key>UTTypeConformsTo</key><array><string>public.text</string><string>public.data</string></array>
    <key>UTTypeTagSpecification</key><dict><key>public.filename-extension</key><array><string>sgf</string></array><key>public.mime-type</key><array><string>application/x-go-sgf</string></array></dict>
  </dict></array>
</dict></plist>
PLIST

# A Developer ID/notarization flow replaces this ad-hoc signature in P3.
codesign --force --deep -s - "$BUNDLE_DIR"
DMG_PATH="$OUTPUT_DIR/ryusei-v$VERSION-macos.dmg"
rm -f "$DMG_PATH"
if ! hdiutil create -volname "$APP_NAME" -srcfolder "$BUNDLE_DIR" -ov -format UDZO "$DMG_PATH" >/dev/null 2>&1; then
  echo "warning: could not create dmg; the complete .app remains at $BUNDLE_DIR" >&2
fi
printf 'app: %s\n' "$BUNDLE_DIR"
test ! -f "$DMG_PATH" || printf 'dmg: %s\n' "$DMG_PATH"
