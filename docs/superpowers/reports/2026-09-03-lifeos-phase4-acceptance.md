# LifeOS Phase 4 Acceptance Evidence

Date: 2026-09-03 (Asia/Shanghai)

Scope: LifeOS resource links, trusted ACP resource results, isolated Life Dock,
one-time embedded sessions, browser bridge, CSP/frame boundaries, exact-origin
CORS, and Desktop/LifeOS end-to-end validation.

## Result

Phase 4 is functionally complete. Pacioli can open strict `life://` resources in
an independent Life Dock, follow only verified resource references from the
current trusted agent turn, and create an isolated embedded LifeOS session from
the existing Workbench identity. Business and Life retain separate navigation,
dirty, pin, follow, fullscreen, iframe, and session state.

The final browser exercise found and fixed two timing defects:

- the host now retries `HOST_INIT` with the same nonce until `LIFE_READY`, so a
  hydrated Next.js client cannot miss the one-shot handshake; and
- the Life iframe remains `about:blank` until the gateway returns a validated
  bootstrap URL. This prevents an unauthenticated pre-connection and eliminates
  a race that could issue two embed sessions.

After its first explicit open, the iframe stays mounted while Life and Business
Dock visibility changes, preserving the user's LifeOS state.

## Security boundaries

- Life resource resolution uses a fixed type-to-route inventory with bounded,
  canonical identifiers. Unknown schemes, types, paths, queries, fragments,
  credentials, and cross-origin URLs fail closed.
- Markdown preserves only resolver-valid `life://` links. Automatic navigation
  never parses conversational Markdown; it consumes verified ACP extension
  result tags from a managed agent in the current channel and turn.
- Automatic navigation is blocked by a pinned Dock, dirty state, disabled
  follow mode, or another active workspace security domain.
- Embed sessions bind the Workbench user, workspace, Life user, client ID,
  exact return origin, resource, expiry, and one-time bootstrap code. Bootstrap
  creates a dedicated `HttpOnly`, `Secure`, `SameSite=None` cookie and redirects
  without retaining the code.
- Gateway CORS accepts only configured exact Workbench origins and only the
  required methods and headers. Wildcards and malformed origins are rejected at
  startup.
- Every bridge message is checked for exact origin, iframe `source`, protocol
  version, request ID, and session nonce. Repeated `HOST_INIT` with the same
  nonce is idempotent; a different nonce is rejected.
- LifeOS embed CSP uses a configured `frame-ancestors` allowlist. Development
  permits the script behavior required by Next.js hydration; the production CSP
  test proves `unsafe-eval` is absent.

## End-to-end evidence

Pacioli Desktop passed the two real-browser Life Dock scenarios:

1. bootstrap redemption, bridge readiness, theme sync, trusted-turn automatic
   navigation, ordinary `life://` click, back/forward/home, dirty guard, pin,
   Business/Life switching, and iframe instance preservation;
2. forged wrong-source/wrong-nonce message rejection and exactly one recovery
   session after expiry.

The latest focused run completed `2 passed (4.6s)`. Its two scoped screenshots
were visually distinct:

- `01-life-dock.png`: `8d6c...`
- `02-life-dock-restored.png`: `96c0...`

LifeOS's Playwright parent/attacker harness passed against the real Next.js
development server. It verified CSP headers, delayed hydration plus host retry,
bootstrap auth, resource routing, exact source/nonce enforcement, theme updates,
and rejection of illegal navigation. `npm run build`, `npx tsc --noEmit`, Prisma
generation, and the complete `npm run test:static` suite also passed.

## Service and contract verification

Pacioli passed:

- the repository-wide `just ci` gate, including formatting, Clippy, Desktop and
  Web static checks and production builds, Rust/Tauri tests, all 5,524 Desktop
  tests, and all Mobile tests;
- all PostgreSQL-backed `life-auth-gateway` suites against the isolated
  `life_auth_test_pacioli_phase4` database (including delegation, races, domain
  isolation, embed sessions, IAM, binding, JWT, membership, database security,
  and write confirmation);
- `cargo test -p buzz-acp life_response`;
- Desktop `pnpm test`, TypeScript checking, `pnpm check:px-text`, and
  `pnpm build:e2e`;
- the focused Life Dock Playwright suite after the final session-race fix.

## Repository-wide smoke baseline

A complete local `pnpm test:e2e:smoke` run exercised 1,178 tests and ended with
1,165 passed, 1 skipped, and 12 failures outside the Life Dock spec and changed
implementation paths. To test causality, the same browser, build mode, and 12
selected cases were run from a temporary detached worktree at the personal
repository's exact `origin/main` (`46f4ec928`), which contains no LifeOS phase-4
changes. Seven failures reproduced on that baseline: composer caret formatting,
the background-theme event sampling assertion, persistent-agent draft cleanup,
terminal wheel delivery, and a 438-pixel screenshot delta. The temporary
worktree was removed afterward.

The focused Life Dock suite is green, and unopened Life Dock instances no longer
load an iframe. The remaining repository-wide smoke failures are therefore
recorded as existing nondeterministic baseline failures rather than hidden or
attributed to this integration.

## Exit review

No Critical or Important finding remains in the Phase 4 scope. The final design
does not grant a generic write capability, does not reuse Business Dock session
state, does not trust the current visible channel as authorization, and does not
allow the embedded app or conversational text to select an arbitrary origin or
route.
