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
