# Operational returns

Sales and purchase returns are independent, append-only business documents. They are
not aliases for reversing an incorrect shipment or goods receipt.

## Sales return

A draft references one confirmed shipment and one or more of its lines. Draft and
confirmed returns reserve the source line's returnable quantity. Cancelling a draft
releases that quantity while retaining its event and audit trail.

Confirmation is atomic: inventory is received at the shipment's frozen unit cost,
the linked operational receivable is reduced by the original proportional sales
amount, and negative revenue/cost projection facts are queued. A return cannot be
confirmed for more than the receivable's open amount; settled value is never
silently rewritten.

Confirmed sales returns enter quarantine. Quarantined quantity remains on hand but
is excluded from sellable availability. Inspection disposes every line exactly once:
accepted quantity is released for sale, while scrap quantity and its frozen return
cost are removed from stock. The inspection record is immutable and retains the
operator, date, note and inventory movement trace.

## Purchase return

A draft references one confirmed goods receipt and one or more of its lines.
Confirmation removes available (unreserved) inventory at the current moving-average
cost and reduces the linked operational payable by the original proportional gross
amount. It fails if stock is unavailable or if the payable has already been settled
beyond the return amount.

After confirmation, the physical return can be marked dispatched with carrier and
tracking evidence, then supplier-acknowledged. These logistics transitions do not
change inventory or payable amounts.

## Operating metrics

The monthly return view reports sales and purchase return rates against same-month
confirmed shipment/receipt amounts. Sales return loss is a management measure:
returned sales amount minus returned product cost plus inspected scrap cost. It is
not a statutory loss or general-ledger balance.

## Boundary

These records govern inventory, operational receivables/payables and management
profit. They do not create journal entries, tax invoices, bank transactions or a
general-ledger posting model.

Confirmed returns are immutable in this release. Corrections use a new compensating
business document rather than editing confirmed quantities or amounts.

## 2026-10-03 workflow audit

A new isolated PostgreSQL regression (`postgres_returns`, enabled with `BUSINESS_CORE_RETURNS_TEST_DATABASE_URL`) reproduced a confirmation/version defect: sales confirmation returned version 2 while the stored return remained at version 1, so an immediate inspection using the returned version failed with VersionConflict. Both sales and purchase confirmation statements omitted the version increment. They now increment the locked document version in the same transaction as inventory and receivable/payable updates. This change has not yet been deployed.

The sales regression exercises opening 20 units at cost 5, shipping 8 at price 100, returning 2, and inspecting 1 accepted plus 1 scrapped. It verifies 14 on-hand / 2 quarantined / value 70 after confirmation, receivable original/open amount 600, and 13 on-hand / zero quarantined / value 65 after inspection. It checks persisted versus returned version, rejected incomplete inspection, confirmation and inspection idempotent replay, rejection of a second inspection, and exactly one scrap movement. The original test failed before the fix and passes after it on a fresh isolated database; all-target Business Core clippy also passes.

Outstanding: independent purchase return/dispatch/acknowledgment coverage, settlement and over-return guards, UI return workflow checks, then release and native verification. No production business data was changed by these tests.

### Purchase return audit follow-up

The new `postgres_purchase_returns` integration test (environment: `BUSINESS_CORE_PURCHASE_RETURNS_TEST_DATABASE_URL`, fresh isolated database required) found another real blocker before the version assertion: purchase return creation passed the supplier ID into the sales/customer authorization helper and returned NotFoundOrForbidden for a correctly scoped purchase operator. Purchase create/confirm/cancel and dispatch/acknowledgment now use the existing purchasing authorization helper. Customer authorization remains on sales paths.

Verified: receiving 10 at 100; a draft reserves 2 units of returnable quantity; a further 9-unit draft is rejected; confirming returns stored/API version 2 and leaves 8 units/value 800 and payable 800; confirmation replay is idempotent; acknowledgment before dispatch is rejected; dispatch and acknowledgment replay do not double-apply; logistics completion leaves inventory/payable unchanged. Settling the remaining payable via real payment/allocation commands then attempting another return fails with PayableAlreadySettled, preserves stock and draft version, and allows draft cancellation to release returnable quantity. Removing supplier scope rejects creation; restoring it allows the workflow without granting any customer scope. All-target clippy passes. This repair remains local pending release.

Still open: sales settled/over-return guards, browser return workflow checks, release and native verification.
