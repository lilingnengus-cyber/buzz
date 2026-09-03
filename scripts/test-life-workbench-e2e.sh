#!/usr/bin/env bash
set -euo pipefail

repo_dir=$(cd "$(dirname "$0")/.." && pwd)
. "$repo_dir/bin/activate-hermit"

: "${LIFE_AUTH_TEST_DATABASE_URL:?set LIFE_AUTH_TEST_DATABASE_URL to an isolated PostgreSQL database}"
: "${LIFEOS_STAGE5_DATABASE_URL:?set LIFEOS_STAGE5_DATABASE_URL to an isolated migrated LifeOS database}"

LIFE_AUTH_TEST_DATABASE_URL="$LIFE_AUTH_TEST_DATABASE_URL" \
  cargo test -p life-iam -p life-auth-gateway -p life-notifier
cargo test -p buzz-acp life_
cargo test -p buzz-test-client --test e2e_life_workbench

(
  cd "$repo_dir/desktop"
  node --import ./test-loader.mjs --experimental-strip-types --test \
    src/features/messages/lib/messageQueryKeys.test.mjs
)
(
  cd "$repo_dir/mobile"
  flutter test test/features/channels/life_notification_dedup_test.dart
)

lifeos_dir=${LIFEOS_REPO_DIR:-/Users/aaronli/Projects/life-os}
(
  cd "$lifeos_dir"
  npm run prisma:generate
  node scripts/test-pacioli-disclosure-policy.mjs
  node scripts/test-pacioli-outbox-transaction.mjs
  node scripts/test-pacioli-outbox-worker-api.mjs
  node scripts/test-pacioli-outbox-replay.mjs
  node scripts/test-workbench-feature-flags.mjs
  node scripts/test-workbench-audit-redaction.mjs
  node scripts/test-pacioli-integration-e2e.mjs
  DATABASE_URL="$LIFEOS_STAGE5_DATABASE_URL" npm run pacioli:test:runtime
)

echo "Life Workbench component acceptance passed. Run the rollout runbook's real App/service exercise before enabling any production flag."
