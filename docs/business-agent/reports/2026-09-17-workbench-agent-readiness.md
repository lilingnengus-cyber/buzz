# 企业工作台独立代理接入核查 — 2026-09-17

## 结论

截至 2026-09-19：独立企业助手已上线，当前身份签名绑定成功；用户授权的真实销售订单查询与可点击查询审计回执已经验收。草稿工具开关已启用、隔离环境写入测试通过，但尚缺用户提供的真实业务字段，生产写入未验收。gpt-5.5 已验证可用，gpt-5.6-terra 复测未调用业务工具。以下保留分阶段证据与尚未完成事项。

## 已核查的证据

- `crates/buzz-acp/src/business_agent.rs`：签名 Buzz 事件向授权网关换取每回合短期委托，注入专用 MCP；服务端凭据不进入提示词。
- `docs/business-agent/agent-delegation.md`：委托取当前用户权限与任务范围交集，绑定事件、频道、Agent、有效期与调用预算，结束时撤销。
- `services/business-read-api/src/config.rs`：业务 Core 独立配置和写入开关；Core 最终校验权限。
- `scripts/business-agent-runtime-acceptance.mjs`：已有专用运行时工具集合验收脚本。本轮未运行；本地缺少已构建的 business-read-mcp。
- 线上 Docker 只读检查：Business Gateway、Read API、Core 正在运行；这不等于接口或业务端到端验证通过。
- 线上 Gateway：READ=true、DRAFT_WRITE=true；Read API：DRAFT_WRITE=true、AUTHORIZATION=true。两者 CHAT_APPROVAL 均未设置，按仓库默认不开启。
- Relay 容器运行且 healthy；未完成用户此前报告的客户端连接故障诊断。

## 首批能力

查询：销售/采购订单、库存、应收/应付、订单利润、经营报表。数据真实性和时效以工具结果为准；不把验收参考数据当作企业真实数据。

草稿：销售订单、采购订单、出库、入库、客户收款、供应商付款。创建草稿不等于库存变更、资金收付或业务审批。

未开放：通用修改、删除、付款执行、核销、记账及聊天审批。现有销售/采购审批为绑定版本与预览 hash 的 `/approve` 协议，尚未满足用户要求的自然语言“确认”体验。后续须实现服务端可验证的确认绑定，不能只修改提示词。

## 上线前未完成项

1. 选择并配置独立 Agent 的运行主机、现有模型提供方和安全服务凭据注入。
2. 验证企业用户身份绑定、create 权限与实际业务数据来源。
3. 专用运行时工具隔离验收，以及真实客户端的查询、草稿、幂等和拒绝路径验收。
4. 核验 biz:// 详情跳转；后续接入聊天确认与统一入口分派。

下一步：先完成独立 Agent 部署及只读真实查询验收，再使用明确测试范围验收草稿。

## 2026-09-19 接续验证

发现已有 LaunchAgent `com.paqiaoli.business-agent.assistant` 引用了不存在的
`Paqiaoli-buzz-latest/target/release/buzz-acp` 与 `business-read-mcp`；其日志持续报文件不存在。
这是当前企业助手无法启动的已证实阻塞，而非业务 API 尚未实现。

本工作树已成功构建 buzz-acp、buzz-agent、business-read-mcp。
运行 `node scripts/business-agent-runtime-acceptance.mjs` 通过：模型看到 38 个固定 Business 工具，完整完成一轮本地桩模型请求。
`cargo test -p business-read-mcp --lib` 的 12 项测试全部通过，覆盖输入约束、草稿写入开关、错误与超时处理等。
这些测试不证明生产订单查询或草稿写入成功。

新增 `scripts/business-agent/run-workbench-agent.sh`，从现有私有 runtime.env 读取服务配置，显式隔离 Life 扩展，启用企业查询与草稿、禁用审批和 Action 执行。脚本语法检查通过。
尚未替换 LaunchAgent；需完成专用模型运行时配置后启动，并进行真实链路验收。

### 启动故障已修复

已备份并更新本机 `com.paqiaoli.business-agent.assistant` LaunchAgent，改用仓库启动脚本；补齐 launchd 的 Node PATH。沿用既有企业 Agent 身份及 Codex ACP 登录，模型维持原配置 gpt-5.5，运行模式 read-only；Business MCP 的服务端写权限独立控制。

已观察到 ACP 初始化、Relay 连接、频道订阅与 online presence。查询和草稿开关启用，Life 扩展、heartbeat 与核心记忆注入关闭。尚未发送验收消息或创建业务数据。

重要验证边界：此前 38 工具集合验收使用 buzz-agent 与桩模型；本机实际运行的是已有 Codex ACP，不能以该测试证明其内建工具完全隔离。已设置 read-only 模式，但仍需真实回合验证其专用 MCP 可用性。不能将“在线”当作查询或写入成功。

### 实际身份检查与数据库验收

2026-09-19 已在隔离 PostgreSQL 16（端口 55439，独立临时数据目录）执行：
- Gateway `agent_delegation`：1 项通过，覆盖签名委托、范围、预算、幂等和撤销。
- Core `postgres_b2` 与 `postgres_b3`：各 1 项通过，覆盖销售/采购数据库闭环、查询和并发行为。
- Read API library：14 项通过，覆盖用户隔离、严格输入、草稿转发与服务端幂等键。
测试后已停止临时 PostgreSQL；日志位于 `/tmp/pacioli-business-agent-validation/`。未触碰生产业务数据。

用户已明确授权在企业助手私聊发送“查询最近五笔销售订单”。发送前，客户端名录没有旧企业助手，按其公钥搜索也无匹配；运行日志显示旧 LaunchAgent 的 owner 为 `5e7a016b…`，与当前使用账号的既有身份不符。因此旧服务已 unload，测试消息未发送。不能沿用旧 runtime.env 的 Agent 身份完成当前账号验收。

当前真正剩余工作：为当前账号建立独立企业 Agent 身份，完成私有服务配置注入和账号绑定，再执行已授权查询测试及写入验收。上一节“已在线”只证明旧服务的连接状态，不证明当前用户可用。

### 当前账号企业助手已创建

已通过当前 Pacioli 客户端创建 `助理Agent_企业工作台`，公钥：
`eb6bb6a52686ab816ba63391dcf8af26818a68fd07e21652e3631f96f6bbb114`。
已配置 Business 专用开关与查询/草稿职责；此身份不同于已停用的旧企业服务身份。

Host 新增 `BUSINESS_READ_SERVICE_CREDENTIAL_FILE`，支持绝对路径、限制文件大小、Unix owner-only 权限、互斥凭据来源及长度/控制字符验证。新增 2 项测试及现有 Business Agent 9 项测试通过，buzz-acp 编译通过。新 Host 尚未替换客户端捆绑版本。

仍需完成：新身份的专用 Host 配置、私有凭据文件及网关绑定核验、已授权查询消息发送与真实草稿写入验收。客户端显示 Agent 在线不能证明这些项目完成。

### 新助手连接配置已保存

已通过 UI 给新企业助手保存网关/API 地址、凭据文件路径、专用 MCP 路径、草稿开关和 Life 隔离开关。服务凭据由线上网关配置写入当前客户端私有目录，目录 0700、文件 0600，未显示密钥。

启动日志已确认新助手 owner 为当前账号 `e7aaf087…`。此前失败原因为缺少 BUSINESS_READ_API_BASE_URL；客户端却残留 Online presence、Runtime 为 Stopped，启动按钮被禁用。测试消息仍未发送。

客户端实际执行 `/Users/aaronli/Projects/Paqiaoli-buzz-v0.5.23/target/release/buzz-acp`。为避免不同分支 Host 替换影响现有 Agent，尝试替换的二进制已恢复；正在当前发布源码上仅移植凭据文件加载改动并构建兼容 release。构建日志 `/tmp/business-compatible-host-build.log`。尚未完成兼容版本部署与新实例启动。

### 兼容 Host 已上线，查询已实际到达授权网关

兼容发布源码上的凭据测试通过，release 构建成功。修复启动配置校验在 online presence 之后的问题，现已在发布 online 之前校验扩展配置；部署至实际 release 路径及应用内副本。重载客户端后补齐企业助手的 LIFE_DOCK_ENABLED/LIFE_NOTIFIER_ENABLED/LIFE_CHAT_HIGH_RISK_WRITE_ENABLED=false，实例成功订阅企业私聊并在线。

已发送用户明确授权的“查询最近五笔销售订单”，频道 f3381e7c-9c65-4b31-bad9-5a7f7a2f6e69。网关 2026-09-18 16:36:53 UTC 审计返回 AGENT_TURN_REJECTED / binding_or_user_inactive，Trace dfc80c41-c14b-462a-b0d4-802cedbb5095。查询尚未执行；没有创建单据。

客户端企业工作台页面仍处于 authentik Default Admin 登录状态。源码表明 WorkbenchAuthProvider 仅调用 /api/me，未像 LifeDockProvider 那样发起当前 Nostr 身份的挑战签名绑定；下一步应核对当前公钥的实际绑定状态，并补齐经过本人签名的绑定流程，不能通过直接数据库插入绕过证明。

### 当前身份绑定修复与客户端候选（2026-09-19）

生产数据库只读核验：当前用户公钥 `e7aaf087…` 在 `buzz_identity_bindings` 中无记录；不是已有绑定被撤销。客户端现已实现经过当前 OIDC 账号校验的 challenge → 原生签名 → verify 流程，核对 issuer、subject、pubkey、audience 与有效期。已有 active 绑定不轮换；revoked 绑定不自动恢复；退出登录/签名中身份变化时不提交验证。并发登录刷新共享同一请求，旧请求结果不能覆盖新会话。

新增绑定测试 12 项通过；Workbench auth 测试总计 21 项通过；TypeScript 类型检查与相关文件 Biome 检查通过。另外修复 Business extension 缺少 begin_error_message 导致授权失败时聊天无回复，增加安全错误映射测试（不回传原始错误/秘密）。

已在当前安装版本对应源码 `/Users/aaronli/Projects/Paqiaoli-buzz-v0.5.23` 仅移植这些改动，构建桌面客户端与兼容 Host，并签名验证后安装至 `/Applications/Pacioli.app`。企业助手模型已改为现有操作手册要求的 `gpt-5.5`。旧客户端备份在 `~/Library/Application Support/com.shiyueshizi.pacioli/business-agent/Pacioli.before-binding-20260919.app`。本地候选使用 ad-hoc 签名，不是已公证分发包。

启动阻塞已经进程采样确认：主线程停在 SecretStore::probe → SecKeychainFindGenericPassword，macOS SecurityAgent 在等待钥匙串授权。Computer Use 明确禁止操作 SecurityAgent，已请用户在本机允许访问；没有读取/显示私钥，没有修改钥匙串访问控制或直接插入绑定。当前生产绑定仍未建立，查询未重发，生产写入未执行。日志：`/tmp/business-binding-desktop-build.log`、`/tmp/business-feedback-build.log`、`/tmp/business-binding-tests.log`。

### 钥匙串放行后，真实查询成功（2026-09-19 07:57 Asia/Shanghai）

用户确认已允许钥匙串访问后，采样确认旧进程不再卡在 Keychain。重启客户端后原生挑战签名成功：生产审计 `IDENTITY_BINDING_CREATED` / `IDENTITY_BINDING_VERIFIED`（2026-09-18 23:55:25 UTC）；当前绑定与企业用户均为 active。Business Dock 的 Continue SSO 成功恢复嵌入会话。

已在企业助手私聊重试用户授权的“查询最近五笔销售订单”。真实聊天回执返回三张草稿订单 SO-202608-000003、SO-202608-000002、SO-202608-000001，金额分别 CNY 1、0、1。查询 Trace `160a346b-9d3d-4def-b550-0b847680fcd4`；网关审计记录回合授权、委托签发、回复发布成功和回合结束撤销。Business Dock 销售列表亦显示 3 单、合计 CNY 2，与查询结果相符。

回执中的裸 biz:// URI 未渲染为可点击链接，已补充 Host 合约要求使用 Markdown 链接。兼容 Host 已构建，需重载企业助手并验证可点击回执。生产草稿写入仍未执行：已向用户询问要验收的真实单据与必填业务字段，不能把只读查询授权扩大为任意业务单据创建。

### 模型切换后的兼容性复测

用户操作客户端配置期间，企业助手模型从 gpt-5.5 变为 gpt-5.6-terra。重启后对同一已授权查询复测，08:01 聊天回复“当前会话未提供可用的业务查询工具”。本地 ACP 会话记录核验：07:57 的 gpt-5.5 回合有 tool_search_call 与 search_sales_orders；08:01 的 gpt-5.6-terra 回合无业务工具调用。没有把这个拒绝结果当作查询成功。已询问用户恢复已验证模型还是保留新模型继续修复。Fizz 已恢复 Online。

### 可点击回执与订单详情修复（2026-09-19 08:05 Asia/Shanghai）

在等待模型偏好回复后按已说明的默认选择恢复 gpt-5.5，并重启企业助手加载兼容 Host。查询返回三笔相同订单，Trace `cd6d25db-7881-4aa1-b066-f2d4fd177aee`，订单和查询记录均为可点击链接。点击第一笔订单后发现业务网页将 ID 当作列表搜索词，导致空列表。

已新增浏览器会话下销售/采购订单按 ID 读取的 GET 方法，使用现有读权限及经营主体、客户/供应商、业务单元范围约束；不存在与越权统一拒绝。详情页直接请求目标订单并复用系统原有详情展示，不依赖最近 200 条列表。提交 `028d0f4b4`、`1a5301089` 已推送个人 origin。

验证：独立临时 PostgreSQL 的 B2/B3 业务闭环测试均通过，新增按 ID 读取、缺失订单、客户/供应商范围撤销、业务单元范围撤销检查；14 项浏览器功能测试、28 项网页单元测试、网页类型/展示格式检查、Core Clippy 严格检查均通过。数据库测试首次复用了旧测试库而因 fixture 单例重复失败，随后改用全新隔离库通过；生产库未用于这些测试。线上镜像构建中，尚待部署和客户端再次点击验证。生产草稿仍未创建。

客户端另已点击该查询记录链接，Business Dock 正确打开 `/embed/agent-queries/cd6d25db-7881-4aa1-b066-f2d4fd177aee`：状态“已回传”，工具 search_sales_orders，结果数 3；授权、签发、工具调用、查询成功、回传、撤销共六步均显示 SUCCESS。

### 详情修复已发布，最终点击验收待解锁

Business Core 已切换到 `shiyue-business-core:order-detail-scoped-20260919`，健康检查通过；原镜像保留为 `shiyue-business-core:before-order-detail-20260919`。部署附加配置 `/opt/business-platform/app/compose.order-detail-20260919.yml`，构建源码 `/opt/business-platform/releases/order-detail-20260919`，未更换其他业务服务。

网页已通过正式发布脚本上线 `business-web-362c19628-167e2eafd904`，发布提交 `362c1962839e2a1fb2186204eaf37268b4e09789`，入口 JS SHA256 `49245152afd2860ddff4713e899e16fecafc76d6e3ac60375919cec3cc889300`；静态资源、IAM、Core 检查成功，保留旧静态树回滚指针。

发布后尝试客户端实机验收时，Computer Use 返回 Mac 已锁定且无法自动解锁，已请用户手动解锁。因此详情页面的线上客户端点击验收尚未完成；不能把发布健康检查当作详情交互验证。未读取浏览器令牌或绕过锁屏。
