# Atomic LifeOS action families

The request to create 注册杭州公司 with three named children previously consumed
the single allowed write on the parent. Keep that write budget and submit
`childTitles` in the same `create_action` operation.

The MCP boundary and LifeOS schema accept up to 20 validated direct child titles.
LifeOS creates the parent, children, idempotency receipt and audit within the
existing write transaction. Children share the workspace/project and parent
priority; no unspecified child deadlines are invented. The receipt includes all
resource references and requested/created counts. The harness reports complete
or partial counts using verified service data; legacy receipts explicitly lack
child results. The agent prompt requires using the atomic operation for named
children and clarification for unsupported child fields or more than 20 children.

This preserves the LifeOS product boundary and the existing scoped write grant;
it does not add a new endpoint or increase delegation writes.

Validation:
- MCP crate tests, including concurrent duplicate submissions and forwarding of
  all three Chinese child titles.
- Harness life_response tests for complete, partial and legacy receipts.
- Clippy for both crates, all targets, warnings denied; release binaries built.
- LifeOS action family test exercises the real write service with a transactional
  fake, including rollback on failure at each child, scope denial and replay.
  This is not fault injection against a real PostgreSQL database.
- Existing LifeOS write API/idempotency checks and TypeScript check passed.

LifeOS service commit: 5cd123098eefdb752c21aac7e436a9f6b437ad2f.
Production deployment and local agent activation are recorded below after verification.
No existing parent or child actions are recreated for this change.
