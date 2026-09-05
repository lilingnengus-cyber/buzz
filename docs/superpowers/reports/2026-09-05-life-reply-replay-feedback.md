# Life Proxy reply and replay feedback verification

## Changes

Life turns now select harness-owned response delivery in the prompt formatter.
Their context no longer instructs the agent to publish via the Buzz CLI, and
the Life base prompt explicitly leaves publication to the harness. Standard
agents keep their existing delivery instructions. This removes contradictory
instructions; it is not a sandbox for built-in runtime tools.

Successful Workbench envelopes accept optional `idempotencyReplayed` metadata.
The MCP client marks successful in-process cache hits as replayed while keeping
the original result, references, audit and trace. Failed/unknown writes remain
failures and are never retried. The trusted reply shows reuse only for an
explicit true marker, first execution for false, and makes no idempotency claim
when an older service omits the marker.

The companion LifeOS checkout at `/Users/aaronli/Projects/life-os` adds this
marker in `lib/workbench/write-service.ts` and accepts it in
`lib/workbench/api-handler.ts`. Cached database receipts remain unchanged.
`scripts/test-workbench-idempotency.mjs` covers fresh execution, same-call and
cross-call replay, unchanged receipts, and no repeated mutation.

## Validation

- 886 Rust tests passed across `buzz-acp`, `life-workbench-mcp`, and
  `life-workbench-contracts`; doc tests passed.
- Clippy for those packages and all targets, Rust formatting, and diff
  whitespace checks passed.
- Release build of `buzz-acp` and `life-workbench-mcp` passed. Both binaries
  were installed in the configured local agent runtime directory with backups;
  only Life Proxy was restarted.
- LifeOS TypeScript checking and the idempotency, write API, exact-confirmation,
  authorization boundary, read API, and audit-redaction checks passed.
- Repository-wide `just ci` was not rerun for this change. The previous full
  gate result applies to the previous revision, not this patch.

## Live read acceptance

Application: `/Applications/Pacioli.app`, relay `wss://buzz.shiyueshizi.com`.
Agent: Life Proxy. Date: 2026-09-05, Asia/Shanghai.

The initial reads were rejected by identity authorization while Life Dock
showed an expired session. The application's Connect again action restored
the session. A fresh read then completed with exactly one thread reply after
the agent became idle. It contained both the answer (two focus actions,
including the requested action) and the harness-verified receipt.

- Source event: `d3ce535d54c5b13b94c8452ce8d6ab73ac94100cb67c7d513402f4457befdaf3`
- Audit ID: `c0b7aafc-6265-421d-933e-46eb27e283ea`
- Trace ID: `45366911-8d2c-4aab-9853-102ebdfc4378`
- Target: `life://action/cmtobzdf0000jwmmt0e8k4rak`, version 1

No LifeOS domain data was changed during this live check. Cross-turn replay
feedback is implemented and tested locally, but the LifeOS server changes
had not yet been deployed at the time of the initial read acceptance.

## Production deployment and cross-turn replay acceptance

Completed on 2026-09-05 at approximately 21:40 Asia/Shanghai.

- LifeOS production now runs `09db335c96a1061a71f99660df1aaa06f43475b8`,
  including replay metadata commit `71ca417` (the main-based cherry-pick of
  `4117cbb`). Deployment run:
  https://github.com/lilingnengus-cyber/life-os/actions/runs/33969390468
- Two untracked configuration backups blocked the existing deployment script.
  They were preserved outside the repository under
  `/home/ubuntu/life-os-config-backups/20260905-replay-deploy/`; no backup was
  deleted or included in Git.
- The first deployment then exposed three stale dashboard tests that still
  inspected the page wrapper after its logic moved into shared content.
  Commit `09db335` points those tests at the shared component without removing
  assertions. The complete static gate passed locally and in deployment.
- Production build and Next/MCP health checks passed. Independent checks
  returned HTTP 200 for both services. The server checkout is clean.
- The legacy Hermes today-context probe was skipped because no workspace-bound
  token is configured; the actual delegated Life Proxy workflow below passed.

Replayed the identical acceptance request in a fresh Life Proxy DM turn with
UUID `0a01540c-3ff3-4236-9c2b-7c8b88312275`. After the agent became idle, the
thread contained exactly one reply, including:

> 幂等命中，已复用成功结果，未重复执行。

- Source event: `f3142012940c834d295aa6fb1fd82db81f32d4af4f10969be488c3ae7b2ba70f`
- Resource: `life://action/cmtobzdf0000jwmmt0e8k4rak`, version 1
- Audit ID: `700f8c28-16d6-49d6-957d-caacbb41cee8`
- New trace: `82122d1e-3e70-4ed6-9f35-976984451f62`

The resource, version, and audit match the original creation receipt. Life Dock
also shows the same action at version 1 with status PENDING. This verifies
server-confirmed reuse across separate turns, with no repeated execution.

## Full CI after deployment acceptance

The complete `just ci` gate passed (exit 0) on revision `05365a521` on
2026-09-05. This supersedes the earlier note that full CI had not been rerun.

- Rust tests in the full gate: 4,309 passed; configured ignored tests retained.
- Desktop JavaScript tests: 5,533 passed, zero failures.
- Flutter tests: 1,663 passed.
- Formatting, lint/static checks, file-size gates, desktop/native checks,
  desktop frontend build, and web build passed.
- Log: `/tmp/pacioli-life-reply-full-ci.log`.

Used `CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0`
to keep artifacts within available disk space. After the root Rust test stage
finished and desktop JavaScript tests started, cleaned only the completed root
development artifacts (3.7 GiB). The separate Tauri target was retained and all
remaining stages completed normally. No quality gate was skipped.
