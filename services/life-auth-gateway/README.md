# Life Auth Gateway

This service is the isolated security boundary for Pacioli's personal LifeOS
workbench. It does not share a database, service credential, token audience,
signing key, session, or audit chain with Business Workbench or Hermes.

The service exposes fixed identity and health routes only:

- `GET /health/live` — process liveness; never queries a dependency.
- `GET /health/ready` — verifies PostgreSQL and Ed25519 signing readiness.
- `POST /v1/workbench/sessions` — verifies OIDC plus nonce, resolves the
  explicit LifeOS identity mapping, and returns a hash-only Workbench Session.
- `POST /v1/identity-bindings/challenges` — creates a session/deployment-bound
  canonical challenge.
- `POST /v1/identity-bindings` — verifies a complete signed kind `24243` event.
- `DELETE /v1/identity-bindings/{binding_id}` — revokes the binding and its
  active delegations and Embed Sessions atomically.
- `GET /v1/me` — returns the mapped opaque user, memberships, and bindings.

All other routes return `404`; there is no generic proxy surface. Session
creation requires the verified OIDC bearer and nonce. The remaining identity
routes require the deployment-bound Workbench Session bearer.

Required configuration:

```text
LIFE_AUTH_DATABASE_URL
LIFE_AUTH_BIND_ADDR
LIFE_AUTH_DEPLOYMENT_ID
LIFE_AUTH_PACIOLI_SERVICE_TOKEN
LIFE_AUTH_MCP_SERVICE_TOKEN
LIFE_AUTH_LIFEOS_SERVICE_TOKEN
LIFE_AUTH_LIFEOS_BASE_URL
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

`LIFE_AUTH_LIFEOS_BASE_URL` is a fixed HTTPS origin for the one internal
identity-resolution route; non-production loopback HTTP is accepted. Identity
challenge TTL defaults to 90 seconds and can be configured from 30–300 seconds
with `LIFE_AUTH_IDENTITY_CHALLENGE_TTL_SECONDS`.

Service tokens must contain at least 32 safe bytes and must be pairwise
distinct. The process retains only their SHA-256 digests for fixed-length,
constant-time comparisons. The Ed25519 private key is a 32-byte seed encoded as
64 lower-case hexadecimal characters. Configuration `Debug` and `Display`
output redact database credentials, service credentials, and private key
material.
