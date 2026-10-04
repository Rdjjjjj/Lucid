#!/bin/zsh
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

failures=0
report_failure() {
  print -u2 "ERROR: $1"
  failures=$((failures + 1))
}

# Inspect files that would be included by `git add .`, while excluding ignored
# build products and local release artifacts.
if ! command -v python3 >/dev/null 2>&1; then
  report_failure "python3 is required for the public-release scan"
else
  if ! python3 - <<'PY'
import os
import re
import subprocess
import sys

raw = subprocess.check_output(
    ["git", "ls-files", "-co", "--exclude-standard", "-z"],
)
paths = [p.decode("utf-8", "surrogateescape") for p in raw.split(b"\0") if p]

secret_patterns = [
    ("private key", re.compile(rb"-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----")),
    ("OpenAI-style key", re.compile(rb"\bsk-[A-Za-z0-9_-]{20,}\b")),
    ("GitHub token", re.compile(rb"\b(?:gh[pousr]_|github_pat_)[A-Za-z0-9_]{20,}\b")),
    ("AWS access key", re.compile(rb"\bAKIA[0-9A-Z]{16}\b")),
    ("Google API key", re.compile(rb"\bAIza[0-9A-Za-z_-]{20,}\b")),
    ("Slack token", re.compile(rb"\bxox[baprs]-[A-Za-z0-9-]{20,}\b")),
]
local_path = re.compile(rb"/(?:Users|home)/[^\s\"']+")
credential_file = re.compile(r"(?:^|/)[^/]+\.(?:p12|pfx|mobileprovision|provisionprofile)$", re.I)

found = False
scanned_blobs = set()

def scan_data(label, data):
    global found
    for name, pattern in secret_patterns:
        if pattern.search(data):
            print(f"{name}: {label}")
            found = True
    if local_path.search(data):
        print(f"local absolute path: {label}")
        found = True

def scan_path(path):
    global found
    if credential_file.search(path):
        print(f"credential/provisioning file: {path}")
        found = True
        return
    try:
        data = open(path, "rb").read()
    except (OSError, IsADirectoryError):
        return
    scan_data(path, data)

# Scan the files that would be included by `git add .`.
for path in paths:
    scan_path(path)

# Also scan every blob reachable from Git history. A secret removed from the
# working tree is still uploadable when the repository history is pushed.
if subprocess.call(["git", "rev-parse", "--verify", "HEAD"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL) == 0:
    objects = subprocess.check_output(["git", "rev-list", "--objects", "--all"], text=True)
    object_ids = [line.split(maxsplit=1)[0] for line in objects.splitlines() if line]
    process = subprocess.Popen(
        ["git", "cat-file", "--batch"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
    )
    assert process.stdin is not None and process.stdout is not None
    for object_id in object_ids:
        if object_id in scanned_blobs:
            continue
        process.stdin.write((object_id + "\n").encode())
        process.stdin.flush()
        header = process.stdout.readline().decode("ascii", "replace").strip().split()
        if len(header) != 3:
            continue
        size = int(header[2])
        data = process.stdout.read(size)
        process.stdout.read(1)  # trailing newline
        if header[1] != "blob":
            continue
        scanned_blobs.add(object_id)
        scan_data(f"Git history ({object_id})", data)
    process.stdin.close()
    process.wait()

if found:
    sys.exit(1)
PY
  then
    report_failure "possible secret, credential file, or local absolute path detected"
  fi
fi

# Release metadata must stay aligned between the settings app and input method.
APP_VERSION="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$ROOT_DIR/apps/macos/Info.plist")"
for plist in "$ROOT_DIR/apps/macos/SettingsInfo.plist" "$ROOT_DIR/LucidApp/Info.plist" "$ROOT_DIR/LucidInputMethod/Info.plist"; do
  actual="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$plist")"
  if [[ "$actual" != "$APP_VERSION" ]]; then
    report_failure "version mismatch: $plist=$actual, expected $APP_VERSION"
  fi
done

# Public source must not accidentally contain generated release/build files.
while IFS= read -r path; do
  case "$path" in
    *.dmg|*.pkg|.build/*|build/*|dist/*|DerivedData/*|*/xcuserdata/*|*.xcuserstate)
      report_failure "generated/local file would be committed: $path" ;;
  esac
done < <(git ls-files -co --exclude-standard)

for plist in "$ROOT_DIR/LucidApp/Info.plist" "$ROOT_DIR/LucidInputMethod/Info.plist" "$ROOT_DIR/Sources/LucidCore/Info.plist"; do
  if ! /usr/bin/plutil -lint "$plist" >/dev/null; then
    report_failure "invalid plist: $plist"
  fi
done

if (( failures > 0 )); then
  print -u2 "Public release check failed with $failures issue(s)."
  exit 1
fi

print "Public release check passed (version $APP_VERSION)."
