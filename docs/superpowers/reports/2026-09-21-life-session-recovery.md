# LifeOS session recovery — 2026-09-21

## Findings

Production Workbench sessions expire with their OIDC credential (about 60 minutes). Normal renewal works, but the desktop discarded its Workbench token on temporary failures, making nonce-free refresh credentials unusable for recovery. Workbench tokens were memory-only. More directly, the native secure-store key validator accepted only `buzz.oidc.user.user:` while LifeAuth uses `buzz.life-workbench.oidc.user.user:`; Life credentials therefore fell back to sessionStorage and disappeared on restart.

## Change

- Keep distinct OS secret-store slots for legacy Workbench OIDC, Life OIDC and the Life recovery session. Validate namespace and exact key before returning a stored value. Namespace-local clearing prevents one OIDC manager from deleting the other one's credentials.
- Persist the Life Workbench session through existing native secret-store commands; no localStorage bearer tokens. Keyring unavailability preserves in-memory operation, but cannot guarantee persistence across restart.
- Preserve credentials on transport/5xx failures, use bounded delayed retries plus online/focus/visibility recovery. Explicit auth rejection clears the saved Workbench session. A new interactive login may replace obsolete recovery proof through the nonce-validated initial login endpoint.
- A dedicated `/v1/workbench/sessions/resume` requires both possession of an active, non-revoked session in the same deployment (expired no more than seven days ago) AND a fresh signed, unexpired OIDC credential matching its issuer and subject. Identity and current memberships are re-resolved. Expired sessions still fail all normal session-authorized APIs and `/renew`; the recovery proof alone authorizes no access.
- Logout cancels in-flight recovery and removes its saved session. The existing Dock logout remains distinct from global OIDC sign-out.

## Validation

Passed: TypeScript, affected Biome checks; 12 focused frontend tests; five Life Dock browser scenarios, including nonce-free renewal, transient 503 retry preserving iframe/dirty form, and logout while renewal is in flight; real PostgreSQL identity/session tests with wrong deployment, expired access, recent recovery proof, revoked/too-old proof and disabled user; JWT signature/issuer/audience/subject/expiry/nonce tests; gateway strict all-target Clippy. Native secure-store namespace/integration tests: 2 passed. Installed runtime acceptance is recorded below. Full repository `just ci` was not run; no PR opened.

## Server release

Built from deployed gateway base `18315cfdf` with only the recovery API/store additions. Image `life-auth-gateway:recovery-20260921`, manifest `sha256:526c3f6fdfc79744b8a03ab6ddae084a245543b54c62cccfabc566a12aa0b476`. Original Compose retained at `/opt/life-auth/compose.before-recovery-20260921.yml`. Readiness passes and the public resume endpoint rejects an invalid session with 401. No migration, business record change or session-expiry manipulation was needed.

## macOS installation

Installed a locally signed 0.5.23-compatible build in `/Applications/Pacioli.app`; executable SHA-256 `e7d612803a3de2a26f42fe01a000b54f584829bf266d2f228fb6779193fa63c8`. Existing agent/CLI/MCP sidecars were preserved. Backup: `/Users/aaronli/Library/Application Support/Pacioli Backups/life-recovery-20260921/Pacioli.app.backup`. Deep strict signature verification passed, the old app quit gracefully and the new executable launched. After a slow initial startup the real app rendered correctly. Life Dock showed `Life OIDC session expired` (expected: the old build never persisted Life credentials). Clicking Sign in opened the correct personal-workbench authentik login form, requiring user authentication because browser SSO had also expired. Installed login/restart acceptance remains pending that user step. At 22:08 China time the server still had no new session after the pre-update 21:32:40 session.

Full native library tests could not compile because of 12 pre-existing errors in unrelated test fixtures (including missing AgentDefinition fields). A dedicated integration test includes the production credential command module with an in-memory store to check namespace isolation independently; both tests passed in the compatible 0.5.23 source tree. This does not substitute for actual OS-keychain restart verification.
