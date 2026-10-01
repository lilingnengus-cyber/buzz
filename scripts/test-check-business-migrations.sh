#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
preflight="$repo_root/scripts/check-business-migrations.sh"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

fail_test() {
  echo "test failure: $*" >&2
  exit 1
}

assert_rejected() {
  local expected=$1
  shift
  local output status
  set +e
  output=$("$@" 2>&1)
  status=$?
  set -e
  [[ $status -ne 0 ]] || fail_test "expected rejection containing: $expected"
  grep -Fq "$expected" <<<"$output" || fail_test "missing rejection '$expected': $output"
}

mkdir -p "$tmp/migrations"
printf 'SELECT 1;\n' > "$tmp/migrations/0001_initial.sql"
printf 'SELECT 2;\n' > "$tmp/migrations/0002_second.sql"
one=$(shasum -a 384 "$tmp/migrations/0001_initial.sql" | awk '{print $1}')
two=$(shasum -a 384 "$tmp/migrations/0002_second.sql" | awk '{print $1}')
printf '1\tt\t%s\n2\tt\t%s\n' "$one" "$two" > "$tmp/compatible.tsv"

output=$("$preflight" --migrations-dir "$tmp/migrations" --database-manifest "$tmp/compatible.tsv")
grep -Fq 'database head=2, release head=2, pending=0' <<<"$output" ||
  fail_test "compatible history summary was not reported: $output"

printf '1\tt\t%s\n' "$one" > "$tmp/pending.tsv"
output=$(cat "$tmp/pending.tsv" | "$preflight" --migrations-dir "$tmp/migrations" --database-manifest -)
grep -Fq 'pending=1' <<<"$output" || fail_test "pending migration was not accepted: $output"

printf '1\tt\t%s\n2\tt\tdeadbeef\n' "$one" > "$tmp/mismatch.tsv"
assert_rejected 'checksum mismatch for database version 2' \
  "$preflight" --migrations-dir "$tmp/migrations" --database-manifest "$tmp/mismatch.tsv"

printf '1\tt\t%s\n2\tt\t%s\n3\tt\tdeadbeef\n' "$one" "$two" > "$tmp/missing.tsv"
assert_rejected 'database version 3 is missing from the release migrations' \
  "$preflight" --migrations-dir "$tmp/migrations" --database-manifest "$tmp/missing.tsv"

printf '1\tt\t%s\n2\tf\t%s\n' "$one" "$two" > "$tmp/failed.tsv"
assert_rejected 'database version 2 is not successfully applied' \
  "$preflight" --migrations-dir "$tmp/migrations" --database-manifest "$tmp/failed.tsv"

mv "$tmp/migrations/0002_second.sql" "$tmp/migrations/0003_gap.sql"
assert_rejected 'expected migration version 2' \
  "$preflight" --migrations-dir "$tmp/migrations" --database-manifest "$tmp/compatible.tsv"

bash -n "$preflight"
echo "Business migration preflight tests passed"
