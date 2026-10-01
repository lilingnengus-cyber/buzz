# Order draft deletion

Sales and purchase order rows offer Delete only while the order is a draft.
The confirmation dialog identifies the order number; Cancel sends no command.
Confirmed orders use the existing cancel-remaining operation and retain their
fulfillment, inventory and settlement history.

`POST /api/v1/{sales,purchase}-orders/{id}/delete-draft` accepts
`expectedVersion` and an `Idempotency-Key`. The command requires the existing
`sales_order:cancel` or `purchase_order:cancel_remaining` permission and the
actor's current legal entity, partner, brand and operating unit scopes.
It locks the order, verifies the expected version and draft status, marks its
lifecycle cancelled, and appends a `draft_deleted` order event, audit event and
outbox event in the same transaction. This event is the deletion tombstone;
ordinary cancelled orders are not deleted. Idempotency hashes include the order
ID so a key cannot accidentally replay a different order's deletion.

Operational order lists, direct order reads and dashboard/trend order totals
exclude tombstones. Order rows, lines, numbers and immutable event history stay
in the database. No inventory, receivable or payable facts are changed, and
numbers are never recycled. A stale command, non-draft order or revoked scope
is rejected without a deletion event.

Validation: `postgres_b2` and `postgres_b3` exercise authorization, versions,
idempotent replay, cross-order key reuse, tombstones, preserved records and
unchanged inventory movements. `order-deletion.functional.spec.ts` checks both
confirmation dialogs, cancellation, error handling and list refresh on success.
