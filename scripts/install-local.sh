#!/bin/zsh
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
BUILD_DIR="$ROOT_DIR/build/LocalDebug"
STAGE_APP="$BUILD_DIR/LucidInputMethod.app"
STAGE_SETTINGS="$BUILD_DIR/Lucid.app"
USER_APP="$HOME/Library/Input Methods/LucidInputMethod.app"
SYS_APP="/Library/Input Methods/LucidInputMethod.app"
SETTINGS_APP="/Applications/Lucid.app"
UID_NUM="$(id -u)"
USER_NAME="$(id -un)"
stamp="$(date +%s)"

mkdir -p "$BUILD_DIR" "$HOME/Library/Input Methods"

echo "==> Building LucidInputMethod + Lucid"
xcodebuild \
  -project "$ROOT_DIR/Lucid.xcodeproj" \
  -scheme LucidInputMethod \
  -configuration Release \
  -sdk macosx \
  -derivedDataPath "$BUILD_DIR/DerivedData" \
  build CODE_SIGNING_ALLOWED=NO CODE_SIGNING_REQUIRED=NO

xcodebuild \
  -project "$ROOT_DIR/Lucid.xcodeproj" \
  -scheme LucidApp \
  -configuration Release \
  -sdk macosx \
  -derivedDataPath "$BUILD_DIR/DerivedData" \
  build CODE_SIGNING_ALLOWED=NO CODE_SIGNING_REQUIRED=NO

SRC_IM="$BUILD_DIR/DerivedData/Build/Products/Release/LucidInputMethod.app"
SRC_APP="$BUILD_DIR/DerivedData/Build/Products/Release/Lucid.app"
ditto --norsrc "$SRC_IM" "$STAGE_APP"
ditto --norsrc "$SRC_APP" "$STAGE_SETTINGS"
/usr/bin/xattr -cr "$STAGE_APP" || true
/usr/bin/xattr -cr "$STAGE_SETTINGS" || true
# 本地安装用 ad-hoc 签名，不依赖开发者证书，和打包脚本保持一致。
/usr/bin/codesign --force --deep --sign - "$STAGE_APP"
/usr/bin/codesign --force --deep --sign - "$STAGE_SETTINGS"

echo "==> Stopping old input method"
/usr/bin/killall LucidInputMethod 2>/dev/null || true
/usr/bin/killall EnglishInputMethod 2>/dev/null || true
sleep 0.5

# 永远先清掉用户目录副本，避免和 /Library 各登记一份、添加列表被系统藏起来。
if [ -e "$USER_APP" ]; then
  mv "$USER_APP" "$HOME/.Trash/LucidInputMethod-user-$stamp.app"
fi
if [ -e "$HOME/Library/Input Methods/EnglishInputMethod.app" ]; then
  mv "$HOME/Library/Input Methods/EnglishInputMethod.app" "$HOME/.Trash/EnglishInputMethod-user-$stamp.app"
fi

# 设置 App 只是配置界面，装不上也不影响输入法本体，这里失败不终止安装。
if [ -w "/Applications" ] || [ -d "$SETTINGS_APP" ]; then
  ditto --norsrc "$STAGE_SETTINGS" "$SETTINGS_APP" 2>/dev/null || true
  /usr/bin/xattr -cr "$SETTINGS_APP" 2>/dev/null || true
fi

install_system_copy() {
  /usr/bin/osascript <<APPLESCRIPT
do shell script "
/usr/bin/killall LucidInputMethod 2>/dev/null || true
/usr/bin/killall EnglishInputMethod 2>/dev/null || true
/bin/mkdir -p '/Library/Input Methods'
if [ -e '$SYS_APP' ]; then
  /bin/mv '$SYS_APP' '/tmp/LucidInputMethod-old-$stamp.app'
fi
if [ -e '/Library/Input Methods/EnglishInputMethod.app' ]; then
  /bin/mv '/Library/Input Methods/EnglishInputMethod.app' '/tmp/EnglishInputMethod-old-$stamp.app'
fi
/usr/bin/ditto --norsrc '$STAGE_APP' '$SYS_APP'
/usr/bin/xattr -cr '$SYS_APP' || true
/usr/sbin/chown -R root:wheel '$SYS_APP'
/bin/chmod -R 755 '$SYS_APP'
" with administrator privileges
APPLESCRIPT
}

echo "==> Installing system copy (需要输入本机密码)"
if install_system_copy; then
  echo "Installed to $SYS_APP"
  TARGET_APP="$SYS_APP"
else
  echo "系统目录安装失败，改用用户目录: $USER_APP"
  ditto --norsrc "$STAGE_APP" "$USER_APP"
  /usr/bin/xattr -cr "$USER_APP" || true
  TARGET_APP="$USER_APP"
fi

BIN="$TARGET_APP/Contents/MacOS/LucidInputMethod"

echo "==> Registering inside Aqua session"
# Debug/Release products and old quarantine copies must never stay registered
# as input methods. LaunchServices indexes bundles even after they are renamed,
# so renaming an .app to .disabled-input-method creates duplicate Lucid rows.
LSREGISTER="/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister"
unregister_stale_input_methods() {
  local root="$1"
  [ -d "$root" ] || return 0
  find "$root" -type d \
    \( -name 'LucidInputMethod.app' -o -name 'EnglishInputMethod.app' \) \
    -prune -print0 2>/dev/null |
    while IFS= read -r -d '' stale; do
      "$LSREGISTER" -u "$stale" >/dev/null 2>&1 || true
    done
}

# Remove registrations first, then discard generated build/quarantine bundles.
for stale_root in "$ROOT_DIR/build" "$ROOT_DIR/target" \
  "$HOME/LucidDisabledCopies" "$HOME/Library/Caches/LucidTypingFix"; do
  unregister_stale_input_methods "$stale_root"
done
if [ -d "$ROOT_DIR/build" ]; then
  find "$ROOT_DIR/build" -type d \
    \( -name 'LucidInputMethod.app' -o -name 'EnglishInputMethod.app' \) \
    -prune -exec rm -rf {} + 2>/dev/null || true
fi
/bin/launchctl asuser "$UID_NUM" /usr/bin/sudo -u "$USER_NAME" /usr/bin/killall -HUP cfprefsd 2>/dev/null || true
/bin/launchctl asuser "$UID_NUM" /usr/bin/sudo -u "$USER_NAME" "$BIN" --deactivate || true
/bin/launchctl asuser "$UID_NUM" /usr/bin/sudo -u "$USER_NAME" /usr/bin/killall -HUP cfprefsd 2>/dev/null || true
sleep 1
/bin/launchctl asuser "$UID_NUM" /usr/bin/sudo -u "$USER_NAME" "$BIN" --install || true
/bin/launchctl asuser "$UID_NUM" /usr/bin/sudo -u "$USER_NAME" /usr/bin/killall -HUP cfprefsd 2>/dev/null || true
/bin/launchctl asuser "$UID_NUM" /usr/bin/sudo -u "$USER_NAME" /usr/bin/killall imklaunchagent TextInputSwitcher TextInputMenuAgent 2>/dev/null || true

echo "Installed: $TARGET_APP"
echo "如果系统设置没有立即显示 Lucid，请重新登录 macOS 后，在系统键盘设置中添加 Lucid。"
