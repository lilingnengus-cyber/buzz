#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "Usage: $0 <path-to-local-app>" >&2
  exit 2
fi

app="$1"
info_plist="$app/Contents/Info.plist"

[[ "$(uname -s)" == "Darwin" ]] || {
  echo "macOS bundles can only be signed on macOS." >&2
  exit 1
}
[[ -d "$app" ]] || { echo "Missing app bundle: $app" >&2; exit 1; }
[[ -f "$info_plist" ]] || { echo "Missing app Info.plist: $info_plist" >&2; exit 1; }

bundle_id="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$info_plist")"
[[ -n "$bundle_id" ]] || { echo "App bundle identifier is empty: $app" >&2; exit 1; }

# Local Tauri signatures can become stale after resources are copied. This
# ad-hoc signature makes the final bundle executable for local testing only;
# it is not a substitute for Developer ID signing or Apple notarization.
codesign --force --deep --sign - --identifier "$bundle_id" "$app"
codesign --verify --deep --strict --verbose=2 "$app"

echo "Applied local ad-hoc signature and verified: $app"
