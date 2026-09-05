# Life action deletion: text confirmation

The user requested a message before deletion and execution after a confirmation
message, without adding buttons. An action preview now names the action, explains
permanent deletion (including permitted cascading child records), and asks for
`确认删除`. The existing long confirmation remains compatible for other operations.

The harness records preview command/version/hash/expiry only after the signed
reply is accepted by the relay. The gateway links that reply to the preview
delegation, author, agent, community, channel and topic. The original signed short
message is preserved throughout validation and delegation issuance. Only the
single `write_command:execute` capability is issued, with the existing bound
command context; the MCP execution tool continues to take no arguments.

A new preview supersedes older previews in its topic. A confirmation signed
before or in the same second as the replacement cannot authorize it. Cross-topic
replies are rejected; a top-level confirmation requires exactly one active
preview in the DM. Multiple topics, expired previews, invalid signatures, and
already-used confirmations fail closed. Gateway persistence survives agent
restarts. LifeOS continues to enforce resource version, hash, expiration and
one-time execution at the actual mutation boundary.

Validation:

- 907 tests passed across buzz-acp and life-auth-gateway, with PostgreSQL enabled.
- The final replacement-preview changes passed all five confirmation integration
  tests, including concurrent duplicate confirmations, expiry, wrong routing,
  early consent and replacement invalidation.
- Clippy with warnings denied, Rust formatting and the repository file-size gate
  passed.
- No real action was deleted during implementation. Production rollout and a
  fresh user-confirmed workflow are recorded separately when completed.

Failure behavior: if relay publishing or preview registration fails, no short
confirmation authority is available. The user must request a fresh preview.

## Rollout

- Production gateway updated from `e2f982a5e` to image `a66eaa0db`; the previous
  compose file is preserved at `/opt/life-auth/compose.before-short-delete-a66eaa0db.yml`.
- The local release build of `buzz-acp` was installed at the configured Life Proxy
  binary path, with a timestamped backup of the prior binary.
- Activating the local binary still requires restarting Life Proxy. Computer Use
  reported that the Mac was locked and could not be unlocked automatically; the
  user was asked to unlock it. No agent restart or live deletion was claimed.

## Activation follow-up

After the user requested execution again, the Mac was available. Life Proxy was
restarted through its profile's Restart agent control and returned to Online /
running. The configured binary matches the tested release by SHA-256. The gateway
readiness endpoint returned 204.

Live workflow acceptance remains pending: the Life Dock reports `LifeOS session
expired` and `Workbench OIDC nonce is unavailable.` Clicking Connect again left
the same error. No new confirmation was sent and no deletion was performed in
this activation turn. Restoring the LifeOS login session is the next prerequisite
before generating a fresh preview and obtaining the user's actual confirmation.
