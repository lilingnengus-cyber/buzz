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

## Live acceptance completed

On the next execution request, Connect again restored the Life Dock session;
the expired-session notice disappeared and Sign out became available. A new
preview-only request was sent for action `cmtobzdf0000jwmmt0e8k4rak`.

- Topic: `5c1345dce010795e8850094b8c9ed2bcb9c630bac8cacee2b7212e0fd0bee7bf`.
- Preview command: `e246b8a5-5d02-4a0c-a122-60761fb19e72`, version 1.
- The visible prompt named the action and asked for `确认删除`, with no long
  command to copy. Gateway persistence recorded the published preview.
- The user independently sent `确认删除` in that Life Proxy topic at 00:37
  Asia/Shanghai. The assistant did not send that confirmation.
- At 00:38, Life Proxy published `execute_confirmed_life_write succeeded` and
  reported a first execution. Execution trace:
  `32a9adef-e031-491d-9d15-ee7b1c7afd5e`; audit:
  `414730f0-6f78-4b1d-b435-cd9d46ec5a1f`.
- Read-only database verification found zero remaining rows for the target
  action. Both the gateway confirmation and LifeOS command were consumed;
  LifeOS recorded consumption at `2026-09-05 16:38:17.545 UTC`.

The previously pending login and live workflow acceptance are now resolved.
