# Business IAM Admin API

This is the separately deployable management plane for Business IAM. It has no
dependency on Buzz relay, ACP, Desktop, or Agent credentials.

Every request uses an Authentik OIDC bearer token. The token must contain a
recent `auth_time` and one configured MFA value in `amr`; the mapped active
human principal must also hold the applicable `business_iam:read`,
`business_iam:request`, or `business_iam:approve` capability.

Mutations are never applied by the create endpoint. They become immutable
change requests with a 24-hour lifetime and an optimistic target version.
High-risk changes need one approver; critical grants, revocations, disables,
and sensitive role assignments need two. The requester cannot approve their
own request, approvers cannot vote twice, and every decision carries a hash of
the exact Step-up JWT evidence. Approval and audit rows are append-only.

## API

- `GET /api/iam/catalog`
- `GET /api/iam/change-requests?status=pending`
- `POST /api/iam/change-requests`
- `POST /api/iam/change-requests/{id}/approve`
- `POST /api/iam/change-requests/{id}/reject`

All responses use `Cache-Control: no-store`; request bodies are limited to 32
KiB. The service accepts bearer authentication only and therefore does not use
ambient browser cookies.

## Required environment

```text
BUSINESS_IAM_ADMIN_DATABASE_URL
BUSINESS_IAM_ADMIN_BIND_ADDR=0.0.0.0:3110
AUTHENTIK_ISSUER
BUSINESS_IAM_ADMIN_CLIENT_ID
BUSINESS_IAM_ADMIN_ALLOWED_ORIGINS
BUSINESS_IAM_STEP_UP_MAX_AGE_SECONDS=300
BUSINESS_IAM_REQUIRED_MFA_AMR=mfa
```

Run migrations with the schema-owner credential and grant the narrowly scoped
runtime role:

```bash
BUSINESS_IAM_ADMIN_DATABASE_URL=... \
BUSINESS_IAM_ADMIN_RUNTIME_DATABASE_ROLE=business_iam_admin_runtime \
business-iam-admin-api --migrate-only
```

The offline `business-iam-admin` CLI remains the explicit bootstrap and
break-glass path. Its database credential must not be shared with this API.
