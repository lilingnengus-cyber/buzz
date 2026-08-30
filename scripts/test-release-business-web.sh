#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
release_script="$repo_root/scripts/release-business-web.sh"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# shellcheck source=release-business-web.sh
source "$release_script"

fail_test() {
  echo "test failure: $*" >&2
  exit 1
}

assert_rejected() {
  local expected="$1"
  shift
  local output status
  set +e
  output=$("$@" 2>&1)
  status=$?
  set -e
  [[ $status -ne 0 ]] || fail_test "expected rejection containing: $expected"
  grep -Fq "$expected" <<<"$output" || fail_test "missing rejection '$expected': $output"
}

mkdir -p "$tmp/dist/assets"
printf '<script type="module" src="/assets/index-test.js"></script>\n<link href="/assets/index-test.css">\n' > "$tmp/dist/index.html"
printf 'console.log("Production")\n' > "$tmp/dist/assets/index-test.js"
printf 'body{}\n' > "$tmp/dist/assets/index-test.css"

make_asset_manifest "$tmp/dist" "$tmp/one.manifest"
make_asset_manifest "$tmp/dist" "$tmp/two.manifest"
cmp -s "$tmp/one.manifest" "$tmp/two.manifest" || fail_test "asset manifest is not deterministic"
[[ $(wc -l < "$tmp/one.manifest") -eq 3 ]] || fail_test "asset manifest omitted files"
[[ $(extract_entry_asset "$tmp/dist/index.html" js) == assets/index-test.js ]] || fail_test "JS entry extraction failed"
[[ $(extract_entry_asset "$tmp/dist/index.html" css) == assets/index-test.css ]] || fail_test "CSS entry extraction failed"

validate_remote_root /opt/business-platform/shared
validate_deploy_host ubuntu@example.com
validate_public_url https://business.shiyueshizi.com
validate_local_health_url http://127.0.0.1:3120/health

assert_rejected "cannot be /" validate_remote_root /
assert_rejected "absolute simple path" validate_remote_root '/opt/business platform'
assert_rejected "unsupported characters" validate_deploy_host 'ubuntu@example.com;touch-owned'
assert_rejected "HTTPS origin" validate_public_url 'http://business.shiyueshizi.com'
assert_rejected "server-local" validate_local_health_url 'https://example.com/health'

"$release_script" --help | grep -Fq 'BUSINESS_WEB_DEPLOY_HOST' || fail_test "help omits required host"
assert_rejected "unknown argument" "$release_script" --unknown
assert_rejected "BUSINESS_WEB_DEPLOY_HOST is required" "$release_script" --dry-run

bash -n "$release_script"
echo "Business Web release script tests passed"
