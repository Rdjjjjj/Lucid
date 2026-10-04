#!/bin/zsh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
INPUT_APP="$ROOT/target/LucidInputMethod.app"
SETTINGS_APP="$ROOT/target/Lucid.app"
BIN="$ROOT/target/release/LucidInputMethod"

VERSION=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$ROOT/apps/macos/Info.plist")
BUILD=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleVersion' "$ROOT/apps/macos/Info.plist")
for key in CFBundleShortVersionString CFBundleVersion; do
  input=$(/usr/libexec/PlistBuddy -c "Print :$key" "$ROOT/apps/macos/Info.plist")
  settings=$(/usr/libexec/PlistBuddy -c "Print :$key" "$ROOT/apps/macos/SettingsInfo.plist")
  [[ "$input" == "$settings" ]] || { echo "输入法和设置 App 版本不一致" >&2; exit 1; }
done
# Lucid has one keyboard input source. A default-enabled child mode can
# reappear after removing the child while its parent remains enabled.
/usr/bin/python3 - "$ROOT/apps/macos/Info.plist" <<'PYPLIST'
import plistlib, sys
with open(sys.argv[1], 'rb') as f:
    info = plistlib.load(f)
assert info['TISInputSourceID'] == 'io.github.rdj.inputmethod.lucid'
assert 'ComponentInputModeDict' not in info, 'Lucid must remain a single input source'
PYPLIST
printf '==> 构建 Lucid %s (%s)（Rust）\n' "$VERSION" "$BUILD"
cargo build --release -p lucid-macos --manifest-path "$ROOT/Cargo.toml"

rm -rf "$INPUT_APP" "$SETTINGS_APP"
mkdir -p "$INPUT_APP/Contents/MacOS" "$INPUT_APP/Contents/Resources"
mkdir -p "$SETTINGS_APP/Contents/MacOS" "$SETTINGS_APP/Contents/Resources"

cp "$BIN" "$INPUT_APP/Contents/MacOS/LucidInputMethod"
cp "$BIN" "$SETTINGS_APP/Contents/MacOS/Lucid"
cp "$ROOT/apps/macos/Info.plist" "$INPUT_APP/Contents/Info.plist"
cp "$ROOT/apps/macos/SettingsInfo.plist" "$SETTINGS_APP/Contents/Info.plist"
cp -R "$ROOT/apps/macos/resources/." "$INPUT_APP/Contents/Resources/"
cp -R "$ROOT/apps/macos/resources/." "$SETTINGS_APP/Contents/Resources/"
chmod 755 "$INPUT_APP/Contents/MacOS/LucidInputMethod" "$SETTINGS_APP/Contents/MacOS/Lucid"

xattr -cr "$INPUT_APP" "$SETTINGS_APP" 2>/dev/null || true
codesign --force --deep --sign - "$INPUT_APP"
codesign --force --deep --sign - "$SETTINGS_APP"
printf '已打包：%s\n已打包：%s\n' "$INPUT_APP" "$SETTINGS_APP"
