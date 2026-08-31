#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
build_plan="$(
  cd "$repo_root"
  "$repo_root/bin/just" --dry-run business-dock-macos-build 2>&1
)"

for forbidden in "docker" "buzz-admin -- migrate" "seed-local-community"; do
  if grep -Fqi "$forbidden" <<<"$build_plan"; then
    echo "Business Dock macOS build unexpectedly requires: $forbidden"
    exit 1
  fi
done

for required in "cargo build --release" "pnpm tauri build --bundles app"; do
  if ! grep -Fq "$required" <<<"$build_plan"; then
    echo "Business Dock macOS build is missing: $required"
    exit 1
  fi
done

echo "Business Dock macOS build remains infrastructure-independent."
