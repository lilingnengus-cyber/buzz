#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
. ./bin/activate-hermit

env_file="${AUTHENTIK_POC_ENV_FILE:-deploy/authentik-poc/.env}"
if [[ ! -f "$env_file" ]]; then
  echo "error: copy deploy/authentik-poc/.env.example to $env_file and populate it" >&2
  exit 2
fi

set -a
# shellcheck disable=SC1090
. "$env_file"
set +a

: "${PG_PASS:?PG_PASS is required in $env_file}"
: "${POC_USER_PASSWORD:?POC_USER_PASSWORD is required in $env_file}"
: "${WORKBENCH_OIDC_CLIENT_ID:?WORKBENCH_OIDC_CLIENT_ID is required in $env_file}"

compose=(
  docker compose --env-file "$env_file"
  -f deploy/authentik-poc/docker-compose.yml
)
authentik_container="$("${compose[@]}" ps -q server)"
postgres_container="$("${compose[@]}" ps -q business-auth-postgresql)"
if [[ -z "$authentik_container" || -z "$postgres_container" ]]; then
  echo "error: Authentik POC and business-auth-postgresql must be running" >&2
  exit 2
fi

cert="${AUTHENTIK_POC_CERT_FILE:-$(dirname "$env_file")/certs/rootCA.pem}"
if [[ ! -f "$cert" ]]; then
  echo "error: $cert is required" >&2
  exit 2
fi

api_port="${BUSINESS_IAM_ADMIN_E2E_PORT:-3111}"
if lsof -nP -iTCP:"$api_port" -sTCP:LISTEN >/dev/null 2>&1; then
  echo "error: port $api_port is already in use" >&2
  exit 2
fi

suffix="$(date +%s)_$$"
database="iam_auth_e2e_${suffix}"
owner_role="iam_auth_e2e_owner_${suffix}"
runtime_role="iam_auth_e2e_runtime_${suffix}"
owner_password="$(openssl rand -hex 24)"
runtime_password="$(openssl rand -hex 24)"
totp_name="Business IAM E2E TOTP"
tmp_dir="$(mktemp -d /tmp/business-iam-authentik.XXXXXX)"
api_pid=""
vite_pid=""

cleanup() {
  status=$?
  trap - EXIT INT TERM
  if [[ -n "$api_pid" ]]; then kill "$api_pid" >/dev/null 2>&1 || true; fi
  if [[ -n "$vite_pid" ]]; then kill "$vite_pid" >/dev/null 2>&1 || true; fi
  docker exec "$authentik_container" ak shell -c \
    "from authentik.stages.authenticator_totp.models import TOTPDevice; TOTPDevice.objects.filter(user__username='poc-user',name='$totp_name').delete()" \
    >/dev/null 2>&1 || true
  docker exec "$postgres_container" psql -v ON_ERROR_STOP=1 -U business_auth -d postgres \
    -c "DROP DATABASE IF EXISTS \"$database\" WITH (FORCE)" \
    -c "DROP ROLE IF EXISTS \"$runtime_role\"" \
    -c "DROP ROLE IF EXISTS \"$owner_role\"" >/dev/null 2>&1 || true
  rm -rf "$tmp_dir"
  exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

docker exec "$postgres_container" psql -v ON_ERROR_STOP=1 -U business_auth -d postgres \
  -c "CREATE ROLE \"$owner_role\" LOGIN PASSWORD '$owner_password'" \
  -c "CREATE ROLE \"$runtime_role\" LOGIN PASSWORD '$runtime_password'" \
  -c "CREATE DATABASE \"$database\" OWNER \"$owner_role\"" >/dev/null

owner_url="postgres://${owner_role}:${owner_password}@127.0.0.1:55442/${database}"
runtime_url="postgres://${runtime_role}:${runtime_password}@127.0.0.1:55442/${database}"
BUSINESS_IAM_ADMIN_DATABASE_URL="$owner_url" \
BUSINESS_IAM_ADMIN_RUNTIME_DATABASE_ROLE="$runtime_role" \
  cargo run -q -p business-iam-admin-api -- --migrate-only

oidc_subject="$(docker exec "$authentik_container" ak shell -c \
  'from authentik.core.models import User; print(User.objects.get(username="poc-user").uid)' \
  2>/dev/null | tail -n 1)"
if [[ ! "$oidc_subject" =~ ^[0-9a-f]{64}$ ]]; then
  echo "error: could not resolve the POC user's Authentik OIDC subject" >&2
  exit 1
fi

docker exec -i -e PGPASSWORD="$owner_password" "$postgres_container" \
  psql -v ON_ERROR_STOP=1 -U "$owner_role" -d "$database" \
  -v oidc_subject="$oidc_subject" <<'SQL' >/dev/null
WITH enterprise_user AS (
  INSERT INTO enterprise_users(id,oidc_issuer,oidc_subject,email,display_name)
  VALUES (
    '10000000-0000-4000-8000-000000000001',
    'https://auth.bizfin.test/application/o/workbench/',
    :'oidc_subject',
    'poc-user@bizfin.test',
    'POC User'
  )
  RETURNING id
), human_principal AS (
  INSERT INTO business_iam.principals(id,kind,external_id,display_name)
  SELECT
    '20000000-0000-4000-8000-000000000001',
    'human',
    id::text,
    'POC User'
  FROM enterprise_user
  RETURNING id
)
INSERT INTO business_iam.principal_permissions(principal_id,permission_id,reason)
SELECT human_principal.id,permission.id,'real Authentik least-privilege acceptance'
FROM human_principal
CROSS JOIN business_iam.permissions permission
WHERE permission.capability='business_iam:read';
SQL

mapping_counts="$(docker exec -e PGPASSWORD="$runtime_password" "$postgres_container" \
  psql -At -v ON_ERROR_STOP=1 -U "$runtime_role" -d "$database" \
  -c "SELECT (SELECT count(*) FROM enterprise_users),(SELECT count(*) FROM business_iam.principals),(SELECT count(*) FROM business_iam.principal_permissions),(SELECT count(*) FROM enterprise_users user_row JOIN business_iam.principals principal ON principal.kind='human' AND principal.external_id=user_row.id::text WHERE user_row.oidc_issuer='https://auth.bizfin.test/application/o/workbench/' AND user_row.oidc_subject='$oidc_subject' AND user_row.status='active' AND principal.status='active')")"
if [[ "$mapping_counts" != "1|1|1|1" ]]; then
  echo "error: temporary IAM seed counts were $mapping_counts; expected 1|1|1|1" >&2
  exit 1
fi

BUSINESS_IAM_ADMIN_DATABASE_URL="$runtime_url" \
BUSINESS_IAM_ADMIN_BIND_ADDR="127.0.0.1:$api_port" \
AUTHENTIK_ISSUER='https://auth.bizfin.test/application/o/workbench/' \
AUTHENTIK_BACKCHANNEL_ISSUER='http://127.0.0.1:9000/application/o/workbench/' \
BUSINESS_IAM_ADMIN_CLIENT_ID="$WORKBENCH_OIDC_CLIENT_ID" \
BUSINESS_IAM_ADMIN_ALLOWED_ORIGINS='https://workbench.bizfin.test' \
RUST_LOG=business_iam_admin_api=info \
  cargo run -q -p business-iam-admin-api >"$tmp_dir/admin-api.log" 2>&1 &
api_pid=$!

for _ in {1..60}; do
  if curl -fsS "http://127.0.0.1:$api_port/health/ready" >/dev/null 2>&1; then break; fi
  if ! kill -0 "$api_pid" 2>/dev/null; then
    cat "$tmp_dir/admin-api.log" >&2
    exit 1
  fi
  sleep 0.25
done
curl -fsS "http://127.0.0.1:$api_port/health/ready" >/dev/null || {
  cat "$tmp_dir/admin-api.log" >&2
  exit 1
}

if ! curl --resolve workbench.bizfin.test:443:127.0.0.1 --cacert "$cert" \
  -fsS 'https://workbench.bizfin.test/?e2e=mock' >/dev/null 2>&1; then
  VITE_OIDC_ISSUER='https://auth.bizfin.test/application/o/workbench/' \
  VITE_OIDC_CLIENT_ID="$WORKBENCH_OIDC_CLIENT_ID" \
  VITE_OIDC_REDIRECT_URI='https://workbench.bizfin.test/auth/callback' \
  VITE_OIDC_POST_LOGOUT_REDIRECT_URI='https://workbench.bizfin.test/' \
  VITE_BUSINESS_APP_ORIGIN='https://business.bizfin.test' \
  VITE_BUSINESS_APP_URL='https://business.bizfin.test/' \
  VITE_BUSINESS_IAM_ADMIN_URL="http://127.0.0.1:$api_port" \
    pnpm --dir desktop exec vite --mode e2e --host 0.0.0.0 --port 1420 --strictPort \
    >"$tmp_dir/vite.log" 2>&1 &
  vite_pid=$!
  for _ in {1..60}; do
    if curl --resolve workbench.bizfin.test:443:127.0.0.1 --cacert "$cert" \
      -fsS 'https://workbench.bizfin.test/?e2e=mock' >/dev/null 2>&1; then break; fi
    if ! kill -0 "$vite_pid" 2>/dev/null; then
      cat "$tmp_dir/vite.log" >&2
      exit 1
    fi
    sleep 0.25
  done
fi

install_ephemeral_totp() {
  local key
  key="$(openssl rand -hex 20)"
  docker exec "$authentik_container" ak shell -c \
    "from authentik.stages.authenticator_totp.models import TOTPDevice; TOTPDevice.objects.filter(user__username='poc-user',name='$totp_name').delete()" \
    >/dev/null 2>&1
  docker exec -e E2E_TOTP_KEY="$key" "$authentik_container" ak shell -c \
    "import os; from authentik.core.models import User; from authentik.stages.authenticator_totp.models import TOTPDevice; user=User.objects.get(username='poc-user'); TOTPDevice.objects.create(user=user,name='$totp_name',confirmed=True,key=os.environ['E2E_TOTP_KEY'],digits=6,tolerance=1,step=30)" \
    >/dev/null 2>&1
  printf '%s' "$key"
}

# Each scenario gets a new device so Authentik's last-used TOTP counter cannot
# leak from one browser context into another and create an order-dependent test.
scenario="${BUSINESS_IAM_AUTHENTIK_SCENARIO:-all}"
if [[ "$scenario" == "all" || "$scenario" == "session" ]]; then
  totp_key="$(install_ephemeral_totp)"
  POC_USER_TOTP_KEY="$totp_key" \
    pnpm --dir desktop exec playwright test \
      --config=playwright.authentik.config.ts \
      tests/real-authentik/business-iam-session.spec.ts
fi

if [[ "$scenario" == "all" || "$scenario" == "api" ]]; then
  totp_key="$(install_ephemeral_totp)"
  POC_USER_TOTP_KEY="$totp_key" \
  BUSINESS_IAM_ADMIN_E2E_URL="http://127.0.0.1:$api_port" \
  POC_USER_OIDC_SUBJECT="$oidc_subject" \
    pnpm --dir desktop exec playwright test \
      --config=playwright.authentik.config.ts \
      tests/real-authentik/business-iam-api.spec.ts
fi

echo "PASS: existing Authentik session, IAM read, and overreach denial"
