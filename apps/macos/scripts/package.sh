#!/bin/zsh
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
case "${1:-}" in
  --installed)
    INPUT_APP='/Library/Input Methods/LucidInputMethod.app'
    SETTINGS_APP='/Applications/Lucid.app'
    ;;
  *)
    if [[ "${1:-}" != '--existing-build' ]]; then
      "$ROOT/apps/macos/scripts/bundle.sh"
    fi
    INPUT_APP="$ROOT/target/LucidInputMethod.app"
    SETTINGS_APP="$ROOT/target/Lucid.app"
    ;;
esac
VERSION=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$INPUT_APP/Contents/Info.plist")
for key in CFBundleShortVersionString CFBundleVersion; do
  input=$(/usr/libexec/PlistBuddy -c "Print :$key" "$INPUT_APP/Contents/Info.plist")
  settings=$(/usr/libexec/PlistBuddy -c "Print :$key" "$SETTINGS_APP/Contents/Info.plist")
  [[ "$input" == "$settings" ]] || { echo '输入法和设置 App 版本不一致，取消打包' >&2; exit 1; }
done
codesign --verify --deep --strict "$INPUT_APP"
codesign --verify --deep --strict "$SETTINGS_APP"
WORK=$(mktemp -d /tmp/lucid-package.XXXXXX)
cleanup() {
  local ls='/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister'
  for app in "$WORK/root/Library/Input Methods/LucidInputMethod.app" "$WORK/root/Applications/Lucid.app"; do
    [[ ! -d "$app" ]] || "$ls" -u "$app" >/dev/null 2>&1 || true
  done
  rm -rf "$WORK"
}
trap cleanup EXIT
mkdir -p "$WORK/root/Library/Input Methods" "$WORK/root/Applications" "$WORK/scripts" "$ROOT/dist"
ditto --norsrc "$INPUT_APP" "$WORK/root/Library/Input Methods/LucidInputMethod.app"
ditto --norsrc "$SETTINGS_APP" "$WORK/root/Applications/Lucid.app"
cat > "$WORK/scripts/postinstall" <<'POST'
#!/bin/bash
set -eu
BIN='/Library/Input Methods/LucidInputMethod.app/Contents/MacOS/LucidInputMethod'
"$BIN" --self-check
user=$(/usr/bin/stat -f '%Su' /dev/console)
if [[ "$user" != root && "$user" != loginwindow ]]; then
  uid=$(/usr/bin/id -u "$user")
  run_user() { /bin/launchctl asuser "$uid" /usr/bin/sudo -u "$user" "$@"; }
  /usr/bin/killall -u "$user" LucidInputMethod Lucid imklaunchagent TextInputMenuAgent TextInputSwitcher 2>/dev/null || true
  LS='/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister'
  run_user "$LS" -f '/Library/Input Methods/LucidInputMethod.app' '/Applications/Lucid.app'
  run_user "$BIN" --install
fi
exit 0
POST
chmod 755 "$WORK/scripts/postinstall"
pkgbuild --analyze --root "$WORK/root" "$WORK/components.plist"
# Never let Installer relocate a bundle to an old build/Trash copy.
/usr/bin/python3 - "$WORK/components.plist" <<'PYPLIST'
import plistlib, sys
path = sys.argv[1]
with open(path, 'rb') as file:
    components = plistlib.load(file)
for component in components:
    component['BundleIsRelocatable'] = False
    component['BundleOverwriteAction'] = 'upgrade'
with open(path, 'wb') as file:
    plistlib.dump(components, file)
PYPLIST
pkgbuild --root "$WORK/root" --component-plist "$WORK/components.plist" --install-location / --identifier io.github.rdj.lucid.installer --version "$VERSION" --scripts "$WORK/scripts" "$ROOT/dist/Lucid-$VERSION.pkg"
echo "安装包：$ROOT/dist/Lucid-$VERSION.pkg"

# Do not leave bundle-shaped build artifacts for LaunchServices to rediscover.
if [[ "$INPUT_APP" == "$ROOT/target/LucidInputMethod.app" ]]; then
  LS='/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister'
  "$LS" -u "$INPUT_APP" "$SETTINGS_APP" >/dev/null 2>&1 || true
  rm -rf "$INPUT_APP" "$SETTINGS_APP"
fi
