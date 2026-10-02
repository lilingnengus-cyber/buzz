# Inventory ledger

`inventory_movements` is the append-only authority for quantity and value.
Opening, shipment issue and explicit reversal types retain source IDs, cost
snapshots, actor and trace. `inventory_balances` stores on-hand, reserved,
moving-average cost and value as a transactionally maintained projection.
Database triggers reject updates and deletes of movement facts.

## Inventory summary read model (2026-10-03)

Balances expose the SKU product's base `unitOfMeasureId` and `unitName`. Purchasing validates this unit against the product base unit; the read model does not invent unit conversions. The browser groups quantities by unit ID and amounts by currency, using the currently filtered, loaded balances. Loading does not display fabricated zero totals.

`currency` is populated only when the complete movement history for the legal-entity/warehouse/SKU key has exactly one distinct currency. `currencyConflict` identifies multiple historic currencies. No movement currency means unknown; neither case falls back to legal-entity functional currency. Historic postings currently allow different currencies into one balance, so a valuation cannot safely be labeled with the most recent movement's currency. Ambiguous balances show a reconciliation notice, are excluded explicitly from monetary aggregation, and their unit-cost/value cells are marked for currency review. This does not recalculate, repair, convert, or rewrite historical facts.

The local PostgreSQL B2 test now verifies single-currency metadata and a deliberately mixed CNY/USD posting scenario. The B2 full closure/concurrency suite and B3 receiving/payment/reversal/concurrency suite pass on separate isolated databases. A browser test verifies distinct piece/kg quantities, CNY/USD totals, conflict exclusion, and filtering. All 85 browser tests pass, including three inventory zoom snapshots checked after the unit/currency labels were introduced. Return workflows lack dedicated integration coverage and remain a separate open audit item.

## Release evidence

2026-10-03: backend source be3dafcc4, frontend source 2810ee31d. Core image `shiyue-business-core:inventory-metadata-20261003` resolved to `sha256:3d1f958af964cd1464d2b4d0cd8b117f8f9c4400392b6f77545f32cb35ecf347`. Migration preflight: database/release head 80, pending 0. Release evidence: `/opt/business-platform/releases/inventory-metadata-20261003/release-evidence/release.sbEIikHl`. Core health returned status ok after replacement.

Frontend pointer is `business-web-2810ee31d`; public HTML references `index-DMZBkE6X.js` and `index-DdiLmMSU.css`. Public JS is byte-identical to the tested local build. Previous frontend is `business-web-90b9a83fa`; previous Core image is `sha256:bde9d6c16fd111aa89fa588122d54420d98dc4b00e4f785a0bab5d9d04ddc0ff`. No migrations or production business writes were performed.

Post-release native Pacioli verification was attempted but could not run because the Mac was locked. It remains pending; browser tests and public asset checks do not replace that check. The overall workflow audit remains active, including dedicated return transitions and the currency semantics of aging/count readers.
