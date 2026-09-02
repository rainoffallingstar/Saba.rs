#!/usr/bin/env bash
# Builds a Linux AppImage of the native Ryusei client.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUTPUT_DIR="${1:-$ROOT_DIR/dist/linux}"
APP_NAME="ryusei"
VERSION="$("$ROOT_DIR/scripts/release-version.sh")"
TOOLS_DIR="$ROOT_DIR/target/appimage-tools"
LINUXDEPLOY_URL="https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage"
APPIMAGETOOL_URL="https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage"

cd "$ROOT_DIR"
cargo build --release --locked -p ryusei-gpui --bin "$APP_NAME"
BINARY="$ROOT_DIR/target/release/$APP_NAME"
test -x "$BINARY" || { echo "error: release binary not found: $BINARY" >&2; exit 1; }

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

APPDIR="$ROOT_DIR/target/AppDir"
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/licenses/ryusei" "$APPDIR/usr/share/mime/packages" "$APPDIR/usr/share/icons/hicolor/scalable/apps"
cp "$BINARY" "$APPDIR/usr/bin/$APP_NAME"
cp "$ROOT_DIR/LICENSE.md" "$APPDIR/usr/share/licenses/ryusei/LICENSE.md"
cp "$ROOT_DIR/flatpak/dev.ryusei.app.xml" "$APPDIR/usr/share/mime/packages/dev.ryusei.app.xml"

cat > "$APPDIR/$APP_NAME.desktop" <<DESKTOP
[Desktop Entry]
Name=Ryusei
Comment=Modern Go board and SGF editor
Exec=$APP_NAME %F
Icon=$APP_NAME
Terminal=false
Type=Application
Categories=Game;BoardGame;
MimeType=application/x-go-sgf;application/x-cyberoro-ngf;application/x-tygem-gib;application/x-pandanet-ugf;
StartupWMClass=ryusei
DESKTOP

cp "$ROOT_DIR/fig/ryusei-logo.svg" "$APPDIR/usr/share/icons/hicolor/scalable/apps/$APP_NAME.svg"

APPIMAGE_EXTRACT_AND_RUN=1 "$LINUXDEPLOY" --appdir "$APPDIR" --desktop-file "$APPDIR/$APP_NAME.desktop" --icon-file "$APPDIR/usr/share/icons/hicolor/scalable/apps/$APP_NAME.svg" --executable "$APPDIR/usr/bin/$APP_NAME"
mkdir -p "$OUTPUT_DIR"
OUTPUT="$OUTPUT_DIR/ryusei-v$VERSION-linux-x86_64.AppImage"
APPIMAGE_EXTRACT_AND_RUN=1 "$APPIMAGETOOL" "$APPDIR" "$OUTPUT"
printf 'appimage: %s\n' "$OUTPUT"
