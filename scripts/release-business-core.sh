#!/usr/bin/env bash
set -euo pipefail

fail() {
  echo "business-core-release: error: $*" >&2
  exit 1
}

usage() {
  cat <<'USAGE'
Usage: scripts/release-business-core.sh --image IMAGE --container NAME --release-root DIR [--dry-run]

Run on the Docker host. Release an already built local Business Core image.
Inspect the running container's Compose configuration, check the candidate
image's embedded migrations, pin both image IDs, then replace only Core.
Failed startup or health checks restore the previous image automatically.

--dry-run performs the real read-only migration check without replacing Core.
New database migrations must be applied separately before this release.
No database migrations are reversed by rollback.
USAGE
}

main() {
  local image= container= release_root= dry_run=false
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --image|--container|--release-root)
        [[ $# -ge 2 ]] || fail "$1 requires a value"
        case "$1" in
          --image) image=$2 ;;
          --container) container=$2 ;;
          --release-root) release_root=$2 ;;
        esac
        shift 2
        ;;
      --dry-run) dry_run=true; shift ;;
      -h|--help) usage; return ;;
      *) fail "unknown argument: $1" ;;
    esac
  done
  [[ -n "$image" && -n "$container" && -n "$release_root" ]] ||
    fail "--image, --container, and --release-root are required"
  [[ "$release_root" == /* && "$release_root" != / ]] ||
    fail "--release-root must be an absolute directory other than /"
  command -v flock >/dev/null || fail "flock is required"
  command -v docker >/dev/null || fail "docker is required"
  mkdir -p "$release_root"

  local project
  project=$(docker inspect "$container" --format '{{index .Config.Labels "com.docker.compose.project"}}')
  [[ "$project" =~ ^[a-zA-Z0-9_-]+$ ]] || fail "container has no valid Compose project"
  exec 9>"$release_root/$project.lock"
  flock -n 9 || fail "another Core release is running"
  release_locked "$image" "$container" "$release_root" "$dry_run" "$project"
}

release_locked() (
  set -euo pipefail
  image=$1
  container=$2
  release_root=$3
  dry_run=$4
  project=$5
  service=$(docker inspect "$container" --format '{{index .Config.Labels "com.docker.compose.service"}}')
  [[ "$service" == business-core ]] || fail "container must be the business-core Compose service"
  working_dir=$(docker inspect "$container" --format '{{index .Config.Labels "com.docker.compose.project.working_dir"}}')
  config_files=$(docker inspect "$container" --format '{{index .Config.Labels "com.docker.compose.project.config_files"}}')
  [[ -d "$working_dir" && -n "$config_files" ]] || fail "Compose configuration is unavailable"
  previous_image=$(docker inspect "$container" --format '{{.Image}}')
  candidate_image=$(docker image inspect "$image" --format '{{.Id}}')
  [[ "$previous_image" =~ ^sha256:[a-f0-9]{64}$ && "$candidate_image" =~ ^sha256:[a-f0-9]{64}$ ]] ||
    fail "could not resolve immutable image IDs"

  compose=(docker compose --project-directory "$working_dir" --project-name "$project")
  IFS=',' read -r -a files <<< "$config_files"
  for file in "${files[@]}"; do
    [[ -f "$file" ]] || fail "missing Compose configuration: $file"
    compose+=(-f "$file")
  done
  release_dir=$(mktemp -d "$release_root/release.XXXXXXXX")
  printf 'services:\n  business-core:\n    image: %s\n' "$candidate_image" > "$release_dir/candidate.yml"
  printf 'services:\n  business-core:\n    image: %s\n' "$previous_image" > "$release_dir/rollback.yml"
  printf '%s\n' "$config_files" > "$release_dir/previous-config-files"
  printf '%s\n' "$candidate_image" > "$release_dir/candidate-image"
  printf '%s\n' "$previous_image" > "$release_dir/previous-image"
  candidate=("${compose[@]}" -f "$release_dir/candidate.yml")
  rollback=("${compose[@]}" -f "$release_dir/rollback.yml")
  "${candidate[@]}" config --quiet
  echo "business-core-release: checking candidate $candidate_image"
  "${candidate[@]}" run --rm --no-deps --pull never \
    --entrypoint business-core business-core --check-migrations > "$release_dir/preflight.log"
  cat "$release_dir/preflight.log"
  # The current binary validates every applied version at startup. Applying a
  # new version would therefore also prevent the old binary from restarting.
  grep -Eq '^business-migration-preflight: compatible; .*pending=0$' "$release_dir/preflight.log" ||
    fail "pending migrations require a separate migration rollout before this release"
  if [[ "$dry_run" == true ]]; then
    echo "business-core-release: dry-run passed; evidence=$release_dir"
    return
  fi

  changed=false
  finish() {
    code=$?
    trap - EXIT INT TERM
    if [[ $code -ne 0 && "$changed" == true ]]; then
      echo "business-core-release: restoring $previous_image" >&2
      if "${rollback[@]}" up -d --no-deps --no-build --pull never business-core &&
        wait_healthy "$container" "$previous_image"; then
        echo "business-core-release: rollback healthy; evidence=$release_dir" >&2
      else
        echo "business-core-release: rollback failed; inspect $release_dir" >&2
      fi
    fi
    exit "$code"
  }
  trap finish EXIT
  trap 'exit 130' INT
  trap 'exit 143' TERM
  changed=true
  "${candidate[@]}" up -d --no-deps --no-build --pull never business-core
  wait_healthy "$container" "$candidate_image"
  changed=false
  echo "business-core-release: healthy; evidence=$release_dir"
)

wait_healthy() {
  local container=$1 expected_image=$2 attempt actual_image running
  for ((attempt=0; attempt<30; attempt++)); do
    actual_image=$(docker inspect "$container" --format '{{.Image}}' 2>/dev/null) || actual_image=
    running=$(docker inspect "$container" --format '{{.State.Running}}' 2>/dev/null) || running=
    if [[ "$actual_image" == "$expected_image" && "$running" == true ]] &&
      docker exec "$container" curl --fail --silent --max-time 3 http://127.0.0.1:3120/health >/dev/null; then
      return
    fi
    sleep 2
  done
  echo "business-core-release: error: Core did not become healthy on the expected image" >&2
  return 1
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  main "$@"
fi
