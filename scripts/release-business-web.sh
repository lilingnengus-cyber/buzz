#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)

fail() {
  echo "error: $*" >&2
  exit 1
}

info() {
  echo "business-web-release: $*"
}

usage() {
  cat <<'USAGE'
Usage: scripts/release-business-web.sh [--dry-run]

Build and atomically publish apps/business-web. The release is content-addressed,
the previous static tree is retained as a rollback pointer, and failed smoke
checks restore the prior version automatically.

Required environment:
  BUSINESS_WEB_DEPLOY_HOST       SSH destination, for example ubuntu@example.com

Optional environment:
  BUSINESS_WEB_SSH_KEY           SSH private key path
  BUSINESS_WEB_REMOTE_ROOT       Static release root
                                 (default: /opt/business-platform/shared)
  BUSINESS_WEB_PUBLIC_URL        Public site URL
                                 (default: https://business.shiyueshizi.com)
  BUSINESS_WEB_IAM_HEALTH_URL    Server-local IAM readiness URL
                                 (default: http://127.0.0.1:3110/health/ready)
  BUSINESS_WEB_CORE_HEALTH_URL   Server-local Business Core health URL
                                 (default: http://127.0.0.1:3120/health)
  BUSINESS_WEB_REMOTE_WEB_USER   Web-server account used for readability checks
                                 (default: www-data)
  BUSINESS_WEB_GIT_REMOTE        Git remote that must contain HEAD (default: origin)
  BUSINESS_WEB_REQUIRE_PUSHED    Set to false only for a dry-run (default: true)
USAGE
}

sha256_file() {
  local path="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$path" | awk '{print $1}'
  else
    shasum -a 256 "$path" | awk '{print $1}'
  fi
}

make_asset_manifest() {
  local dist_dir="$1" output="$2"
  (
    cd "$dist_dir"
    find . -type f ! -name '._*' -print | LC_ALL=C sort | while IFS= read -r file; do
      local relative=${file#./}
      [[ "$relative" != *$'\n'* ]] || fail "asset path contains a newline"
      printf '%s  %s\n' "$(sha256_file "$file")" "$relative"
    done
  ) > "$output"
  [[ -s "$output" ]] || fail "production build did not produce any assets"
}

extract_entry_asset() {
  local index_file="$1" extension="$2"
  local asset
  asset=$(sed -nE "s#.*(src|href)=\"/([^\"]+\\.${extension})\".*#\\2#p" "$index_file" | head -n 1)
  [[ "$asset" =~ ^assets/[A-Za-z0-9._/-]+\.${extension}$ ]] ||
    fail "could not resolve the entry .$extension asset"
  printf '%s\n' "$asset"
}

validate_remote_root() {
  [[ "$1" != "/" ]] || fail "BUSINESS_WEB_REMOTE_ROOT cannot be /"
  [[ "$1" =~ ^/[A-Za-z0-9._/-]+$ ]] || fail "BUSINESS_WEB_REMOTE_ROOT must be an absolute simple path"
}

validate_deploy_host() {
  [[ "$1" =~ ^[A-Za-z0-9@._:-]+$ ]] || fail "BUSINESS_WEB_DEPLOY_HOST contains unsupported characters"
}

validate_public_url() {
  [[ "$1" =~ ^https://[A-Za-z0-9._:-]+/?$ ]] || fail "BUSINESS_WEB_PUBLIC_URL must be an HTTPS origin"
}

validate_local_health_url() {
  [[ "$1" =~ ^http://127\.0\.0\.1:[0-9]+/[A-Za-z0-9._/-]+$ ]] ||
    fail "health URLs must use a server-local 127.0.0.1 HTTP endpoint"
}

main() {
  local dry_run=false
  case "${1:-}" in
    "") ;;
    --dry-run) dry_run=true ;;
    -h|--help) usage; return 0 ;;
    *) usage >&2; fail "unknown argument: $1" ;;
  esac
  [[ $# -le 1 ]] || fail "only one argument is supported"

  local deploy_host=${BUSINESS_WEB_DEPLOY_HOST:-}
  local ssh_key=${BUSINESS_WEB_SSH_KEY:-}
  local remote_root=${BUSINESS_WEB_REMOTE_ROOT:-/opt/business-platform/shared}
  local public_url=${BUSINESS_WEB_PUBLIC_URL:-https://business.shiyueshizi.com}
  local iam_health_url=${BUSINESS_WEB_IAM_HEALTH_URL:-http://127.0.0.1:3110/health/ready}
  local core_health_url=${BUSINESS_WEB_CORE_HEALTH_URL:-http://127.0.0.1:3120/health}
  local remote_web_user=${BUSINESS_WEB_REMOTE_WEB_USER:-www-data}
  local git_remote=${BUSINESS_WEB_GIT_REMOTE:-origin}
  local require_pushed=${BUSINESS_WEB_REQUIRE_PUSHED:-true}

  [[ -n "$deploy_host" ]] || fail "BUSINESS_WEB_DEPLOY_HOST is required"
  validate_deploy_host "$deploy_host"
  validate_remote_root "$remote_root"
  validate_public_url "$public_url"
  validate_local_health_url "$iam_health_url"
  validate_local_health_url "$core_health_url"
  [[ "$remote_web_user" =~ ^[A-Za-z_][A-Za-z0-9_-]*$ ]] || fail "BUSINESS_WEB_REMOTE_WEB_USER is invalid"
  [[ "$git_remote" =~ ^[A-Za-z0-9._/-]+$ ]] || fail "BUSINESS_WEB_GIT_REMOTE is invalid"
  [[ "$require_pushed" == true || "$require_pushed" == false ]] || fail "BUSINESS_WEB_REQUIRE_PUSHED must be true or false"
  if [[ -n "$ssh_key" ]]; then
    [[ "$ssh_key" == /* && -f "$ssh_key" ]] || fail "BUSINESS_WEB_SSH_KEY must be an existing absolute file"
  fi

  for command in git pnpm ssh tar sed find awk; do
    command -v "$command" >/dev/null 2>&1 || fail "$command is required"
  done
  command -v sha256sum >/dev/null 2>&1 || command -v shasum >/dev/null 2>&1 || fail "sha256sum or shasum is required"

  cd "$repo_root"
  git diff --quiet -- apps/business-web || fail "apps/business-web has unstaged changes"
  git diff --cached --quiet -- apps/business-web || fail "apps/business-web has staged but uncommitted changes"
  git diff --quiet -- scripts/release-business-web.sh || fail "release script has uncommitted changes"
  git diff --cached --quiet -- scripts/release-business-web.sh || fail "release script has staged but uncommitted changes"

  local commit branch remote_commit
  commit=$(git rev-parse HEAD)
  branch=$(git symbolic-ref --quiet --short HEAD) || fail "release requires a named Git branch"
  if [[ "$require_pushed" == true ]]; then
    remote_commit=$(git ls-remote "$git_remote" "refs/heads/$branch" | awk 'NR == 1 {print $1}')
    [[ -n "$remote_commit" ]] || fail "$git_remote has no refs/heads/$branch"
    [[ "$remote_commit" == "$commit" ]] || fail "HEAD is not pushed to $git_remote/$branch"
  elif [[ "$dry_run" != true ]]; then
    fail "BUSINESS_WEB_REQUIRE_PUSHED=false is allowed only with --dry-run"
  fi

  info "building commit $commit"
  pnpm --dir apps/business-web build

  local work_dir package_dir manifest dist_dir
  work_dir=$(mktemp -d)
  trap "rm -rf -- '$work_dir'" EXIT
  package_dir="$work_dir/package"
  manifest="$work_dir/asset-manifest.sha256"
  dist_dir="$repo_root/apps/business-web/dist"
  mkdir -p "$package_dir"
  make_asset_manifest "$dist_dir" "$manifest"
  cp -R "$dist_dir"/. "$package_dir"/
  cp "$manifest" "$package_dir/.asset-manifest.sha256"
  printf '%s\n' "$commit" > "$package_dir/.release-commit"

  local bundle_sha release_name entry_js entry_css entry_js_sha
  bundle_sha=$(sha256_file "$manifest")
  release_name="business-web-${commit:0:9}-${bundle_sha:0:12}"
  entry_js=$(extract_entry_asset "$package_dir/index.html" js)
  entry_css=$(extract_entry_asset "$package_dir/index.html" css)
  entry_js_sha=$(sha256_file "$package_dir/$entry_js")

  info "release $release_name"
  info "entry assets $entry_js and $entry_css"
  if [[ "$dry_run" == true ]]; then
    info "dry-run complete; no server changes made"
    return 0
  fi

  local -a ssh_args
  ssh_args=(-o BatchMode=yes -o IdentitiesOnly=yes)
  if [[ -n "$ssh_key" ]]; then
    ssh_args+=(-i "$ssh_key")
  fi

  local remote_release="$remote_root/$release_name"
  local remote_stage="$remote_root/.$release_name.uploading"
  local release_state
  release_state=$(ssh "${ssh_args[@]}" "$deploy_host" "bash -s -- $remote_root $release_name $commit $bundle_sha $remote_web_user" <<'REMOTE_PREPARE'
set -euo pipefail
remote_root=$1
release_name=$2
commit=$3
bundle_sha=$4
web_user=$5
release="$remote_root/$release_name"
stage="$remote_root/.$release_name.uploading"
[[ "$remote_root" != / && "$stage" == "$remote_root"/.*.uploading ]]
command -v flock >/dev/null
command -v sha256sum >/dev/null
if [[ -e "$release" ]]; then
  test "$(cat "$release/.release-commit")" = "$commit"
  test "$(cat "$release/.bundle-sha256")" = "$bundle_sha"
  (cd "$release" && sha256sum -c .asset-manifest.sha256 >/dev/null)
  sudo -u "$web_user" test -r "$release/index.html"
  printf 'existing\n'
else
  sudo test ! -e "$stage"
  sudo install -d -o "$(id -un)" -g "$(id -gn)" -m 0755 "$stage"
  printf 'upload\n'
fi
REMOTE_PREPARE
  )

  if [[ "$release_state" == upload ]]; then
    if ! COPYFILE_DISABLE=1 tar --no-xattrs -C "$package_dir" -czf - . |
      ssh "${ssh_args[@]}" "$deploy_host" "tar -xzf - -C $remote_stage"; then
      ssh "${ssh_args[@]}" "$deploy_host" "sudo rm -rf -- $remote_stage" || true
      fail "asset upload failed; live static files were not changed"
    fi
    ssh "${ssh_args[@]}" "$deploy_host" "bash -s -- $remote_root $release_name $commit $bundle_sha $remote_web_user" <<'REMOTE_FINALIZE'
set -euo pipefail
remote_root=$1
release_name=$2
commit=$3
bundle_sha=$4
web_user=$5
release="$remote_root/$release_name"
stage="$remote_root/.$release_name.uploading"
cleanup() {
  code=$?
  if [[ -d "$stage" && "$stage" == "$remote_root"/.*.uploading ]]; then
    sudo rm -rf -- "$stage"
  fi
  exit "$code"
}
trap cleanup ERR INT TERM
test "$(cat "$stage/.release-commit")" = "$commit"
printf '%s\n' "$bundle_sha" > "$stage/.bundle-sha256"
find "$stage" -name '._*' -print -quit | grep -q . && exit 1 || true
(cd "$stage" && sha256sum -c .asset-manifest.sha256 >/dev/null)
sudo -u "$web_user" test -r "$stage/index.html"
sudo test ! -e "$release"
sudo mv "$stage" "$release"
trap - ERR INT TERM
REMOTE_FINALIZE
  elif [[ "$release_state" != existing ]]; then
    fail "unexpected server release state: $release_state"
  else
    info "verified existing content-addressed release"
  fi

  ssh "${ssh_args[@]}" "$deploy_host" \
    "bash -s -- $remote_root $release_name $commit $bundle_sha $public_url $entry_js $entry_js_sha $iam_health_url $core_health_url" <<'REMOTE_SWITCH'
set -euo pipefail
remote_root=$1
release_name=$2
commit=$3
bundle_sha=$4
public_url=${5%/}
entry_js=$6
entry_js_sha=$7
iam_health_url=$8
core_health_url=$9
release="$remote_root/$release_name"
current="$remote_root/business-web"
rollback_pointer="$remote_root/business-web.rollback-$release_name"
next="$remote_root/.business-web.next-$release_name"
restore="$remote_root/.business-web.restore-$release_name"
lock="$remote_root/.business-web-release.lock"

exec 9>"$lock"
flock -n 9 || { echo "another Business Web release is active" >&2; exit 1; }
test "$(cat "$release/.release-commit")" = "$commit"
test "$(cat "$release/.bundle-sha256")" = "$bundle_sha"
(cd "$release" && sha256sum -c .asset-manifest.sha256 >/dev/null)

if [[ -e "$current" || -L "$current" ]]; then
  prior=$(readlink -f "$current")
else
  echo "current Business Web path does not exist" >&2
  exit 1
fi
if [[ "$prior" == "$release" ]]; then
  echo "release already active"
  exit 0
fi
sudo test ! -e "$rollback_pointer"
sudo test ! -e "$next"
sudo test ! -e "$restore"

if [[ -L "$current" ]]; then
  sudo ln -s "$prior" "$rollback_pointer"
else
  sudo mv "$current" "$rollback_pointer"
  prior="$rollback_pointer"
fi
sudo ln -s "$release" "$next"

rollback() {
  code=$?
  sudo ln -s "$prior" "$restore"
  sudo mv -Tf "$restore" "$current"
  echo "release validation failed; restored $prior" >&2
  exit "$code"
}
trap rollback ERR INT TERM
sudo mv -Tf "$next" "$current"

html=$(curl -fsS --max-time 20 "$public_url/")
grep -Fq "$entry_js" <<<"$html"
served_js_sha=$(curl -fsS --max-time 20 "$public_url/$entry_js" | sha256sum | awk '{print $1}')
test "$served_js_sha" = "$entry_js_sha"
curl -fsS --max-time 10 "$iam_health_url" >/dev/null
core_health=$(curl -fsS --max-time 10 "$core_health_url")
grep -Fq '"status":"ok"' <<<"$core_health"

trap - ERR INT TERM
printf 'prior_static=%s\n' "$prior"
printf 'current_static=%s\n' "$(readlink -f "$current")"
printf 'rollback_pointer=%s\n' "$rollback_pointer"
printf 'release_commit=%s\n' "$(cat "$current/.release-commit")"
printf 'published_asset=%s\n' "$entry_js"
printf 'published_asset_sha256=%s\n' "$served_js_sha"
printf 'iam_admin=healthy\n'
printf 'business_core=%s\n' "$core_health"
REMOTE_SWITCH

  info "production release complete"
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  main "$@"
fi
