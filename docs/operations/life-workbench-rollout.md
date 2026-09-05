# Life Workbench Rollout Gates

All six capability switches default to false and the parent-child graph is validated at startup:

```text
LIFE_EXTENSION_ENABLED
├── LIFE_AGENT_READ_ENABLED
│   └── LIFE_AGENT_WRITE_ENABLED
│       └── LIFE_CHAT_HIGH_RISK_WRITE_ENABLED
├── LIFE_DOCK_ENABLED
└── LIFE_NOTIFIER_ENABLED
```

Both systems must declare contract version `1`; mismatches are rejected. Production values belong in the deployment secret/config system, not source control.
When Agent reads are enabled, `LIFE_AGENT_ALLOWED_AGENT_IDS` is mandatory and
must contain only the dedicated Life Proxy Nostr pubkey. An unlisted Agent does
not receive the Life MCP server, even if global configuration reaches its
process.

| Stage | Scope | Proceed when | Immediate rollback |
|---|---|---|---|
| Extension | internal users | ordinary and Business conversations unchanged | disable extension; revoke active Life delegations |
| DM read | one 1:1 DM | 100% identity match; no cross-user/workspace result | disable read |
| Write | low/medium risk | preview/version conflicts and idempotency pass; no unknown outcomes | disable write |
| Exact write | one internal user | one signed confirmation consumes once on one device | disable high-risk write |
| Dock | internal desktop | origin, bootstrap, revoke, timeout checks pass | disable Dock |
| Notifier DM | one bound identity | encrypted delivery, stable dedup, ack/retry pass | disable notifier; preserve outbox |
| Channel | one channel/policy | summary-only output; expiry/revoke deny immediately | revoke policy |

At every stage alert on any sensitive body in logs, unknown error text, user/resource/workspace metric labels, rising consume conflicts, dead-letter growth, or missing trace correlation. A security invariant breach has a zero tolerance threshold.
