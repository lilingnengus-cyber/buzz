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
