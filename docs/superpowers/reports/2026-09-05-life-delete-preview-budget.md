# Delete preview call budget repair

The reported trace `7b92e7b8-da74-4c1b-8dde-19684bbf978b` received a valid
delegation at 15:15:42 UTC. Its only call was `action:read` for action
`cmtobzdf0000jwmmt0e8k4rak` at 15:16:07 UTC. That exhausted the one-call
delegation before a delete preview could be issued. No delete call was issued.

## Repair

- Delegations containing both read and preview authority permit four calls:
  at most three reads followed by one preview or write. Reads cannot spend the
  final reserved call. Any non-read closes the delegation immediately, including
  an operation whose transport outcome is unknown.
- Existing row locks serialize consumers, including competing previews. Exact
  confirmation execution remains a single-call delegation. IAM grants, scopes,
  versions, session checks, and signed confirmation validation remain enforced.
- Exhaustion now returns HTTP 429 and a distinct budget message. Permission
  denial remains separate. Disabled high-risk execution reports explicitly that
  deletion was not executed and the feature is not enabled.
- The agent prompt directs title resolution and version lookup before preview,
  disambiguation for duplicate titles, and a separate confirmation turn.
- The gateway Docker build uses Rust 1.95, matching the root Dockerfile and
  the pinned local toolchain.

## Validation and rollout state

- 929 tests passed across `buzz-acp`, `life-workbench-mcp`, and
  `life-auth-gateway`. Gateway database tests used a dedicated local PostgreSQL
  instance on port 55439. The new test verifies three reads, rejection of a
  fourth read without spending the reserved slot, and exactly one successful
  preview from concurrent requests.
- Clippy for all targets of the affected packages, formatting, and diff checks
  passed. Logs: `/tmp/pacioli-preview-tests.log`,
  `/tmp/pacioli-preview-clippy.log`.
- Local release binaries were built and copied with backups to the configured
  Pacioli runtime directory. Life Proxy still needs a controlled restart.
- Production image `life-auth-gateway:e2f982a5e` was built successfully from
  revision `e2f982a5e`. The existing production image has not yet been switched.
- Live preview validation is pending an available application interaction
  window. No action was deleted and no high-risk feature flag was enabled.
