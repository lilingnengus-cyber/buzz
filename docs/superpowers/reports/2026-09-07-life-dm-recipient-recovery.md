# LifeOS DM delivery investigation — 2026-09-07

## Production evidence

The 09:21 Asia/Shanghai request to create 侠客宇宙 was stored in the relay
with an `h` tag only. The 09:37 retry in the same DM also carried a `p` tag
for the LifeOS agent and received a verified create_project reply.
The harness uses mention-filtered subscriptions. Reconnect replay uses those
same filters, so reconnecting cannot recover an event missing its recipient.
Connection logs separately show pong timeouts and successful reconnects at
09:26 and 09:31. These do not prove the missing tag was caused by reconnect.

The existing desktop recipient helper relies on cached DM member and participant
arrays. Empty arrays allow signing an unaddressed message. The change resolves
an incomplete DM roster through the existing member API before either HTTP or
WebSocket publication. Failure or a roster without another recipient aborts the
send. Explicit mentions cannot mask missing roster data. Complete DMs and stream
messages keep their existing path. No historical writes are blindly replayed.

The user's original project was retried once after confirming absence. The
resulting project cmtqkmspx000zwmpx5j979pae was verified as the only exact-title
row. The agent mistakenly used 商业系统 as purpose; its domain association was
subsequently corrected through the native project editor. That separate intent
interpretation issue is not fixed by this delivery change.

## Validation

- 23 recipient and send-channel-binding tests passed.
- Desktop TypeScript check passed.
- Changed-file Biome check passed.
- Harness replay-watermark, replay after rate-limit gate, and duplicate-event
  rejection tests passed (3 focused checks).
- New cases cover empty roster recovery, explicit mentions with incomplete
  membership, unavailable/empty member responses, and unchanged complete-DM
  and stream behavior.

The evidence identifies the missing recipient, not the exact cache transition
that caused the original event. Real network fault injection against the user's
production connection is not part of this validation.

## Installed runtime acceptance

The production desktop bundle built successfully and was installed at
/Applications/Pacioli.app with its existing agent sidecars preserved and its
local signature verified. The previous app was retained as a timestamped backup.

At 09:57 Asia/Shanghai, a plain DM read-only request was sent from the restarted
app without an explicit @mention. The relay persisted message
87efdf113ad9ee5d8f849e3ccd74dbe81bc49379fb6efbe4d401a9b437469663 with the expected
agent recipient p tag. At 09:58 the native thread showed the agent's verified
read-only reply, including the project name and domain ID. Trace:
d22d01b4-e40e-4884-a8c4-c3f70715ce22. This confirms installed normal DM delivery;
the incomplete-roster branch is covered by the focused automated tests above.
