# BizOS Independent Legal Entity and Operating Unit Tree Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make legal entities and operating units independent business dimensions, model operating units as an unlimited-depth tree, and let every business record select both dimensions independently.

**Architecture:** Add a nullable parent edge to `business_units`, migrate existing units under one root, and expand authorized operating-unit roots with PostgreSQL recursive CTEs. Existing transaction columns remain stable; commands validate legal entity and operating unit independently, while reports expand a selected operating node to its descendants. The rollout uses expand–migrate–contract so the old legal-entity column remains available until all runtime dependencies are removed.

**Tech Stack:** PostgreSQL migrations and recursive CTEs, Rust/Axum/SQLx, React 19/TypeScript, Node test runner, repository integration tests.

**Spec:** `docs/superpowers/specs/2026-09-22-independent-legal-entity-operating-unit-tree-design.md`

## Global Constraints

- Legal entities and operating units have no mapping table or pair-validity rule.
- Every applicable business record keeps both `legal_entity_id` and `business_unit_id` as independent dimensions.
- Operating units form one unlimited-depth tree and must never contain a cycle.
- Existing record IDs, amounts, statuses, audit records, `legal_entity_id`, and leaf `business_unit_id` values remain unchanged during migration.
- First-phase historical rollups use the current organization tree; facts retain their original leaf unit.
- Warehouses remain legally owned and must match a document's legal entity; their operating-unit field is only a management default.
- Currency totals remain single-currency and are never summed across currencies.
- Use only additive migration `0035`; do not edit previously applied migrations.
- Production Rust paths must not add `unwrap()` or `expect()`, and new public APIs require doc comments.
- Every commit uses `git commit -s` after activating `. ./bin/activate-hermit`.

## File Structure

- `services/business-auth-gateway/migrations/0035_operating_unit_tree.sql`: additive schema, root migration, compatibility view, tree constraints and indexes.
- `services/business-core/src/operating_units.rs`: one module for descendant expansion and cycle-safe parent validation.
- `services/business-core/src/lib.rs`: exports the operating-unit module.
- `services/business-core/src/store.rs`: expands operating-unit scope roots into effective descendant IDs.
- `services/business-core/src/master_data.rs`: exposes tree fields and saves/moves operating units without a legal-entity dependency.
- `services/business-core/src/master_data_api.rs`: maps tree-specific validation errors through the existing master-data routes.
- `services/business-core/src/b2/sales.rs`, `services/business-core/src/b3/purchasing.rs`, `services/business-core/src/crm/mod.rs`: independently validate document dimensions.
- `services/business-core/src/s1/trends.rs`, `services/business-core/src/crm/registers.rs`: accept operating-unit subtree filters and expose filter semantics.
- `services/business-core/src/store/master_search.rs`: returns independent legal-entity and operating-unit candidates to agents.
- `apps/business-web/src/api.ts`: tree DTOs and request types.
- `apps/business-web/src/CoreMasterDataCenter.tsx`: legal-entity list plus operating-organization tree and parent selector.
- `apps/business-web/src/core-master-data.css`: tree indentation, path and action styling.
- `apps/business-web/src/OperatingUnitTree.test.mjs`: deterministic tree builder and selection behavior tests.
- `services/business-core/tests/postgres_operating_units.rs`: database-backed migration, tree, authorization and cross-entity workflow tests.

## Review Focus

- A moved node whose proposed parent is a deep descendant must fail with `OPERATING_UNIT_CYCLE`, covered in Task 2.
- Two authorized roots with overlapping descendants must produce a deduplicated effective scope, covered in Task 3.
- A legal entity and operating unit that were never paired before must create valid sales, purchase and CRM drafts, covered in Task 4.
- A disabled descendant must remain visible in historical detail but unavailable for new writes, covered in Tasks 2 and 4.
- Reparenting a leaf must change current subtree rollups without changing the leaf ID stored on existing facts, covered in Task 5.

---

### Task 1: Add the compatible operating-unit tree schema

**Files:**
- Create: `services/business-auth-gateway/migrations/0035_operating_unit_tree.sql`
- Create: `services/business-core/tests/postgres_operating_units.rs`

**Interfaces:**
- Produces: `business_units.parent_business_unit_id`, `business_operating_root` marker, and compatibility view columns `parent_business_unit_id`, `business_unit_path`, `business_unit_depth`.
- Consumes: existing `business_units`, `business_group_profile`, and `core_master_data_maintenance` objects.

- [ ] **Step 1: Write the migration integration test**

Add `migration_preserves_fact_dimensions_and_builds_one_tree` in `postgres_operating_units.rs`. Seed two legal entities, two operating units with different old `legal_entity_id` values, and one sales order per unit. Run migrations and assert both sales rows retain their original IDs, exactly one active root exists, every non-root unit has a parent, and the recursive walk returns each unit once.

```rust
let fact = sqlx::query_as::<_, (Uuid, Uuid)>(
    "SELECT legal_entity_id,business_unit_id FROM sales_orders WHERE id=$1",
)
.bind(order_id)
.fetch_one(&pool)
.await?;
assert_eq!(fact, (legal_entity_id, business_unit_id));
```

- [ ] **Step 2: Run the migration test and confirm the missing-column failure**

Run: `cargo test -p business-core --test postgres_operating_units migration_preserves_fact_dimensions_and_builds_one_tree -- --nocapture`

Expected: FAIL because migration `0035` and `parent_business_unit_id` do not exist.

- [ ] **Step 3: Add migration 0035**

Implement these concrete operations in order:

```sql
ALTER TABLE business_units
  ADD COLUMN parent_business_unit_id uuid
  REFERENCES business_units(id) ON DELETE RESTRICT,
  ADD COLUMN is_operating_root boolean NOT NULL DEFAULT false,
  ADD CONSTRAINT business_units_not_own_parent
    CHECK (parent_business_unit_id IS NULL OR parent_business_unit_id <> id);

CREATE UNIQUE INDEX business_units_one_root
  ON business_units (is_operating_root) WHERE is_operating_root;
CREATE INDEX business_units_parent_idx
  ON business_units (parent_business_unit_id);
```

Use a `DO` block to reuse the sole existing unit as root when only one exists; otherwise insert one root with code `GROUP_OPERATIONS`, name from `business_group_profile`, and attach every pre-existing unit beneath it. Replace `core_master_data_maintenance` so operating-unit rows return `legal_entity_id = NULL`, plus parent ID, path and depth from a recursive CTE. Keep the old `business_units.legal_entity_id` column during this release.

- [ ] **Step 4: Verify migration invariants**

Run the Task 1 test and SQL assertions for one root, no orphans, no duplicate traversal rows, and unchanged sales-order dimensions.

Expected: PASS.

- [ ] **Step 5: Commit the additive schema**

```bash
git add services/business-auth-gateway/migrations/0035_operating_unit_tree.sql services/business-core/tests/postgres_operating_units.rs
git commit -s -m "feat(bizos): add compatible operating unit tree schema"
```

### Task 2: Implement cycle-safe operating-unit commands and tree reads

**Files:**
- Create: `services/business-core/src/operating_units.rs`
- Modify: `services/business-core/src/lib.rs`
- Modify: `services/business-core/src/master_data.rs`
- Modify: `services/business-core/src/master_data_api.rs`
- Test: `services/business-core/tests/postgres_operating_units.rs`

**Interfaces:**
- Produces: `validate_parent(tx, unit_id, parent_id) -> Result<(), DomainError>`, `descendant_ids(pool, roots, include_disabled) -> Result<BTreeSet<Uuid>, DomainError>`, and master-data DTO fields `parentBusinessUnitId`, `ancestorPath`, `depth`, `descendantCount`.
- Consumes: Task 1 schema and existing master-data save/status APIs.

- [ ] **Step 1: Add failing tree-command tests**

Add tests for creating a fourth-level child, moving a leaf, rejecting own-parent, rejecting a deep cycle with code `OPERATING_UNIT_CYCLE`, rejecting a disabled parent, and refusing to disable a node with active descendants.

```rust
let error = move_unit(&client, root_id, leaf_id, root_version).await;
assert_eq!(error.code, "OPERATING_UNIT_CYCLE");
```

- [ ] **Step 2: Run only the tree-command tests**

Run: `cargo test -p business-core --test postgres_operating_units operating_unit_tree -- --nocapture`

Expected: FAIL because parent input, tree DTOs and cycle validation are absent.

- [ ] **Step 3: Add the focused operating-unit module**

Implement `validate_parent` with a recursive CTE that starts at the candidate parent and walks upward; reject if it reaches `unit_id`. Implement `descendant_ids` with `UNION` rather than `UNION ALL` as an additional safety guard and collect into `BTreeSet<Uuid>`.

```rust
pub async fn descendant_ids(
    pool: &PgPool,
    roots: &BTreeSet<Uuid>,
    include_disabled: bool,
) -> Result<BTreeSet<Uuid>, DomainError>;
```

- [ ] **Step 4: Update master-data save, list, move and status behavior**

For `CoreMasterType::BusinessUnit`, require `legalEntityId` to be absent, accept `parentBusinessUnitId`, validate the parent in the same transaction, and update parent/name/version atomically. Return a stable `OPERATING_UNIT_CYCLE` error code through `MasterApiError`. Make disable impact count active descendants and require callers to disable children first.

- [ ] **Step 5: Run tree tests and master-data regression tests**

Run:

```bash
cargo test -p business-core --test postgres_operating_units -- --nocapture
cargo test -p business-core --test postgres_b1 -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Commit tree behavior**

```bash
git add services/business-core/src/operating_units.rs services/business-core/src/lib.rs services/business-core/src/master_data.rs services/business-core/src/master_data_api.rs services/business-core/tests/postgres_operating_units.rs
git commit -s -m "feat(bizos): manage operating units as a tree"
```

### Task 3: Expand operating-unit authorization roots to descendants

**Files:**
- Modify: `services/business-core/src/store.rs`
- Modify: `services/business-core/src/model.rs`
- Test: `services/business-core/tests/postgres_operating_units.rs`

**Interfaces:**
- Produces: `DataScopes.business_unit_ids` as the effective, deduplicated set of authorized roots and descendants; existing callers keep the same field name and exact-membership checks.
- Consumes: Task 2 `descendant_ids` and existing `business_unit_scopes` grants.

- [ ] **Step 1: Write failing authorization tests**

Build a tree `root -> north -> hangzhou` and `root -> south`. Grant `north`, assert the snapshot contains `north` and `hangzhou` but not `south`. Grant both `root` and `north`, assert every ID appears exactly once. Disable `hangzhou`, assert historical read scope still includes it while new-write validation rejects it.

- [ ] **Step 2: Run the authorization tests**

Run: `cargo test -p business-core --test postgres_operating_units operating_unit_scope -- --nocapture`

Expected: FAIL because scope grants currently return exact IDs only.

- [ ] **Step 3: Expand roots while constructing the authorization snapshot**

Rename the private query helper to `business_unit_scope_roots`, call `descendant_ids(..., true)`, and store the result in `DataScopes.business_unit_ids`. Preserve stable hashing so the hash changes when tree membership changes. Update tree-move transactions to increment `business_authorization_revision` and emit the existing authority-change outbox/audit event, which invalidates cached agent grants.

- [ ] **Step 4: Run authorization and approval regressions**

Run:

```bash
cargo test -p business-core --test postgres_operating_units operating_unit_scope -- --nocapture
cargo test -p business-core document_approval -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit subtree authorization**

```bash
git add services/business-core/src/store.rs services/business-core/src/model.rs services/business-core/tests/postgres_operating_units.rs
git commit -s -m "feat(bizos): authorize operating unit subtrees"
```

### Task 4: Remove legal-entity/operating-unit pair validation from workflows

**Files:**
- Modify: `services/business-core/src/b2/sales.rs`
- Modify: `services/business-core/src/b3/purchasing.rs`
- Modify: `services/business-core/src/crm/mod.rs`
- Modify: `services/business-core/src/master_data.rs`
- Test: `services/business-core/tests/postgres_b2.rs`
- Test: `services/business-core/tests/postgres_b3.rs`
- Test: `services/business-core/tests/postgres_crm.rs`
- Test: `services/business-core/tests/postgres_operating_units.rs`

**Interfaces:**
- Produces: independent active/existence validation for `legalEntityId` and `businessUnitId`; request and response field names remain unchanged.
- Consumes: Task 3 effective scopes and the existing warehouse legal-ownership rule.

- [ ] **Step 1: Add cross-entity failing workflow tests**

Create two legal entities and a unit whose retained compatibility `legal_entity_id` points at the first. With scopes for both dimensions, create a sales draft, purchase draft and CRM opportunity using the second legal entity and that unit. Assert all three succeed and preserve both selected IDs. Add negative cases for disabled legal entity, disabled unit, missing legal scope, missing unit subtree scope, and warehouse legal-entity mismatch.

- [ ] **Step 2: Run the cross-entity tests**

Run: `cargo test -p business-core --test postgres_operating_units independent_dimensions -- --nocapture`

Expected: FAIL at the old join or equality checks.

- [ ] **Step 3: Replace pair checks with independent checks**

In sales, verify the legal entity and operating unit independently; retain customer activity and warehouse legal ownership. In purchasing, remove supplier `business_unit_id` equality and independently verify the chosen unit. In CRM, replace the `business_units JOIN business_legal_entities` pair check with two `EXISTS` checks and validate an optional customer by activity/access rather than the document's selected pair. In core-master validation, remove `u.legal_entity_id=e.id` conditions.

- [ ] **Step 4: Run affected workflow suites**

Run:

```bash
cargo test -p business-core --test postgres_operating_units -- --nocapture
cargo test -p business-core --test postgres_b2 -- --nocapture
cargo test -p business-core --test postgres_b3 -- --nocapture
cargo test -p business-core --test postgres_crm -- --nocapture
```

Expected: PASS, including warehouse mismatch rejection.

- [ ] **Step 5: Commit independent transaction validation**

```bash
git add services/business-core/src/b2/sales.rs services/business-core/src/b3/purchasing.rs services/business-core/src/crm/mod.rs services/business-core/src/master_data.rs services/business-core/tests
git commit -s -m "feat(bizos): validate business dimensions independently"
```

### Task 5: Add subtree reporting and agent lookup semantics

**Files:**
- Modify: `services/business-core/src/s1/trends.rs`
- Modify: `services/business-core/src/crm/registers.rs`
- Modify: `services/business-core/src/store/master_search.rs`
- Modify: `services/business-core/src/master_data.rs`
- Test: `services/business-core/tests/postgres_operating_units.rs`
- Test: `services/business-core/tests/postgres_crm.rs`

**Interfaces:**
- Produces: subtree-filtered reports with `businessUnitFilterMode: "subtree"`, independent master-data candidates, and operating-unit path metadata.
- Consumes: Task 2 descendants and Task 3 effective scope.

- [ ] **Step 1: Add failing report and lookup tests**

Seed facts at `hangzhou` and `beijing`; assert the `north` report sums both exactly once and a legal-entity filter intersects independently. Move `hangzhou` under `east`; assert the historical fact keeps `hangzhou` while the current `east` subtree includes it. Search by unit name and assert `legalEntityId` is null and `ancestorPath` is present.

- [ ] **Step 2: Run focused reporting tests**

Run: `cargo test -p business-core --test postgres_operating_units subtree_reporting -- --nocapture`

Expected: FAIL because current queries accept only exact scope sets and the directory still joins units to legal entities.

- [ ] **Step 3: Apply subtree IDs at report boundaries**

Resolve requested operating roots to descendants once per request, intersect them with `snapshot.scopes.business_unit_ids`, and bind the result to existing `ANY($n)` queries. Add response metadata with the requested root IDs and `subtree` mode. Do not change currency grouping.

- [ ] **Step 4: Return independent master-data candidates**

Update the directory/search projection so operating-unit rows carry no legal entity and include their full path. Keep `legalEntityId: null` during the compatibility window. Ensure Agent lookup produces separate candidate lists and never narrows operating units after a legal entity is selected.

- [ ] **Step 5: Run reporting, CRM and master-search tests**

Run:

```bash
cargo test -p business-core --test postgres_operating_units subtree_reporting -- --nocapture
cargo test -p business-core --test postgres_crm -- --nocapture
cargo test -p business-core master_search -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Commit reporting and lookup behavior**

```bash
git add services/business-core/src/s1/trends.rs services/business-core/src/crm/registers.rs services/business-core/src/store/master_search.rs services/business-core/src/master_data.rs services/business-core/tests
git commit -s -m "feat(bizos): report and search operating unit subtrees"
```

### Task 6: Replace the linear master-data UI with an operating tree

**Files:**
- Modify: `apps/business-web/src/api.ts`
- Modify: `apps/business-web/src/CoreMasterDataCenter.tsx`
- Modify: `apps/business-web/src/core-master-data.css`
- Create: `apps/business-web/src/OperatingUnitTree.test.mjs`

**Interfaces:**
- Produces: `buildOperatingTree(records) -> OperatingUnitNode[]`, independent legal-entity list and operating tree, path-aware parent selection.
- Consumes: Task 2 DTO fields and unchanged business form fields `legalEntityId`, `businessUnitId`.

- [ ] **Step 1: Write failing deterministic tree tests**

Test unordered input, four levels, disabled nodes, a search hit that retains ancestors, missing-parent fallback, and independent form selection. Assert selecting a legal entity leaves `businessUnitId` unchanged and selecting a unit leaves `legalEntityId` unchanged.

```js
assert.equal(nextAfterLegalChange.businessUnitId, "hangzhou-id");
assert.equal(nextAfterUnitChange.legalEntityId, "legal-cn-id");
```

- [ ] **Step 2: Run the frontend tree test**

Run: `cd apps/business-web && node --test src/OperatingUnitTree.test.mjs`

Expected: FAIL because the tree builder and new DTO fields do not exist.

- [ ] **Step 3: Add DTO fields and pure tree builder**

Extend `CoreMasterRecord` with `parentBusinessUnitId`, `ancestorPath`, `depth`, and `descendantCount`. Export a pure `buildOperatingTree` helper that sorts siblings by code and places malformed orphan records in a visible “未归入树” group instead of dropping them.

- [ ] **Step 4: Implement the two-pane master-data experience**

Replace the linear relationship spine with parallel legal and operating sections. Render the operating tree with expand/collapse and full paths. The create/edit modal for an operating unit contains a parent selector and no legal-entity selector. Remove the handler that clears `businessUnitId` when `legalEntityId` changes. Keep legal and unit filters independent in business forms.

- [ ] **Step 5: Run frontend validation**

Run:

```bash
cd apps/business-web
node --test src/OperatingUnitTree.test.mjs
pnpm exec tsc --noEmit
pnpm exec biome check src/api.ts src/CoreMasterDataCenter.tsx src/OperatingUnitTree.test.mjs
pnpm build
```

Expected: PASS.

- [ ] **Step 6: Commit the operating-tree UI**

```bash
git add apps/business-web/src/api.ts apps/business-web/src/CoreMasterDataCenter.tsx apps/business-web/src/core-master-data.css apps/business-web/src/OperatingUnitTree.test.mjs
git commit -s -m "feat(bizos): manage the operating organization as a tree"
```

### Task 7: Run migration and real workflow acceptance

**Files:**
- Modify: `docs/superpowers/specs/2026-09-22-independent-legal-entity-operating-unit-tree-design.md`
- Create: `docs/superpowers/reports/2026-09-23-operating-unit-tree-acceptance.md`

**Interfaces:**
- Produces: reproducible acceptance evidence and an explicit decision on when the later contract migration may remove `business_units.legal_entity_id`.
- Consumes: all earlier tasks.

- [ ] **Step 1: Run formatting, lint and targeted suites**

Run:

```bash
. ./bin/activate-hermit
cargo fmt --all -- --check
cargo clippy -p business-core --all-targets -- -D warnings
cargo test -p business-core --test postgres_operating_units -- --nocapture
cargo test -p business-core --test postgres_b1 --test postgres_b2 --test postgres_b3 --test postgres_b4 --test postgres_crm -- --nocapture
cd apps/business-web && pnpm exec tsc --noEmit && pnpm test && pnpm build
```

Expected: PASS. Record any repository-wide failures that are unrelated and already present; do not label the feature complete until its affected suites pass.

- [ ] **Step 2: Audit remaining runtime dependencies on the compatibility column**

Run: `rg -n 'business_units[^\n]*legal_entity_id|u\.legal_entity_id|bu\.legal_entity_id' services apps examples`

Expected: only migration history, compatibility migration text, and explicitly documented warehouse/legal-ownership logic remain. Any production query that pairs an operating unit with a legal entity blocks release.

- [ ] **Step 3: Exercise the real Business Dock workflow**

In the deployed test environment, create or use a four-level operating tree, create one draft using a legal entity and operating unit that had no old pairing, and verify the saved detail and subtree report. Do not confirm, ship, receive, invoice, settle, or post the draft. Record Trace IDs and read-only result IDs, then remove only the test draft through the product's normal reversible cleanup if authorized.

- [ ] **Step 4: Record migration evidence and contract-column decision**

Document row counts and aggregate amounts before/after, unchanged historical IDs, tree invariants, permission results, UI workflow, and whether production search found any old-column dependency. Mark the later drop-column migration as allowed only when all checks are clean; do not include that destructive migration in this release.

- [ ] **Step 5: Commit acceptance evidence**

```bash
git add docs/superpowers/specs/2026-09-22-independent-legal-entity-operating-unit-tree-design.md docs/superpowers/reports/2026-09-23-operating-unit-tree-acceptance.md
git commit -s -m "docs(bizos): record operating unit tree acceptance"
```

