# Business IAM

## 定位

Business IAM 是业务权限的唯一权威来源。Authentik 负责人员登录、MFA、OIDC 和 Step-up；Buzz 负责协作、消息和 Agent 运行；两者都不直接决定业务资源权限。

当前采用“逻辑独立、物理共置”的第一阶段形态：

- 纯策略契约位于 `crates/business-iam`，不依赖 Buzz、HTTP、SQLx 或 Authentik；
- PostgreSQL 对象位于独立的 `business_iam` schema；
- `business-auth-gateway` 是当前持久化和 HTTP 适配器；
- 将来拆成独立服务时，保留策略 crate、schema 契约和决策格式，只替换适配器。

## 主体模型

| 主体 | 权限来源 | 是否持久 | 执行语义 |
|---|---|---:|---|
| Human | 自身角色和直接权限 | 是 | 普通业务人员 |
| Independent Agent | Agent 自身角色和直接权限 | 是 | 数字员工，不继承创建者或触发者权限 |
| Proxy Agent | Agent 仅保存能力上限 | 否 | 每个任务计算 `Human ∩ Agent ceiling ∩ Request` |

代理 Agent 的能力上限不是业务授权。例如代理 Agent 允许调用 `sales_order:read`，只有被代理人当前也拥有该权限时才能获得临时委托。

## 数据模型

`business_iam` schema 包含：

- `principals`：人员、独立 Agent、代理 Agent；
- `roles`、`permissions`、`role_permissions`、`principal_roles`；
- `principal_permissions`：直接授权或代理 Agent 能力上限；
- `authorization_decisions`：不可修改、不可删除的决策快照。

`agent_read_delegations` 不是权限源。它只保存一次 IAM 决策签发出的短时、单任务、有限调用次数凭证，并关联：

```text
human + agent + task + source event + channel + capability
+ effective data scope + trace + IAM decision
```

## 决策规则

### 独立 Agent

```text
effective = agent persistent permission ∩ requested scope
```

触发消息的人员不参与权限并集。人员拥有而 Agent 没有的权限不会被继承。

### 代理 Agent

```text
effective = delegating human current permission
          ∩ proxy agent capability ceiling
          ∩ requested task scope
```

请求多个 capability 时允许返回安全子集。未授权 capability 不进入委托；若交集为空则整个任务拒绝。

### 数据范围

数据范围按命名维度求交集，例如法人、仓库、客户、供应商、销售人员和期间。任一共同维度交集为空时，对应 capability 被拒绝；不会退化为全量数据。

## 撤销语义

- 正常 Agent 回合结束：ACP 同步等待 gateway 撤销完成；
- 取消、错误或对象提前释放：Drop 路径执行补偿撤销；
- 人员/Agent 停用、直接权限、角色绑定或角色权限发生变化：数据库触发器在同一事务中撤销所有相关活动委托；
- TTL 和调用次数上限只负责异常兜底，不代替正常撤销。

每次撤销都保留 IAM 决策快照，并向 `security_audit_events` 追加事件；不修改历史决策。

## Fail-closed

- 数据库只预置 capability 目录，不预置任何主体、角色或授权；
- 未登记 Agent、缺少人员主体、主体停用、权限交集为空、数据范围无交集均拒绝；
- gateway 运行账户只能读取 IAM 配置和写入决策，不能管理主体或授权；
- IAM 管理面必须使用独立管理员身份和审计链，不能复用 Agent service credential。

## 管理面

无网络监听的 `business-iam-admin` 仅作为引导和紧急运维入口。它使用
`BUSINESS_IAM_ADMIN_DATABASE_URL` 连接专用管理数据库角色，并要求
`BUSINESS_IAM_ADMIN_ACTOR` 标识操作人。每次变更与审计记录位于同一事务，审计同时
记录操作人声明和数据库 `current_user`，便于把操作追溯到真实凭据。

正常管理流量由独立进程 `business-iam-admin-api` 承载。它不依赖 Buzz Relay、ACP、
Desktop 或 Agent 凭据，仅接受 Authentik OIDC bearer token，并同时验证：当前人员已映射为
有效 Human 主体、持有 `business_iam:read/request/approve` 中对应能力、`auth_time` 在配置的
5 分钟窗口内、`amr` 包含要求的 MFA 方法。

所有变更先写入 24 小时有效的不可变 change request。申请人不能审批自己的申请；高风险
变更需要一名独立审批人，关键授权、撤权、停用及敏感角色分配需要两名不同审批人。目标
版本不匹配时不会覆盖新状态。Step-up JWT 只保存 SHA-256 证据哈希，不保存令牌；审批和
管理审计表禁止更新或删除。详细接口及部署参数见
[`services/business-iam-admin-api/README.md`](../../services/business-iam-admin-api/README.md)。

Agent 运行凭据不能调用该管理面；在线服务凭据与离线 break-glass 凭据必须分离。

桌面端通过 Buzz 的通用扩展组合点挂载 `Authority ledger`，不修改频道、成员或 Relay
授权模型。界面提供待审队列、连续的职责分离审批轨道、主体/角色/能力目录和结构化变更
申请；不接受任意 JSON。目录中的主体与角色版本会自动绑定到申请，避免管理员基于旧状态
覆盖新授权。API URL 只允许 HTTPS，开发和 E2E 仅放行 loopback HTTP；请求只携带当前
Workbench OIDC bearer token，不使用环境 Cookie，并在 Step-up 失效时回到 Authentik
重新验证。

## 写权限契约（尚未开放执行）

IAM capability 目录已登记 `sales_order:write`、`purchase_order:write`、
`inventory:adjust`、`payment:execute` 和 `business_approval:approve`。系统级默认义务与
角色/直接授权上的附加义务取并集，授权人不能通过发放角色移除默认控制。

这些记录只描述未来执行所需的权限和控制，不签发写委托，也不开放 `/execute`。真正
执行前还必须验证 Step-up 证据绑定当前人员与任务、审批人具备审批权限、发起人与审批人
分离、双人复核身份互异，并在可回滚的 Business API 上完成预演和验收。在此之前状态保持
`V7_BLOCKED`。

## 当前状态与下一步

已完成策略模型、数据库 schema、gateway 决策接入、权限子集签发、同步撤销、受控 IAM
管理 API、桌面 Authority ledger，以及 PostgreSQL 双人审批和桌面浏览器集成测试。仍需完成：

1. Authentik 真实 MFA/Step-up 管理流验收；
2. 写执行适配器、可回滚 staging 验证（在此之前继续 `V7_BLOCKED`）；
3. 生产部署、密钥轮换、监控和灾备演练。
