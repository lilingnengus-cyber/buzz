# life-notifier

Independent, default-off worker that claims minimized LifeOS outbox envelopes and publishes them to Pacioli. It never receives LifeOS database credentials or document bodies.

Required configuration when enabled:

- `LIFE_NOTIFIER_ENABLED=true`
- `LIFE_NOTIFIER_LIFEOS_URL=https://…`
- `LIFE_NOTIFIER_SERVICE_TOKEN=…`
- `LIFE_NOTIFIER_RELAY_URL=wss://…`
- `LIFE_NOTIFIER_COMMUNITY_ID=…` (must match every claimed target; run one worker per community)
- `LIFE_NOTIFIER_METRICS_BIND_ADDR=127.0.0.1:9104` (optional; defaults to loopback)
- `LIFE_NOTIFIER_PRIVATE_KEY=…`

DM notifications use NIP-17 gift wraps. Channel notifications use the existing stream-message kind with an `h` tag. Both carry the stable business idempotency and trace values; the DM values are inside the encrypted rumor. Relay acceptance precedes acknowledgement. Failed publication is retried through the LifeOS lease state machine and never rerouted to another target.

Each worker is pinned to exactly one community and relay. A mismatched envelope is dead-lettered rather than being published to the wrong relay. The dedicated notifier identity must have only the relay permissions needed for the configured DM/channel targets.

Messages tagged `source=life-notifier` are context only. The ACP notification guard will not classify them as an automatic Life Agent turn; an explicit new user-signed reply remains a distinct turn.
