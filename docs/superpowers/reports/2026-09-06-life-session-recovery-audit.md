# Life session recovery audit

The live Dock displayed `LifeOS session expired` / `Dock session expired`.
Its first Connect again attempt reported `Workbench OIDC nonce is unavailable.`;
a subsequent attempt restored the native dashboard. No credential contents were
collected and no business data was changed.

The existing client renews the Workbench session 90 seconds before expiry,
checks Dock authentication every minute while open, and permits one automatic
recovery attempt per expiry. A missing nonce cannot be repaired by that automatic
attempt; interactive OIDC login is required.

Code inspection identified a recovery race: pending interactive resume was a
boolean gated by a generic authenticated phase and a mutable startup lock.
A generic phase cannot distinguish credentials read before the new callback;
clearing only the mutable lock does not itself trigger another React effect.

The correction tracks successful interactive callback revisions and resumes
only after a newer callback. Startup completion now also updates React state,
so a callback that arrives while startup is locked is reconsidered after unlock.
Nonce validation, authorization and the bounded automatic recovery policy remain
intact. This hardens the identified race; it does not prove that race caused the
observed production failure.

Validation: affected Biome checks, TypeScript and E2E build passed. All three
Life Dock smoke tests passed, including near-expiry renewal without iframe
reload or lost dirty state, and one recovery attempt per expiry. These fixtures
do not exercise the real identity provider's missing-nonce interactive callback.

Release status: source fix only. The running installed desktop app has not been
replaced. Remaining validation is a production-configured desktop build and a
real reconnect check; the live session is currently restored.

## Installed desktop acceptance

The production-configured desktop release build completed successfully (8m51s)
with the existing mesh-llm feature, followed by local signing verification.
Installed at `/Applications/Pacioli.app`; the prior app is preserved at
`/Applications/Pacioli.backup-before-session-recovery-20260906-181446.app`.
The current installed buzz-acp and life-workbench-mcp binaries were retained in
the replacement bundle to avoid reverting earlier agent fixes.

After restart, opening the Dock displayed `Life OIDC session expired.`. One
click on `Sign in to LifeOS` restored the authenticated Dock and loaded the
native `/dashboard` page. No second click was needed, and the authenticated
Sign out control became available. No business data was changed.

This establishes successful one-click recovery after installation. It is not a
long-duration production soak test and did not reproduce a missing-nonce token
on demand. The automated near-expiry/dirty-state coverage remains as above.

## Real expiry follow-up (19:16 Asia/Shanghai)

The scheduled read-only check found no new Dock session since the baseline
`73825065-5e96-4725-9958-636ab2036ff1`. Its expiresAt remained
2026-09-06T11:15:32Z; lastSeenAt was 2026-09-06T11:13:09.911Z, before the
expected 11:14:02Z renewal. Its stored ACTIVE status does not override expiry.

The Mac was locked, and Computer Use could not inspect the running page.
No login, refresh, reconnection or session mutation was attempted. Locking was
observed; whether sleep or another condition interrupted renewal is unknown.
Consequently this run did not establish successful automatic renewal and cannot
isolate its cause. The one-time follow-up automation was paused after reporting.
Next step: unlock the Mac and inspect the client/authentication failure without
first manually reconnecting, to preserve diagnostic evidence.

## Unlocked failure inspection

After the user unlocked the Mac, a read-only UI inspection showed the native
`/dashboard` still mounted, with disabled Sign out and the exact error
`Workbench OIDC nonce is unavailable.` plus Connect again. No reconnect or
refresh was clicked. Thus an actual automatic recovery failure is confirmed;
locking alone is not an adequate explanation for this observed error.

The current client clears the Workbench session token before scheduled renewal,
then gets access and ID tokens and requires a nonce to create a fresh Workbench
session. `getValidWorkbenchUser` uses `signinSilent` near OIDC expiry. The pinned
OIDC library's refresh response validation permits an ID token without nonce;
therefore successful token refresh does not itself establish that a subsequent
nonce-required session creation can work. No raw tokens were read, so the exact
refresh response in this incident remains unverified.

The installed callback-revision fix addresses interactive login completion,
not this separate automatic credential/session renewal boundary. Next work:
implement and test a supported session renewal path for refreshed credentials
without removing nonce checks from initial login or reusing an expired token.

## Refreshed-credential renewal correction

Added a separate `/v1/workbench/sessions/renew` path. It requires an active,
unexpired Workbench session in the configured deployment plus a fresh RS256
OIDC credential with the configured issuer/audience, valid expiry and the same
issuer/subject as that session. Identity resolution is repeated before creating
the replacement session. Initial session creation still requires the login nonce.
No expired credential or nonce is fabricated or retained to bypass validation.

The desktop retains the current session through scheduled renewal and submits
it alongside refreshed credentials to this endpoint. It replaces the token only
after a successful renewal response and keeps the existing iframe renewal path.
An already expired session still requires normal interactive recovery.

Validation passed: JWT verification (including missing nonce on refresh versus
initial login, wrong identity and expired credentials); two real PostgreSQL
identity/session tests (including invalid, revoked, expired and wrong-deployment
session rejection); all three Life Dock smoke tests. The near-expiry fixture
now replaces OIDC credentials with a nonce-free token and checks the renewal
request carries the previous session; it preserves iframe instance, selected
action and dirty state. Gateway all-target Clippy with warnings denied passed.
The temporary local test database was stopped after verification.

Release status: this renewal correction is committed source, not yet deployed.
Publish the gateway endpoint before installing the desktop that calls it, then
perform one normal login to establish a valid baseline for a real expiry cycle.

## Renewal correction deployed

Gateway image `life-auth-gateway:18315cfdf` was built from the committed source
and deployed. Previous compose preserved at
`/opt/life-auth/compose.before-renewal-18315cfdf.yml`. Readiness returned 204;
an invalid-session request to the new renewal endpoint returned 401.

The production-configured desktop build completed (3m40s), was locally signed
and installed at `/Applications/Pacioli.app`. Previous bundle preserved at
`/Applications/Pacioli.backup-before-bound-renewal-20260906-233640.app`.
Current agent/MCP binaries were retained. One normal login restored native
`/dashboard`; this is startup validation, not yet automatic-renewal acceptance.

New baseline Dock session: `bc4655fc-f791-49b7-a69f-4bb8758c72c1`, created
2026-09-06T15:37:40.005Z, expires 2026-09-06T16:37:27Z. Expected renewal begins
16:35:57Z. The existing one-time follow-up was reactivated for 2026-09-07 00:38
Asia/Shanghai, with instructions to stop after reporting and not manipulate
sessions or reconnect during observation.

## Real expiry follow-up after renewal rollout

At the scheduled 2026-09-07 00:39 Asia/Shanghai check, read-only production
records showed a replacement Dock session created at
2026-09-06T16:35:58.912Z, about two seconds after the expected renewal trigger
and before baseline expiry at 16:37:27Z:

- New session: `55309271-2018-4774-bb5e-f81e6ca8f5c9`, ACTIVE,
  expiresAt 2026-09-06T17:35:31Z, lastSeenAt 2026-09-06T16:38:52.714Z.
- Baseline `bc4655fc-f791-49b7-a69f-4bb8758c72c1`: REVOKED,
  lastSeenAt 2026-09-06T16:35:58.873Z.

This establishes timely replacement and continued server session activity after
the original expiry, consistent with automatic renewal. No matching renewal
route lines were present in the gateway container logs for the inspected window;
therefore those logs do not independently establish the request's trigger.
The Mac was locked: even listing running apps failed, so the client page could
not be inspected and absence of manual intervention could not be established.
Full automatic-renewal acceptance remains unconfirmed; do not claim an end-to-end
pass from session records alone. No login, refresh, reconnect or business write
was performed by this check. The one-time automation was paused.

Next step: unlock the Mac and inspect the current native page and auth state
without clicking reconnect, then distinguish confirmed server renewal from
remaining client continuity evidence.
