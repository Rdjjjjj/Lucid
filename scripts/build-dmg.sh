#!/bin/zsh
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
BUILD_DIR="$ROOT_DIR/build/ReleaseDerivedDataFixed"
DIST_DIR="$ROOT_DIR/dist"
COMPONENT_PLIST="$ROOT_DIR/build/components.plist"
VERSION="0.1.3"
IDENTIFIER="io.github.rdj.englishinput.installer"

# 每次构建使用全新的临时目录，避免清理旧文件的 rm -rf
STAGE_DIR="$(mktemp -d "$ROOT_DIR/build/package-root.XXXXXX")"
DMG_ROOT="$(mktemp -d "$ROOT_DIR/build/dmg-root.XXXXXX")"

mkdir -p "$ROOT_DIR/build" "$DIST_DIR"

# 构建 universal（Apple Silicon + Intel），先不签名，稍后统一 ad-hoc 签名
xcodebuild \
  -project "$ROOT_DIR/EnglishInput.xcodeproj" \
  -scheme EnglishInputApp \
  -configuration Release \
  -sdk macosx \
  -arch arm64 -arch x86_64 \
  -derivedDataPath "$BUILD_DIR" \
  build CODE_SIGNING_ALLOWED=NO

xcodebuild \
  -project "$ROOT_DIR/EnglishInput.xcodeproj" \
  -scheme EnglishInputMethod \
  -configuration Release \
  -sdk macosx \
  -arch arm64 -arch x86_64 \
  -derivedDataPath "$BUILD_DIR" \
  build CODE_SIGNING_ALLOWED=NO

mkdir -p "$STAGE_DIR/Applications" "$STAGE_DIR/Library/Input Methods"
ditto --norsrc "$BUILD_DIR/Build/Products/Release/EnglishInput.app" "$STAGE_DIR/Applications/EnglishInput.app"
ditto --norsrc "$BUILD_DIR/Build/Products/Release/EnglishInputMethod.app" "$STAGE_DIR/Library/Input Methods/EnglishInputMethod.app"
find "$STAGE_DIR" -name '._*' -delete
find "$STAGE_DIR" -name '.DS_Store' -delete

# 关键：完整 ad-hoc 签名。输入法必须完整签名（Info.plist 绑定 + 嵌套 framework 签名），
# 否则 InputMethodKit 不会把它注册为可用输入法。
codesign --force --deep --sign - "$STAGE_DIR/Library/Input Methods/EnglishInputMethod.app"
codesign --force --deep --sign - "$STAGE_DIR/Applications/EnglishInput.app"

# 自检：确认嵌套代码通过严格校验
codesign --verify --deep --strict --verbose=1 "$STAGE_DIR/Library/Input Methods/EnglishInputMethod.app"

pkgbuild --analyze --root "$STAGE_DIR" "$COMPONENT_PLIST"
plutil -replace '0.BundleIsRelocatable' -bool false "$COMPONENT_PLIST"
plutil -replace '1.BundleIsRelocatable' -bool false "$COMPONENT_PLIST"

pkgbuild \
  --root "$STAGE_DIR" \
  --component-plist "$COMPONENT_PLIST" \
  --identifier "$IDENTIFIER" \
  --version "$VERSION" \
  --install-location / \
  "$DIST_DIR/EnglishInput.pkg"

mkdir -p "$DMG_ROOT"
cp "$DIST_DIR/EnglishInput.pkg" "$DMG_ROOT/"
cat > "$DMG_ROOT/安装说明.txt" <<'EOF'
English Input 安装说明

推荐方式（已安装 Homebrew 的用户）：
  brew install --cask rdj/tap/english-input
  或按仓库 README 中的 cask 命令安装。

手动安装（双击本 pkg）：
  1. 双击 EnglishInput.pkg 完成安装（会请求管理员密码）。
  2. 注销并重新登录。
  3. 打开“系统设置 → 键盘 → 文本输入 → 编辑”，启用 English Input。
  4. 从“应用程序”打开 EnglishInput，填写 AI 中转站、模型和 API Key。
EOF

hdiutil create \
  -volname "English Input" \
  -srcfolder "$DMG_ROOT" \
  -format UDZO \
  -ov \
  "$DIST_DIR/EnglishInput-$VERSION.dmg"

printf '\nCreated:\n  %s\n  %s\n' "$DIST_DIR/EnglishInput.pkg" "$DIST_DIR/EnglishInput-$VERSION.dmg"
