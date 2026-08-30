# 企业工作台管理员简要手册

本文面向企业 IAM 管理员和授权复核人，说明如何在帕乔利 AI 中维护用户、Agent、操作权限和数据权限。

> 权限管理入口与 Buzz 社区成员管理相互独立。加入 Buzz 频道不等于获得企业业务数据权限；企业工作台始终以 Business IAM 和业务系统的实时校验结果为准。

## 1. 管理前准备

管理员登录企业工作台后，从桌面端顶部打开 **Authority ledger（权限台账）**。管理面使用当前 Authentik OIDC 登录会话，不再要求 MFA 二次验证或单独 Step-up。

本地开发时还需启动独立的 `business-iam-admin-api`，并在桌面端构建环境中配置 `VITE_BUSINESS_IAM_ADMIN_URL=http://127.0.0.1:3111`。修改 `VITE_*` 变量后必须重新启动 Vite/Tauri；`3110` 是本地 `business-core` 的端口，不应再用于 IAM 管理 API。

管理员至少需要以下权限：

| 权限 | 用途 |
|---|---|
| `business_iam:read` | 查看用户、Agent、角色、能力和变更记录 |
| `business_iam:request` | 提交新增、授权、撤权或停用申请 |
| `business_iam:approve` | 复核并批准或驳回申请 |

系统采用“申请—审批—生效”流程。提交申请不会立即改变权限；每项变更需要 1 次审批，申请人可以审批自己的申请，同一人员不能对同一申请重复表决。申请 24 小时内未完成审批会过期。

## 2. 增加用户

增加用户分两步完成：先建立登录身份，再登记业务权限主体。

1. 在 Authentik 中创建或同步用户，并确认用户已映射为有效的企业用户。
2. 在 **Authority ledger → New request** 中选择 **Add or restore principal**。
3. **Principal type** 选择 **Human**。
4. 填写：
   - **External ID**：该用户在企业用户目录中的唯一 ID；当前系统使用企业用户 UUID，不要填写昵称或 Buzz 用户名。
   - **Display name**：用户显示名称。
   - **Business reason**：新增原因、岗位或工单号。
5. 点击 **Create review request**，由持有审批权限的管理员审批；申请人也可以自行审批。
6. 生效后，在 **Authority catalog → Principals** 中确认用户状态为 `active`。

仅创建主体不会自动赋予业务权限。用户离职或停用时，提交 **Disable principal**；停用会使后续鉴权失败，并撤销关联的有效 Agent 委托。

## 3. 用户授权

优先使用角色授权；仅在例外场景使用直接授权。

### 通过角色授权

1. 如尚无合适角色，在 **New request** 选择 **Add or restore role**，填写角色编码和名称并完成审批。
2. 选择 **Add permission to role**，为角色选择能力并配置数据范围，完成审批。
3. 选择 **Assign role**，选择用户和角色，填写业务原因并提交审批。
4. 在 **Authority catalog** 中核对用户角色及角色包含的能力。

### 直接授权

1. 在 **New request** 选择 **Grant direct permission**。
2. 选择用户和 **Capability（能力）**。
3. 配置数据范围及附加控制要求。
4. 填写业务原因，提交并完成审批。

撤权时分别使用 **Remove role**、**Revoke direct permission** 或 **Remove permission from role**。角色权限变更会影响该角色下的全部用户和 Agent，应先核对影响范围。

## 4. 增加 Agent

先判断 Agent 类型：

- **Independent Agent（独立 Agent）**：数字员工，持有自己的长期业务权限；需要在 Authority ledger 中登记主体。
- **Proxy Agent（代理 Agent）**：代表当前用户执行一次受限任务，不是 IAM 主体；不要在 Authority ledger 中为它创建主体或长期授权。

独立 Agent 的运行实例和业务权限主体是两个对象，必须分别创建。

### 创建运行实例

1. 打开桌面端 **Agents** 页面，点击新建 Agent。
2. 填写 Agent 名称，选择运行位置、Agent Runtime/Provider、模型及所需凭据。
3. 保存后启动或部署 Agent，确认状态正常，并记录其稳定的 Agent ID。
4. 生产业务 Agent 只应配置经批准的业务 MCP 工具；不要授予 Shell、文件系统、浏览器、SQL 或通用 HTTP 工具。

### 登记业务权限主体

1. 打开 **Authority ledger → New request**，选择 **Add or restore principal**。
2. **Principal type** 选择 **Independent Agent**。
3. **External ID** 填写运行实例使用的稳定 Agent ID；不要填写显示名称、模型名或临时 Turn ID。
4. 填写显示名称和业务原因，提交并完成审批。
5. 在 **Authority catalog → Principals** 中确认该 Agent 为 `active`。

## 5. Agent 授权

Independent Agent 的授权方式与用户相同，可分配角色或直接能力，但必须遵守最小权限原则：

1. 只授予 Agent 实际工具需要的能力，例如 `sales_order:read` 或 `inventory:read`。
2. 对每项能力设置最小数据范围，避免使用不受限范围。
3. 写入类能力按目录要求保留 **Human approval** 等附加控制；Business IAM 管理流程本身不再要求 Step-up 或双人复核。
4. 不向 Agent 授予确认、审批、过账、付款执行、冲销等超出其工具边界的能力。

Independent Agent 的有效权限是“Agent 自身长期权限与请求范围”的交集，不继承触发者或创建者的权限。Proxy Agent 的有效权限是“被代理用户当前权限与本次任务请求范围”的交集；它没有自己的长期角色或权限。两种模式都由业务系统再次校验，交集为空即拒绝。

停用 Agent 使用 **Disable principal**；撤销角色或能力后，系统会撤销相关有效委托。运行实例如不再使用，还应在 **Agents** 页面停止或移除，避免只停权限、不停运行。

## 6. 企业工作台操作授权

操作权限通过 `资源:动作` 形式的 Capability 控制，例如：

| 类型 | 示例 | 说明 |
|---|---|---|
| 查询 | `sales_order:read`、`inventory:read` | 查看对应业务对象 |
| 业务维护契约 | `sales_order:write`、`purchase_order:write` | 高风险能力目录记录；当前版本尚未开放自动执行 |
| 关键操作契约 | `inventory:adjust`、`payment:execute` | 关键能力目录记录；当前版本尚未开放自动执行 |
| 业务审批契约 | `business_approval:approve` | 高风险能力目录记录；当前版本尚未开放自动执行 |
| IAM 管理 | `business_iam:read/request/approve` | 查看、申请和复核权限变更 |

授权步骤：

1. 在 **Authority catalog → Capabilities** 查询系统已启用的能力、风险级别和附加控制。
2. 根据岗位选择角色授权或直接授权。
3. 明确区分 `read`、`create`、`write/adjust`、`approve/execute`；读取权限不包含新增、修改或审批权限。
4. 提交申请并完成 1 次审批；申请人可以审批自己的申请。
5. 使用对应用户登录企业工作台，验证可访问功能和越权拒绝结果。

能力目录是当前可授权范围的唯一依据。目录中没有的能力不能通过手工填写或自然语言指令获得；即使目录中已有写能力记录或已经授权，当前版本也没有自动业务执行接口，服务端会安全拒绝。未来开放写入需经过独立的产品与安全评审。

## 7. 数据授权

操作权限决定“能做什么”，数据权限决定“可以对哪些数据做”。授予能力时，在 **Data boundary** 中选择：

- **Capability default**：采用该能力的默认数据范围。仅在默认范围已经过安全评审时使用。
- **Restrict by dimension**：按维度限制，并填写允许值；生产授权优先使用此项。

常用维度如下：

| 维度 | 含义 | 示例值 |
|---|---|---|
| `legal_entity` | 法人主体 | 法人 UUID 或约定编码 |
| `business_unit` | 业务单元 | 业务单元 UUID 或约定编码 |
| `warehouse` | 仓库 | 仓库 UUID 或约定编码 |
| `customer` | 客户 | 客户 UUID 或约定编码 |
| `supplier` | 供应商 | 供应商 UUID 或约定编码 |
| `brand` | 品牌 | 品牌 UUID 或约定编码 |

操作方法：

1. 在授权申请中将 **Scope** 设为 **Restrict by dimension**。
2. 在 **Dimension** 中填写一个业务系统支持的维度名。
3. 在 **Allowed values** 中填写允许值，多个值用英文逗号分隔。
4. 如需多个维度共同约束，应按当前权限目录/管理接口支持的结构配置；桌面简表单一次只录入一个维度，不要把多个维度拼进同一字段。
5. 提交审批，并用目标身份验证列表查询、精确 ID 查询和写操作均不能越界。

业务系统实际执行时取“请求范围与授权范围的交集”。无权限对象和不存在对象对外返回相同结果，管理员不要通过错误信息判断对象是否存在。

## 8. 复核与日常检查

复核人在 **Review queue** 中检查主体、能力、数据范围、附加控制、业务原因、风险级别和目标版本，填写复核意见后选择 **Approve review** 或驳回。每项申请只需 1 次审批，申请人可以自批；同一人员不能对同一申请重复表决。

日常建议：

- 每月复核高风险权限、直接授权和不受限数据范围。
- 人员转岗时先撤旧角色，再授新角色；离职时立即停用主体。
- Agent 更换职责或运行实例后，重新核对稳定 Agent ID 与 IAM 主体的对应关系。
- 使用 **History** 和 `traceId` 追踪申请、审批和生效记录；审批和审计证据为追加式记录。
- 授权后同时做正向与反向验证：应允许的操作成功，应拒绝的跨法人、跨仓库或未授权动作失败。
