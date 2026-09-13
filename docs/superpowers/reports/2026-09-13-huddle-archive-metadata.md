# Huddle automatic archive metadata

Production channel 74839142-dad1-49a3-bebf-7790dc4e25ca was archived in the channel table at 2026-09-07 14:01:45 UTC. Its only kind:39000 metadata remains the 14:01:17 UTC creation event, without an archived tag. This explains why clients still show it and the relay rejects writes.

AppShell already excludes archived channels. No sidebar filtering change is needed. The normal archive/unarchive mock UI workflows passed (2 tests). The audio last-peer cleanup path updates the database but omitted emit_group_discovery_events. It now publishes canonical discovery after successful auto-archive. cargo check -p buzz-relay and cargo fmt passed.

Pending: deploy the Relay change, repair historical archived-channel metadata using canonical relay-signed events, and verify the actual sidebar after metadata refresh. No production channel was unarchived or deleted. The existing roster-only admin repair is not appropriate for metadata repair.

## Production rollout

Deployed source 5ad13397ca2a7f4bfb6cf22ca10fe28ec1b5dbac from the existing production base d2f5111c7b4729bc8c0caf5a73a5cbf70d659bf9. Image digest: sha256:26ef57a1689da3c015313d7b08b32f7b4a28af701642e3088396d99add8900b0. Build run 34734527228 succeeded. Only relay was recreated; health check passed. Compose backup: /opt/buzz/compose.override.yml.before-huddle-archive-20260913.

The targeted repair command passed its metadata-preservation and wrong-signer rejection unit test. Production dry run verified the original relay signature and archived database row. Applied repair only to 74839142-dad1-49a3-bebf-7790dc4e25ca, producing metadata event 661071b293f315b614b4066c041286ffa21c7ae6d46d7e3c9ade381fa937d792. A subsequent dry run reported already_archived=true; read-only SQL confirmed the latest metadata archived tag and original archive date.

Desktop visual verification is pending because the Mac was locked. Windows visual verification and a fresh auto-ending huddle workflow were not exercised. The earlier pending deployment and historical repair items are now complete; sidebar disappearance still needs a client refresh/visual check.
