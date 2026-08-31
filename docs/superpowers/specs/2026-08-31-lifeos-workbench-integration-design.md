# Pacioli × LifeOS 完整工作台接口设计

状态：设计已确认，尚未进入实现计划或代码实施。

日期：2026-08-31

## 1. 摘要

本设计把独立项目 LifeOS 接入 Pacioli，形成与现有企业工作台平行的“个人工作台”。Pacioli 继续负责签名会话、Agent Host、Dock 宿主、协作和最终答复；LifeOS 继续负责领域模型、Workspace 数据、业务规则和持久化。

目标体验包括：

- 用户在 Pacioli 会话中查询和正式提交 LifeOS 操作；
- 低、中风险操作可由明确的签名消息在同一 Turn 内完成；
- 高风险操作必须经过服务端预览和新的精确签名确认；
- `life://` 链接把会话结果定位到 Life Dock 中的具体资源；
- LifeOS 可通过可靠 Outbox 主动向 Pacioli 投递最小化通知；
- 企业与个人工作台共享通用扩展契约，但身份、IAM、Session、委托、Token、密钥和审计完全隔离；
- 现有 Hermes HTTP MCP 和直接写入模型保持不变。

本设计不把 LifeOS 数据复制到 Pacioli Relay，也不让 Pacioli Core 理解 LifeOS 的权限或数据模型。

## 2. 已选方案与边界

采用“平行安全域 + 共享通用契约”：

```text
Pacioli Core
├─ Relay / signed source event / conversation context
├─ ACP Agent Host
└─ 通用扩展契约
   ├─ TurnExtensionRegistry
   ├─ WorkspaceDockHost
   ├─ ResourceLinkRegistry
   └─ ExtensionResultEnvelope

Business Extension                 Life Extension
├─ Business TurnExtension          ├─ Life TurnExtension
├─ business-auth-gateway           ├─ life-auth-gateway
├─ Business MCP                    ├─ life-workbench-mcp
├─ Business IAM                    ├─ Life IAM
├─ Business Dock                   ├─ Life Dock
├─ biz:// resolver                 ├─ life:// resolver
└─ Business audit                  └─ Life audit
                                      │
                                      ▼
                                  LifeOS API
                                      │
                                      ▼
                              LifeOS PostgreSQL/Redis
```

没有采用以下方案：

- 不使用一个 Gateway 同时处理企业和个人授权，以避免共享故障域和权限串用；
- 不让 Pacioli 通过适配器复用 Hermes HTTP MCP，因为 Hermes 的持久 Workspace Token 和自主管理确认的语义不满足 Turn 级委托；
- 不直接把 LifeOS 原生化成 Pacioli Nostr 事件模型。

## 3. 组件所有权

### 3.1 Pacioli Core

Pacioli Core 只负责：

- 验证和承载 Nostr 事件；
- 提供可信的 source event、author、community、channel/DM 和 Turn 上下文；
- 启动 ACP Session 和扩展生命周期；
- 承载通用 Dock、资源链接和结果渲染接口；
- 发布 Agent 最终答复。

Pacioli Core 不允许包含 LifeOS 工具名、权限名、Workspace 选择逻辑、风险规则或结果解析规则。

### 3.2 Pacioli 通用扩展层

通用扩展层定义稳定插槽，不实现产品策略：

```rust
trait TurnExtension {
    fn id(&self) -> &'static str;
    fn classify_turn(&self, context: &VerifiedTurnContext) -> TurnApplicability;
    async fn prepare(&self, context: &VerifiedTurnContext) -> Result<TurnPolicy>;
    fn mcp_server(&self) -> Option<McpServer>;
    async fn observe(&self, update: &SessionUpdate) -> Result<()>;
    async fn finish(&self, outcome: TurnOutcome) -> Result<()>;
}
```

`VerifiedTurnContext` 只能由 Pacioli Host 构建，至少包含：

```text
source_event
source_event_id
source_pubkey
community_id
conversation_type
channel_id or DM participant set
agent_id
agent_turn_id
trace_id
```

扩展不能接受 prompt 自报的 author、channel、community 或 Turn ID。

### 3.3 Life Extension

Life Extension 拥有：

- Life Turn 分类和双工作台路由；
- Life Delegation 的申请、注入和撤销；
- Life MCP 的固定工具目录；
- `life://` 解析和 Life Dock；
- Life 结果的最小化渲染；
- LifeOS 通知目标和披露策略的适配。

### 3.4 life-auth-gateway

`life-auth-gateway` 是个人安全域中以下行为的唯一入口：

- Workbench OIDC 身份解析；
- LifeOS 用户映射；
- Nostr pubkey 身份绑定；
- Life IAM 决策；
- Agent Delegation 签发、消费、撤销、过期和预算；
- Embed Session 签发；
- WriteCommand 确认验证；
- 安全审计。

它与 `business-auth-gateway` 共享通用契约和安全库，但不共享数据库表、Token Audience、服务凭证、签名密钥或审计账本。

### 3.5 life-workbench-mcp

`life-workbench-mcp` 是每 Turn 启动的 stdio MCP。它只做：

- MCP JSON-RPC；
- 固定工具 schema 校验；
- tool → capability/risk 的版本化映射；
- Delegation 原子消费；
- 调用固定 LifeOS Workbench API；
- 结果和错误清洗。

它不直连数据库、不接受任意 SQL/URL/Prisma 条件、不负责身份绑定，也不自行决定权限。

### 3.6 LifeOS

LifeOS 是个人领域数据事实源。LifeOS API 必须在每次调用时重新检查 Workspace、资源关系、当前主体状态、版本和领域规则。Delegation 只提供临时授权上限，不能替代 LifeOS 的最终鉴权。

## 4. 身份模型

### 4.1 Workbench 用户映射

Pacioli 集成使用与企业工作台同构的 Workbench OIDC 会话。`life-auth-gateway` 维护 Workbench 主体到 LifeOS 用户的映射：

```text
(oidc_issuer, oidc_subject) → active LifeWorkbenchUser → life_os_user_id
```

LifeOS 现有原生登录可以继续存在，但不能被 Pacioli Agent 委托直接使用。Pacioli 集成只信任 Gateway 验证后的 Workbench 身份。

### 4.2 Nostr 身份绑定

绑定关系为：

```text
active buzz_pubkey（Gateway 安全域内唯一）
    → active LifeWorkbenchUser
    → canonical LifeOS user
```

Workspace 不存入 IdentityBinding。Workspace 是 IAM 数据范围，并由 LifeOS 最终资源关系再次验证。

建议接口：

```http
GET /v1/me
POST /v1/identity-bindings/challenges
POST /v1/identity-bindings/{challengeId}/verify
DELETE /v1/identity-bindings/{bindingId}
```

创建 challenge：

```json
{
  "pubkey": "64-char-lower-hex"
}
```

响应：

```json
{
  "challengeId": "uuid",
  "audience": "life-workbench-identity-binding",
  "canonicalPayload": "versioned canonical text",
  "expiresAt": "RFC3339",
  "traceId": "uuid"
}
```

Pacioli 使用目标 pubkey 签署 kind `24243` 事件。Verify 请求提交完整签名事件；Gateway 验证：

- challenge 属于当前 Workbench 用户；
- challenge 有效、未消费且未过期；
- event kind、pubkey、content、ID 和签名完全匹配；
- 目标 pubkey 没有绑定给另一个有效用户；
- replay 和并发消费只能成功一次。

设备字段只可作为历史审计信息，不参与授权。

### 4.3 撤销级联

以下变化必须事务性撤销相关活动委托，并撤销或使相关 Dock Session 失效：

- LifeWorkbenchUser 禁用；
- IdentityBinding 撤销；
- Workbench Session 注销或撤销；
- 独立 Agent Principal 禁用；
- 角色、直接授权或 Workspace membership 变化。

## 5. Agent 主体与授权计算

### 5.1 两种 Agent

独立 Agent 是 Life IAM 中的持久 Principal，只使用自身权限。

代理 Agent 不是持久 IAM Principal。它在每个 Turn 中使用发起人的当前权限，并被请求范围和运行时策略进一步缩小。

Gateway 按与企业工作台相同的规则识别：如果 `agent_id` 对应有效的 `independent_agent` Principal，使用独立 Agent 权限；否则按代理 Agent 处理。

### 5.2 有效权限

代理 Agent：

```text
human authority
∩ requested capability and scope
∩ runtime policy ceiling
∩ resource and Workspace scope
∩ risk obligations
∩ disclosure policy
```

独立 Agent：

```text
independent-agent authority
∩ requested capability and scope
∩ runtime policy ceiling
∩ resource and Workspace scope
∩ risk obligations
∩ disclosure policy
```

独立 Agent 不继承发起人的写权限；代理 Agent 不拥有自己的持久 LifeOS 权限。

### 5.3 能力目录

能力目录是版本化、服务器控制的配置。模型不能发明能力名，也不能改变风险等级。

```text
workspace:read
domain:read | create | update
goal:read | create | update | archive
project:read | create | update | archive
action:read | create | update | status_update | reorder | delete
focus:read | update | replace
calendar:read | create | update | delete | invite
journal:read | create | update | delete
knowledge:read | create | update | delete | export
review:read | create | update
ai_execution:read | start | append_output | finish | policy_update
notification:read | acknowledge
```

每条目录记录至少包含：

```text
capability
allowed_tools
risk_class
requires_expected_version
default_max_calls
max_batch_size
obligations
catalog_version
```

### 5.4 数据范围

授权数据范围使用确定性交集：

```json
{
  "workspace": ["workspace-id"],
  "domain": ["domain-id"],
  "project": ["project-id"],
  "resource": ["opaque-resource-id"],
  "sensitivity": ["normal", "private"],
  "operationCount": ["1"]
}
```

`Unrestricted` 仅表示在当前 LifeOS 安全域中不附加额外维度，不表示跨部署、跨用户或跨事实源无限访问。

### 5.5 授权义务

支持以下义务：

```text
human_confirmation
step_up_authentication
dual_control
dm_only
redact_sensitive
max_batch
```

义务只能增加限制。未满足任一义务时不得签发对应委托。

## 6. Turn Delegation

### 6.1 签发接口

```http
POST /v1/life-agent/delegations
Authorization: Service <pacioli-host-credential>
Content-Type: application/json
```

```json
{
  "sourceEvent": {},
  "sourceChannelId": "uuid-or-null",
  "conversation": {
    "type": "dm",
    "participantPubkeys": ["64-char-lower-hex"]
  },
  "agentId": "stable-agent-id",
  "agentTurnId": "uuid",
  "requestedCapabilities": ["action:status_update"],
  "requestedDataScope": {
    "workspace": ["uuid"],
    "resource": ["opaque-id"]
  },
  "resourceContext": {
    "type": "action",
    "id": "opaque-id",
    "expectedVersion": 7
  },
  "writeCommandId": null,
  "traceId": "uuid"
}
```

`sourceEvent` 必须是完整签名事件。Gateway 验证 ID、签名、作者、kind、时间窗口、source event 唯一性，以及 author 的有效身份绑定。

成功响应只返回一次明文 token：

```json
{
  "delegationId": "uuid",
  "token": "32-byte-base64url",
  "audience": "life-workbench-mcp",
  "effectiveCapabilities": ["action:status_update"],
  "effectiveDataScope": {
    "workspace": ["uuid"],
    "resource": ["opaque-id"]
  },
  "obligations": [],
  "maxCalls": 1,
  "expiresAt": "RFC3339",
  "iamDecisionId": "uuid",
  "traceId": "uuid"
}
```

Token 使用 32 字节密码学随机值和 Base64URL 无填充编码；数据库只保存 SHA-256 hash。默认 TTL 300 秒，最大 900 秒。

### 6.2 委托绑定字段

委托必须绑定：

```text
LifeWorkbenchUser / IdentityBinding
human or independent-agent IAM decision
agent_id / agent_turn_id
source_event_id / source_channel_id / conversation audience
audience = life-workbench-mcp
effective capabilities / data scope / obligations
write command fields when present
max_calls / used_calls / expires_at
trace_id / policy and catalog versions
```

### 6.3 状态机

```text
active → exhausted
   ├──→ revoked
   └──→ expired
```

Turn 完成、失败、取消和超时都同步撤销。Host 的 drop guard 只作异常退出兜底；TTL 是异常上限，不是正常回收机制。

### 6.4 原子消费

```http
POST /v1/life-agent/delegations/consume
Authorization: Bearer <delegation-token>
```

```json
{
  "agentId": "stable-agent-id",
  "agentTurnId": "uuid",
  "tool": "update_action_status",
  "capability": "action:status_update",
  "resource": {
    "type": "action",
    "id": "opaque-id",
    "expectedVersion": 7
  },
  "normalizedInputHash": "sha256:...",
  "idempotencyKey": "uuid",
  "traceId": "uuid"
}
```

一次 PostgreSQL 原子更新同时检查 status、expiry、audience、Agent、Turn、binding/user/principal 状态、capability、data scope、obligations 和 call budget，然后递增 `used_calls`。达到预算的那次调用可以成功并把状态置为 exhausted；之后的调用失败。

Consume 返回一个短时、单调用、由 Gateway Ed25519 签名的 `LifeCallGrant`。其 claims 绑定 delegation、call ID、LifeOS API audience、capability、data scope、resource、expectedVersion、normalizedInputHash、idempotency key、expiry 和 trace。MCP 以独立服务身份把该 Grant 交给 LifeOS API。LifeOS API 验证签名、issuer、audience、expiry、request hash 和 call ID，并防止同一 call ID 被不同 payload 使用。

## 7. MCP 工具与 LifeOS Workbench API

### 7.1 MCP 启动配置

`life-workbench-mcp` 只从子进程环境读取：

```text
LIFE_DELEGATION_TOKEN
LIFE_AUTH_GATEWAY_URL
LIFE_API_URL
LIFE_AGENT_ID
LIFE_AGENT_TURN_ID
LIFE_TRACE_ID
```

Token 不能出现在 prompt、工具参数、工具结果、错误文本或日志中。

### 7.2 固定工具目录

读取工具至少包括：

```text
get_today_context
get_system_overview
list_projects
get_project_context
list_actions
get_action_detail
search_journal
get_review_context
get_weekly_review_context
search_knowledge
get_knowledge_item
get_ai_execution_context
```

正式写工具至少包括：

```text
create_goal
create_project
create_action
update_action
update_action_status
reorder_action_children
set_today_focus
create_journal_entry
create_daily_review
create_project_review
apply_weekly_review
create_knowledge_item
start_ai_execution
append_ai_execution_output
finish_ai_execution
```

高风险操作通过固定 preview 工具产生 WriteCommand，并最终调用零参数：

```text
execute_confirmed_life_write
```

这个零参数工具不是通用 mutation。它只能执行委托中已经绑定、由服务端持久化且经过确认的 WriteCommand；模型不能提供或改变目标和参数。

删除、批量覆盖、外部邀请、敏感导出和 AI 自动执行策略变化不得通过普通写工具绕过 WriteCommand。

### 7.3 Workbench API

Pacioli MCP 只调用 `/api/workbench/*` 固定路由。浏览器 Cookie 和 Hermes Token 不能调用该前缀；只有受信 MCP 服务身份和有效 LifeCallGrant 可以调用。

路由按领域拆分，不提供通用 patch：

```text
/api/workbench/context/*
/api/workbench/projects/*
/api/workbench/actions/*
/api/workbench/focus/*
/api/workbench/calendar/*
/api/workbench/journal/*
/api/workbench/knowledge/*
/api/workbench/reviews/*
/api/workbench/ai-executions/*
/api/workbench/write-commands/*
```

LifeOS API 的最终有效范围为：

```text
requested resource
∩ delegated data scope
∩ current LifeOS user/agent authority
∩ current Workspace membership and resource ownership
```

API 不信任 MCP 自报的 user、Workspace、role 或权限。

### 7.4 幂等和结果

写入幂等键由 `delegationId + tool + normalizedInputHash` 派生。重复请求返回首次结果；相同 key 携带不同 payload 必须拒绝。

统一成功结果：

```json
{
  "ok": true,
  "data": {},
  "resourceRefs": [
    {
      "scheme": "life",
      "type": "action",
      "id": "opaque-id",
      "version": 8,
      "title": "完成接口设计"
    }
  ],
  "auditId": "uuid",
  "traceId": "uuid"
}
```

MCP 结果有大小、条数、文本片段长度和时间范围上限。

## 8. 会话写入和风险模型

### 8.1 风险目录

风险由服务端版本化目录决定：

| 等级 | 典型操作 | 授权方式 |
|---|---|---|
| READ | 查询、搜索、读取上下文 | 当前 Turn 委托 |
| LOW | 追加非敏感备注、创建无外部副作用的草稿 | 明确签名请求，同 Turn 提交 |
| MEDIUM | 创建行动/项目/目标、改单个行动、完成行动、调整今日焦点 | 明确签名请求，同 Turn 提交 |
| HIGH | 删除、批量修改、覆盖性替换、敏感日志、外部日历邀请、AI 自动执行策略变化 | 服务端预览 + 新签名 Turn |
| PROHIBITED | 任意 SQL/URL、绕过审计、导出密钥、跨 Workspace 越权 | 永久拒绝 |

数量、资源类型、敏感字段和外部副作用可以提升风险，不能降低风险。

Gateway 不解析普通自然语言。Agent 负责把用户意图映射为低、中风险工具；Gateway 只允许固定目录中、当前主体有权使用的能力。高风险操作不依赖模型的语义判断。

### 8.2 低、中风险写入

```text
签名用户消息
→ Agent Host 请求具体 capability
→ Gateway/IAM 计算最小权限
→ MCP 固定工具调用
→ LifeOS API 验证 expectedVersion
→ 正式提交
→ Agent 返回服务端摘要、life:// 链接和 Trace ID
```

请求含糊时 Agent 必须澄清，不能猜测资源、日期、金额、状态或 Workspace。

### 8.3 高风险 WriteCommand

LifeOS API 生成不可变 WriteCommand：

```json
{
  "commandId": "uuid",
  "tool": "delete_action",
  "resourceType": "action",
  "resourceId": "opaque-id",
  "expectedVersion": 7,
  "normalizedInputHash": "sha256:...",
  "risk": "HIGH",
  "sideEffectSummary": ["删除行动及其允许级联的子资源"],
  "previewHash": "64-char-lower-hex",
  "expiresAt": "RFC3339",
  "status": "pending"
}
```

Agent 只能原样展示服务端返回的精确命令：

```text
/confirm life-write <command-id> v7 <64-hex-preview-hash>
```

用户的新签名消息必须只包含这一条命令。普通“确认/同意”、引用、附加说明、改写、不同资源或过期命令均无效。

新 Turn 确定性解析命令；Gateway 把 command ID、版本、hash 和 decision 写入一次性委托。`execute_confirmed_life_write` 不接受参数，所有执行字段来自委托上下文。

状态机：

```text
pending → consumed | expired | cancelled
```

默认有效期 10 分钟，只能成功消费一次。

### 8.4 并发和不确定结果

- 更新和删除强制使用 `expectedVersion`；冲突返回 `version_conflict`，不自动覆盖或重新预览；
- 单 Workspace 的一个批量命令默认事务性全成或全败；跨 Workspace 批量操作禁止；
- 写入成功但响应丢失时，用幂等键查询首次结果，不重复执行；
- Agent 遇到超时或不确定结果时必须查询状态，不能擅自宣称成功或失败。

## 9. 双工作台路由

路由优先级：

1. 回复目标或消息中的有效 `biz://` / `life://` 资源引用；
2. 用户明确指定“企业/业务”或“个人/LifeOS”；
3. 当前 Turn 已绑定的资源安全域；
4. 仍有歧义时必须澄清。

当前打开的 Dock、当前频道、模型猜测和资源名称相似度都不构成授权依据。

跨域请求拆成两次独立委托和两条审计链；一边失败不回滚另一边，也不能把部分成功描述为整体成功。

## 10. 个人数据披露

### 10.1 默认 DM-only

Life Agent 工具默认只在绑定用户与目标 Agent 的 1:1 DM 中启用。TurnExtension 必须使用 Relay 验证后的 DM 参与者集合，不能信任事件 tags 或 prompt 自报。

普通多人频道默认只能展示和打开不含数据的 `life://` 链接，不能调用 LifeOS 数据工具。

### 10.2 ChannelDisclosurePolicy

多人频道使用需要 LifeOS 中的显式、限时策略：

```json
{
  "lifeUserId": "uuid",
  "pacioliCommunityId": "uuid",
  "pacioliChannelId": "uuid",
  "allowedCategories": ["action_summary", "project_status"],
  "maxSensitivity": "normal",
  "expiresAt": "RFC3339",
  "status": "active"
}
```

该策略只允许披露指定类别，不授予写权限。日志正文、知识正文、健康、财务和关系等高敏感内容默认不能在多人频道返回。频道是 private、成员少或当前打开都不能自动放宽。

## 11. WorkspaceDockHost 与 Life Dock

### 11.1 通用注册

```ts
type WorkspaceDockExtension = {
  id: "business" | "life";
  title: string;
  scheme: "biz" | "life";
  origin: string;
  homeUrl: string;
  resolveResource(input: string | object): WorkspaceResource | null;
  Provider: React.ComponentType<React.PropsWithChildren>;
  Dock: React.ComponentType;
  TopChromeAction: React.ComponentType;
};
```

`WorkspaceDockHost` 只负责布局、活动 Dock 和通用切换。每个 Dock 独立保存：

```text
open / active / pinned / followConversation / fullscreen
currentResource / navigation history / dirty
iframeRef / sessionNonce / authPhase / pendingNavigation
```

同一时间只显示一个 Dock；未激活 Dock 使用零宽度和 `visibility:hidden`，iframe 保持挂载。所有会导致丢失未保存内容的操作都受 Dirty State 保护。

### 11.2 Life Dock 配置和 CSP

```text
VITE_LIFE_APP_ORIGIN=https://life.example.com
VITE_LIFE_APP_URL=https://life.example.com/embed/
```

Origin 必须是无路径、userinfo、query 和 fragment 的精确 HTTP(S) Origin。Home URL 必须属于同一 Origin。

Pacioli Tauri CSP 的 `frame-src` 只列出经过验证的 Business/Life Origin，不允许 `*`、`http:` 或 `https:` 通配。LifeOS 使用精确 `frame-ancestors` 允许实际 Pacioli Origin。

### 11.3 Embed Session

桌面链路：

```text
Pacioli Workbench OIDC Session
→ life-auth-gateway 验证 access token
→ POST /v1/embed-sessions
→ 返回单次 embedUrl
→ iframe 加载 /embed/bootstrap?code=...
→ LifeOS 原子消费 code
→ 建立 HttpOnly Life Dock Session + CSRF
→ 重定向到 allowlist /embed/... 路径
```

Embed Code 使用 32 字节随机值、只保存 hash、短时有效且单次消费。它绑定：

```text
LifeWorkbenchUser
optional IdentityBinding
Workbench Session
deployment_id
target resource and path
expiry
IP/User-Agent risk facts
trace_id
```

Embed Code 不是 API bearer token。浏览器版可以使用顶层登录回跳，但最终仍建立独立 Dock Session。

解绑、Workbench 登出、用户禁用和主动 Logout 联动撤销。Dock 最多自动恢复一次；再次失败后明确要求重新登录。

## 12. Bridge 协议

Life Bridge 复用 Business Bridge 的安全信封和版本语义：

```ts
type LifeBridgeEnvelope<T> = {
  version: 2 | 3;
  type: string;
  requestId: string;
  sessionNonce: string;
  payload?: T;
};
```

Version 2 承载导航、资源、动作和 Dirty State；Version 3 只承载认证状态与会话失效消息。Life Bridge 不改变 Business Bridge 的既有 wire schema。

Host → Life：

```text
HOST_INIT
SET_THEME
REFRESH
NAVIGATE
REQUEST_CURRENT_RESOURCE
CHECK_AUTH
LOGOUT
```

Life → Host：

```text
LIFE_READY
TITLE_CHANGED
ROUTE_CHANGED
RESOURCE_CHANGED
ACTION_COMPLETED
ACTION_FAILED
DATA_CHANGED
DIRTY_STATE_CHANGED
AUTH_STATUS
AUTH_REQUIRED
SESSION_EXPIRED
```

所有入站消息同时验证：

```text
event.origin
event.source
version
type
requestId
sessionNonce
per-type payload schema
length and collection bounds
```

`postMessage` 不传 token、Cookie、Workspace ID、个人正文或权限，不触发 Agent，也不直接发布 Pacioli 消息。

## 13. life:// 资源协议

### 13.1 资源对象

```ts
type WorkspaceResource = {
  version: 1;
  extensionId: "life";
  type:
    | "dashboard"
    | "domain"
    | "goal"
    | "project"
    | "action"
    | "calendar"
    | "journal"
    | "knowledge"
    | "review"
    | "ai_execution"
    | "draft";
  id?: string;
  path: string;
  title?: string;
  metadata?: Record<string, string>;
};
```

### 13.2 固定映射

```text
life://dashboard                 → /embed/dashboard
life://domain/{id}               → /embed/domains/{id}
life://goal/{id}                 → /embed/goals/{id}
life://project/{id}              → /embed/projects/{id}
life://action/{id}               → /embed/actions/{id}
life://calendar/{yyyy-mm-dd}      → /embed/calendar?date=...
life://journal/{id}              → /embed/journal/{id}
life://knowledge/{id}            → /embed/knowledge/{id}
life://review/{id}               → /embed/reviews/{id}
life://ai-execution/{id}         → /embed/ai-executions/{id}
life://draft/{id}                → /embed/drafts/{id}
```

ID 是 1–128 字符的不透明标识，严格编码和解码一次。Resolver 拒绝空 ID、路径穿越、额外层级、userinfo、fragment、未知 query 和重复参数。

链接不携带 token、Workspace ID、正文、邮箱、权限或可执行命令。`metadata` 只允许展示字段。

### 13.3 会话联动

- 普通点击在 Life Dock 打开；Cmd/Ctrl+Click 在系统浏览器打开；
- `followConversation=true` 时，只有当前受信 Turn 的已验证 `resourceRefs` 可以请求自动导航；
- Dock pinned、dirty 或当前安全域不同则不自动切换，只显示提示；
- 普通消息中的链接必须由用户点击，不因消息到达自动打开；
- `life://` 只负责定位，LifeOS 页面仍按 Dock Session 鉴权。

## 14. 扩展结果契约

```json
{
  "extensionId": "life",
  "operation": "action.status.update",
  "status": "succeeded",
  "summary": "行动已标记为完成",
  "resourceRefs": [
    {
      "scheme": "life",
      "type": "action",
      "id": "opaque-id",
      "version": 8,
      "title": "完成接口设计"
    }
  ],
  "traceId": "uuid",
  "auditId": "uuid"
}
```

Pacioli 按语义渲染结果，不关心结果来自 MCP 还是其他受信扩展。Agent 最终消息必须使用服务端摘要和资源引用，不得自行制造资源 ID、版本或成功状态。

`DATA_CHANGED` 只失效相关查询；`ACTION_COMPLETED/FAILED` 只显示受限状态和 Trace ID。Bridge 原始错误栈不能进入 Pacioli。

## 15. LifeOS → Pacioli Outbox

LifeOS 领域事务内写 Outbox：

```json
{
  "id": "uuid",
  "workspaceId": "uuid",
  "category": "action_due",
  "resourceType": "action",
  "resourceId": "opaque-id",
  "resourceVersion": 8,
  "sanitizedSummary": "一个今日行动已到期",
  "targetBindingId": "uuid",
  "idempotencyKey": "stable-key",
  "status": "pending",
  "attempts": 0,
  "nextAttemptAt": "RFC3339",
  "traceId": "uuid"
}
```

Notifier 使用独立 Life Agent Nostr identity：

- 默认通过现有加密 DM 路径投递给绑定用户；
- 只有有效 ChannelDisclosurePolicy 时才可发频道消息；
- 频道消息使用正常 channel kind 和 `h` tag；
- 消息携带受控来源、idempotency 和 trace tags，供去重和 loop prevention；
- 不新增 LifeOS 专用 Nostr kind。

重试采用指数退避和抖动。达到上限进入 dead letter，不删除。目标绑定或披露策略失效时停止投递，不自动改投其他目标。Dead letter 只能由管理员或绑定用户在确认策略仍有效后重放。

## 16. 审计和可观测性

### 16.1 两条审计链

安全审计由 `life-auth-gateway` 保存身份、Session、IAM、委托、确认和披露决策。领域审计由 LifeOS 保存资源状态变化。两者均只追加；领域写入与领域审计、Outbox 在同一事务。

统一 Trace：

```text
source_event_id
→ agent_turn_id
→ iam_decision_id
→ delegation_id
→ mcp_call_id
→ life_domain_audit_id
→ outbox_id
→ response_event_id
```

### 16.2 安全事件

```text
IDENTITY_BINDING_*
WORKBENCH_SESSION_*
EMBED_SESSION_*
LIFE_AGENT_TURN_GRANTED | DENIED
LIFE_DELEGATION_ISSUED | CONSUMED | EXHAUSTED | REVOKED | EXPIRED
WRITE_PREVIEW_CREATED | CONFIRMED | REJECTED | EXPIRED | CONFLICTED
MCP_TOOL_CALLED | SUCCEEDED | FAILED
DISCLOSURE_ALLOWED | DENIED | REDACTED
OUTBOX_DELIVERED | FAILED | DEAD_LETTERED
```

审计不保存 token、Cookie、embed code、完整 pubkey、prompt、个人正文、API 原始错误、查询结果或模型思维过程。

### 16.3 指标和日志

指标只使用工具、结果码、风险等级和主体类型等低基数标签；user、resource 和 Workspace ID 不作为 metrics label。结构化日志只记录 Trace ID 和受控 ID，错误详情留在服务端。

## 17. 错误模型

统一错误：

```json
{
  "ok": false,
  "error": {
    "code": "version_conflict",
    "message": "资源已发生变化，请重新读取后再操作。",
    "retryable": false
  },
  "traceId": "uuid"
}
```

稳定错误码至少包括：

```text
validation_failed
unknown_tool
binding_required
principal_inactive
scope_denied
dm_required
confirmation_required
version_conflict
command_consumed
command_expired
rate_limited
gateway_unavailable
life_api_unavailable
write_outcome_unknown
internal_error
```

规则：

- 输入、身份、授权和状态错误不自动重试；
- 限流只按 `retryAfter` 重试；
- 只读暂时错误可有限重试；
- 不确定写入不重写，只按幂等键查询；
- 内部异常和敏感详情不返回 Agent。

## 18. 降级和恢复

- Gateway 不可用：不签发 Life 委托，Pacioli、Business 和 Hermes 继续工作；
- LifeOS API 不可用：MCP 明确失败，不降级为数据库直连；
- MCP 子进程崩溃：Turn 失败并撤销委托，不复用 stdio Session；
- Life Dock 不可用：会话 Agent 能力仍可独立工作；Dock 登录不授予 Agent 权限；
- OIDC/Workbench Session 失效：停止新委托并撤销 Embed/Dock Session；已提交领域事务不回滚；
- Outbox 失败不回滚已经成功的领域事务；
- 审计写入失败时授权和领域写入失败关闭；
- 服务凭证、Gateway 签名 key 和 Notifier Nostr key 可独立轮换；
- 配置、能力目录或 schema 版本不匹配时服务拒绝启动。

## 19. 安全属性

- Prompt injection 不能增加工具、能力、数据范围或降低风险；
- Delegation 只通过环境传入，hash-only 存储，并绑定 audience、Agent、Turn、source event 和预算；
- source event、challenge 和确认命令都防重放；
- Workspace 由资源关系和授权上下文确定，prompt 字段不可信；
- expectedVersion 和 preview hash 防止陈旧写入；
- iframe 使用精确 CSP、frame-ancestors、Origin/source/nonce/schema 联合验证；
- MCP 只能访问配置 Origin 的固定路由，阻断 SSRF；
- DM-only、披露策略、字段脱敏和结果上限限制个人数据泄露；
- 通知来源 tags、幂等键和 Workflow 排除规则阻断循环；
- Hermes、Business 和 Life 使用不同 Token 格式、Audience、服务身份、表和密钥。

## 20. 测试与验收

### 20.1 单元和契约测试

- 通用扩展：双 Dock 注册、双域路由、资源 resolver、Bridge 校验和 Dirty State；
- Life IAM：主体、范围交集、义务、部分授权、DM-only 和频道披露；
- Gateway：challenge、pubkey 唯一性、TTL、预算、并发 consume、级联撤销和精确命令解析；
- MCP：固定 tools/list、schema、映射、URL/SQL 拒绝、token 脱敏和输出边界；
- LifeOS API：服务身份、LifeCallGrant、Workspace 隔离、版本、幂等、审计和 Outbox；
- Embed/Bridge：单次交换、CSRF、撤销、CSP、非法导航和非法消息。

### 20.2 集成和竞态测试

- consume/revoke、consume/expiry 和双重 consume；
- 相同幂等键不同 payload；
- 预览后资源变化、高风险命令重放和两设备同时确认；
- 独立 Agent 权限变化、代理人解绑、用户禁用和 membership 移除；
- Outbox 重试、重复投递、策略过期和 dead letter；
- Hermes Token 调用 Life Workbench MCP 失败；Life Delegation 调用 Hermes MCP 失败。

### 20.3 端到端验收路径

1. LifeOS 登录用户绑定 Nostr pubkey；
2. 绑定用户在 1:1 DM 查询今日上下文；
3. 代理 Agent 以最小权限创建行动并返回可打开的 `life://action/...`；
4. 单行动更新成功，陈旧版本明确冲突；
5. 删除先预览；普通“确认”失败；精确命令成功一次，重放失败；
6. 独立 Agent 只使用自身权限；
7. 多 Workspace 越权和资源枚举不可见；
8. 多人频道默认拒绝，有效策略只返回允许摘要；
9. Life Dock 的登录、主题、历史、Dirty、Pin 和恢复工作；
10. Outbox 只投递一次最小通知；
11. Gateway/API/Dock 分别故障时其他安全域保持可用；
12. 解绑或禁用后，下一次工具调用和 Dock heartbeat 失败关闭。

Pacioli 最终验证包括相关 Rust/桌面测试、`pnpm build:e2e`、Dock/Agent E2E、真实 relay + ACP + LifeOS 流程和 `just ci`。LifeOS 复用现有静态、MCP、Workspace 和 runtime 测试，并补 Gateway、Embed、IAM、Outbox 和浏览器 E2E。

## 21. 分阶段交付边界

1. 通用扩展契约与双 Dock Host，Business 行为零变化；
2. `life-auth-gateway`、Life IAM、身份绑定和安全审计；
3. `life-workbench-mcp` 和只读 Agent Turn；
4. 低、中风险正式写入、版本和幂等；
5. 高风险 WriteCommand、精确确认和 Step-up；
6. Life Dock、Embed Session、Bridge 和 `life://`；
7. Outbox、频道披露和运维控制；
8. 全链路安全测试、灰度和故障演练。

每阶段有独立开关，默认关闭：

```text
LIFE_EXTENSION_ENABLED
LIFE_AGENT_READ_ENABLED
LIFE_AGENT_WRITE_ENABLED
LIFE_CHAT_HIGH_RISK_WRITE_ENABLED
LIFE_DOCK_ENABLED
LIFE_NOTIFIER_ENABLED
```

关闭扩展不删除 LifeOS 数据。回滚停止签发新委托并撤销活动委托；Hermes 不受影响。

## 22. 明确非目标

- 不把 LifeOS 数据同步到 Pacioli Relay 或 Search；
- 不新增 LifeOS 专用 Nostr kind；
- 不让 MCP 直连数据库或访问任意 URL；
- 不共享 Business/Life/Hermes Token、Session、IAM 表、Audience 或密钥；
- 不用 Dock Bridge 传递授权；
- 不让频道成员关系替代 Life IAM；
- 不做 Business 与 Life 的分布式事务；
- 不替换或收紧 Hermes 当前直接写入模型；
- 本规格不包含逐文件实现步骤，也不授权修改代码、数据库、配置或部署。

## 23. 验收结论

完整形态成立的判断标准不是“LifeOS 能在 iframe 打开”或“Agent 能调用一个 MCP”，而是以下属性同时成立：

- 身份绑定可验证且可撤销；
- 独立和代理 Agent 权限不混用；
- 每次调用均受短时、细粒度、可消费的 Turn Delegation 约束；
- 明确会话请求可以正式提交，且高风险提交不可被自然语言模糊确认绕过；
- LifeOS API 是最终授权和领域规则执行者；
- Dock、Agent 和通知三条路径互不充当对方的授权凭证；
- 个人数据默认 DM-only，频道披露显式且最小化；
- Business、Life 和 Hermes 任一安全域故障或凭证泄露不会自动扩散到其他安全域；
- 全链路具有可关联但不复制敏感正文的审计证据。
