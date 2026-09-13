# Huddle automatic archive metadata

Production channel 74839142-dad1-49a3-bebf-7790dc4e25ca was archived in the channel table at 2026-09-07 14:01:45 UTC. Its only kind:39000 metadata remains the 14:01:17 UTC creation event, without an archived tag. This explains why clients still show it and the relay rejects writes.

AppShell already excludes archived channels. No sidebar filtering change is needed. The normal archive/unarchive mock UI workflows passed (2 tests). The audio last-peer cleanup path updates the database but omitted emit_group_discovery_events. It now publishes canonical discovery after successful auto-archive. cargo check -p buzz-relay and cargo fmt passed.

Pending: deploy the Relay change, repair historical archived-channel metadata using canonical relay-signed events, and verify the actual sidebar after metadata refresh. No production channel was unarchived or deleted. The existing roster-only admin repair is not appropriate for metadata repair.
