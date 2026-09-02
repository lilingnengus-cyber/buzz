# LifeOS Phase 3 Acceptance Evidence

Date: 2026-09-02 (UTC)

Scope: Task 10 real relay + ACP + Life Auth Gateway + Life Workbench MCP +
LifeOS acceptance. All evidence came from isolated databases
`buzz_life_e2e`, `life_auth_e2e`, and `lifeos_life_e2e`; no personal LifeOS
records were used.

## Result

The complete read, ordinary write, optimistic-conflict, high-risk preview,
exact-confirmation, and replay-protection paths passed. The exercise found one
wire-boundary defect: Nostr kind `9` deserializes to a named `Kind` variant, but
write-confirmation validation compared enum variants as though every accepted
kind remained `Kind::Custom`. Commit `d98112ad9` now validates the numeric kind
and adds an HTTP-wire round-trip regression test.

## Trace chain

| Scenario | Source event | Agent turn | IAM decision | Delegation | Call | Trace | Domain audit | Response event |
|---|---|---|---|---|---|---|---|---|
| Read action v1 | `e75f9ea3346ab7ba9cc8de76c62a77ba26054a29ad22cdaa29ec593551a97729` | `53f9ea12-6665-4ed3-9bd1-03ea5f7ede97` | `009265ff-a92d-44bb-afac-9d87938f6167` | `9ca35318-f7dd-49be-bdd1-f419b31504fa` | `da6ef569-a62f-4c6c-8a3e-e3cdaf96e80f` | `6ba5b0e4-aafc-4a62-a280-191bc48e2c06` | `2205533e-509c-4f92-b65e-ba39c7ef8ab1` | `f000f6cecd73d32e6cda75e2d0d9f1604056cc741a37a624c4a75a90bb2a4c44` |
| Create action v1 | `ee70a11069119cf063e40046e55cb022239d50194d3ba892ba6287528d877bfd` | `692cbb0d-2c1a-47a9-a0d4-0dd16ba77ea5` | `e2f79cef-55bb-453e-a752-67d0241d9e23` | `548a54aa-d178-4249-a034-b4d5388a78f3` | `55257a81-2a4d-4f1f-bfe2-ff088712f9ba` | `88d2c31f-9376-45a6-851a-24ac181d143d` | `6b17060d-1e1e-4650-b652-8292cdf8c842` | `d99a1866a4535b80208700789ebd80fd370ef4022e91f3f9103fe7506c82a4eb` |
| Status PENDING -> DOING, v1 -> v2 | `5cc1dda019c194b4e1fbc0298e342b63914d30b7f078143fdb8bc75d67ae69be` | `f70f5c49-375e-46e6-b1f9-12d58cb366e2` | `0068a2c8-d895-4795-9175-91202b49eaa3` | `fbca90c6-d111-4d79-b570-9664908cd1ad` | `499c8256-ec51-4cf4-a37f-058c992f2775` | `160cfdc6-9cd2-4c3f-aaee-e5718037ebeb` | `5dfc303c-1e2a-4f8a-8867-612774123a21` | `dce59a4f3f850c5efd3212d17b528c241a3293afd36551a834613956d6ee0bd6` |
| Stale v1 update rejected | `24cfcc57f010efbde0a90c5599e4b8a47a5230958744ee5f36994e8aa2afa0d9` | `43dad238-35cc-4e14-920b-60bda9d46c86` | `d5fc9c82-9d34-4a3d-a75b-7e027198437d` | `41eb250d-21f5-4622-9730-8f3879413276` | `9809090f-1ce1-4f50-ab2e-b05866d8a61d` | `1195cc5d-13b9-4664-84ec-3a3859a0378f` | none | `18ce81388c7a5b9170e4391e6be40e5ff99e40f59ed61a10d862892f631a84ce` |
| Preview delete at v2 | `3c5bba79eb0811aa4675c28d0cb1e07113c47002024a6900ddab487f237b0abf` | `e2dda77e-4830-4a96-a560-a9f9dbefe91a` | `f3a37fff-f652-480d-bfdd-7ca34f3f0aa9` | `d9ad6260-b872-4db6-be8f-adb55a150160` | `6cf2d4a6-a392-46e7-9868-c7faa8e05193` | `aa6957ad-56b1-47a1-a085-c5a59047544d` | `965b4b33-0d07-4caa-a3a4-73139475e8c8` | `e54cd6079f01074887990cd77586b78e14da48bb805db28b7e7d67bf02a31efb` |
| Exact confirmation and delete | `05a7ada74cb21e1964af3fa180fff5d584525059a8af36985d4139b077e6e8f9` | `d105c9b1-8ca3-468c-9fcc-09176a73cdad` | `a36f239d-0a0b-413f-8210-0b734b661bee` | `a3986e59-ae81-4a69-a790-2d7c707a724b` | `fe9ba1e8-cb0e-4069-9312-830bf1d24522` | `aa06a6e0-3df3-4972-b8e3-93835cd4ab9f` | `f647011c-630b-4c41-86bf-64c96bcd7079` | `a41563c223d14d0e5877968c15764610ae38f09d37bdd7907c7152269b7d1f09` |

## Negative-path evidence

- A plain Chinese confirmation, source event
  `723eb367828f649c56d088548fdbe465f1a3f3eadc73cbd4dc350657c0dd03d5`,
  did not create a Gateway confirmation, did not consume the pending LifeOS
  command, and left the action at `DOING v2`.
- The stale update returned `Resource version conflict`; it created neither a
  `LifeDomainAudit` row nor a `WorkbenchCallReceipt`, and the action stayed at
  `DOING v2`.
- The accepted exact command used WriteCommand
  `c62c20f9-6027-4998-83b5-db56d8784aa2`. Gateway recorded
  `WRITE_PREVIEW_CONFIRMED` and `WRITE_CONFIRMATION_CONSUMED` against the same
  source event and trace, and LifeOS atomically changed the command to
  `CONSUMED` while deleting the v2 action.
- Replaying the same exact command in newly signed source event
  `e163de771db4812589e320069dc07d5f01e035656d0e651cf72f6f6fbced9cba`
  was rejected as already used before IAM/delegation issuance. The one Gateway
  confirmation remained consumed, the LifeOS command remained consumed, and
  the action row count remained zero.

## Receipt and response checks

The successful create, status update, preview, and confirmed delete each have
one `WorkbenchCallReceipt` keyed by the Gateway call ID and one persisted
idempotency result. ACP response events contain the matching trace and domain
audit identifiers. The confirmed-delete receipt reports `deleted: true`; the
final database query reports zero rows for the target action.

## Exit review

- Workbench read code contains no sample fallback or default-workspace lookup.
- MCP URLs are exact startup origins; tool input cannot override a host, path,
  query, SQL expression, or Prisma filter.
- Versioned writes use conditional `updateMany`/`deleteMany` operations inside
  the same database transaction as idempotency state and domain audit records.
- MCP sends a write request at most once. A lost or temporary write response
  returns non-retryable `write_outcome_unknown` rather than replaying the
  mutation. Automatic status reconciliation is not implemented.
- Hermes direct-write behavior remains available to Hermes credentials, while
  Hermes, Life, and Business credentials fail closed at the other domains'
  boundaries.

No Critical or Important review findings remained after the numeric Nostr-kind
fix.

## Verification

Pacioli passed:

- `cargo fmt --all -- --check`
- Clippy with warnings denied for `life-workbench-contracts`,
  `life-workbench-mcp`, `buzz-acp`, and `life-auth-gateway`
- All tests for `life-workbench-contracts`, `life-workbench-mcp`, and
  `buzz-acp` (including 835 `buzz-acp` unit tests)
- All PostgreSQL-backed `life-auth-gateway` tests, including the kind-9 wire
  round-trip regression

LifeOS passed Prisma generation, all eight Workbench/Hermes-isolation scripts,
and the production Next.js build. The repository-wide `npm run test:static`
reached and passed every Workbench test, then stopped at the pre-existing
knowledge-history assertion that expects one space in
`knowledgeMergeRecords KnowledgeMergeRecord[]`; the schema line has two spaces
and predates this phase's Workbench commits. That unrelated baseline mismatch
was left unchanged.
