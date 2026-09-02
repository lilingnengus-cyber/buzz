# LifeOS 接入阶段 5：Outbox、披露、可观测性与灰度实施计划

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. This is the final production-readiness plan; do not enable production flags until every exit test passes.

**Goal:** 完成 LifeOS → Pacioli 的最小通知、多人频道披露控制、全链路审计/指标、故障演练和渐进启用，使 Life 安全域可独立上线、暂停和回滚。

**Architecture:** LifeOS 领域事务写 Outbox；独立 `life-notifier` 服务以自己的 Nostr identity claim/发布/ack，默认加密 DM，只有有效 ChannelDisclosurePolicy 才能发频道。Gateway 记录安全决策，LifeOS 记录领域变化；两条审计通过低敏 trace ID 关联。所有功能由依赖明确、默认关闭的开关控制。

**Tech Stack:** Prisma/PostgreSQL；Rust/Tokio/reqwest/nostr/buzz-ws-client；Prometheus/tracing；Nostr DM/NIP-29；Playwright/集成测试。

---

### Task 1: 建立 Pacioli 目标绑定与频道披露策略

**Files (LifeOS):**
- Modify: `prisma/schema.prisma`
- Create: `lib/pacioli/bindings.ts`
- Create: `lib/pacioli/disclosure-policy.ts`
- Create: `app/api/pacioli/bindings/route.ts`
- Create: `app/api/pacioli/disclosure-policies/route.ts`
- Create: `components/settings/pacioli-binding-panel.tsx`
- Create: `components/settings/channel-disclosure-panel.tsx`
- Modify: `app/settings/page.tsx`
- Create: `scripts/test-pacioli-disclosure-policy.mjs`

**Step 1: 写失败测试**

覆盖绑定 user/pubkey/community/默认 DM target；策略绑定 user/community/channel、allowedCategories、maxSensitivity、expiry/status；频道 policy 不授予 write；健康/财务/关系/日志正文/知识正文默认禁止；过期/撤销即时拒绝。

**Step 2: 运行并确认失败**

```bash
cd /Users/aaronli/Projects/life-os
node scripts/test-pacioli-disclosure-policy.mjs
```

Expected: FAIL。

**Step 3: 扩展 schema**

新增 `PacioliTargetBinding` 和 `ChannelDisclosurePolicy`，使用明确 enum、active partial semantics 和 expiry 索引。策略只存 category/sensitivity，不存频道内容。

**Step 4: 实现 API/UI**

只有绑定用户可创建/撤销；目标 pubkey 必须与 Gateway active IdentityBinding 对应；频道 ID/community 由 Pacioli 受信选择器提供，不接受自由文本伪造。UI 明确显示策略不授予写权限和到期时间。

**Step 5: 运行并提交**

```bash
npm run prisma:generate
node scripts/test-pacioli-disclosure-policy.mjs
npm run build
git add prisma/schema.prisma lib/pacioli app/api/pacioli components/settings app/settings/page.tsx scripts/test-pacioli-disclosure-policy.mjs
git commit -m "feat: add pacioli disclosure policies"
```

### Task 2: 在 Gateway 和 ACP 执行 DM-only/频道披露

**Files (Pacioli):**
- Create: `services/life-auth-gateway/src/disclosure.rs`
- Create: `services/life-auth-gateway/tests/disclosure.rs`
- Modify: `services/life-auth-gateway/src/agent.rs`
- Modify: `services/life-auth-gateway/src/iam.rs`
- Modify: `services/life-auth-gateway/src/http.rs`
- Modify: `crates/buzz-acp/src/life_agent.rs`
- Modify: `crates/buzz-acp/src/life_response.rs`
- Test: `crates/buzz-acp/src/life_agent.rs`

**Step 1: 写失败测试**

1:1 DM 使用 Relay 验证 participant set；多人频道无策略拒绝；有效策略只允许指定 category/sensitivity；任何频道 write 拒绝；策略在 delegation consume 前过期则拒绝；结果字段按 `redact_sensitive` obligation 清洗。

**Step 2: 运行并确认失败**

```bash
cd /Users/aaronli/Projects/Paqiaoli
. ./bin/activate-hermit
cargo test -p life-auth-gateway --test disclosure
cargo test -p buzz-acp life_disclosure
```

Expected: FAIL。

**Step 3: 实现固定 policy lookup**

Gateway 通过受信 LifeOS service API 按 user/community/channel 查询策略；禁止 caller 自报 `allowedCategories`。读取失败 fail closed。decision 写安全审计但不记录正文。

**Step 4: 强制只读和结果最小化**

多人频道即便 policy active，也把 effective capabilities 限制为 read；Life response renderer 只接受允许 category 的服务端 sanitized summary/resource link，不能输出 journal/knowledge 正文。

**Step 5: 运行并提交**

```bash
cargo test -p life-auth-gateway --test disclosure
cargo test -p buzz-acp life_disclosure
git add services/life-auth-gateway crates/buzz-acp/src/life_agent.rs crates/buzz-acp/src/life_response.rs
git commit -s -m "feat: enforce life disclosure policy"
```

### Task 3: 把领域写入与 Outbox 放进同一事务

**Files (LifeOS):**
- Create: `lib/pacioli/outbox.ts`
- Create: `lib/pacioli/notification-policy.ts`
- Modify: `lib/workbench/write-service.ts`
- Create: `scripts/test-pacioli-outbox-transaction.mjs`

**Step 1: 写失败测试**

测试需要通知的领域写入同时产生一个 outbox；领域写失败无 outbox；outbox 插入失败领域事务回滚；相同稳定 idempotency key 只有一行；sanitizedSummary 不含正文；不需要通知的操作不产生 outbox。

**Step 2: 运行并确认失败**

```bash
cd /Users/aaronli/Projects/life-os
node scripts/test-pacioli-outbox-transaction.mjs
```

Expected: FAIL。

**Step 3: 实现 transaction-aware enqueue**

`enqueuePacioliNotification(tx, event)` 必须接收现有 Prisma transaction client，禁止内部开启第二事务。category/resource/version/target/idempotency/trace 来自服务端领域结果；summary 使用固定模板，不拼接 journal/knowledge 原文。

**Step 4: 运行并提交**

```bash
node scripts/test-pacioli-outbox-transaction.mjs
npm run test:static
git add lib/pacioli/outbox.ts lib/pacioli/notification-policy.ts lib/workbench/write-service.ts scripts/test-pacioli-outbox-transaction.mjs
git commit -m "feat: enqueue pacioli notifications transactionally"
```

### Task 4: 为 Notifier 建立固定 claim/ack/dead-letter API

**Files (LifeOS):**
- Create: `lib/pacioli/notifier-auth.ts`
- Create: `lib/pacioli/outbox-service.ts`
- Create: `app/api/internal/pacioli-outbox/claim/route.ts`
- Create: `app/api/internal/pacioli-outbox/ack/route.ts`
- Create: `app/api/internal/pacioli-outbox/fail/route.ts`
- Create: `app/api/internal/pacioli-outbox/replay/route.ts`
- Create: `scripts/test-pacioli-outbox-worker-api.mjs`
- Modify: `middleware.ts`

**Step 1: 写失败测试**

覆盖独立 notifier service credential、`FOR UPDATE SKIP LOCKED` 等价 claim、lease expiry、ack idempotent、错误 event ID 不可覆盖、退避、最大次数 dead letter、binding/policy 失效停止、replay 需要绑定用户/管理员显式确认。

**Step 2: 运行并确认失败**

```bash
node scripts/test-pacioli-outbox-worker-api.mjs
```

Expected: FAIL。

**Step 3: 实现状态机**

```text
pending → leased → delivered
              └→ pending (retry)
              └→ dead_letter
```

Claim response只返回已脱敏发送 envelope：target pubkey 或 channel、category、sanitized summary、`life://` resource ref、idempotency、trace。绝不返回 workspace 正文或数据库 token。

**Step 4: 运行并提交**

```bash
node scripts/test-pacioli-outbox-worker-api.mjs
git add lib/pacioli app/api/internal/pacioli-outbox scripts/test-pacioli-outbox-worker-api.mjs middleware.ts
git commit -m "feat: expose reliable pacioli outbox claims"
```

### Task 5: 创建独立 `life-notifier` Nostr 发布服务

**Files (Pacioli):**
- Create: `services/life-notifier/Cargo.toml`
- Create: `services/life-notifier/src/main.rs`
- Create: `services/life-notifier/src/config.rs`
- Create: `services/life-notifier/src/outbox_client.rs`
- Create: `services/life-notifier/src/publisher.rs`
- Create: `services/life-notifier/src/message.rs`
- Create: `services/life-notifier/tests/message_contract.rs`
- Create: `services/life-notifier/tests/publish_retry.rs`
- Create: `services/life-notifier/README.md`
- Modify: `Cargo.toml`

**Step 1: 写失败测试**

默认 target 产生现有加密 DM；频道只产生正常 channel event 和 `h` tag；消息带受控 source/idempotency/trace tags；无 Life 专用 kind；同 outbox 重试保持稳定 dedup identity；私钥和正文不进日志。

**Step 2: 运行并确认失败**

```bash
cd /Users/aaronli/Projects/Paqiaoli
. ./bin/activate-hermit
cargo test -p life-notifier
```

Expected: FAIL，service 不存在。

**Step 3: 实现固定轮询器**

Notifier 读取独立配置：LifeOS internal API URL/service credential、Relay URL、Notifier Nostr private key、poll/lease/backoff。使用 `buzz-ws-client` 连接/鉴权/发布；DM 使用现有 Nostr 加密消息路径；频道使用现有 kind 和 `h` tag。

**Step 4: 实现 publish/ack 语义**

只有 relay 接受 event 后才 ack；网络结果不确定时先按稳定 event ID 查询或安全重发相同 event，不创建新 event；策略失效错误不改投其他 target。

**Step 5: 运行并提交**

```bash
cargo test -p life-notifier
git add Cargo.toml Cargo.lock services/life-notifier
git commit -s -m "feat: publish lifeos outbox notifications"
```

### Task 6: 防循环、去重和 dead-letter 运维

**Files (Pacioli):**
- Modify: `services/life-notifier/src/message.rs`
- Modify: `crates/buzz-acp/src/life_agent.rs`
- Create: `crates/buzz-acp/src/life_notification_guard.rs`
- Create: `crates/buzz-acp/src/life_notification_guard.rs` test module
- Modify: `services/life-notifier/README.md`

**Files (LifeOS):**
- Create: `components/settings/pacioli-outbox-panel.tsx`
- Modify: `app/settings/page.tsx`
- Create: `scripts/test-pacioli-outbox-replay.mjs`

**Step 1: 写失败测试**

带 Life notifier source tag 的消息不会触发自动写回/重复 workflow；同 idempotency 只显示一次；dead letter 不自动删除；replay 重新验证 binding/policy 且仍使用同业务 idempotency。

**Step 2: 实现 guard**

ACP 只把通知作为普通消息上下文，不自动把它分类成新的 Life write Turn。用户对通知的明确新签名回复仍可产生新 Turn，但使用新 source event/trace。

**Step 3: 实现受控运维 UI**

只展示 category/resource ref/attempts/last error code/trace，不展示内部 stack。Replay 是显式动作并记录审计。

**Step 4: 运行并分别提交**

运行 Rust 定向测试和 LifeOS replay/static/build 后，在两个仓库分别提交。

### Task 7: 补齐安全审计、指标和低敏日志

**Files (Pacioli):**
- Create: `services/life-auth-gateway/src/audit.rs`
- Create: `services/life-auth-gateway/src/metrics.rs`
- Modify: `services/life-auth-gateway/src/agent.rs`
- Modify: `services/life-auth-gateway/src/embed.rs`
- Modify: `services/life-auth-gateway/src/write_confirmation.rs`
- Create: `services/life-notifier/src/metrics.rs`
- Create: `services/life-auth-gateway/tests/audit_redaction.rs`

**Files (LifeOS):**
- Modify: `lib/workbench/domain-audit.ts`
- Modify: `lib/pacioli/outbox-service.ts`
- Create: `scripts/test-workbench-audit-redaction.mjs`

**Step 1: 写失败测试**

每个规格安全事件都有记录；trace 链字段完整；审计不含 token/cookie/code/full pubkey/prompt/个人正文/raw error/模型思维；metrics label 不含 user/resource/workspace 高基数字段。

**Step 2: 实现统一 trace 传播**

```text
source_event_id → agent_turn_id → iam_decision_id → delegation_id
→ mcp_call_id → life_domain_audit_id → outbox_id → response_event_id
```

每个服务只记录它拥有的边；通过 trace ID 关联，不复制其他域的敏感记录。

**Step 3: 实现指标**

低基数标签只允许 service/tool/result_code/risk/subject_type/notification_category。增加 delegation active/consume conflict、MCP latency/error、write conflict/unknown、outbox lag/retry/dead-letter、Dock auth failure 等指标。

**Step 4: 运行并分别提交**

所有 redaction tests PASS 后分别提交。人工用测试 token 值搜索捕获日志，必须零命中。

### Task 8: 建立跨系统 E2E 与故障演练

**Files (Pacioli):**
- Create: `crates/buzz-test-client/tests/e2e_life_workbench.rs`
- Create: `scripts/test-life-workbench-e2e.sh`
- Create: `docs/operations/life-workbench-runbook.md`

**Files (LifeOS):**
- Create: `scripts/test-pacioli-integration-e2e.mjs`

**Step 1: 编写 12 条最终验收 E2E**

逐条实现路线图第 5 节的身份绑定、DM read/write、version conflict、精确确认、独立 Agent、workspace isolation、频道披露、Dock、Outbox、故障隔离、撤销。每条测试使用隔离用户/workspace/pubkey，结束后清理。

**Step 2: 增加竞态与故障注入**

并发 consume/revoke/expiry、同幂等不同 payload、preview 后变更、两设备确认、Outbox 重试/策略过期、Gateway/API/Dock/Notifier 分别断开。验证审计失败时授权/领域写 fail closed，Outbox 失败不回滚已提交领域事务。

**Step 3: 运行真实链路**

```bash
cd /Users/aaronli/Projects/Paqiaoli
. ./bin/activate-hermit
./scripts/test-life-workbench-e2e.sh
```

Expected: 全部 PASS，并输出一组可关联但不含正文的 trace IDs。

**Step 4: 编写 runbook**

记录启动依赖、开关、健康检查、key/token 轮换、活动委托撤销、dead-letter replay、故障隔离、回滚。禁止写真实凭证。

**Step 5: 提交**

分别提交 Pacioli E2E/runbook 和 LifeOS E2E 文件。

### Task 9: 灰度开关和回滚门禁

**Files (Pacioli):**
- Modify: `.env.example`
- Modify: `crates/buzz-acp/src/config.rs`
- Modify: `desktop/src/features/life-dock/lifeDockConfig.ts`
- Modify: `services/life-auth-gateway/src/config.rs`
- Modify: `services/life-notifier/src/config.rs`
- Create: `docs/operations/life-workbench-rollout.md`

**Files (LifeOS):**
- Modify: `.env.example`
- Create: `lib/workbench/feature-flags.ts`
- Create: `scripts/test-workbench-feature-flags.mjs`

**Step 1: 写失败测试**

六个开关默认 false；非法父子组合启动失败；关闭 write 后 read 仍可；关闭 notifier 停止 claim 不删 outbox；关闭 extension 撤销 active delegation；关闭 dock 不影响 Agent；版本不匹配拒绝启动。

**Step 2: 实现 flags**

按路线图依赖图实现强校验。不要在数据库里混入隐式开关；运行时配置的来源和当前值进入低敏启动日志。

**Step 3: 编写灰度顺序**

```text
内部测试用户 → 1:1 DM read → 低/中风险 write
→ 精确高风险 write → Life Dock → Outbox DM
→ 单一白名单频道披露 → 扩大用户范围
```

每一步定义观察窗口、成功率/冲突/拒绝/泄露告警门槛和一键回滚动作。

**Step 4: 运行并分别提交**

验证开关测试后，在两个仓库分别提交配置和文档。

### Task 10: 最终质量门禁与完成审查

**Step 1: Pacioli 全门禁**

```bash
cd /Users/aaronli/Projects/Paqiaoli
. ./bin/activate-hermit
just ci
just test
```

Expected: 全部 PASS；`just test` 使用隔离 Postgres/Redis。

**Step 2: LifeOS 全门禁**

```bash
cd /Users/aaronli/Projects/life-os
npm run prisma:generate
npm run test:static
npm run test:runtime
npm run build
```

Expected: 全部 PASS。

**Step 3: 真实 App 验证**

在实际 Pacioli Desktop、真实 relay、ACP、Gateway 和隔离 LifeOS 环境完成最终 12 条流程。报告设备/社区/账号范围、实际操作和 trace；静态测试不能替代。

**Step 4: 验证默认关闭和域隔离**

清除所有 `LIFE_*_ENABLED` 后再运行普通会话、Business 会话/Dock、Hermes read/write；行为必须与接入前一致。

**Step 5: 请求最终代码审查**

使用 `superpowers:requesting-code-review`，逐项核对设计规格第 19、20、23 节。所有审查问题解决并重跑门禁后，再使用 `superpowers:finishing-a-development-branch` 决定合并方式；未经用户明确授权不得打开生产开关或部署。
