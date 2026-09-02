# Pacioli × LifeOS 完整接入实施路线图

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Do not start a later plan until the preceding exit gate is green.

**Goal:** 在不改变 Business Workbench 和 Hermes 既有行为的前提下，为 Pacioli 增加完整的个人 LifeOS 工作台：可验证身份绑定、每 Turn 最小委托、正式会话写入、高风险精确确认、Life Dock、`life://`、通知 Outbox 和全链路审计。

**Architecture:** Pacioli Core 只提供通用扩展生命周期和 Dock Host；Life 安全域由独立的 `life-auth-gateway`、`life-iam`、`life-workbench-mcp`、LifeOS `/api/workbench/*` 与 Life Dock 组成。Business、Life、Hermes 不共享身份表、Token、Audience、Session、密钥或审计链。

**Tech Stack:** Rust 1.88 / Tokio / Axum / SQLx / rmcp；Tauri 2 / React 19 / TypeScript；Next.js 15 / Prisma / PostgreSQL；Nostr；Playwright。

---

## 0. 执行约束

- 本文件和五个子计划是实施说明，不是实施授权；当前任务只创建计划。
- Pacioli 仓库：`/Users/aaronli/Projects/Paqiaoli`。
- LifeOS 仓库：`/Users/aaronli/Projects/life-os`。
- 执行前先在两个仓库分别运行 `git status --short`。LifeOS 当前已有用户未提交修改，实施者只能暂存本计划明确列出的路径，禁止 `git add -A`。
- Pacioli 运行 Git、Rust 或 hook 前执行 `. ./bin/activate-hermit`。
- Pacioli 每个提交使用 `git commit -s`；LifeOS 遵循其仓库自己的 `AGENTS.md` 和提交约定。
- 新开关全部默认关闭；每个阶段必须证明关闭开关时运行路径与当前版本等价。
- 不修改 `/Users/aaronli/Projects/life-os/mcp-server/server.mjs` 的 Hermes 鉴权或写入语义；只有在最终兼容性测试中增加独立测试文件。
- 不新增 LifeOS 专用 Nostr kind，不把 LifeOS 数据复制到 Relay/Search，不允许 MCP 直连 LifeOS 数据库。

## 1. 计划顺序与出口门槛

| 顺序 | 子计划 | 交付物 | 必须通过的出口门槛 |
|---|---|---|---|
| 1 | [通用扩展与双 Dock 基座](2026-09-01-lifeos-01-extension-platform.md) | 通用 Turn Extension Registry、WorkspaceDockHost、Business 适配 | Business Agent/Dock 全回归；Life 开关关闭；Core 无 Life 工具/权限名 |
| 2 | [Life 身份、IAM 与委托](2026-09-01-lifeos-02-auth-iam.md) | `life-iam`、`life-auth-gateway`、绑定/主体/委托/审计 | 唯一绑定、权限交集、并发 consume、级联撤销、跨域 token 拒绝 |
| 3 | [LifeOS Workbench API、MCP 与会话写入](2026-09-01-lifeos-03-agent-api-writes.md) | `/api/workbench/*`、`life-workbench-mcp`、读写、WriteCommand | 只读、低/中风险写、高风险精确确认、幂等、版本冲突、Hermes 不变 |
| 4 | [Life Dock、Embed、Bridge 与 life://](2026-09-01-lifeos-04-dock-embed.md) | Life Dock、单次 Embed Session、受控 Bridge、资源链接 | CSP/origin/nonce/schema、Dirty/Pin/历史、撤销恢复、非法链接拒绝 |
| 5 | [Outbox、披露、可观测性与灰度](2026-09-01-lifeos-05-notifier-rollout.md) | DM 通知、频道策略、dead letter、跨系统 E2E、运维手册 | 默认 DM-only、去重/防循环、故障隔离、全链路 trace、真实流程验收 |

## 2. 开关依赖

```text
LIFE_EXTENSION_ENABLED
├── LIFE_AGENT_READ_ENABLED
│   └── LIFE_AGENT_WRITE_ENABLED
│       └── LIFE_CHAT_HIGH_RISK_WRITE_ENABLED
├── LIFE_DOCK_ENABLED
└── LIFE_NOTIFIER_ENABLED
```

开关规则：

- 子开关不能在父开关关闭时生效；服务启动时发现非法组合立即失败。
- 关闭 `LIFE_EXTENSION_ENABLED` 时停止新委托并撤销活动 Life 委托，不删除 LifeOS 数据。
- `LIFE_DOCK_ENABLED` 与 Agent 能力独立；Dock 故障不能关闭 Agent，Agent 故障不能使 Dock Session 获得额外权限。
- `LIFE_NOTIFIER_ENABLED` 只影响新 Outbox claim，不回滚已完成的 LifeOS 领域事务。

## 3. 跨计划稳定契约

所有子计划必须共同遵守设计规格：

- 设计源：[2026-08-31-lifeos-workbench-integration-design.md](../specs/2026-08-31-lifeos-workbench-integration-design.md)
- 可信 Turn 字段只能由 Relay/ACP Host 构建：`source_event_id`、`source_pubkey`、`community_id`、会话类型、channel/DM 参与者、`agent_id`、`agent_turn_id`、`trace_id`。
- 代理 Agent 权限是当前 human authority 的确定性交集；独立 Agent 只使用其持久 authority。
- Delegation token 只进子进程环境，数据库只存 SHA-256 hash，Audience 固定为 `life-workbench-mcp`。
- MCP consume 后拿到 Ed25519 `LifeCallGrant`；LifeOS 仍执行最终 Workspace、资源关系、主体状态、版本和领域规则检查。
- 写入成功结果只能来自服务端，统一包含 `resourceRefs`、`auditId` 和 `traceId`。
- 高风险写必须经过不可变 WriteCommand 和新的、只包含精确 `/confirm life-write <command-id> v<expected-version> <preview-hash>` 的签名消息。
- Bridge、Dock Session、Agent Delegation 和 Outbox service identity 互不充当对方凭证。

## 4. 每阶段公共验证

Pacioli 最小门禁：

```bash
cd /Users/aaronli/Projects/Paqiaoli
. ./bin/activate-hermit
cargo fmt --all -- --check
cargo test -p buzz-acp
cd desktop && pnpm test && pnpm build:e2e
```

最终门禁：

```bash
cd /Users/aaronli/Projects/Paqiaoli
. ./bin/activate-hermit
just ci
```

LifeOS 最小门禁：

```bash
cd /Users/aaronli/Projects/life-os
npm run prisma:generate
npm run test:static
npm run build
```

涉及数据库或浏览器的阶段还必须运行对应子计划列出的隔离数据库集成测试和 Playwright E2E；不能用静态检查替代真实工作流证据。

## 5. 端到端最终验收

按顺序保留以下证据（命令输出、trace ID、必要截图），全部成功才算完整形态完成：

1. LifeOS 用户通过签名 challenge 绑定唯一 Nostr pubkey。
2. 绑定用户在 1:1 DM 查询今日上下文。
3. 代理 Agent 以最小权限创建行动并返回可打开的 `life://action/{id}`。
4. 单行动更新成功；陈旧 `expectedVersion` 返回稳定 `version_conflict`。
5. 删除操作先返回服务端 preview；普通“确认”失败；精确确认成功一次；重放返回 `command_consumed`。
6. 独立 Agent 不继承发起人的权限。
7. 跨 Workspace 枚举和修改均不可见。
8. 多人频道默认拒绝；有效披露策略只返回允许的最小摘要且不能写。
9. Life Dock 完成登录、主题同步、导航历史、Dirty Guard、Pin、Session 失效与单次恢复。
10. Outbox 默认通过加密 DM 投递一次；重复 claim 不重复发布。
11. Gateway、LifeOS API、Dock、Notifier 分别故障时，Business 与 Hermes 保持可用。
12. 解绑或禁用后，下一次工具调用和 Dock heartbeat 都失败关闭。

## 6. 完成后的集成选择

所有子计划执行并验证后，使用 `superpowers:finishing-a-development-branch` 处理每个仓库的合并策略。不要把两个仓库的提交压成一个不可审阅的跨仓库“大提交”；每个阶段保持可独立回滚。
