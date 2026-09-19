# 企业工作台独立代理接入核查 — 2026-09-17

## 结论

最新履约写入发布状态见 [2026-09-19 验收记录](2026-09-19-fulfillment-writes.md)，包含客户端待解锁边界。以下为此前分阶段记录。

截至 2026-09-19：独立企业助手已上线，当前身份签名绑定成功；用户授权的真实销售订单查询与可点击查询审计回执已经验收。草稿工具开关已启用、隔离环境写入测试通过，且已在用户逐项批准后完成一笔真实销售草稿的创建、数据库核对、审计与详情跳转验收。gpt-5.5 已验证可用，gpt-5.6-terra 复测未调用业务工具。以下保留分阶段证据与尚未完成事项。

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

### 解锁后完成客户端详情验收（2026-09-19）

用户回复“已解锁”后，通过已安装的 `/Applications/Pacioli.app`，依次点击企业助手查询回执内三条订单链接。Business Dock 均加载对应的系统只读详情：

- SO-202608-000003：ID `54a738b6-49ad-4c5b-9a08-6a16a0a119e2`，CNY 1.00，草稿，2026-08-29。
- SO-202608-000002：ID `0aaf1169-e62d-4411-a022-26dab61582d9`，CNY 0.00，草稿，2026-08-28。
- SO-202608-000001：ID `7e2008fc-ca9c-4aa8-9121-819e5d779c82`，CNY 1.00，草稿，2026-08-28。

连续切换链接时 URL、订单编号和金额同步更新，未残留上一单数据；此前 ID 搜索导致的空列表已消除。销售订单真实查询、查询审计回执、订单详情跳转现已完成实机验收。生产草稿写入仍待用户提供具体业务字段，未创建新单据。本段替代上一阶段“详情点击待解锁”的阻塞状态。

### 写入验收的具体候选已准备（2026-09-19）

检查当前 MCP 草稿 schema 确认销售创建需要法律主体、客户、业务单元、币种、日期，以及 SKU、仓库、计量单位、数量、单价。当前工具清单没有基础资料查找工具，不能宣称仅提供自然语言客户/商品名就已具备完整写入闭环。

通过真实工作台打开但未提交销售录入表单，核对现有选项：LE_CN_01 默认法人主体、CUS_DEFAULT 默认客户、BU_CN_01 默认业务单元、SKU_DEFAULT 默认商品 SKU、WH_DEFAULT 默认仓库、UOM_EA 件。再以已获授权查询返回的订单 `54a738b6-49ad-4c5b-9a08-6a16a0a119e2` 为范围，只读核对对应字段 ID。

已准备待用户批准的测试候选：2026-09-19，1 件 × CNY 1.00，折扣与税率 0，参考号 AGENT-WRITE-ACCEPTANCE-20260919，备注明确仅作草稿验收。输入保存在本机 `/tmp/business-draft-acceptance-20260919.json`，尚未发送创建聊天消息或调用写入 API。已向用户展示完整业务内容并询问是否允许发送创建消息及生成此草稿；不将先前只读查询许可扩展为这次真实写入许可。


### 真实销售草稿写入验收完成（2026-09-19 08:51 Asia/Shanghai）

用户明确回复“允许创建这笔测试草稿”后，通过 Pacioli 企业助手私聊发送一次完整字段创建请求，源消息 `b4e6abf13df9ed01bdad8b44c272a5bbb8fb090f298b90ab0574da63dfc6a901`。助手成功返回：

- 订单 SO-202609-000004，ID `f4c090c6-bede-402b-a02a-e99d2e4ef6d2`。
- Trace `a2b23a59-94a2-4929-9c38-5560bc4a34ab`。
- 明确声明仅创建草稿，未确认、未出库、未收款；附可点击系统详情链接。

生产只读核对：参考号 AGENT-WRITE-ACCEPTANCE-20260919 对应订单数量恰为 1；状态 draft、version 1，订单日期 2026-09-19，数量 1、单价 CNY 1、折扣 0、税率 0、总额 CNY 1，已出库数量 0，关联出库单数量 0。备注与批准内容一致。该 Trace 的授权、委托签发、工具调用、工具成功、回传、委托撤销六条审计均 success。没有通过重复发送创建消息来冒充幂等验证。

实际点击新订单链接后，Business Dock 打开对应 ID 的系统详情，显示 SO-202609-000004、CNY 1.00、草稿、2026-09-19、版本 1，与数据库和聊天回执一致。本次生产写入已验收，不再处于等待业务字段的状态。

完成范围：按当前账号权限的经营查询，以及已约定首阶段的业务草稿写入；本轮以真实销售订单创建完成端到端验收。其余五类草稿未逐一创建生产单据。聊天审批、通用更新/删除、付款、核销和记账未开放；本轮未把这些行为当作草稿写入成功。当前需要完整结构化业务字段，按客户/商品名称自动查询并消歧基础资料仍是后续改进。当前实际 Codex Host 的工具隔离也不等同于专用 buzz-agent 运行时，不声称已完成完整工具隔离上线。


### 按名称查找基础资料与补充字段验收完成（2026-09-19 09:11 Asia/Shanghai）

实现提交 `8661e1cff`、迁移验证提交 `12a0c0bd9` 已推送个人 origin。新增 search_business_master_data，支持法人主体、客户、供应商、业务单元、SKU、仓库、计量单位和品牌八类基础资料；按当前权限过滤后再做名称/代码查找及分页。助手使用查询结果中的 ID，不要求用户填写内部 UUID；遇到同名或缺失业务字段时集中询问，不从历史订单推断数量、单价、日期。

生产以原已部署源码为基线选择性移植该功能，未顺带发布聊天审批功能。部署配置 `/opt/business-platform/app/compose.master-search-20260919.yml`，源码 `/opt/business-platform/releases/master-search-20260919`，四个服务镜像为 `shiyue-business-master-{gateway,business-core,business-read-api,iam-admin-api}:20260919`。四个服务健康检查通过。迁移 0033 仅登记能力，不自动授予；通过现有审计管理 CLI 为当前使用者配置 business_master_data:read，限制到已有默认法人主体。旧服务镜像保留 before-20260919 备份；旧 Core 回退还需处理新迁移版本，不能只替换镜像。

本地验证包括 contracts、MCP、Read API、ACP 与 Gateway 定向测试、严格 Clippy，以及独立 PostgreSQL 闭环测试。数据库测试覆盖超过 200 条未授权资料前置、同名、稳定分页、字面量百分号、法人主体范围和新能力目录迁移；新迁移文件需触发 SQLx migrate 宏重新编译，最终测试明确验证目录存在一条且没有自动授予。兼容版客户端 Host 与 MCP 已构建，实际企业助手已重启加载。

用户明确批准后，通过已安装 Pacioli 私聊仅发送一次只读验收消息：“请按名称查找‘默认客户’和‘默认商品 SKU’，列出匹配项，并说明创建销售订单还缺哪些信息。不要创建或修改单据。”

- 源消息：`1bff62f07203c74a8b45ed71bdab9bbffa7846796ebb9c83cd1bcfae62c833a0`。
- Trace：`4398d07a-ce3b-448d-857a-452c520c70e9`。
- 实际调用 search_business_master_data 两次，每次成功返回一项：CUS_DEFAULT 默认客户、SKU_DEFAULT 默认商品 SKU，均 active。
- 回复集中列出尚缺法人主体、业务单元、币种、订单日期、仓库、计量单位、数量、单价、折扣、税率，并询问可选参考号/备注；未沿用上次测试订单的数量、价格和日期。
- 点击查询记录链接，Business Dock 打开对应系统审计页，八条处理轨迹均 SUCCESS。数据库审计也确认仅两次只读查询、成功回传、授权撤销。
- 验收前后生产销售订单均为 4 张，最大更新时间仍为 `2026-09-19 00:51:02.467213+00`；本轮未创建或修改单据。

本次真实验收覆盖客户与 SKU 查询、缺失字段说明和审计跳转；其余六类资料及按名称创建新草稿未新增生产写入验收。下一步建议用用户明确给出的完整业务需求验收按名称创建草稿。
