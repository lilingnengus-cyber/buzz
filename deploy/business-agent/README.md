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

## Inventory-count release profile

`docker-compose.inventory-counts.yml` is an explicit Gateway budget override for
500-line count workflows: 64 consumed calls per signed turn and a 900-second
absolute lifetime. Apply it **last** alongside the matching count server/Host
release and migrations through 53. The base deployment retains its 20-call,
300-second defaults; the profile neither grants capabilities nor changes Core
approval policies. It applies to this Gateway's new delegations, not just one
tool or actor, and does not extend already issued delegations.

A maximum-size submission needs 25 detail pages, one preparation and 24 remaining
preview pages, leaving 14 calls for selection and additional checks. The native
Agent also caps tool calls at 64. Keep MCP output and Agent text/context budgets
at their normal defaults: pagination, not larger responses, handles this load.
Token usage or slow models may still exhaust a turn or its absolute deadline;
do not automatically replay mutations or mint new authority from the same event.
Preserve the intent ID and hash when resuming from a new signed human turn, and
recheck current state before confirmation.

Before activation, validate the actual merged Compose configuration, build and
exercise migration-compatible rollback images, and deploy the Gateway before a
Host that requests 50 ordinary capabilities. After activation, inspect the new
container's two budget variables and test read-only delegation behavior. A
successful tool-budget test alone is not a live model or production chat test.

The reviewed count-agent pause source can be reproduced with
`scripts/prepare-business-count-rollback.py SOURCE DESTINATION --expected-router-sha256 HASH`.
Use the exact reviewed source hash from the inventory-count release report;
the destination must not exist. This pauses the new count-agent endpoints with
503 while retaining schema compatibility, scope fixes, and manual workbench
count handling. It does not undo existing freezes, revert all services, or
replace migration rehearsal and authenticated route checks.
