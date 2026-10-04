#!/bin/zsh
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
DIST_DIR="$ROOT_DIR/dist"
VERSION="${VERSION:-$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$ROOT_DIR/apps/macos/Info.plist")}"
PKG_PATH="$DIST_DIR/Lucid-$VERSION.pkg"
if [[ ! "$VERSION" =~ '^[0-9]+\.[0-9]+\.[0-9]+([.-][0-9A-Za-z.-]+)?$' ]]; then
  print -u2 "Invalid VERSION: $VERSION"
  exit 1
fi

for plist in "$ROOT_DIR/apps/macos/Info.plist" "$ROOT_DIR/apps/macos/SettingsInfo.plist" "$ROOT_DIR/LucidApp/Info.plist" "$ROOT_DIR/LucidInputMethod/Info.plist"; do
  actual="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$plist")"
  [[ "$actual" == "$VERSION" ]] || { print -u2 "Version mismatch: $plist=$actual, expected $VERSION"; exit 1; }
done

mkdir -p "$DIST_DIR"
"$ROOT_DIR/apps/macos/scripts/package.sh"
[[ -s "$PKG_PATH" ]] || { print -u2 "Missing package: $PKG_PATH"; exit 1; }
printf '\nCreated:\n  %s\n' "$PKG_PATH"
