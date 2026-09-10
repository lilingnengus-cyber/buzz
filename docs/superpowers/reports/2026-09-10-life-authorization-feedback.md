# LifeOS authorization failure feedback

A rejected begin_turn returned an ACP Protocol error before response capture was
installed. The harness consequently emitted only an activity error and respawned
the healthy model process. A logged-in retry on September 10 returned the two
current focus actions, confirming the normal path is functional.

Add a product-owned, static begin-error message hook. The harness publishes that
message as a signed reply to the original source/thread, then returns an
application error without invoking the model/tools or respawning the runtime.
LifeOS distinguishes 401/403, gateway unavailability and quota; unknown failures
use generic safe guidance and duplicate source events produce no extra reply.
No raw gateway payload, authorization credential, or LifeOS result claim appears
in this notice. The publisher uses the existing Nostr /events bridge and a bounded
timeout; no new HTTP surface or relaxed authorization is introduced.

Tests cover fixed error wording, no raw error leakage, duplicate suppression,
signed publication and original-thread association with a local mock relay.
Existing LifeOS authorization regressions and clippy are also run.
The patch is applied to both this worktree and the active v0.5.23 checkout to
avoid replacing the current installed runtime with an older implementation.
Live verification does not invalidate the user's session to force a denial.
