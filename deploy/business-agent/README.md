# Persistent Business Read Agent

This deployment keeps Business IAM and business data authority outside the Buzz
relay while running one dedicated proxy executor. The proxy executor is not an
IAM principal and has no roles or grants. Every turn receives a short-lived
snapshot of the signed event author's current Human authority. A registered
Independent Agent remains a durable IAM principal and uses only its own grants.

## Prerequisites

- an Authentik OIDC application matching `AUTHENTIK_ISSUER`;
- a reachable Buzz relay and a dedicated Nostr key already added to the target
  channel;
- three HTTPS routes terminating TLS in front of localhost ports 3100, 3120,
  and 3130;
- independent random database, Business Read, Business Core, Agent, and model
  provider credentials.

Release builds deliberately reject plain HTTP Business URLs. Do not point the
Agent at Docker service names over cleartext. Configure the reverse proxy routes
first and use their HTTPS base URLs in `.env`.

## Start

```bash
cd deploy/business-agent
cp .env.example .env
# replace every placeholder, then:
docker compose config --quiet
docker compose up -d --build
docker compose ps
```

The database volume is persistent. `migrate` applies the shared migration
history, including the proxy-executor IAM migration, before services start.
Gateway, Core and Read API listen only on `127.0.0.1` host ports for a local
reverse proxy. The MCP server has no network listener and is spawned per turn by
`buzz-acp`.

Before replacing a Business Core or gateway image, compare the migrations in
the release source with the target database:

```bash
BUSINESS_MIGRATION_DATABASE_URL='postgresql://...' \
  just business-migration-preflight
```

The check is read-only. It accepts unapplied migrations in the new release and
rejects an incomplete release history, failed database migrations, version
gaps, or a checksum mismatch. When the database is only reachable from its
host or Compose network, query `version`, `success`, and the hex-encoded
`checksum` from `_sqlx_migrations`, then pipe the tab-separated rows to
`scripts/check-business-migrations.sh --database-manifest -`.

## Business Core image releases

Build the candidate from the release commit, then run this command on the
Docker host with a writable evidence directory:

```bash
scripts/release-business-core.sh \
  --image your-core-image:release \
  --container your-project-business-core-1 \
  --release-root /opt/business-platform/shared/core-releases \
  --dry-run
# Remove --dry-run to switch Core after the check passes.
```

The candidate binary must support `business-core --check-migrations`. The
script pins both images by Docker image ID, reads the complete Compose
configuration chain from the current container, serializes releases with
`flock`, and checks the candidate's actual embedded SQLx migrations in a
read-only database transaction. It replaces only the Core service and restores
the previous image if startup or health checks fail. Each release directory
retains the preflight output, both image IDs, and Compose overrides.

Automatic rollback requires zero pending migrations: apply new forward-only
migrations in a separate, compatible rollout before using this script.
Rollback restores the service image, never the database schema. Core may be
briefly unavailable while Compose replaces its single container.

## IAM bootstrap

Create or map the Human principal using the enterprise user UUID as
`external_id`, then grant that Human the required read capabilities. Do not
create a `proxy_agent` principal and do not grant the proxy executor any role.
Its `agent_id` appears only as `executor_id` in immutable authorization decisions
and as `agent_id` in security audit events.

## Acceptance

Ask the dedicated Agent to call `search_sales_orders` for a period the Human may
read. Verify all of the following:

1. the answer contains the data-as-of time, Trace ID, and `biz://` references;
2. `agent_read_delegations.used_calls` is greater than zero and status becomes
   `revoked` after the turn;
3. the decision has `executor_type='proxy_agent'`, the responsible
   `human_principal_id`, and a null `agent_principal_id`;
4. removing the Human grant immediately revokes an active delegation;
5. the same query is denied for a Human without the capability;
6. no Shell, filesystem, SQL, generic HTTP, or write execution tool is exposed.

`BUSINESS_ACTION_ENABLED=false` remains fixed in this Compose deployment. The
Business Action execution adapter stays blocked. Sales/purchase document chat
approval is a separate canary path controlled by
`BUSINESS_CHAT_APPROVAL_ENABLED`; it is disabled by default and must have scoped
IAM grants plus Business Core approval policies before it is enabled.
