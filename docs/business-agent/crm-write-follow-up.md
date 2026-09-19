# 企业助手 CRM 写入接入

## 当前 Core 实现（2026-09-20，尚未部署）

三类固定命令 create/update/followup 分别对应 crm_creation_intent、crm_update_intent、crm_followup_intent。创建商机不创建销售订单；修改保留法人和业务单元；跟进原子追加不可变记录并更新阶段和下一步。命令及嵌套字段拒绝未知输入，更新/跟进要求当前版本，创建不能携带 expectedVersion。

准备保存完整命令、当前商机及关联法人/业务单元/客户的必要标识、名称、状态和版本，不向 CRM 预览暴露无关财务主数据。意图有效期 30 分钟，触发器拒绝更新或删除。同一幂等键仅可重放相同命令和快照。确认只接受标准审批字段，绑定 v1 意图和完整 SHA-256 摘要，不能临时追加业务命令。

服务路径为 /v1/agent-crm-previews/{kind}、/v1/agent-crm-intents/{kind}、/v1/agent-approval-previews/crm/{kind}/{id} 和 /v1/agent-approvals/crm/{kind}/{id}，沿用现有服务认证。Core 要求当前 crm:manage、法人/业务单元及原客户和目标客户范围，关联对象须仍有效。审批复用角色、人数、本人审批、跨业务单元及附加认证策略，缺策略拒绝。

执行锁定商机和关联主数据后重新计算预览，并在锁定授权修订后检查当前权限；版本未变化但内容变化同样拒绝旧预览。业务写入、跟进、业务审计、成功幂等记录和审批 executed 状态在同一事务提交。迁移 0054 建立意图表、扩展审批/委托类型约束、登记六项 create/approve 能力；approve 保留 fresh_signed_chat_command，不自动授权或初始化策略。浏览器原命令幂等摘要格式保持兼容。

## 隔离验证

本机 PostgreSQL 55439 新库实际跑通创建 → 修改报价阶段 → 跟进成交，版本为 1/2/3，审批均 executed，销售订单数量不变。覆盖同键重放/换内容、不可变记录、未知输入、缺策略、错误摘要、确认夹带参数、客户撤权、过期、重复确认和拒绝不执行。

客户名称/更新时间变化导致预览失效，必须重新准备。跟进插入后注入异常，商机、跟进及业务审计全部回滚，审批标记 execution_failed。真实行锁等待期间修改商机内容但保留版本，完整快照比较仍拒绝执行；移除比较的负向控制错误执行并使测试失败，恢复后在新库通过。上一批撤权并发验证和共享迁移下 B2 销售/库存/退货/盘点闭环继续通过。

日志 /tmp/crm-intents-{initial,final,negative,restored,b2,clippy-final,size}.log。Core/Gateway 严格 Clippy、Rust 格式、差异及文件大小检查通过。上述为隔离 Core API/数据库证据，来源事件是合成值，不是真实签名聊天。

测试需显式设置 BUSINESS_CORE_CRM_TEST_DATABASE_URL 与 BUSINESS_CORE_DATABASE_URL 指向同一隔离新库，提供至少 32 字符的 BUSINESS_CORE_SERVICE_CREDENTIAL 和 BUSINESS_WEB_ORIGIN，然后运行 cargo test -p business-core --test postgres_crm。不设置测试库时跳过不算验收。

## 后续

Gateway 委托、Read API 范围与返回契约、六个固定准备/确认工具、Host 签名解析尚未接入；还需商机查找、分页、缺字段补问、直接详情链接、权限配置和配套部署，以及获准真实聊天验收。线上及已安装 Mac 仍为盘点 121 工具。本域进展不缩减主数据、费用、报表、行动、履约细节和纠错等完整业务目标。

## 2026-09-20 商机定位、分页详情与直接链接候选 123

新增 search_crm_opportunities/get_crm_opportunity，贯通严格查询契约、Core 服务读取、Read API 委托范围交集、MCP 和 Host/Gateway crm:read 能力。既有 IAM 目录已有 crm:read，本批不增加迁移或自动授权。Gateway 固定能力为 82、Host 普通回合能力为 51；CRM 准备/批准能力尚未接入。

搜索支持精确 ID、字面标题/公司/联系人、法人、业务单元、客户、阶段和到期日期；每页最多 20 条简短摘要及当前版本，Core 在分页前做用户范围过滤，Read API 保留源 nextOffset，包括当前页被委托范围全部过滤为空时。expectedAmountMinor 明示币种最小单位，CNY 100 为 1 元。

详情每页最多 3 条完整跟进，后续页必须携带首次版本；Core 锁定商机读取，防止同次详情与跟进历史混合版本，后续版本变化返回冲突。跟进 note 为业务读取所需，MCP 仅对此精确工具使用严格的跟进字段/类型/长度校验，最多 3 条、每条 4000 字；其他工具继续禁止 note，未知密钥字段仍拒绝。原始跟进内容保留，明确作为不可信业务数据而非指令。

资源链接 biz://crm-opportunity/{id} 映射至 /embed/crm/opportunities/{id}，网页同时支持独立 /crm/opportunities/{id}。使用既有 CrmPage 的初始 ID 打开详情，无需再次从列表选择。

验证：隔离 PostgreSQL CRM 全流程及查询场景通过，6 条跟进分两页 ID 无重复，3 条 4000 中文字笔记的完整 Core 返回小于 50 KiB，新增跟进后旧版本页被拒绝，越权查询/详情不可见；Read API 模拟上游验证身份、trace、源分页、过滤后空页和精确 ID 绑定。契约/Gateway/Read API/MCP 单元测试通过（11/8/40/15，随后新增的完整 CRM 笔记校验 2 项单独通过），Host 10 项、桌面链接 26 项通过；网页构建和独立/嵌入链接 2 项 Playwright 功能验收通过。原生 buzz-agent 运行探针证明实际加载 123 个固定工具并完成模拟模型回合，无线上业务调用。严格 Clippy、格式与差异/文件大小检查通过。

日志 /tmp/crm-reads-{unit-final,notes,postgres,host-final,clippy,runtime,links,web-build,web-functional,size}.log。此批未部署，线上及已安装 Mac 仍为 121 工具。下一步接入三个 CRM 准备工具、三个无参数签名确认工具与其 Read API 范围/结果校验，再完成整批发布和获准真实聊天；其他业务域保持未完成。

## 2026-09-20 CRM 准备、签名确认与按回合工具配置

六个固定 prepare/approve_crm_creation、update、followup 工具已贯通 Gateway、Host、MCP、Read API 和既有 Core 意图接口。准备请求固定操作类型，修改/跟进要求当前版本；确认工具无模型参数，仅使用签名来源事件绑定的意图、版本、完整预览哈希和决定。Read API 在持久化前校验委托范围，修改同时检查原客户及目标客户，清空客户不能绕过原范围；返回的实际对象 ID、版本和 trace 必须匹配，只有执行成功才报告实际商机资源链接。

静态目录达到 129，实际原生运行发现每会话最多 128 个工具。现由 Host 根据已验证的当回合委托设置 BUSINESS_AGENT_APPROVAL_SCOPE：普通回合不注册 approve 工具，确认回合只注册对应确认工具及读取工具。可见性不替代每次调用的委托消费和权限校验。原生 buzz-agent + MCP + 模拟模型探针已分别加载 95 和 58 个工具并完成回合；不是同时注册 129 个。所有确认配置的单元测试覆盖容量和唯一确认工具。Host/MCP 必须配套发布；旧 Host 未传此变量时只有普通工具，不能声称已支持新版确认流程。

验证：Read API→真实 Core HTTP→隔离 PostgreSQL 完成创建/修改/跟进版本 1/2/3、准备幂等、错误哈希、重复确认、越权准备和原客户范围拒绝，最终仅 1 个商机、1 条跟进、0 个销售订单。独立 Gateway PostgreSQL 测试使用真实签名事件验证三个 CRM 家族的确认/拒绝、文档 ID/版本/哈希/决定篡改拒绝；上述分层证据不等于真实客户端聊天验收。Gateway/Read API/MCP 单元测试 8/42/18 项、Host 10 项通过；严格 Clippy、Rust 格式、差异及文件大小检查通过。

日志：/tmp/crm-writes-postgres.log、/tmp/crm-writes-signatures.log、/tmp/crm-writes-unit-final.log、/tmp/crm-writes-profile-tests.log、/tmp/crm-writes-host-final.log、/tmp/crm-writes-runtime-{ordinary,approval}.log、/tmp/crm-writes-clippy-final.log、/tmp/crm-writes-size-final.log。本批未部署，未代发聊天，未新增线上业务记录。下一步准备迁移 0054、CRM 权限和审批策略的明确配置、服务端及配套客户端发布与回滚，再进行获准真实聊天和 Windows 验收。全业务目标其余范围仍未完成。
