#!/usr/bin/env bash
# Emits the release version. CI may override it with APP_VERSION for a tagged
# prerelease; local builds use the single workspace.package value.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ -n "${APP_VERSION:-}" ]]; then
  printf '%s\n' "$APP_VERSION"
  exit 0
fi

sed -n '/^\[workspace.package\]/,/^\[/{s/^version = "\([^"]*\)"/\1/p;}' "$ROOT_DIR/Cargo.toml" | head -n 1
