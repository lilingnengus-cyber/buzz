# Preserve clock times on Life action creation

The reported trace `cb709e53-6803-4894-871b-a57436bd64f4` failed locally in the
MCP input compiler before consuming write authority. The actual tool invocation
contained the correct `HIGH` priority, `estimateMin: 30`, and
`dueDate: 2026-09-06T10:00:00+08:00`. The compiler accepted only YYYY-MM-DD;
the LifeOS write schema had the same restriction.

Both boundaries now accept date-only values or RFC3339 instants with an explicit
timezone. LifeOS persists the supplied instant without truncation to midnight.
Creation and update use the same rule; focusDate remains date-only. The agent's
instructions explicitly preserve user-supplied clock times and never invent a
reminder. Existing date-only behavior and idempotency remain unchanged.

Validation: all MCP tests, Clippy, the LifeOS write API script and TypeScript
checks passed. Coverage verifies the original +08:00 value is forwarded with
HIGH and 30 minutes, rejects timezone-free/invalid clock times, retains date-only
inputs, and forbids timestamps in focusDate. Before retry, a read-only production
query found no `财轻松周例会` action in the authorized workspace.

Implementation: Pacioli `7076f5163`; LifeOS `49664c6`.

## Deployment

LifeOS production deploy run `33981819382` completed successfully for
`49664c691ca69b0ead188b1a2af8fbae9ab558ab`; the system status endpoint returned
HTTP 200. Both local release binaries were installed with backups. The existing
Life Proxy persona, now named 助理Agent_LifeOS, restarted successfully: its log
records channel subscription and online presence at 2026-09-05T17:42:35Z.

A second read-only production check found zero matching actions. After the
client became available, the original request was sent once at 01:52 local time.
The harness rejected the turn before tool execution at
2026-09-05T17:52:35.892747Z with `Life Agent turn was not authorized for this
identity`. A subsequent read-only database check still found zero matching rows.
No successful live creation is claimed. Restore the LifeOS session connection
before retrying; then verify one action with priority HIGH, estimateMin 30, and
dueDate 2026-09-06T02:00:00Z (10:00 Asia/Shanghai).
