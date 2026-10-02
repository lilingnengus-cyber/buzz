# Business Web

`apps/business-web` is an independent React/Vite client with full navigation
and compact `/embed` routes for orders, shipments, inventory, receivables and
receipts. Requests use an HttpOnly BusinessSession, CSRF, exact same-origin,
idempotency and expected versions. Tokens/service secrets are never stored in
JavaScript. Business Dock receives only validated resource routes and declared
parent origins.

The navigation footer reads `VITE_BUSINESS_ENVIRONMENT_LABEL` when it is set.
Without an override, `business.shiyueshizi.com` is labeled `Production` and
other hosts are labeled `Staging`.

## Visual regression

The deterministic Playwright suite covers the sales, purchasing, and inventory
pages plus their create dialogs at 80%, 100%, and 125% page zoom. It compares
the page, dialog, and collapsed-navigation screenshots and also asserts that
tables stay inside the page, monetary values remain on one unclipped line,
dialogs remain inside the viewport, and the navigation preference survives a
reload.

```bash
cd apps/business-web
pnpm test:visual
```

Use `pnpm test:visual:update` only after reviewing an intentional visual change.

## Production static release

`scripts/release-business-web.sh` builds the current pushed commit, creates a
content-addressed release, verifies every asset hash, and atomically switches
the production static pointer. It retains the prior tree as a rollback pointer
and automatically restores it if the public asset, IAM readiness, or Business
Core health checks fail.

```bash
BUSINESS_WEB_DEPLOY_HOST=ubuntu@business-host \
BUSINESS_WEB_SSH_KEY=/absolute/path/to/ssh-key \
./scripts/release-business-web.sh
```

Use `--dry-run` to build and print the derived release without connecting to
the server. The script requires `HEAD` to exist on the current branch of
`origin`; it never pushes Git refs itself.

## 2026-10-03 business workflow self-check (ongoing)

Scope: sales order entry, goods fulfillment, service fulfillment, purchasing, and inventory. Check navigation and record details, reads and failed reads, monetary semantics, write guards, zoom/layout, and real embedded-client behavior. The goal remains active; this is an incremental audit, not a claim that all business transitions are verified.

Findings corrected in this frontend round:

- Purchasing summed different currencies and kept totals independent of the visible order filter. Amounts now group by currency, orders follow the current filter, reversed payables are excluded, and labels identify loaded records rather than implying unbounded business-wide totals.
- Inventory health used one failing Promise.all to discard all three datasets, then displayed zero counts and an empty register. Each query now settles independently; failed/loading values show an unavailable state, successful datasets remain usable, and retry is available. The monthly turnover label declares its CNY scope.
- Linked sales-order details loaded the same record twice. The existing detailed response now supplies the initial progress snapshot; manual refresh still fetches current facts.
- Workflow stage CSS assumed five stages despite six purchase stages. Stages now wrap by available width. At 125% zoom, narrow order rows put action buttons on their own line so they are not clipped. Tests assert row overflow rather than only the outer page.
- Visual fixtures retained the old sales navigation title, old detail selector, and obsolete indentation threshold from before abbreviation removal. Fixtures now model the current API and navigation; five pages are exercised at 80/100/125%, with stage selection, dialogs, row overflow and navigation persistence checks. Updated screenshots were inspected, including 125% sales/purchase rows, goods/service stages and order forms.

Coverage evidence is recorded at release below. Remaining audit items: inventory balances do not expose currency or unit-of-measure, so cross-SKU and cross-entity headline sums require a source-level currency/unit review. Do not assume legal-entity currency without verifying inventory posting invariants. Receiving, payment and return transitions also need explicit coverage review against their backend test suites; page screenshots alone are insufficient. Production verification remains read-only.
