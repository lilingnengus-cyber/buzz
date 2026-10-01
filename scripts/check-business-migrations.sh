#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
work_dir=

cleanup() {
  [[ -z "$work_dir" ]] || rm -rf -- "$work_dir"
}

fail() {
  echo "business-migration-preflight: error: $*" >&2
  exit 1
}

usage() {
  cat <<'USAGE'
Usage: scripts/check-business-migrations.sh [options]

Verify that a Business deployment's SQLx migration history is compatible with
the migrations embedded by the current source tree. The check is read-only.

Options:
  --migrations-dir DIR       Migration directory (default:
                             services/business-auth-gateway/migrations)
  --database-url URL         PostgreSQL URL to inspect
  --database-manifest FILE   Previously queried tab-separated database manifest;
                             use - to read it from stdin
  -h, --help                 Show this help

BUSINESS_MIGRATION_DATABASE_URL may be used instead of --database-url. A
database manifest has three fields: version, success, and hex checksum.
USAGE
}

make_local_manifest() {
  local migrations_dir=$1 output=$2 file file_name version_text version checksum
  : > "$output"
  while IFS= read -r file; do
    file_name=${file##*/}
    [[ "$file_name" =~ ^([0-9]+)_.+\.sql$ ]] ||
      fail "invalid migration file name: $file_name"
    version_text=${BASH_REMATCH[1]}
    version=$((10#$version_text))
    checksum=$(shasum -a 384 "$file" | awk '{print $1}')
    printf '%d\t%s\t%s\n' "$version" "$checksum" "$file_name" >> "$output"
  done < <(find "$migrations_dir" -maxdepth 1 -type f -name '*.sql' -print | LC_ALL=C sort)
  [[ -s "$output" ]] || fail "no SQL migrations found in $migrations_dir"
  LC_ALL=C sort -n -k1,1 "$output" -o "$output"

  awk -F '\t' '
    BEGIN { expected = 1 }
    $1 != expected {
      printf "business-migration-preflight: error: expected migration version %d, found %s (%s)\n", expected, $1, $3 > "/dev/stderr"
      exit 1
    }
    { expected++ }
  ' "$output"
}

query_database_manifest() {
  local database_url=$1 output=$2
  command -v psql >/dev/null 2>&1 || fail "psql is required when using a database URL"
  psql -X -v ON_ERROR_STOP=1 --no-align --tuples-only \
    --field-separator=$'\t' "$database_url" \
    -c "SELECT version,success,encode(checksum,'hex') FROM _sqlx_migrations ORDER BY version" \
    > "$output"
}

verify_manifests() {
  local local_manifest=$1 database_manifest=$2
  awk -F '\t' '
    NR == FNR {
      local_checksum[$1] = tolower($2)
      local_file[$1] = $3
      local_head = $1
      next
    }
    NF != 3 {
      printf "business-migration-preflight: error: malformed database manifest at line %d\n", FNR > "/dev/stderr"
      failed = 1
      next
    }
    $1 !~ /^[0-9]+$/ {
      printf "business-migration-preflight: error: invalid database migration version: %s\n", $1 > "/dev/stderr"
      failed = 1
      next
    }
    $1 != expected_database_version + 1 {
      printf "business-migration-preflight: error: database migration history is not contiguous at version %s\n", $1 > "/dev/stderr"
      failed = 1
    }
    !($1 in local_checksum) {
      printf "business-migration-preflight: error: database version %s is missing from the release migrations\n", $1 > "/dev/stderr"
      failed = 1
      next
    }
    $2 != "t" && $2 != "true" {
      printf "business-migration-preflight: error: database version %s is not successfully applied\n", $1 > "/dev/stderr"
      failed = 1
    }
    tolower($3) != local_checksum[$1] {
      printf "business-migration-preflight: error: checksum mismatch for database version %s (%s)\n", $1, local_file[$1] > "/dev/stderr"
      failed = 1
    }
    {
      expected_database_version = $1
      applied++
    }
    END {
      if (failed) exit 1
      printf "business-migration-preflight: compatible; database head=%d, release head=%d, pending=%d\n", expected_database_version, local_head, local_head - applied
    }
  ' "$local_manifest" "$database_manifest"
}

main() {
  local migrations_dir="$repo_root/services/business-auth-gateway/migrations"
  local database_url=${BUSINESS_MIGRATION_DATABASE_URL:-}
  local database_manifest=

  while [[ $# -gt 0 ]]; do
    case "$1" in
      --migrations-dir)
        [[ $# -ge 2 ]] || fail "--migrations-dir requires a value"
        migrations_dir=$2
        shift 2
        ;;
      --database-url)
        [[ $# -ge 2 ]] || fail "--database-url requires a value"
        database_url=$2
        shift 2
        ;;
      --database-manifest)
        [[ $# -ge 2 ]] || fail "--database-manifest requires a value"
        database_manifest=$2
        shift 2
        ;;
      -h|--help)
        usage
        return 0
        ;;
      *)
        usage >&2
        fail "unknown argument: $1"
        ;;
    esac
  done

  [[ -d "$migrations_dir" ]] || fail "migration directory does not exist: $migrations_dir"
  [[ -z "$database_url" || -z "$database_manifest" ]] ||
    fail "use either a database URL or a database manifest, not both"
  [[ -n "$database_url" || -n "$database_manifest" ]] ||
    fail "--database-url, --database-manifest, or BUSINESS_MIGRATION_DATABASE_URL is required"
  command -v shasum >/dev/null 2>&1 || fail "shasum is required"

  local local_manifest queried_manifest
  work_dir=$(mktemp -d)
  trap cleanup EXIT
  local_manifest="$work_dir/local.tsv"
  queried_manifest="$work_dir/database.tsv"
  make_local_manifest "$migrations_dir" "$local_manifest"

  if [[ -n "$database_url" ]]; then
    query_database_manifest "$database_url" "$queried_manifest"
  elif [[ "$database_manifest" == - ]]; then
    cat > "$queried_manifest"
  else
    [[ -f "$database_manifest" ]] || fail "database manifest does not exist: $database_manifest"
    cp "$database_manifest" "$queried_manifest"
  fi

  verify_manifests "$local_manifest" "$queried_manifest"
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  main "$@"
fi
