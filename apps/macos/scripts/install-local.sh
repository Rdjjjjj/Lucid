#!/bin/zsh
# Install one matched Rust build. Never fall back to a second input-method copy.
set -euo pipefail
setopt null_glob
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
"$ROOT/apps/macos/scripts/bundle.sh"
STAGE="$ROOT/target/LucidInputMethod.app"
SETTINGS_STAGE="$ROOT/target/Lucid.app"
TARGET="/Library/Input Methods/LucidInputMethod.app"
SETTINGS="/Applications/Lucid.app"
LSREGISTER="/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister"
stamp="$(date +%s)-$$"
BACKUP="/var/tmp/lucid-install-backup-$stamp"
"$STAGE/Contents/MacOS/LucidInputMethod" --self-check

# Copy both bundles before replacing either. The admin request is intentionally
# a single standard macOS dialog; cancellation leaves the existing install alone.
echo '==> 安装匹配的输入法和设置 App（需要一次本机管理员确认）'
osascript - "$STAGE" "$SETTINGS_STAGE" "$TARGET" "$SETTINGS" "$BACKUP" <<'APPLESCRIPT'
on run argv
    set inputSource to quoted form of item 1 of argv
    set settingsSource to quoted form of item 2 of argv
    set inputTarget to quoted form of item 3 of argv
    set settingsTarget to quoted form of item 4 of argv
    set backupDir to quoted form of item 5 of argv
    do shell script "set -eu; /bin/mkdir -p '/Library/Input Methods' '/Applications' " & backupDir & "; /bin/rm -rf '/Library/Input Methods/.LucidInputMethod.next' '/Applications/.Lucid.next'; /usr/bin/ditto --norsrc " & inputSource & " '/Library/Input Methods/.LucidInputMethod.next'; /usr/bin/ditto --norsrc " & settingsSource & " '/Applications/.Lucid.next'; /usr/sbin/chown -R root:wheel '/Library/Input Methods/.LucidInputMethod.next' '/Applications/.Lucid.next'; /bin/chmod -R 755 '/Library/Input Methods/.LucidInputMethod.next' '/Applications/.Lucid.next'; /usr/bin/killall LucidInputMethod Lucid 2>/dev/null || true; if [ -e " & inputTarget & " ]; then /bin/mv " & inputTarget & " " & backupDir & "/input-method.saved; fi; if [ -e " & settingsTarget & " ]; then /bin/mv " & settingsTarget & " " & backupDir & "/settings.saved; fi; /bin/mv '/Library/Input Methods/.LucidInputMethod.next' " & inputTarget & "; /bin/mv '/Applications/.Lucid.next' " & settingsTarget & "; for dir in /var/tmp/lucid-install-backup-*; do [ -d \"$dir\" ] || continue; for name in input-method.saved settings.saved; do [ -d \"$dir/$name\" ] || continue; /usr/bin/tar -czf \"$dir/$name.tar.gz.tmp\" -C \"$dir\" \"$name\"; /usr/bin/tar -tzf \"$dir/$name.tar.gz.tmp\" >/dev/null; /bin/mv \"$dir/$name.tar.gz.tmp\" \"$dir/$name.tar.gz\"; /bin/rm -rf \"$dir/$name\"; done; done" with administrator privileges
end run
APPLESCRIPT

# A renamed .app.saved still contains Contents/Info.plist and can be
# rediscovered by LaunchServices. Preserve obsolete bundles as real archives.
archive_bundle() {
  local bundle="$1" backup_dir="$HOME/Library/Application Support/Lucid/Install Backups/$stamp"
  "$LSREGISTER" -u "$bundle" >/dev/null 2>&1 || true
  mkdir -p "$backup_dir"
  local archive="$backup_dir/$(basename "$bundle")-$(uuidgen).tar.gz"
  tar -czf "$archive.tmp" -C "$(dirname "$bundle")" "$(basename "$bundle")"
  tar -tzf "$archive.tmp" >/dev/null
  mv "$archive.tmp" "$archive"
  rm -rf "$bundle"
}
BIN="$TARGET/Contents/MacOS/LucidInputMethod"
# Verify installation before removing even the packaging intermediates.
cmp "$STAGE/Contents/MacOS/LucidInputMethod" "$BIN"
cmp "$SETTINGS_STAGE/Contents/MacOS/Lucid" "$SETTINGS/Contents/MacOS/Lucid"
for root in "$ROOT/build" "$ROOT/target"; do
  [[ -d "$root" ]] || continue
  find "$root" -type d \( -name 'LucidInputMethod.app' -o -name 'EnglishInputMethod.app' -o -name 'Lucid.app' \) -prune -print0 |
    while IFS= read -r -d '' bundle; do
      if [[ "$bundle" == "$STAGE" || "$bundle" == "$SETTINGS_STAGE" ]]; then
        "$LSREGISTER" -u "$bundle" >/dev/null 2>&1 || true
        rm -rf "$bundle"
      else
        archive_bundle "$bundle"
      fi
    done
done
for stale in "$HOME/Library/Input Methods/LucidInputMethod.app" "$HOME/Applications/Lucid.app"; do
  [[ ! -d "$stale" ]] || archive_bundle "$stale"
done
"$LSREGISTER" -f "$TARGET" "$SETTINGS"
BIN="$TARGET/Contents/MacOS/LucidInputMethod"
# Existing app input contexts may still reference the pre-update IMK server.
killall LucidInputMethod Lucid imklaunchagent TextInputMenuAgent TextInputSwitcher 2>/dev/null || true
"$BIN" --install
"$BIN" --self-check
echo "已安装输入法：$TARGET"
echo "已安装设置 App：$SETTINGS"
echo "旧版备份（不注册为输入法）：$BACKUP"
echo '请重新打开要输入的应用，再选择 Lucid；设置和 API Key 均保留。'
