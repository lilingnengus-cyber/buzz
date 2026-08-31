#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

dev_plan="$($repo_root/scripts/build-pacioli-macos.sh --plan dev)"
production_plan="$($repo_root/scripts/build-pacioli-macos.sh --plan production)"

for plan in "$dev_plan" "$production_plan"; do
  for forbidden in "docker" "buzz-admin -- migrate" "seed-local-community"; do
    if grep -Fqi "$forbidden" <<<"$plan"; then
      echo "Pacioli build unexpectedly requires: $forbidden"
      exit 1
    fi
  done
  for required in "cargo build --release" "pnpm tauri" "sign-macos-local-bundle.sh"; do
    if ! grep -Fq "$required" <<<"$plan"; then
      echo "Pacioli build plan is missing: $required"
      exit 1
    fi
  done
done

grep -Fq 'src-tauri/tauri.dev.conf.json' <<<"$dev_plan"
grep -Fq 'Pacioli Dev.app' <<<"$dev_plan"
grep -Fq 'expected_bundle_id=com.shiyueshizi.pacioli.dev' <<<"$dev_plan"
grep -Fq 'expected_scheme=pacioli-dev' <<<"$dev_plan"

if grep -Fq 'tauri.dev.conf.json' <<<"$production_plan"; then
  echo "Production plan must not load the Dev Tauri configuration."
  exit 1
fi
grep -Fq -- '--features mesh-llm' <<<"$production_plan"
grep -Fq -- '--no-sign' <<<"$production_plan"
grep -Fq 'Pacioli.app' <<<"$production_plan"
grep -Fq 'expected_bundle_id=com.shiyueshizi.pacioli' <<<"$production_plan"
grep -Fq 'expected_scheme=pacioli' <<<"$production_plan"

if PROFILE=invalid "$repo_root/scripts/build-pacioli-macos.sh" >/dev/null 2>&1; then
  echo "Invalid Pacioli build profiles must fail."
  exit 1
fi

echo "Pacioli Dev and production build profiles are isolated and infrastructure-independent."
