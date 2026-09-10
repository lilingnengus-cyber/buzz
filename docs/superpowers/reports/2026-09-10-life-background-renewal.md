# LifeOS renewal independent of Dock visibility

Production session metadata showed hourly credentials and successful approximately
58-minute renewals until the sequence stopped. The renewal effect required open,
active and bridge-ready Dock UI. Closing or switching the Dock cleared the timer,
although the LifeOS agent still required that Workbench authorization.

Replace the UI-gated timer with an authenticated-session watcher. It renews
90 seconds before expiry even when the panel is hidden, and rechecks the deadline
on focus, online and document-visible events after sleep/network suspension.
One watcher invokes renewal once; expiry changes create the next watcher.
Cleanup removes event handlers and prevents stale timers after logout/unmount.

This preserves the existing gateway, OIDC verification and retry limits.
A wake check attempts existing renewal; expired or revoked credentials that the
gateway will not renew still require login. This change does not extend server
credential lifetimes or grant offline authority.

Validation in the active v0.5.23 checkout: eight session/watcher tests passed,
including hidden-panel timers, coalesced wake events, late timers and logout
cleanup. Biome and TypeScript checks passed. Live cross-hour sleep/renewal has
not been performed in this bounded run.

Installed the active v0.5.23 build on September 10 at 20:02 Asia/Shanghai after
preserving installed agent sidecars and verifying its signature. Backup:
 /Applications/Pacioli.backup-before-background-renew-20260910-200252.app
The application restarted. The prior OIDC credential was expired; the existing
browser sign-in restored LifeOS without password entry. A hidden-panel read-only
query was sent at 20:04 and received a reply. This is immediate-query verification,
not proof of a full hourly background renewal.
