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
LIFE_AUTH_ALLOWED_WORKBENCH_ORIGINS=tauri://localhost,http://tauri.localhost
```

`LIFE_AUTH_BIND_ADDR` is required and must be a concrete socket address.
Delegation TTL defaults to 300 seconds and is restricted to 30–900 seconds.
Call-grant TTL defaults to 30 seconds and is restricted to 1–60 seconds.
Production OIDC issuers must use HTTPS; development/test HTTP is accepted only
for loopback hosts.

`LIFE_AUTH_ALLOWED_WORKBENCH_ORIGINS` is a comma-separated exact-origin
allowlist used for browser/webview CORS. Wildcards, paths, credentials, query
strings, and fragments are rejected.

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

## Credential rotation and fail-closed operations

Life, Business, and Hermes are separate security domains. Never copy a service
credential, database role, delegation token, session cookie, signing seed, or
verification key between them. Rotate each Life credential independently and
record only its secret-manager version and change ticket, never the value:

- **Service credentials:** rotate the Pacioli, MCP, and LifeOS credentials one
  role at a time. The Gateway accepts one value per role, so coordinate the
  producer/consumer deployment within a controlled maintenance window (or use
  an external secret-distribution overlap), verify the fixed endpoint, and then
  revoke the old value. A mismatch returns `401`; do not bypass it or fall back
  to a browser cookie or another domain's credential.
- **Call-grant signing key:** publish the new public key and `kid` to LifeOS
  before switching `LIFE_AUTH_ED25519_PRIVATE_KEY`. LifeOS should accept the old
  verification key only for the maximum issued grant lifetime plus its bounded
  clock-skew allowance, then remove it. The current grant lifetime is at most 60
  seconds. Never retain or distribute the old private seed.
- **Database credential:** create a new least-privilege Life database role or
  password, grant only the Life schema permissions, deploy
  `LIFE_AUTH_DATABASE_URL`, verify `/health/ready`, and revoke the old role.
  Business or Hermes schemas must not be on the role's search path.

For routine revocation, call the fixed delegation revoke endpoint for every
known active delegation. Revoking an identity binding also revokes its active
delegations and Embed Sessions atomically. During suspected credential or
signing-key compromise, first stop new issuance, revoke affected active
delegations, rotate the compromised material, and restore issuance only after
readiness and negative cross-domain checks pass. A database, resolver, audit,
or signing failure is fail-closed: do not mint delegations, reuse an MCP
process, connect directly to the Life database, or substitute Business/Hermes
credentials.
