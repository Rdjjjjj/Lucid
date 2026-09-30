#!/bin/zsh
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
BUILD_DIR="$ROOT_DIR/build/ReleaseDerivedData"
DIST_DIR="$ROOT_DIR/dist"
COMPONENT_PLIST="$ROOT_DIR/build/components.plist"
VERSION="${VERSION:-$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$ROOT_DIR/LucidApp/Info.plist")}"
IDENTIFIER="io.github.rdj.lucid.installer"
PKG_PATH="$DIST_DIR/Lucid-$VERSION.pkg"
DMG_PATH="$DIST_DIR/Lucid-$VERSION.dmg"

if [[ ! "$VERSION" =~ '^[0-9]+\.[0-9]+\.[0-9]+([.-][0-9A-Za-z.-]+)?$' ]]; then
  print -u2 "Invalid VERSION: $VERSION"
  exit 1
fi
INPUT_METHOD_VERSION="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$ROOT_DIR/LucidInputMethod/Info.plist")"
if [[ "$INPUT_METHOD_VERSION" != "$VERSION" ]]; then
  print -u2 "Version mismatch: LucidApp=$VERSION, LucidInputMethod=$INPUT_METHOD_VERSION"
  exit 1
fi

mkdir -p "$ROOT_DIR/build" "$DIST_DIR"

# Temporary staging directories are cleaned automatically, including on build failure.
STAGE_DIR="$(mktemp -d "$ROOT_DIR/build/package-root.XXXXXX")"
DMG_ROOT="$(mktemp -d "$ROOT_DIR/build/dmg-root.XXXXXX")"
cleanup() {
  /bin/rm -rf "$STAGE_DIR" "$DMG_ROOT"
}
trap cleanup EXIT

# Build universal binaries (Apple Silicon + Intel) without a developer certificate.
xcodebuild \
  -project "$ROOT_DIR/Lucid.xcodeproj" \
  -scheme LucidApp \
  -configuration Release \
  -sdk macosx \
  -arch arm64 -arch x86_64 \
  -derivedDataPath "$BUILD_DIR" \
  build CODE_SIGNING_ALLOWED=NO

xcodebuild \
  -project "$ROOT_DIR/Lucid.xcodeproj" \
  -scheme LucidInputMethod \
  -configuration Release \
  -sdk macosx \
  -arch arm64 -arch x86_64 \
  -derivedDataPath "$BUILD_DIR" \
  build CODE_SIGNING_ALLOWED=NO

assert_universal_binary() {
  local binary="$1"
  local architectures
  [[ -f "$binary" ]] || { print -u2 "Missing build output: $binary"; exit 1; }
  architectures="$(lipo -archs "$binary")"
  if [[ " $architectures " != *" arm64 "* || " $architectures " != *" x86_64 "* ]]; then
    print -u2 "Expected arm64+x86_64 universal binary, got '$architectures': $binary"
    exit 1
  fi
}

assert_universal_binary "$BUILD_DIR/Build/Products/Release/Lucid.app/Contents/MacOS/Lucid"
assert_universal_binary "$BUILD_DIR/Build/Products/Release/LucidInputMethod.app/Contents/MacOS/LucidInputMethod"

mkdir -p "$STAGE_DIR/Applications" "$STAGE_DIR/Library/Input Methods"
ditto --norsrc "$BUILD_DIR/Build/Products/Release/Lucid.app" "$STAGE_DIR/Applications/Lucid.app"
ditto --norsrc "$BUILD_DIR/Build/Products/Release/LucidInputMethod.app" "$STAGE_DIR/Library/Input Methods/LucidInputMethod.app"
find "$STAGE_DIR" -name '._*' -delete
find "$STAGE_DIR" -name '.DS_Store' -delete

# InputMethodKit requires a complete code signature, including nested frameworks.
codesign --force --deep --sign - "$STAGE_DIR/Library/Input Methods/LucidInputMethod.app"
codesign --force --deep --sign - "$STAGE_DIR/Applications/Lucid.app"
codesign --verify --deep --strict --verbose=1 "$STAGE_DIR/Library/Input Methods/LucidInputMethod.app"
codesign --verify --deep --strict --verbose=1 "$STAGE_DIR/Applications/Lucid.app"

# Keep release artifacts versioned so the GitHub Release and Homebrew cask URL agree.
/bin/rm -f "$COMPONENT_PLIST" "$PKG_PATH" "$DMG_PATH"
pkgbuild --analyze --root "$STAGE_DIR" "$COMPONENT_PLIST"
plutil -replace '0.BundleIsRelocatable' -bool false "$COMPONENT_PLIST"
plutil -replace '1.BundleIsRelocatable' -bool false "$COMPONENT_PLIST"

pkgbuild \
  --root "$STAGE_DIR" \
  --component-plist "$COMPONENT_PLIST" \
  --identifier "$IDENTIFIER" \
  --version "$VERSION" \
  --install-location / \
  "$PKG_PATH"

mkdir -p "$DMG_ROOT"
cp "$PKG_PATH" "$DMG_ROOT/"
cat > "$DMG_ROOT/安装说明.txt" <<INSTALL
Lucid 安装说明

手动安装：
  1. 双击 Lucid-$VERSION.pkg 完成安装（会请求管理员密码）。
  2. 注销并重新登录 macOS。
  3. 打开“系统设置 → 键盘 → 文本输入”，添加并启用 Lucid。
  4. 从“应用程序”打开 Lucid，填写 AI 服务地址和 API Key，获取模型列表后选择模型。

隐私提示：输入内容会发送到你在 Lucid 设置中配置的 AI 服务商。
INSTALL

hdiutil create \
  -volname "Lucid" \
  -srcfolder "$DMG_ROOT" \
  -format UDZO \
  -ov \
  "$DMG_PATH"

printf '\nCreated:\n  %s\n  %s\n' "$PKG_PATH" "$DMG_PATH"
