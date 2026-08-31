# LifeOS 接入阶段 2：身份、IAM 与 Turn Delegation 实施计划

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 建立与企业工作台同安全等级、但完全隔离的 Life 身份域，支持 Nostr pubkey 唯一绑定、独立/代理 Agent、确定性 IAM、单 Turn 可消费委托、Embed Session 和追加式安全审计。

**Architecture:** 新建纯策略 crate `life-iam` 和独立 Axum 服务 `life-auth-gateway`。Gateway 使用自己的 PostgreSQL schema、Audience、服务凭证和 Ed25519 key；它验证完整 Nostr source event，并向 `life-workbench-mcp` 签发 hash-only delegation，再为单次 LifeOS API 调用签发短时 `LifeCallGrant`。

**Tech Stack:** Rust / Axum / SQLx / PostgreSQL / nostr / Ed25519 / SHA-256 / HMAC-safe token compare。

---

### Task 1: 建立 `life-iam` 的失败测试和类型边界

**Files:**
- Create: `crates/life-iam/Cargo.toml`
- Create: `crates/life-iam/src/lib.rs`
- Modify: `Cargo.toml`

**Step 1: 写失败测试**

在 `lib.rs` 的 test module 先定义验收：代理主体使用 human authority；独立 Agent 不继承 human；capability 和每个 scope 维度做交集；义务只增加；DM-only 在多人频道拒绝；部分授权返回明确缩小后的结果。

```rust
#[test]
fn independent_agent_never_inherits_human_write_authority() {
    let decision = evaluate(EvaluationInput::independent_agent(
        authority(["action:read"]),
        requested(["action:read", "action:update"]),
    ));
    assert_eq!(decision.allowed_capabilities, set(["action:read"]));
    assert_eq!(decision.denied_capabilities, set(["action:update"]));
}
```

**Step 2: 运行并确认失败**

```bash
cd /Users/aaronli/Projects/Paqiaoli
. ./bin/activate-hermit
cargo test -p life-iam
```

Expected: FAIL，crate/API 尚不存在。

**Step 3: 实现最小公共 API**

新增有文档注释的公开类型：

```rust
pub enum SubjectAuthority { Human(Authority), IndependentAgent(Authority) }
pub enum Obligation {
    HumanConfirmation,
    StepUpAuthentication,
    DualControl,
    DmOnly,
    RedactSensitive,
    MaxBatch(u32),
}
pub struct DataScope {
    pub workspaces: ScopeSet,
    pub domains: ScopeSet,
    pub projects: ScopeSet,
    pub resources: ScopeSet,
    pub sensitivities: ScopeSet,
    pub operation_count: ScopeSet,
}
pub fn evaluate(input: EvaluationInput) -> Decision;
```

复用 `business-iam` 的确定性交集思想，但不要让两个 crate 互相依赖，也不要复用 Business enum/table/audience。

**Step 4: 运行测试**

```bash
cargo test -p life-iam
cargo fmt --all -- --check
```

Expected: PASS。

**Step 5: 提交**

```bash
git add Cargo.toml Cargo.lock crates/life-iam
git commit -s -m "feat: add isolated life iam policy engine"
```

### Task 2: 创建 Gateway 骨架、强配置和独立数据库

**Files:**
- Create: `services/life-auth-gateway/Cargo.toml`
- Create: `services/life-auth-gateway/src/lib.rs`
- Create: `services/life-auth-gateway/src/main.rs`
- Create: `services/life-auth-gateway/src/config.rs`
- Create: `services/life-auth-gateway/src/model.rs`
- Create: `services/life-auth-gateway/src/security.rs`
- Create: `services/life-auth-gateway/Dockerfile`
- Create: `services/life-auth-gateway/README.md`
- Modify: `Cargo.toml`

**Step 1: 写配置失败测试**

覆盖缺失 database/service credential/deployment/audience/signing key、错误 key 长度、Life audience 与 Business audience 相同、TTL 越界、production 允许不安全 HTTP 等场景。

**Step 2: 运行并确认失败**

```bash
cargo test -p life-auth-gateway config
```

Expected: FAIL。

**Step 3: 实现强类型配置**

至少要求：

```text
LIFE_AUTH_DATABASE_URL
LIFE_AUTH_BIND_ADDR
LIFE_AUTH_DEPLOYMENT_ID
LIFE_AUTH_PACIOLI_SERVICE_TOKEN
LIFE_AUTH_MCP_SERVICE_TOKEN
LIFE_AUTH_LIFEOS_SERVICE_TOKEN
LIFE_AUTH_CALL_GRANT_ISSUER
LIFE_AUTH_CALL_GRANT_AUDIENCE=lifeos-workbench-api
LIFE_AUTH_DELEGATION_AUDIENCE=life-workbench-mcp
LIFE_AUTH_ED25519_PRIVATE_KEY
LIFE_AUTH_WORKBENCH_OIDC_ISSUER
LIFE_AUTH_WORKBENCH_OIDC_AUDIENCE
```

Token 比较用常量时间；日志 Debug/Display 必须脱敏。

**Step 4: 实现只暴露 health 的服务骨架**

`/health/live` 不查依赖；`/health/ready` 检查数据库和签名 key。其他未知路由 404，不加通用代理。

**Step 5: 运行测试并提交**

```bash
cargo test -p life-auth-gateway config security
git add Cargo.toml Cargo.lock services/life-auth-gateway
git commit -s -m "feat: scaffold isolated life auth gateway"
```

### Task 3: 建立身份、Session、IAM、委托和审计 schema

**Files:**
- Create: `services/life-auth-gateway/migrations/0001_life_identity.sql`
- Create: `services/life-auth-gateway/migrations/0002_life_iam.sql`
- Create: `services/life-auth-gateway/migrations/0003_life_delegations.sql`
- Create: `services/life-auth-gateway/migrations/0004_life_embed_and_commands.sql`
- Create: `services/life-auth-gateway/src/store.rs`
- Create: `services/life-auth-gateway/tests/postgres_security.rs`

**Step 1: 写数据库集成测试**

用隔离 database schema 测试：

- 一个 active pubkey 只能绑定一个 active Life user；
- challenge 只能成功消费一次；
- Workbench Session 与 Life user/deployment 绑定；
- capability catalog 有 version/risk/tool/expected-version/budget/obligations；
- delegation 只存 token hash；
- active delegation 的 `(agent_turn_id, source_event_id, audience)` 唯一；
- audit 只追加，应用角色不能 update/delete；
- Business 数据库 URL/表名不会被引用。

**Step 2: 运行并确认失败**

```bash
${LIFE_AUTH_TEST_DATABASE_URL:?set LIFE_AUTH_TEST_DATABASE_URL to an isolated PostgreSQL database} cargo test -p life-auth-gateway --test postgres_security
```

Expected: FAIL，migration/table 不存在。

**Step 3: 实现 migration**

表至少包含：

```text
life_workbench_users
life_identity_binding_challenges
life_identity_bindings
life_workbench_sessions
life_workspace_memberships
life_principals
life_principal_capabilities
life_principal_data_scopes
life_capability_catalog
life_iam_decisions
life_agent_delegations
life_delegation_calls
life_embed_codes
life_embed_sessions
life_write_command_confirmations
life_security_audit
```

所有活动状态使用 partial unique index；所有时间使用 `timestamptz`；所有 token/code 只存 `bytea` hash；审计表禁止敏感正文列。

**Step 4: 实现事务性 store API**

公开方法只接受强类型 ID，不暴露任意 SQL/where；所有状态转换通过带前置状态的 `UPDATE ... WHERE status = ... RETURNING`。

**Step 5: 运行并提交**

```bash
${LIFE_AUTH_TEST_DATABASE_URL:?set LIFE_AUTH_TEST_DATABASE_URL to an isolated PostgreSQL database} cargo test -p life-auth-gateway --test postgres_security
git add services/life-auth-gateway/migrations services/life-auth-gateway/src/store.rs services/life-auth-gateway/tests/postgres_security.rs
git commit -s -m "feat: add life auth security schema"
```

### Task 4: 实现 Workbench OIDC 用户映射和 Nostr 身份绑定

**Files:**
- Create: `services/life-auth-gateway/src/auth.rs`
- Create: `services/life-auth-gateway/src/identity.rs`
- Create: `services/life-auth-gateway/src/http.rs`
- Create: `services/life-auth-gateway/tests/jwt_validation.rs`
- Create: `services/life-auth-gateway/tests/identity_binding.rs`
- Modify (LifeOS): `prisma/schema.prisma`
- Create (LifeOS): `lib/workbench/identity-mapping.ts`
- Create (LifeOS): `lib/workbench/internal-service-auth.ts`
- Create (LifeOS): `app/api/internal/workbench-identities/resolve/route.ts`
- Create (LifeOS): `scripts/test-workbench-identity-mapping.mjs`

**Step 1: 写失败测试**

覆盖 OIDC issuer/audience/expiry/nonce；challenge TTL、user/deployment 绑定；签名 kind `24243`、event ID、pubkey、challenge、时间窗口；pubkey 冲突；challenge/事件重放；解绑审计。

**Step 2: 运行并确认失败**

```bash
cargo test -p life-auth-gateway --test jwt_validation --test identity_binding
```

Expected: FAIL。

**Step 3: 在 LifeOS 实现 OIDC subject 映射**

增加 `WorkbenchExternalIdentity`，使用 `(issuer, subject)` 唯一键关联 `User`。固定 internal route 只接受 Life Gateway service credential，返回 Life user 的 opaque ID、active 状态和当前 memberships；不按 email 自动串联不同 issuer。

```bash
cd /Users/aaronli/Projects/life-os
npm run prisma:generate
node scripts/test-workbench-identity-mapping.mjs
```

Expected: PASS。只暂存本 Step 列出的 LifeOS 文件并单独提交；不要暂存已有用户改动。

**Step 4: 实现 Gateway 固定端点**

```text
POST /v1/workbench/sessions
POST /v1/identity-bindings/challenges
POST /v1/identity-bindings
DELETE /v1/identity-bindings/{binding_id}
GET /v1/me
```

绑定端点接收完整签名 Nostr event；不接受单独 `pubkey + signature`，不从 prompt 读取用户信息。撤销 binding 与相关 delegation/embed session 在同一事务完成。

Gateway 验证 OIDC 后用 `(issuer, subject)` 调 LifeOS 固定映射 route；LifeOS 没有显式映射或用户 inactive 时拒绝创建 Workbench Session，不按 email 猜测。

**Step 5: 运行测试并提交 Pacioli**

```bash
cargo test -p life-auth-gateway --test jwt_validation --test identity_binding
git add services/life-auth-gateway/src services/life-auth-gateway/tests
git commit -s -m "feat: add life workbench identity binding"
```

### Task 5: 实现主体解析、能力目录和确定性授权

**Files:**
- Create: `services/life-auth-gateway/src/iam.rs`
- Create: `services/life-auth-gateway/src/catalog.rs`
- Create: `services/life-auth-gateway/tests/iam_authorization.rs`
- Create: `services/life-auth-gateway/src/membership.rs`
- Create: `services/life-auth-gateway/tests/membership_events.rs`
- Modify: `services/life-auth-gateway/migrations/0002_life_iam.sql`
- Create (LifeOS): `lib/workbench/membership-events.ts`
- Create (LifeOS): `scripts/test-workbench-membership-events.mjs`

**Step 1: 写失败测试**

覆盖完整能力目录；未知 capability/tool fail closed；代理使用当前 human authority；匹配 `agent_id` 的 active independent principal 使用自身 authority；workspace membership/角色变化即时生效；义务不满足拒绝；频道披露不授予写权限。

**Step 2: 运行并确认失败**

```bash
cargo test -p life-auth-gateway --test iam_authorization
```

Expected: FAIL。

**Step 3: 实现 versioned catalog**

Catalog 启动时校验 capability 唯一、tool 唯一映射、risk 不可降低、write 是否要求 expectedVersion、batch 上限和 obligations。把规格中的 capability 全量 seed 到 migration，不允许模型提交自定义 capability。

**Step 4: 实现 membership 变更同步和撤销**

LifeOS 是 membership 事实源。它在 membership 或 user active 状态变化后，以独立 service credential 调 Gateway `POST /v1/workbench/membership-events`；事件含单调 `membershipVersion`，Gateway 幂等更新镜像并在同一事务撤销受影响 delegation/embed session。Gateway 创建新 delegation 前还要读取 LifeOS 当前 membership snapshot；LifeOS Workbench API 始终再做最终检查。

先写并运行双方的乱序、重复、撤销测试；同步失败必须告警并阻止该用户新委托，不能静默继续使用旧权限。

**Step 5: 实现授权事务**

在同一事务读取 binding/user/session/principal/membership/catalog，调用 `life_iam::evaluate`，写入不可变 `life_iam_decisions`。独立 Agent 存在但 inactive 时拒绝，不回退为代理 Agent。

**Step 6: 运行并分别提交**

```bash
cargo test -p life-iam
cargo test -p life-auth-gateway --test iam_authorization --test membership_events
git add services/life-auth-gateway/src/iam.rs services/life-auth-gateway/src/catalog.rs services/life-auth-gateway/src/membership.rs services/life-auth-gateway/tests/iam_authorization.rs services/life-auth-gateway/tests/membership_events.rs services/life-auth-gateway/migrations/0002_life_iam.sql
git commit -s -m "feat: authorize life agent principals"
```

在 LifeOS 单独运行 `node scripts/test-workbench-membership-events.mjs`，只提交 `lib/workbench/membership-events.ts` 和该测试文件。

### Task 6: 实现 Delegation 签发、原子 consume 和撤销

**Files:**
- Create: `services/life-auth-gateway/src/agent.rs`
- Create: `services/life-auth-gateway/src/call_grant.rs`
- Create: `services/life-auth-gateway/tests/agent_delegation.rs`
- Create: `services/life-auth-gateway/tests/delegation_races.rs`
- Modify: `services/life-auth-gateway/src/http.rs`
- Modify: `services/life-auth-gateway/src/store.rs`

**Step 1: 写签发与竞态测试**

测试 source event 签名/作者/绑定/时间窗口/唯一性、DM 参与者、scope 缩小、32-byte token、hash-only、TTL 300/最大 900、maxCalls、双重 consume、consume/revoke、consume/expiry、Turn finish revoke。

**Step 2: 运行并确认失败**

```bash
cargo test -p life-auth-gateway --test agent_delegation --test delegation_races
```

Expected: FAIL。

**Step 3: 实现端点**

```text
POST /v1/life-agent/delegations
POST /v1/life-agent/delegations/consume
POST /v1/life-agent/delegations/{id}/revoke
```

签发只接受 Pacioli Host service credential。Consume 同一 SQL 事务检查状态、expiry、audience、agent/turn、主体、binding、capability、scope、obligations、预算并递增。达到预算的当前调用成功并转 `exhausted`。

**Step 4: 签发单调用 LifeCallGrant**

Claims 固定包含 issuer/audience/expiry、delegation/call、capability/scope/resource/expectedVersion、normalizedInputHash、idempotencyKey、trace。TTL 只覆盖一次 API 调用；使用 Ed25519，不使用 delegation token 签名。

**Step 5: 运行并提交**

```bash
cargo test -p life-auth-gateway --test agent_delegation --test delegation_races
git add services/life-auth-gateway/src services/life-auth-gateway/tests
git commit -s -m "feat: issue consumable life turn delegations"
```

### Task 7: 实现 Embed Code/Session 和确认绑定基础

**Files:**
- Create: `services/life-auth-gateway/src/embed.rs`
- Create: `services/life-auth-gateway/src/write_confirmation.rs`
- Create: `services/life-auth-gateway/tests/embed_session.rs`
- Create: `services/life-auth-gateway/tests/write_confirmation.rs`
- Modify: `services/life-auth-gateway/src/http.rs`

**Step 1: 写失败测试**

Embed：单次 code、hash-only、target path allowlist、deployment/session/user 绑定、TTL、并发消费、撤销。确认：精确 parser、只允许新签名消息、版本/hash/command ID 全匹配、10 分钟、一次消费。

```rust
assert!(parse_exact_confirmation(
    "/confirm life-write 550e8400-e29b-41d4-a716-446655440000 v7 aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
).is_ok());
assert!(parse_exact_confirmation("确认").is_err());
```

**Step 2: 运行并确认失败**

```bash
cargo test -p life-auth-gateway --test embed_session --test write_confirmation
```

Expected: FAIL。

**Step 3: 实现 Gateway 端点**

```text
POST /v1/embed-sessions
POST /v1/embed-sessions/consume
POST /v1/embed-sessions/{id}/revoke
POST /v1/write-confirmations/validate
```

Gateway 不执行 WriteCommand；只验证 LifeOS 持久化 command 的不可变摘要和签名确认，并把 command 字段写入 delegation。

**Step 4: 运行并提交**

```bash
cargo test -p life-auth-gateway --test embed_session --test write_confirmation
git add services/life-auth-gateway/src services/life-auth-gateway/tests
git commit -s -m "feat: add life embed and exact confirmation grants"
```

### Task 8: 阶段出口与跨域隔离验证

**Files:**
- Create: `services/life-auth-gateway/tests/domain_isolation.rs`
- Modify: `services/life-auth-gateway/README.md`

**Step 1: 写跨域负测试**

证明 Business delegation token、Hermes token、browser cookie 均不能访问 Life Gateway agent 端点；Life token 不能访问 Business gateway/MCP；Life DB 无 Business 表依赖。

**Step 2: 执行全套测试**

```bash
cd /Users/aaronli/Projects/Paqiaoli
. ./bin/activate-hermit
cargo fmt --all -- --check
cargo clippy -p life-iam -p life-auth-gateway --all-targets -- -D warnings
cargo test -p life-iam
${LIFE_AUTH_TEST_DATABASE_URL:?set LIFE_AUTH_TEST_DATABASE_URL to an isolated PostgreSQL database} cargo test -p life-auth-gateway
```

Expected: 全部 PASS。

**Step 3: 文档化轮换和故障关闭**

README 记录 service credential、call-grant key、DB credential 独立轮换；记录 active delegation 撤销和旧 key 验证窗口。不要写入真实密钥。

**Step 4: 提交并审查**

```bash
git add services/life-auth-gateway/tests/domain_isolation.rs services/life-auth-gateway/README.md
git commit -s -m "test: prove life auth domain isolation"
```

使用 `superpowers:requesting-code-review`，重点审查原子消费 SQL、撤销竞态、独立 Agent inactive 行为、token 日志脱敏、审计追加性。
