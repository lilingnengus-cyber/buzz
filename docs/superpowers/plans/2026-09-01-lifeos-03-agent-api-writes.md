# LifeOS 接入阶段 3：Workbench API、MCP 与会话写入实施计划

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Work in both repositories, commit each repository separately, and never stage unrelated LifeOS changes.

**Goal:** 让 Pacioli Agent 在受验证的 Life Turn Delegation 下读取 LifeOS，并按风险模型正式提交低/中风险写入；高风险操作必须经不可变 WriteCommand 和精确新签名确认。

**Architecture:** `life-workbench-mcp` 是每 Turn stdio 子进程，只暴露固定工具；每次 tool call 先向 Gateway consume delegation，再带单调用 `LifeCallGrant` 调用 LifeOS `/api/workbench/*`。LifeOS 验证 grant 后按当前 Workspace/资源关系/主体/版本重新鉴权，并在同一事务写领域变更、幂等结果、审计与可选 Outbox。

**Tech Stack:** Rust rmcp / reqwest；Next.js route handlers / Zod / Prisma / PostgreSQL；Ed25519 JWS；ACP Turn Extension。

---

## 执行前保护

```bash
cd /Users/aaronli/Projects/life-os
git status --short
```

把输出保存到执行记录。LifeOS 已有用户修改；每次提交只能逐文件 `git add <本 Task 路径>`。若本计划要求修改的文件已经有用户改动，先用 `git diff -- <file>` 检查并保留，禁止覆盖或还原。

### Task 1: 建立跨服务固定契约 crate

**Files:**
- Create: `crates/life-workbench-contracts/Cargo.toml`
- Create: `crates/life-workbench-contracts/src/lib.rs`
- Create: `crates/life-workbench-contracts/src/catalog.rs`
- Create: `crates/life-workbench-contracts/src/result.rs`
- Modify: `Cargo.toml`

**Step 1: 写失败测试**

固定 tool → capability/risk/expectedVersion/maxBatch 映射，统一成功/错误、`life://` resource ref、input canonicalization/hash；未知字段和未知 tool 拒绝。

```rust
assert_eq!(catalog::tool("update_action_status").capability, "action:status_update");
assert_eq!(catalog::tool("execute_confirmed_life_write").risk, Risk::High);
assert!(catalog::tool("run_sql").is_none());
```

**Step 2: 运行并确认失败**

```bash
cd /Users/aaronli/Projects/Paqiaoli
. ./bin/activate-hermit
cargo test -p life-workbench-contracts
```

Expected: FAIL，crate 不存在。

**Step 3: 实现版本化契约**

固定读工具：`get_today_context`、`get_system_overview`、`list_projects`、`get_project_context`、`list_actions`、`get_action_detail`、`search_journal`、`get_review_context`、`get_weekly_review_context`、`search_knowledge`、`get_knowledge_item`、`get_ai_execution_context`。

固定写工具：`create_goal`、`create_project`、`create_action`、`update_action`、`update_action_status`、`reorder_action_children`、`set_today_focus`、`create_journal_entry`、`create_daily_review`、`create_project_review`、`apply_weekly_review`、`create_knowledge_item`、`start_ai_execution`、`append_ai_execution_output`、`finish_ai_execution`、`execute_confirmed_life_write`。

输入 canonical JSON 必须排序 object key、保留 array 顺序、拒绝非有限数值，再计算 `sha256:<lower-hex>`。

**Step 4: 运行并提交**

```bash
cargo test -p life-workbench-contracts
git add Cargo.toml Cargo.lock crates/life-workbench-contracts
git commit -s -m "feat: define life workbench contracts"
```

### Task 2: 为 LifeOS 增加 Workbench 安全数据模型

**Files (LifeOS):**
- Modify: `prisma/schema.prisma`
- Create: `lib/workbench/types.ts`
- Create: `lib/workbench/canonical-json.ts`
- Create: `lib/workbench/call-grant.ts`
- Create: `lib/workbench/service-auth.ts`
- Create: `scripts/test-workbench-schema-static.mjs`
- Create: `scripts/test-workbench-call-grant.mjs`
- Modify: `package.json`

**Step 1: 写失败测试**

测试 schema 含 resource version、call replay、idempotency、domain audit、WriteCommand、Outbox；测试 browser cookie、Hermes bearer、错误 service token、错误 issuer/audience/expiry/hash 都不能创建 Workbench context。

**Step 2: 运行并确认失败**

```bash
cd /Users/aaronli/Projects/life-os
node scripts/test-workbench-schema-static.mjs
node scripts/test-workbench-call-grant.mjs
```

Expected: FAIL。

**Step 3: 扩展 Prisma schema**

给需要乐观并发的 `Domain`、`Goal`、`Project`、`Action`、`JournalEntry`、`KnowledgeItem`、`AiExecution` 增加 `version Int @default(1)`。新增：

```prisma
model WorkbenchCallReceipt { callId String @id; payloadHash String; result Json?; createdAt DateTime @default(now()) }
model WorkbenchIdempotencyRecord { idempotencyKey String @id; payloadHash String; result Json; createdAt DateTime @default(now()) }
model LifeDomainAudit { id String @id @default(cuid()); workspaceId String; operation String; resourceType String; resourceId String?; beforeVersion Int?; afterVersion Int?; traceId String; createdAt DateTime @default(now()) }
model LifeWriteCommand { id String @id; workspaceId String; tool String; resourceType String; resourceId String; expectedVersion Int; normalizedInput Json; normalizedInputHash String; previewHash String; sideEffectSummary Json; status String; expiresAt DateTime; consumedAt DateTime?; traceId String; createdAt DateTime @default(now()) }
model LifeNotificationOutbox { id String @id; workspaceId String; category String; resourceType String; resourceId String; resourceVersion Int; sanitizedSummary String; targetBindingId String; idempotencyKey String @unique; status String; attempts Int @default(0); nextAttemptAt DateTime; traceId String; createdAt DateTime @default(now()) }
```

使用明确 enum 替换示意 `String` 状态；加 Workspace/状态/时间索引。不要存 grant/token/cookie/prompt/正文到审计。

**Step 4: 实现 grant 验证**

只接受 Life Gateway Ed25519 public key；校验 service identity、issuer、audience、expiry、call ID、normalizedInputHash、idempotency key、capability、resource 和 trace。验证后返回不可由 route body 覆盖的 `WorkbenchCallContext`。

**Step 5: 生成 client 并运行测试**

```bash
npm run prisma:generate
node scripts/test-workbench-schema-static.mjs
node scripts/test-workbench-call-grant.mjs
```

Expected: PASS。

**Step 6: 提交 LifeOS**

```bash
git add prisma/schema.prisma lib/workbench/types.ts lib/workbench/canonical-json.ts lib/workbench/call-grant.ts lib/workbench/service-auth.ts scripts/test-workbench-schema-static.mjs scripts/test-workbench-call-grant.mjs package.json
git commit -m "feat: add life workbench security model"
```

### Task 3: 建立 `/api/workbench/*` 的统一鉴权和错误边界

**Files (LifeOS):**
- Create: `lib/workbench/api-handler.ts`
- Create: `lib/workbench/authorize.ts`
- Create: `lib/workbench/result.ts`
- Create: `app/api/workbench/context/system/route.ts`
- Create: `scripts/test-workbench-auth-boundary.mjs`
- Modify: `middleware.ts`

**Step 1: 写失败测试**

发送 browser session cookie、Hermes `x-mcp-agent-token`、普通 bearer、有效 service 无 grant、grant 与 body hash 不符、跨 workspace resource；只允许有效 MCP service + LifeCallGrant。

**Step 2: 运行并确认失败**

```bash
node scripts/test-workbench-auth-boundary.mjs
```

Expected: FAIL。

**Step 3: 实现独立边界**

`middleware.ts` 对 `/api/workbench/` 不走现有 browser/Hermes 放行；route handler 内先验 MCP service，再验 grant。`authorizeWorkbenchCall` 从资源关系反查 workspace 和当前 membership/authority，不接受 body 中自报 workspace/role/user。

**Step 4: 固定错误格式**

稳定错误码：`validation_failed`、`unknown_tool`、`binding_required`、`principal_inactive`、`scope_denied`、`dm_required`、`confirmation_required`、`version_conflict`、`command_consumed`、`command_expired`、`rate_limited`、`gateway_unavailable`、`life_api_unavailable`、`write_outcome_unknown`、`internal_error`。响应不含 stack/SQL/Prisma/raw upstream。

**Step 5: 运行并提交**

```bash
node scripts/test-workbench-auth-boundary.mjs
git add lib/workbench app/api/workbench/context/system/route.ts scripts/test-workbench-auth-boundary.mjs middleware.ts
git commit -m "feat: isolate life workbench api authentication"
```

### Task 4: 实现只读 Workbench API

**Files (LifeOS):**
- Create: `app/api/workbench/context/today/route.ts`
- Create: `app/api/workbench/projects/route.ts`
- Create: `app/api/workbench/projects/[id]/route.ts`
- Create: `app/api/workbench/actions/route.ts`
- Create: `app/api/workbench/actions/[id]/route.ts`
- Create: `app/api/workbench/journal/search/route.ts`
- Create: `app/api/workbench/reviews/context/route.ts`
- Create: `app/api/workbench/reviews/weekly/route.ts`
- Create: `app/api/workbench/knowledge/search/route.ts`
- Create: `app/api/workbench/knowledge/[id]/route.ts`
- Create: `app/api/workbench/ai-executions/[id]/route.ts`
- Create: `lib/workbench/read-service.ts`
- Create: `scripts/test-workbench-read-api.mjs`

**Step 1: 写失败测试**

覆盖每个固定 route 的 schema、workspace/resource scope、日期/条数/片段长度上限、敏感字段脱敏、空结果、跨 workspace 不可枚举。

**Step 2: 运行并确认失败**

```bash
node scripts/test-workbench-read-api.mjs
```

Expected: FAIL。

**Step 3: 实现 read service**

复用 `lib/repository/*` 的领域查询，但显式传入 `WorkspaceScope`，禁止 `getDefaultWorkspaceScope` 和 sample fallback。Workbench API 在数据库不可用时返回 `life_api_unavailable`，不得把样例数据返回 Agent。

**Step 4: 统一结果限额**

项目/行动列表默认 50、最大 100；搜索文本片段默认 500 字符；日期窗口最大 93 天；结果总 JSON 大小在返回前检查。每个资源都返回可信 `version` 和最小 `resourceRefs`。

**Step 5: 运行并提交**

```bash
node scripts/test-workbench-read-api.mjs
npm run test:static
git add app/api/workbench lib/workbench/read-service.ts scripts/test-workbench-read-api.mjs
git commit -m "feat: add scoped life workbench read api"
```

### Task 5: 创建 per-turn `life-workbench-mcp` 只读服务

**Files (Pacioli):**
- Create: `services/life-workbench-mcp/Cargo.toml`
- Create: `services/life-workbench-mcp/src/lib.rs`
- Create: `services/life-workbench-mcp/src/main.rs`
- Create: `services/life-workbench-mcp/src/config.rs`
- Create: `services/life-workbench-mcp/src/tools.rs`
- Create: `services/life-workbench-mcp/src/client.rs`
- Create: `services/life-workbench-mcp/tests/mcp_contract.rs`
- Create: `services/life-workbench-mcp/tests/token_redaction.rs`
- Modify: `Cargo.toml`

**Step 1: 写失败测试**

固定 `tools/list`，拒绝额外字段/任意 URL/SQL/Prisma where，缺失六个 env fail closed，consume 请求字段完整，输出限额，错误脱敏，token 不出现在 Debug/日志/result。

**Step 2: 运行并确认失败**

```bash
cd /Users/aaronli/Projects/Paqiaoli
. ./bin/activate-hermit
cargo test -p life-workbench-mcp
```

Expected: FAIL。

**Step 3: 实现 stdio MCP**

只读取：`LIFE_DELEGATION_TOKEN`、`LIFE_AUTH_GATEWAY_URL`、`LIFE_API_URL`、`LIFE_AGENT_ID`、`LIFE_AGENT_TURN_ID`、`LIFE_TRACE_ID`。API base URL 启动时固定，tool 参数不能覆盖 host/path。

每个调用流程：validate schema → canonical hash → Gateway consume → 固定 route → 清洗结果。只读暂时错误可有限重试；consume/授权/输入错误不重试。

**Step 4: 运行并提交**

```bash
cargo test -p life-workbench-mcp
git add Cargo.toml Cargo.lock services/life-workbench-mcp
git commit -s -m "feat: add delegated life read mcp"
```

### Task 6: 把只读 Life MCP 接入 ACP Turn

**Files (Pacioli):**
- Modify: `crates/buzz-acp/src/life_agent.rs`
- Modify: `crates/buzz-acp/src/life_agent_prompt.md`
- Modify: `crates/buzz-acp/src/product_extensions.rs`
- Create: `crates/buzz-acp/src/life_response.rs`
- Test: `crates/buzz-acp/src/life_agent.rs`
- Test: `crates/buzz-acp/src/life_response.rs`

**Step 1: 写路由和披露失败测试**

优先级：有效 `biz://`/`life://` → 用户显式域 → 当前受信资源域 → 歧义。默认只允许绑定用户与目标 Agent 的 1:1 DM；多人频道无披露策略不签发数据工具。

**Step 2: 运行并确认失败**

```bash
cargo test -p buzz-acp life_agent life_response
```

Expected: FAIL。

**Step 3: 实现 begin_turn**

从 `VerifiedTurnContext` 请求 Gateway delegation；通过 `McpServer` env 注入 token 和六个固定字段；要求 fresh session；drop/finish guard 在成功、失败、取消、超时全部 revoke。不得把 token 加进 prompt。

**Step 4: 实现结果观察**

只接受 MCP 返回的 `LifeExtensionResult`；Agent 最终答复必须使用服务端 summary/resourceRefs/status/trace，不得自行构造成功。解析失败只返回受控错误，不把原始 tool payload 发布到频道。

**Step 5: 运行并提交**

```bash
cargo test -p buzz-acp life_agent life_response
git add crates/buzz-acp/src/life_agent.rs crates/buzz-acp/src/life_agent_prompt.md crates/buzz-acp/src/life_response.rs crates/buzz-acp/src/product_extensions.rs
git commit -s -m "feat: connect delegated life read turns"
```

### Task 7: 实现低/中风险正式写入、版本与幂等

**Files (LifeOS):**
- Create: `lib/workbench/write-service.ts`
- Create: `lib/workbench/idempotency.ts`
- Create: `lib/workbench/domain-audit.ts`
- Create: `app/api/workbench/goals/route.ts`
- Create: `app/api/workbench/projects/write/route.ts`
- Create: `app/api/workbench/actions/write/route.ts`
- Create: `app/api/workbench/actions/status/route.ts`
- Create: `app/api/workbench/actions/reorder/route.ts`
- Create: `app/api/workbench/focus/route.ts`
- Create: `app/api/workbench/journal/route.ts`
- Create: `app/api/workbench/reviews/route.ts`
- Create: `app/api/workbench/knowledge/route.ts`
- Create: `app/api/workbench/ai-executions/route.ts`
- Create: `scripts/test-workbench-write-api.mjs`
- Create: `scripts/test-workbench-idempotency.mjs`

**Step 1: 写失败测试**

覆盖全部低/中风险工具、明确 capability、expectedVersion、资源归属、相同 key 同 payload 返回首次结果、同 key 不同 payload 拒绝、领域变更+审计同事务、错误时无半写入。

**Step 2: 运行并确认失败**

```bash
node scripts/test-workbench-write-api.mjs
node scripts/test-workbench-idempotency.mjs
```

Expected: FAIL。

**Step 3: 实现事务包装器**

`executeWorkbenchWrite` 在一个 Prisma transaction 中：claim call ID → claim/读取 idempotency → 重新鉴权 → 以 `id + workspaceId + expectedVersion` 为条件更新 → `version += 1` → 写领域审计 → 写首次结果。冲突返回 `version_conflict`，不自动覆盖。

**Step 4: 接入领域 repository**

把写 service 调用限定到固定领域函数；不提供通用 patch。任何允许字段由 route Zod schema 列举，敏感/外部副作用字段不在普通写 route。

**Step 5: 运行并提交 LifeOS**

```bash
node scripts/test-workbench-write-api.mjs
node scripts/test-workbench-idempotency.mjs
npm run test:static
git add lib/workbench app/api/workbench scripts/test-workbench-write-api.mjs scripts/test-workbench-idempotency.mjs
git commit -m "feat: add versioned life workbench writes"
```

**Step 6: 为 MCP 开启对应写工具并提交 Pacioli**

先增加写工具 MCP contract 测试，再在 `services/life-workbench-mcp/src/tools.rs` 和 `client.rs` 映射固定 route。写请求超时后只查询幂等状态，不重发。

```bash
cargo test -p life-workbench-mcp
git add services/life-workbench-mcp crates/life-workbench-contracts
git commit -s -m "feat: enable delegated life workbench writes"
```

### Task 8: 实现高风险 WriteCommand 和精确确认

**Files (LifeOS):**
- Create: `lib/workbench/write-command.ts`
- Create: `app/api/workbench/write-commands/preview/route.ts`
- Create: `app/api/workbench/write-commands/execute/route.ts`
- Create: `app/api/workbench/write-commands/[id]/route.ts`
- Create: `scripts/test-workbench-write-command.mjs`
- Modify: `lib/workbench/write-service.ts`

**Files (Pacioli):**
- Modify: `services/life-workbench-mcp/src/tools.rs`
- Modify: `services/life-workbench-mcp/src/client.rs`
- Modify: `crates/buzz-acp/src/life_agent.rs`
- Modify: `crates/buzz-acp/src/life_response.rs`
- Test: `services/life-workbench-mcp/tests/mcp_contract.rs`
- Test: `crates/buzz-acp/src/life_agent.rs`

**Step 1: 写失败测试**

测试删除、批量覆盖、外部邀请、敏感导出、AI policy 变化不能走普通 write；preview immutable；preview 后 version 变化冲突；普通“确认”/附加文本/引用/旧消息失败；精确新签名命令成功一次；两设备并发只有一个成功。

**Step 2: 运行并确认失败**

```bash
cd /Users/aaronli/Projects/life-os && node scripts/test-workbench-write-command.mjs
cd /Users/aaronli/Projects/Paqiaoli && . ./bin/activate-hermit && cargo test -p life-workbench-mcp -p buzz-acp life_write
```

Expected: FAIL。

**Step 3: 实现 preview**

LifeOS 持久化 normalized input、expectedVersion、风险、受控 side-effect summary、64-hex previewHash、10 分钟 expiry 和 `pending`。返回精确命令字符串，不允许 Agent 改写。

**Step 4: 实现零参数执行**

`execute_confirmed_life_write` 的 MCP schema 必须是空 object 且 `additionalProperties=false`。执行目标和参数只来自 delegation/grant 绑定 command；LifeOS 在事务内 `pending → consumed` 并再次检查 version/hash/expiry。

**Step 5: 运行并分别提交**

```bash
cd /Users/aaronli/Projects/life-os
node scripts/test-workbench-write-command.mjs
git add lib/workbench/write-command.ts lib/workbench/write-service.ts app/api/workbench/write-commands scripts/test-workbench-write-command.mjs
git commit -m "feat: add exact-confirmation life write commands"

cd /Users/aaronli/Projects/Paqiaoli
. ./bin/activate-hermit
cargo test -p life-workbench-mcp -p buzz-acp life_write
git add services/life-workbench-mcp crates/buzz-acp/src/life_agent.rs crates/buzz-acp/src/life_response.rs
git commit -s -m "feat: execute confirmed life writes"
```

### Task 9: Hermes 与跨域兼容性回归

**Files (LifeOS):**
- Create: `scripts/test-workbench-hermes-isolation.mjs`
- Modify: `package.json`

**Files (Pacioli):**
- Create: `services/life-workbench-mcp/tests/domain_isolation.rs`

**Step 1: 写隔离测试**

验证 Hermes token 仍可按当前路径调用现有 HTTP MCP 和直接写；Hermes token 调用 `/api/workbench/*` 失败；Life grant 调用 Hermes MCP 失败；Business delegation 调用 Life MCP 失败。

**Step 2: 运行测试**

```bash
cd /Users/aaronli/Projects/life-os
node scripts/test-workbench-hermes-isolation.mjs

cd /Users/aaronli/Projects/Paqiaoli
. ./bin/activate-hermit
cargo test -p life-workbench-mcp --test domain_isolation
```

Expected: PASS。

**Step 3: 提交测试**

只提交新测试和 package script；不得为通过测试修改 Hermes auth/write implementation。

### Task 10: 阶段出口验证

**Step 1: Pacioli 门禁**

```bash
cd /Users/aaronli/Projects/Paqiaoli
. ./bin/activate-hermit
cargo fmt --all -- --check
cargo clippy -p life-workbench-contracts -p life-workbench-mcp -p buzz-acp --all-targets -- -D warnings
cargo test -p life-workbench-contracts -p life-workbench-mcp -p buzz-acp
```

**Step 2: LifeOS 门禁**

```bash
cd /Users/aaronli/Projects/life-os
npm run prisma:generate
npm run test:static
npm run build
```

**Step 3: 真实 relay + ACP + LifeOS 验收**

在隔离测试账户执行：只读、创建 action、状态更新、陈旧版本、高风险 preview/普通确认拒绝/精确确认/重放。记录 source event、turn、decision、delegation、call、domain audit 和 response event 的 trace 链。

**Step 4: 请求代码审查**

使用 `superpowers:requesting-code-review`，重点检查：Workbench 是否存在 sample fallback、MCP 是否可覆盖 URL、版本更新是否原子、写响应丢失是否会重写、Hermes 是否被意外收紧。
