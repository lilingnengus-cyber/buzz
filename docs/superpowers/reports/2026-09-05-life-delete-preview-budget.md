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

## Production activation and live acceptance

After the user authorized the application interaction, switched the gateway
to `life-auth-gateway:e2f982a5e` and restarted only Life Proxy. The previous
compose file is preserved at `/opt/life-auth/compose.before-preview-budget.yml`.
Gateway readiness returned HTTP 204.

At 23:42 Asia/Shanghai on 2026-09-05, a fresh DM requested an action lookup
followed by `preview_life_write` for `delete_action`. After the agent became
idle, the thread contained one verified preview reply, with no deletion.

- Source event: `fa0a5795c22eec0dc8f661761c5add6349559fe476c5be063700e0043e1a2f88`
- Action: `cmtobzdf0000jwmmt0e8k4rak`
- Preview: `332b0344-e6b5-480d-8234-a13a80efa4cd`, version 1
- Trace: `ebfb967c-1d63-4d13-bb68-5df4d54a944e`
- Audit: `0cf092a6-96df-4f58-b3de-d3e16ee5b6f2`

Actual deletion remains pending a separately signed exact confirmation and
enabled high-risk execution. No high-risk feature flag was changed by this
rollout. The preview expires after ten minutes; regenerate it if needed.

## Confirmed-execution activation

At the user's subsequent instruction, enabled `LIFE_CHAT_HIGH_RISK_WRITE_ENABLED`
in LifeOS production and as a Life Proxy persona override (global agent defaults
remain unchanged). Backed up the server configuration outside its repository,
restarted LifeOS with the updated environment, and restarted only Life Proxy.
LifeOS health returned HTTP 200. At 23:48 Asia/Shanghai, preview
`332b0344-e6b5-480d-8234-a13a80efa4cd` was still PENDING and expires at
23:52:37 on 2026-09-05. Returned the UI to its preview thread. The user must
send the exact confirmation there; this activation did not delete the action.
