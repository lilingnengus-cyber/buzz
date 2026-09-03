#!/usr/bin/env bash
set -euo pipefail

repo_dir=$(cd "$(dirname "$0")/.." && pwd)
lifeos_dir=${LIFEOS_REPO_DIR:-/Users/aaronli/Projects/life-os}

usage() {
  cat <<'EOF'
Run the opt-in, real-process Pacioli -> ACP -> Life Gateway -> MCP -> LifeOS
default-value acceptance test.

Usage:
  scripts/test-life-workbench-live-defaults-e2e.sh

Optional environment:
  LIFEOS_REPO_DIR                 LifeOS checkout
  LIFE_E2E_POSTGRES_BASE_URL      PostgreSQL origin without a database/query
  LIFE_E2E_REDIS_DB               Dedicated empty local Redis DB (default: 15)
  LIFE_E2E_RELAY_PORT             Relay port (default: 3300)
  LIFE_E2E_LIFEOS_PORT            LifeOS port (default: 3302)
  LIFE_E2E_GATEWAY_PORT           Gateway port (default: 3303)
  LIFE_E2E_AGENT_COMMAND          ACP agent command (default: codex-acp)
  LIFE_E2E_MODEL                  ACP model (default: gpt-5.5)
  LIFE_E2E_KEEP                   Set to 1 to preserve databases/logs on exit

The script refuses a non-empty Redis DB, builds all exercised binaries, creates
three uniquely named PostgreSQL databases, and cleans them up by default.
EOF
}

if [[ ${1:-} == "--help" || ${1:-} == "-h" ]]; then
  usage
  exit 0
fi
if [[ $# -ne 0 ]]; then
  usage >&2
  exit 2
fi

. "$repo_dir/bin/activate-hermit"

for command in cargo createdb curl dropdb jq nc node npm psql; do
  if ! command -v "$command" >/dev/null 2>&1; then
    echo "error: required command is missing: $command" >&2
    exit 1
  fi
done
if [[ ! -d $lifeos_dir || ! -f $lifeos_dir/prisma/schema.prisma ]]; then
  echo "error: LIFEOS_REPO_DIR is not a LifeOS checkout: $lifeos_dir" >&2
  exit 1
fi

postgres_base=${LIFE_E2E_POSTGRES_BASE_URL:-"postgresql://$(id -un)@127.0.0.1:5432"}
if [[ $postgres_base == *\?* || $postgres_base == */postgres || $postgres_base == */template1 ]]; then
  echo "error: LIFE_E2E_POSTGRES_BASE_URL must be an origin without a database/query" >&2
  exit 1
fi

redis_db=${LIFE_E2E_REDIS_DB:-15}
if [[ ! $redis_db =~ ^([1-9]|1[0-5])$ ]]; then
  echo "error: LIFE_E2E_REDIS_DB must be an integer from 1 through 15" >&2
  exit 1
fi

relay_port=${LIFE_E2E_RELAY_PORT:-3300}
relay_health_port=${LIFE_E2E_RELAY_HEALTH_PORT:-8300}
relay_metrics_port=${LIFE_E2E_RELAY_METRICS_PORT:-9302}
lifeos_port=${LIFE_E2E_LIFEOS_PORT:-3302}
gateway_port=${LIFE_E2E_GATEWAY_PORT:-3303}
gateway_metrics_port=${LIFE_E2E_GATEWAY_METRICS_PORT:-9303}
for port in "$relay_port" "$relay_health_port" "$relay_metrics_port" \
  "$lifeos_port" "$gateway_port" "$gateway_metrics_port"; do
  if ! [[ $port =~ ^[0-9]+$ ]] || ((port < 1024 || port > 65535)); then
    echo "error: invalid test port: $port" >&2
    exit 1
  fi
  if nc -z 127.0.0.1 "$port" >/dev/null 2>&1; then
    echo "error: test port is already in use: $port" >&2
    exit 1
  fi
done

agent_command=${LIFE_E2E_AGENT_COMMAND:-codex-acp}
if ! command -v "$agent_command" >/dev/null 2>&1; then
  echo "error: ACP agent command is missing: $agent_command" >&2
  exit 1
fi

redis_request() {
  local command=$1
  {
    printf '*2\r\n$6\r\nSELECT\r\n$%d\r\n%s\r\n' "${#redis_db}" "$redis_db"
    printf '*1\r\n$%d\r\n%s\r\n' "${#command}" "$command"
    sleep 0.1
  } | nc -w 2 127.0.0.1 6379
}

redis_size=$(redis_request DBSIZE | tr -d '\r' | tail -n 1 | sed 's/^://')
if [[ $redis_size != 0 ]]; then
  echo "error: Redis DB $redis_db is not empty; refusing to overwrite shared state" >&2
  exit 1
fi

suffix="$$-$(date +%s)"
safe_suffix=${suffix//-/_}
relay_db="pacioli_life_defaults_e2e_$safe_suffix"
gateway_db="life_auth_defaults_e2e_$safe_suffix"
lifeos_db="lifeos_defaults_e2e_$safe_suffix"
relay_database_url="$postgres_base/$relay_db?sslmode=disable"
gateway_database_url="$postgres_base/$gateway_db?sslmode=disable"
lifeos_database_url="$postgres_base/$lifeos_db?sslmode=disable"
tmp_dir=$(mktemp -d "${TMPDIR:-/tmp}/life-defaults-e2e.XXXXXX")
pids=()

cleanup() {
  local status=$1
  trap - EXIT INT TERM
  for pid in "${pids[@]:-}"; do
    kill "$pid" >/dev/null 2>&1 || true
  done
  for pid in "${pids[@]:-}"; do
    wait "$pid" >/dev/null 2>&1 || true
  done
  redis_request FLUSHDB >/dev/null 2>&1 || true
  if [[ ${LIFE_E2E_KEEP:-0} != 1 ]]; then
    for database in "$relay_db" "$gateway_db" "$lifeos_db"; do
      dropdb --if-exists --maintenance-db="$postgres_base/postgres" "$database" \
        >/dev/null 2>&1 || true
    done
  fi
  if ((status != 0)); then
    echo "--- live defaults E2E diagnostics ---" >&2
    for log in "$tmp_dir"/*.log; do
      [[ -f $log ]] || continue
      echo "--- $(basename "$log") ---" >&2
      tail -n 120 "$log" >&2 || true
    done
  fi
  if [[ ${LIFE_E2E_KEEP:-0} == 1 ]]; then
    echo "Preserved databases and logs at $tmp_dir" >&2
  else
    rm -rf "$tmp_dir"
  fi
  exit "$status"
}
trap 'cleanup "$?"' EXIT
trap 'exit 130' INT TERM

start_process() {
  local name=$1
  shift
  "$@" >"$tmp_dir/$name.log" 2>&1 &
  last_pid=$!
  pids+=("$last_pid")
}

wait_http() {
  local name=$1
  local url=$2
  local attempts=${3:-90}
  for ((attempt = 1; attempt <= attempts; attempt++)); do
    if curl --fail --silent --show-error "$url" >/dev/null 2>&1; then
      return 0
    fi
    sleep 1
  done
  echo "error: timed out waiting for $name at $url" >&2
  return 1
}

wait_for_log() {
  local name=$1
  local pattern=$2
  local attempts=${3:-90}
  for ((attempt = 1; attempt <= attempts; attempt++)); do
    if grep -q "$pattern" "$tmp_dir/$name.log" 2>/dev/null; then
      return 0
    fi
    sleep 1
  done
  echo "error: timed out waiting for $pattern in $name.log" >&2
  return 1
}

query_count() {
  local database_url=$1
  local query=$2
  psql "$database_url" -Atqc "$query"
}

echo "Building current relay, CLI, ACP, Gateway, test client, and Life MCP binaries..."
cargo build --release \
  -p buzz-relay -p buzz-cli -p buzz-test-client -p life-auth-gateway
# Life Agent loopback HTTP is intentionally debug-only. Building these targets
# here also prevents a stale ACP or MCP executable from invalidating the run.
cargo build -p buzz-acp -p life-workbench-mcp

for database in "$relay_db" "$gateway_db" "$lifeos_db"; do
  createdb --maintenance-db="$postgres_base/postgres" "$database"
done

echo "Migrating and seeding isolated LifeOS database..."
(
  cd "$lifeos_dir"
  DATABASE_URL="$lifeos_database_url" npx prisma db push --skip-generate
)
psql "$lifeos_database_url" -v ON_ERROR_STOP=1 <<'SQL'
INSERT INTO "User" (id,email,name,status,"createdAt","updatedAt")
VALUES ('life-user-e2e','life-defaults-e2e@invalid.example','Life Defaults E2E','ACTIVE',now(),now());
INSERT INTO "Workspace" (id,name,"ownerId","createdAt","updatedAt")
VALUES ('workspace-e2e','Life Defaults E2E','life-user-e2e',now(),now());
INSERT INTO "WorkspaceMembership"
  (id,"userId","workspaceId",role,"membershipVersion","createdAt","updatedAt")
VALUES ('membership-e2e','life-user-e2e','workspace-e2e','OWNER',1,now(),now());
INSERT INTO "WorkbenchExternalIdentity"
  (id,issuer,subject,"userId",status,"createdAt","updatedAt")
VALUES ('external-identity-e2e','https://identity.invalid.example','life-defaults-e2e',
        'life-user-e2e','ACTIVE',now(),now());
SQL

# Fixed test-only credentials. They are scoped to disposable loopback services.
pacioli_token=$(printf 'p%.0s' {1..32})
mcp_token=$(printf 'm%.0s' {1..32})
lifeos_token=$(printf 'l%.0s' {1..32})
signing_seed=$(printf '11%.0s' {1..32})
signing_kid=10ba682c8ad13513
signing_public=d04ab232742bb4ab3a1368bd4615e4e6d0224ab71a016baf8520a332c9778737
call_grant_issuer=life-defaults-e2e
deployment_id=life-defaults-e2e

start_process lifeos env \
  DATABASE_URL="$lifeos_database_url" \
  LIFE_INTEGRATION_CONTRACT_VERSION=1 \
  LIFE_EXTENSION_ENABLED=true \
  LIFE_AGENT_READ_ENABLED=true \
  LIFE_AGENT_WRITE_ENABLED=true \
  LIFE_CHAT_HIGH_RISK_WRITE_ENABLED=false \
  LIFE_DOCK_ENABLED=false \
  LIFE_NOTIFIER_ENABLED=false \
  LIFE_AUTH_GATEWAY_URL="http://127.0.0.1:$gateway_port" \
  LIFE_AUTH_LIFEOS_SERVICE_TOKEN="$lifeos_token" \
  LIFE_WORKBENCH_MCP_SERVICE_TOKEN="$mcp_token" \
  LIFE_AUTH_CALL_GRANT_ISSUER="$call_grant_issuer" \
  LIFE_AUTH_CALL_GRANT_PUBLIC_KEYS="{\"$signing_kid\":\"$signing_public\"}" \
  LIFE_AUTH_CALL_GRANT_CLOCK_SKEW_SECONDS=5 \
  npm --prefix "$lifeos_dir" run dev -- --port "$lifeos_port" -H 127.0.0.1

for ((attempt = 1; attempt <= 120; attempt++)); do
  if curl --fail --silent --show-error \
    -H "authorization: Bearer $lifeos_token" \
    -H 'content-type: application/json' \
    -d '{"issuer":"https://identity.invalid.example","subject":"life-defaults-e2e"}' \
    "http://127.0.0.1:$lifeos_port/api/internal/workbench-identities/resolve" \
    | jq -e '.ok and .found and .user.id == "life-user-e2e"' >/dev/null 2>&1; then
    break
  fi
  if ((attempt == 120)); then
    echo "error: LifeOS identity resolver did not become ready" >&2
    exit 1
  fi
  sleep 1
done

start_process gateway env \
  LIFE_AUTH_ENVIRONMENT=test \
  LIFE_INTEGRATION_CONTRACT_VERSION=1 \
  LIFE_AUTH_DATABASE_URL="$gateway_database_url" \
  LIFE_AUTH_BIND_ADDR="127.0.0.1:$gateway_port" \
  LIFE_AUTH_METRICS_BIND_ADDR="127.0.0.1:$gateway_metrics_port" \
  LIFE_AUTH_DEPLOYMENT_ID="$deployment_id" \
  LIFE_AUTH_PACIOLI_SERVICE_TOKEN="$pacioli_token" \
  LIFE_AUTH_MCP_SERVICE_TOKEN="$mcp_token" \
  LIFE_AUTH_LIFEOS_SERVICE_TOKEN="$lifeos_token" \
  LIFE_AUTH_LIFEOS_BASE_URL="http://127.0.0.1:$lifeos_port" \
  LIFE_AUTH_CALL_GRANT_ISSUER="$call_grant_issuer" \
  LIFE_AUTH_CALL_GRANT_AUDIENCE=lifeos-workbench-api \
  LIFE_AUTH_DELEGATION_AUDIENCE=life-workbench-mcp \
  LIFE_AUTH_ED25519_PRIVATE_KEY="$signing_seed" \
  LIFE_AUTH_WORKBENCH_OIDC_ISSUER=https://identity.invalid.example \
  LIFE_AUTH_WORKBENCH_OIDC_AUDIENCE=life-defaults-e2e \
  LIFE_AUTH_ALLOWED_WORKBENCH_ORIGINS=tauri://localhost,http://tauri.localhost \
  "$repo_dir/target/release/life-auth-gateway"
wait_http "Life Auth Gateway" "http://127.0.0.1:$gateway_port/health/ready" 90

psql "$gateway_database_url" -v ON_ERROR_STOP=1 <<'SQL'
INSERT INTO life_workbench_users
  (id,oidc_issuer,oidc_subject,life_os_user_id,status,authority_version,
   authority_sync_status,authority_synced_at)
VALUES ('11111111-1111-4111-8111-111111111111','https://identity.invalid.example',
        'life-defaults-e2e','life-user-e2e','active',1,'current',now());
INSERT INTO life_identity_bindings
  (id,workbench_user_id,buzz_pubkey,status,source_event_id)
VALUES ('22222222-2222-4222-8222-222222222222',
        '11111111-1111-4111-8111-111111111111',
        '1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f',
        'active',repeat('a',64));
INSERT INTO life_workbench_sessions
  (id,workbench_user_id,deployment_id,token_hash,oidc_session_id,status,expires_at)
VALUES ('33333333-3333-4333-8333-333333333333',
        '11111111-1111-4111-8111-111111111111','life-defaults-e2e',
        decode(repeat('ab',32),'hex'),'life-defaults-e2e-session','active',now()+interval '2 hours');
INSERT INTO life_workspace_memberships
  (id,workbench_user_id,workspace_id,role_code,status,membership_version)
VALUES ('44444444-4444-4444-8444-444444444444',
        '11111111-1111-4111-8111-111111111111','workspace-e2e','OWNER','active',1);
SQL

user_key=$(printf '01%.0s' {1..32})
agent_key=$(printf '02%.0s' {1..32})
relay_key=$(printf '03%.0s' {1..32})
user_pub=1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f
agent_pub=4d4b6cd1361032ca9bd2aeb9d900aa4d45d9ead80ac9423374c451a7254d0766

start_process relay env \
  DATABASE_URL="$relay_database_url" \
  REDIS_URL="redis://127.0.0.1:6379/$redis_db" \
  BUZZ_AUTO_MIGRATE=true \
  BUZZ_BIND_ADDR="127.0.0.1:$relay_port" \
  BUZZ_HEALTH_PORT="$relay_health_port" \
  BUZZ_METRICS_PORT="$relay_metrics_port" \
  RELAY_URL="ws://127.0.0.1:$relay_port" \
  BUZZ_RELAY_PRIVATE_KEY="$relay_key" \
  RELAY_OWNER_PUBKEY="$user_pub" \
  BUZZ_REQUIRE_RELAY_MEMBERSHIP=false \
  BUZZ_GIT_CONFORMANCE_PROBE=false \
  "$repo_dir/target/release/buzz-relay"
wait_http "Pacioli relay" "http://127.0.0.1:$relay_port/health" 120

cli=("$repo_dir/target/release/buzz" --format compact)
dm_json=$(BUZZ_RELAY_URL="http://127.0.0.1:$relay_port" BUZZ_PRIVATE_KEY="$user_key" \
  "${cli[@]}" dms open --pubkey "$agent_pub")
dm_id=$(jq -er '.dm_id' <<<"$dm_json")

start_process acp env \
  PATH="$repo_dir/target/debug:$repo_dir/target/release:$PATH" \
  BUZZ_RELAY_URL="ws://127.0.0.1:$relay_port" \
  BUZZ_PRIVATE_KEY="$agent_key" \
  BUZZ_ACP_AGENT_OWNER="$user_pub" \
  BUZZ_ACP_AGENT_COMMAND="$agent_command" \
  BUZZ_ACP_AGENT_ARGS="${LIFE_E2E_AGENT_ARGS:-}" \
  BUZZ_ACP_MODEL="${LIFE_E2E_MODEL:-gpt-5.5}" \
  BUZZ_ACP_SUBSCRIBE=all \
  BUZZ_ACP_KINDS=9 \
  BUZZ_ACP_RESPOND_TO=owner-only \
  BUZZ_ACP_NO_MEMORY=true \
  BUZZ_ACP_CONTEXT_MESSAGE_LIMIT=12 \
  BUZZ_ACP_EXIT_AFTER_INACTIVITY=600 \
  LIFE_EXTENSION_ENABLED=true \
  LIFE_INTEGRATION_CONTRACT_VERSION=1 \
  LIFE_AGENT_READ_ENABLED=true \
  LIFE_AGENT_WRITE_ENABLED=true \
  LIFE_CHAT_HIGH_RISK_WRITE_ENABLED=false \
  LIFE_DOCK_ENABLED=false \
  LIFE_NOTIFIER_ENABLED=false \
  LIFE_AUTH_GATEWAY_URL="http://127.0.0.1:$gateway_port" \
  LIFE_API_URL="http://127.0.0.1:$lifeos_port" \
  LIFE_AUTH_PACIOLI_SERVICE_TOKEN="$pacioli_token" \
  LIFE_WORKBENCH_MCP_SERVICE_TOKEN="$mcp_token" \
  LIFE_WORKBENCH_MCP_COMMAND="$repo_dir/target/debug/life-workbench-mcp" \
  RUST_LOG=buzz_acp=debug \
  "$repo_dir/target/debug/buzz-acp"
acp_pid=$last_pid
wait_for_log acp "subscribed to channel $dm_id" 120

echo "Verifying typing does not enter LifeOS authorization..."
before_decisions=$(query_count "$gateway_database_url" 'SELECT count(*) FROM life_iam_decisions')
before_delegations=$(query_count "$gateway_database_url" 'SELECT count(*) FROM life_agent_delegations')
BUZZ_PRIVATE_KEY="$user_key" "$repo_dir/target/release/buzz-test-cli" \
  --url "ws://127.0.0.1:$relay_port" --channel "$dm_id" --kind 20002 \
  --send typing-only-life-defaults-e2e >/dev/null
sleep 5
after_decisions=$(query_count "$gateway_database_url" 'SELECT count(*) FROM life_iam_decisions')
after_delegations=$(query_count "$gateway_database_url" 'SELECT count(*) FROM life_agent_delegations')
[[ $after_decisions == "$before_decisions" ]]
[[ $after_delegations == "$before_delegations" ]]

project_name="默认值链路自动验收 $suffix"
prompt="请在我的 LifeOS 个人工作台 workspace-e2e 创建项目「${project_name}」，用途为“验证 Pacioli 自动化会话提交”。create_project 调用必须省略 color 字段，使用系统默认值。创建后告诉我资源引用、审计 ID 和 trace ID。"
send_json=$(BUZZ_RELAY_URL="http://127.0.0.1:$relay_port" BUZZ_PRIVATE_KEY="$user_key" \
  "${cli[@]}" messages send --channel "$dm_id" --kind 9 --content "$prompt")
source_event_id=$(jq -er '.event_id' <<<"$send_json")

echo "Waiting for the real Agent/MCP write..."
project_id=
for ((attempt = 1; attempt <= 240; attempt++)); do
  project_id=$(psql "$lifeos_database_url" -Atqc \
    "SELECT id FROM \"Project\" WHERE name='$project_name' LIMIT 1")
  [[ -n $project_id ]] && break
  if ! kill -0 "$acp_pid" >/dev/null 2>&1; then
    echo "error: ACP exited before creating the project" >&2
    exit 1
  fi
  sleep 1
done
if [[ -z $project_id ]]; then
  echo "error: timed out waiting for LifeOS project creation" >&2
  exit 1
fi

project_row=$(psql "$lifeos_database_url" -AtF '|' -c \
  "SELECT name,coalesce(purpose,''),color,version,\"workspaceId\" FROM \"Project\" WHERE id='$project_id'")
IFS='|' read -r stored_name stored_purpose stored_color stored_version stored_workspace <<<"$project_row"
[[ $stored_name == "$project_name" ]]
[[ $stored_purpose == "验证 Pacioli 自动化会话提交" ]]
[[ $stored_color == '#197b70' ]]
[[ $stored_version == 1 ]]
[[ $stored_workspace == workspace-e2e ]]

audit_row=$(psql "$lifeos_database_url" -AtF '|' -c \
  "SELECT id,\"traceId\" FROM \"LifeDomainAudit\" WHERE \"resourceId\"='$project_id' AND operation='create_project'")
IFS='|' read -r audit_id trace_id <<<"$audit_row"
[[ -n $audit_id && -n $trace_id ]]

decision_count=$(query_count "$gateway_database_url" \
  "SELECT count(*) FROM life_iam_decisions WHERE source_event_id='$source_event_id' AND decision_reason='allowed'")
delegation_count=$(query_count "$gateway_database_url" \
  "SELECT count(*) FROM life_agent_delegations WHERE source_event_id='$source_event_id'")
call_count=$(query_count "$gateway_database_url" \
  "SELECT count(*) FROM life_delegation_calls WHERE trace_id='$trace_id' AND capability='project:create'")
[[ $decision_count == 1 ]]
[[ $delegation_count == 1 ]]
[[ $call_count == 1 ]]

response_ok=false
for ((attempt = 1; attempt <= 90; attempt++)); do
  messages=$(BUZZ_RELAY_URL="http://127.0.0.1:$relay_port" BUZZ_PRIVATE_KEY="$user_key" \
    "${cli[@]}" messages get --channel "$dm_id" --limit 100 --kinds 9)
  if jq -e --arg agent "$agent_pub" --arg resource "life://project/$project_id" \
    --arg audit "$audit_id" --arg trace "$trace_id" '
      any(.[];
        .pubkey == $agent and
        (.content | contains($resource)) and
        (.content | contains($audit)) and
        (.content | contains($trace)) and
        any(.tags[]?; .[0] == "pacioli-extension-result") and
        any(.tags[]?; .[0] == "pacioli-resource-ref"))
    ' <<<"$messages" >/dev/null; then
    response_ok=true
    break
  fi
  sleep 1
done
[[ $response_ok == true ]]

jq -n \
  --arg sourceEventId "$source_event_id" \
  --arg projectId "$project_id" \
  --arg resourceRef "life://project/$project_id?version=1" \
  --arg auditId "$audit_id" \
  --arg traceId "$trace_id" \
  --arg color "$stored_color" \
  '{ok:true, sourceEventId:$sourceEventId, projectId:$projectId,
    resourceRef:$resourceRef, auditId:$auditId, traceId:$traceId,
    defaultColor:$color, typingAuthorized:false,
    iamDecisionsForMessage:1, delegationsForMessage:1, callsForMessage:1}'

echo "Life Workbench live default-value acceptance passed; isolated state will be cleaned."
