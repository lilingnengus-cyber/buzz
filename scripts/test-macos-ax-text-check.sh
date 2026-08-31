#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
fixture="$(mktemp)"
trap 'rm -f "$fixture"' EXIT

printf '%s\n' \
  '当前登录账号' \
  'authentik Default Admin' \
  '真实数据 ·  Production' >"$fixture"

xcrun swift "$repo_root/scripts/macos-ax-text-check.swift" \
  --input "$fixture" \
  --expect '当前登录账号' \
  --expect 'authentik Default Admin' \
  --expect '真实数据' \
  --expect 'Production'

if xcrun swift "$repo_root/scripts/macos-ax-text-check.swift" \
  --input "$fixture" \
  --expect '未登录' >/dev/null 2>&1; then
  echo "Expected a missing accessibility label to fail verification."
  exit 1
fi

echo "macOS accessibility text checks passed."
