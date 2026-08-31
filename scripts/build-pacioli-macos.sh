#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
profile="${1:-${PROFILE:-}}"
plan_only=0

if [[ "$profile" == "--plan" ]]; then
  [[ $# -eq 2 ]] || { echo "Usage: $0 --plan <dev|production>" >&2; exit 2; }
  plan_only=1
  profile="$2"
fi

case "$profile" in
  dev)
    app_name="Pacioli Dev.app"
    expected_bundle_id="com.shiyueshizi.pacioli.dev"
    expected_scheme="pacioli-dev"
    tauri_args=(build --bundles app --config src-tauri/tauri.dev.conf.json)
    ;;
  production)
    app_name="Pacioli.app"
    expected_bundle_id="com.shiyueshizi.pacioli"
    expected_scheme="pacioli"
    # Official distribution signing and notarization remain CI-owned. This
    # command creates a production-configured local candidate and then applies
    # a clearly non-distributable ad-hoc signature for local verification.
    tauri_args=(build --features mesh-llm --bundles app --no-sign)
    ;;
  *)
    echo "PROFILE must be dev or production." >&2
    echo "Usage: PROFILE=dev just build-pacioli" >&2
    echo "       PROFILE=production just build-pacioli" >&2
    exit 2
    ;;
esac

cargo_args=(
  build --release
  -p buzz-relay
  -p buzz-admin
  -p buzz-acp
  -p buzz-agent
  -p buzz-backend-kubernetes
  -p buzz-dev-mcp
  -p buzz-cli
  -p git-credential-nostr
)
app_path="$repo_root/desktop/src-tauri/target/release/bundle/macos/$app_name"

if (( plan_only == 1 )); then
  printf 'profile=%s\n' "$profile"
  printf 'cargo'
  printf ' %q' "${cargo_args[@]}"
  printf '\n(cd desktop && pnpm tauri'
  printf ' %q' "${tauri_args[@]}"
  printf ')\n'
  printf 'sign-macos-local-bundle.sh "%s"\n' "$app_path"
  printf 'expected_bundle_id=%s\nexpected_scheme=%s\n' \
    "$expected_bundle_id" "$expected_scheme"
  exit 0
fi

[[ "$(uname -s)" == "Darwin" ]] || {
  echo "Pacioli macOS apps can only be built on macOS." >&2
  exit 1
}

export PATH="$repo_root/bin:$PATH"
cd "$repo_root"
cargo "${cargo_args[@]}"

cd "$repo_root/desktop"
pnpm tauri "${tauri_args[@]}"

"$repo_root/scripts/sign-macos-local-bundle.sh" "$app_path"

info_plist="$app_path/Contents/Info.plist"
actual_bundle_id="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$info_plist")"
actual_scheme="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleURLTypes:0:CFBundleURLSchemes:0' "$info_plist")"
[[ "$actual_bundle_id" == "$expected_bundle_id" ]] || {
  echo "Unexpected bundle identifier: $actual_bundle_id" >&2
  exit 1
}
[[ "$actual_scheme" == "$expected_scheme" ]] || {
  echo "Unexpected deep-link scheme: $actual_scheme" >&2
  exit 1
}

echo "Built Pacioli $profile profile: $app_path"
if [[ "$profile" == "production" ]]; then
  echo "Local candidate only: official distribution still requires Developer ID signing and Apple notarization."
fi
