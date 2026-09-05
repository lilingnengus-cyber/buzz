# Life Workbench Operations Runbook

The Life integration is an isolated, default-off security domain. Pacioli never receives a LifeOS database credential. LifeOS never accepts caller-supplied Pacioli pubkeys, communities, or channels: it consumes short-lived, single-use target-selection tickets issued by the Gateway.

## Dependencies and health

Start PostgreSQL for the Gateway, LifeOS and its PostgreSQL, the Pacioli relay, the Gateway, ACP, and finally `life-notifier`. Keep every `LIFE_*_ENABLED` switch false during dependency checks. Verify Gateway `/health/live` and `/health/ready`, LifeOS health, and relay NIP-42 authentication before enabling a child capability.

Scrape Gateway and Notifier only on their private listeners (`127.0.0.1:9103` and `127.0.0.1:9104` by default). Alert on delegation decision failures, active-delegation drift, notifier delivery failures, and dead-letter growth. Never publish these listeners through the user-facing ingress.

Secrets must be distinct between Pacioli→Gateway, Gateway→LifeOS, MCP→Gateway, and Notifier→LifeOS. The notifier uses a dedicated Nostr identity. Rotate one boundary at a time, restart only its two peers, and verify a denied request with the retired credential. Never place values in tickets, logs, screenshots, or this runbook.

## Automated live default-value acceptance

Before enabling Agent writes, run the opt-in real-process acceptance from the
Pacioli checkout:

```bash
just life-workbench-live-defaults-e2e
```

The test builds the current relay, CLI, ACP, Gateway, test client, and Life MCP binaries;
creates three uniquely named PostgreSQL databases; starts isolated loopback
services; submits a typing event and a signed formal DM; and verifies that the
formal message creates exactly one IAM decision, delegation, and MCP call. It
also proves that an omitted project `color` materializes as `#197b70`, and that
the structured Pacioli reply carries the matching `life://` reference, Audit
ID, and Trace ID. Typing must not create a Life IAM decision or delegation.

The test refuses a non-empty Redis database and cleans all isolated state on
exit. It is intentionally not part of ordinary CI because it invokes a real
configured ACP model. Use `LIFE_E2E_KEEP=1` only while diagnosing a failed run;
the command then preserves its disposable databases and log directory.

## Safe rollout order

Enable in this order, with `LIFE_INTEGRATION_CONTRACT_VERSION=1` on both systems:

1. `LIFE_EXTENSION_ENABLED`
2. `LIFE_AGENT_ALLOWED_AGENT_IDS` with only the dedicated Life Proxy pubkey, then
   `LIFE_AGENT_READ_ENABLED` for its 1:1 DM. Other managed Agents must not be
   listed, even when they inherit the global Life service configuration.
3. `LIFE_AGENT_WRITE_ENABLED` for low/medium-risk previewed writes
4. `LIFE_CHAT_HIGH_RISK_WRITE_ENABLED` for exact signed confirmation only
5. `LIFE_DOCK_ENABLED`
6. `LIFE_NOTIFIER_ENABLED` for encrypted DM
7. One short-lived, allowlisted channel disclosure policy

Observe for at least one normal user cycle at each step. Stop on any cross-workspace result, plaintext DM, missing trace edge, duplicate delivery, unexplained authorization increase, or audit write failure.

## Revocation and isolation

- Revoke a Life identity binding to invalidate active delegations and future target selection.
- Revoke a Pacioli target binding in LifeOS to revoke its disclosure policies and dead-letter pending notifications.
- A channel-policy expiry or revocation must deny both new delegation issuance and later delegation consumption.
- Gateway, LifeOS, Dock, and Notifier outages fail closed independently. An unavailable Notifier leaves committed outbox rows pending and does not roll back LifeOS domain transactions.
- Messages carrying `source=life-notifier` are context only and do not recursively start a Life Agent turn.
- The Agent allowlist is enforced before Life tool injection and again before
  delegation issuance. Removing a pubkey takes effect when that Agent runtime
  restarts; revoke any already-issued delegation during urgent removal.

## Dead letters

The LifeOS settings panel exposes only category, `life://` reference, attempt count, fixed error code, and trace ID. It never deletes dead letters automatically. Before choosing **确认并重试**, restore the dependency and verify the target binding and any channel policy. Replay revalidates both and retains the original business idempotency key.

## Rollback

Disable the newest child switch first. Disabling notifier stops claims and preserves outbox rows. Disabling write leaves read available. Disabling the extension stops new Agent/Dock access; revoke active delegations in the Gateway before declaring rollback complete. Do not delete audits, outbox rows, identity bindings, or migration history.

Validate ordinary Pacioli conversation, Business Workbench/Dock, and Hermes read/write after all Life switches are cleared. Their behavior must remain unchanged.
