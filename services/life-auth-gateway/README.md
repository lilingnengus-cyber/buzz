# Life Auth Gateway

This service is the isolated security boundary for Pacioli's personal LifeOS
workbench. It does not share a database, service credential, token audience,
signing key, session, or audit chain with Business Workbench or Hermes.

The Task 2 skeleton intentionally exposes only:

- `GET /health/live` — process liveness; never queries a dependency.
- `GET /health/ready` — verifies PostgreSQL and Ed25519 signing readiness.

All other routes return `404`. Identity, delegation, embed, and confirmation
routes are added only in their corresponding implementation tasks.

Required configuration:

```text
LIFE_AUTH_DATABASE_URL
LIFE_AUTH_BIND_ADDR
LIFE_AUTH_DEPLOYMENT_ID
LIFE_AUTH_PACIOLI_SERVICE_TOKEN
LIFE_AUTH_MCP_SERVICE_TOKEN
LIFE_AUTH_LIFEOS_SERVICE_TOKEN
LIFE_AUTH_CALL_GRANT_ISSUER
LIFE_AUTH_CALL_GRANT_AUDIENCE=lifeos-workbench-api
LIFE_AUTH_DELEGATION_AUDIENCE=life-workbench-mcp
LIFE_AUTH_ED25519_PRIVATE_KEY
LIFE_AUTH_WORKBENCH_OIDC_ISSUER
LIFE_AUTH_WORKBENCH_OIDC_AUDIENCE
```

`LIFE_AUTH_BIND_ADDR` is required and must be a concrete socket address.
Delegation TTL defaults to 300 seconds and is restricted to 30–900 seconds.
Call-grant TTL defaults to 30 seconds and is restricted to 1–60 seconds.
Production OIDC issuers must use HTTPS; development/test HTTP is accepted only
for loopback hosts.

Service tokens must contain at least 32 safe bytes and must be pairwise
distinct. The process retains only their SHA-256 digests for fixed-length,
constant-time comparisons. The Ed25519 private key is a 32-byte seed encoded as
64 lower-case hexadecimal characters. Configuration `Debug` and `Display`
output redact database credentials, service credentials, and private key
material.
