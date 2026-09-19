# Operations

## Production

Run all Gateway migrations through `0032_business_document_chat_approvals.sql`, provision a minimum 32-byte
service credential from the server secret store, deploy the Gateway and
`business-read-mcp`, then configure the dedicated `buzz-acp` process:

```text
BUSINESS_AGENT_READ_ENABLED=true
BUSINESS_AGENT_DRAFT_WRITE_ENABLED=false # independent kill switch; enable only after scoped IAM grants
BUSINESS_CHAT_APPROVAL_ENABLED=false # enable only on the sales/purchase approval canary
BUZZ_ACP_AGENT_COMMAND=buzz-agent # recommended; other ACP runtimes are supported
# When using buzz-agent, configure exactly one model provider, for example:
BUZZ_AGENT_PROVIDER=openai
OPENAI_COMPAT_API_KEY=<secret-store reference>
OPENAI_COMPAT_MODEL=<approved model id>
BUSINESS_AUTH_GATEWAY_BASE_URL=https://business-auth.example.com/
BUSINESS_READ_API_BASE_URL=https://business-api.example.com/
BUSINESS_READ_MCP_COMMAND=/opt/buzz/bin/business-read-mcp
BUSINESS_READ_ADAPTER=production
BUSINESS_READ_SERVICE_AUTH_MODE=shared_secret
BUSINESS_READ_SERVICE_AUDIENCE=business-read-api
BUSINESS_READ_SERVICE_CREDENTIAL=<secret-store reference>
BUSINESS_AUTHORIZATION_ENABLED=true
BUSINESS_ANOMALY_ENABLED=true
BUSINESS_ANOMALY_RULESET_PATH=/etc/buzz/trade-risk-v1.0.json
BUSINESS_ANOMALY_DEFAULT_RULESET_VERSION=trade-risk-v1.0
BUSINESS_DATA_STALE_AFTER_MINUTES=1440
BUSINESS_ANOMALY_MAX_FINDINGS=100
BUSINESS_ANOMALY_MAX_PAYLOAD_BYTES=131072
BUSINESS_ANOMALY_SCHEDULE_ENABLED=false
BUSINESS_ACTION_ENABLED=false # production Action adapter remains blocked
AGENT_DELEGATION_TTL_SECONDS=300
AGENT_DELEGATION_MAX_CALLS=20
AGENT_TURN_TIMEOUT_SECONDS=120
BUSINESS_TOOL_TIMEOUT_SECONDS=10
BUSINESS_TOOL_MAX_PAYLOAD_BYTES=131072
BUSINESS_AGENT_RATE_LIMIT_PER_MINUTE=10
BUZZ_ACP_HEARTBEAT_INTERVAL=0
```

Do not set `BUZZ_ACP_MCP_COMMAND`; dedicated mode drops ordinary MCP servers
anyway. Use `examples/business-query-agent` for ordinary lookups and the
separate `examples/business-anomaly-agent` Persona for broad analysis. Do not
give either runtime Shell, filesystem, browser, SQL, generic HTTP, or memory
tools. Codex, Goose, Claude and other ACP runtimes are supported. Dedicated mode
still removes ordinary configured MCP servers and injects only the per-turn
Business MCP server, but a general-purpose runtime may independently expose its
own built-in tools; operators must evaluate and restrict those capabilities.

Keep `BUSINESS_ACTION_ENABLED=false` until the production Action adapter has
passed its separate execution acceptance. With chat approval disabled,
`BUSINESS_ACTION_API_BASE_URL` is intentionally not required; action-lifecycle
tools fail closed as unavailable and no Business Action execution endpoint is exposed.

For `codex-acp`, select a model that exposes session-scoped MCP tools as direct
tool calls. The verified local acceptance path uses `gpt-5.5`. Do not use
`gpt-5.6-sol` for this path yet: its code-mode tool broker snapshots namespaces
before the per-turn Business MCP server is attached, so the server can report
`ready` while its tools remain unavailable to the model. This is a runtime/model
compatibility constraint, not a reason to switch the dedicated host back to a
hard-coded `buzz-agent` runtime.

The 2026-09-19 installed-client acceptance also found `gpt-5.6-terra` returning
“no available business tools” without a Business tool call. In the same client,
`gpt-5.5` completed `search_sales_orders` against production successfully. Keep
the verified model for this deployment until a newer runtime/model combination
passes the same real query and draft-write acceptance.

The desktop connects the current chat identity to the signed-in Workbench
account through the authenticated identity-binding challenge and native event
signature. It validates the challenge's account, issuer, public key, audience,
and expiry before signing. An existing active binding is reused; a revoked
binding is never restored by token renewal. Resolve revoked bindings through
the account administrator. Do not insert binding rows directly to bypass proof
of key ownership. A local macOS rebuild may require the user to allow access
to the existing Keychain item before the desktop can start.

If turn authorization fails, the Business extension publishes a fixed,
actionable failure response without exposing raw gateway errors or credentials.
An online presence alone does not prove that an authorized query completed.

With `BUSINESS_AGENT_READ_ENABLED=false` or missing real integration, ordinary
Buzz agents continue unchanged. Enabling with a missing credential/API URL
fails startup instead of using fixtures.

Deploy code and migration `0025` with `BUSINESS_AGENT_DRAFT_WRITE_ENABLED=false`
at the Agent Host, Gateway and Business Agent API. Grant only the six fixed
`*:create` capabilities to the canary human and Agent principals, then set the
switch to `true` on the canary deployment. Switching it back to `false` stops
new write delegations and makes both MCP invocation and the API write route
fail closed; existing read tools continue normally.

For chat approval, grant `sales_order:approve` and/or
`purchase_order:approve` only to canary Human principals, configure matching
Business Core approval policies, and set `BUSINESS_CHAT_APPROVAL_ENABLED=true`
on the Agent Host, Gateway, and Business Read API. Obtain the exact command from
the matching approval-preview tool. Verify that one vote remains pending, a
second distinct eligible vote executes, duplicate users and event ids are
rejected, a stale version/hash cannot vote, and `/approve` with trailing text
receives no approval scope.

When deploying with `buzz-agent`, run `just business-agent-runtime-acceptance`. The probe uses
the real `buzz-agent -> session/new -> business-read-mcp` path and a loopback
model stub, then asserts that the model sees exactly 30 fixed reads, six fixed
draft creates, two bound approval tools, and no general-purpose tool. It does not call a model, consume a real
Delegation, or read business data.

## Debug fixture

Debug builds may use `BUSINESS_READ_ADAPTER=mock` plus the exact acknowledgement
from `.env.example`. Every result stays `partial`, names the mock source and
warns that production is disabled.

Monitor denial/rate/timeout counts and audit continuity. Cleanup runs with the
Gateway sweep. Revoking a binding also revokes associated Delegations.

## Desktop-managed Agent service credential

A managed Agent may set `BUSINESS_READ_SERVICE_CREDENTIAL_FILE` to an absolute
path containing the Business service credential instead of storing the value
in its environment-variable configuration. Configure exactly one of this path
or `BUSINESS_READ_SERVICE_CREDENTIAL`. The host reads a regular file containing
32–4096 bytes (an optional final newline is accepted); Unix files must have no
group/other permission bits (use mode 0600). On Windows, restrict the file ACL to
the service account before use. Do not put Agent identity keys in this file.
The host continues to pass the credential privately to the per-turn MCP process.
This option requires an updated buzz-acp binary; older builds cannot use it.

## 按名称录入基础资料（2026-09）

`search_business_master_data` 提供八类固定只读查找：客户、供应商、SKU、仓库、计量单位、法律主体、业务单元、品牌。输入包含 `resourceType`、可选名称/编码字面子串 `query`、可选 `legalEntityId`、`offset` 和 `limit`（1–100）。Core 先应用当前账号全部数据范围，再过滤名称并分页；API 再按当回合 IAM 授权范围过滤。返回 `summary.nextOffset` 与 `pagination.hasMore`，即使某页被委托范围完全过滤也需检查后续页，不能把分页中的单条结果直接当作唯一匹配。

需要当前账号具备 `business_master_data:read` 的 Core 权限与 IAM 委托权限；新增目录迁移只登记能力，不自动给所有人授权。部署后由管理员使用现有 `business-iam-admin permission-grant` 为需要该功能的主体配置明确的数据范围。查询无法获得权限时不得绕过到 SQL、浏览器或通用 HTTP。

助手应从查询结果获取内部 ID，核对用户指定的名称/编码，遇到多个候选、缺少仓库/单位等业务选择时一次性询问。不从旧订单推断价格、数量或日期，不默认选择第一条候选；完整确认业务字段后才创建草稿。该功能不开放通用更新、删除、付款或记账。
