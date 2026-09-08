# Named action deletion lookup

The user reported that `删除 验证 Pacioli 会话直写 LifeOS` produced a generic
no-verified-result reply. Gateway evidence for source
`21ef048c437992e423e7a1badb7cc8c0251b24779ac0b05be78f5fbfe8734440`
showed a valid delegation with four remaining calls, zero actual tool calls,
and one authorized workspace. The harness discarded the gateway's effective
workspace scope, so the fresh agent had no trusted workspace identifier to use.

The harness now passes a workspace hint to the MCP server only when the gateway
authorizes exactly one workspace. MCP instructions expose that identifier for
lookups without broadening authority. Absent or multiple workspaces never select
an arbitrary default; invalid identifiers fail closed.

The first live retry then made a valid `list_actions` call, but the target was
outside the first 100 results. The existing list tool and LifeOS read endpoint
now accept an optional exact `title` filter, applied within the authorized
workspace/project scope before limiting results. Named deletion uses limit 2 to
detect ambiguity, followed by detail/version lookup and preview. No new tool or
permission was introduced.

Validation: 14 harness Life-agent tests, 27 MCP tests, the LifeOS scoped-read API
script, TypeScript checks and Clippy passed. Tests cover absent/single/multiple
workspace hints, invalid hint injection, literal title forwarding, title length,
and the scoped database filter with a bounded result count.

Implementation: Pacioli commits `57e9af9a3`, `a53d97ffc`; LifeOS `c23e92c`.

Live acceptance succeeded after LifeOS deployment `33979350795` and restarting
Life Proxy with both updated binaries. The unchanged short request
`删除 验证 Pacioli 会话直写 LifeOS` produced a named confirmation prompt without
workspace or resource IDs supplied by the user.

- Topic/source: `0dcfb85a4181b398fbee5128151f2fcf538d599313f86f85f3202db58da2e94c`.
- Gateway calls: workspace action read, action detail read, then version-1 preview.
- Command: `242f2e08-437e-43e7-a78d-c1792933c69d`.
- Trace: `c2c9d3f0-d12a-4f5a-ab29-6f49f3f41a6e`.
- Preview audit: `d86783a2-cd32-4dee-b24f-36a3c45430cf`.
- The visible prompt names the intended action and asks for `确认删除`.
  The assistant did not send confirmation or execute deletion.
