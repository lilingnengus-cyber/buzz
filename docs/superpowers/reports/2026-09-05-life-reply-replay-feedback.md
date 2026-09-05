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
have not been deployed. Deploy that companion change, then replay the existing
acceptance UUID and confirm the same resource/audit with explicit reuse text.
