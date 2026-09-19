# 基础资料助手写入接入

目标仍为客户、供应商、仓库、法人/业务单元，以及商品、SKU、品牌、分类、计量单位与换算等既有基础资料的创建、修改和启停。本文按阶段记录接入进度，最新状态见文末；MCP 和真实客户端链路尚未完成，本批源码未部署。

## 2026-09-20 已修复与证据

检查现有 CoreMasterDataService / ProductMasterService 发现：保存和状态变更在事务前读取权限，等待锁后仍复用旧快照；幂等重放直接返回历史对象；版本校验先于实际底表 UPDATE 等待。创建还会尝试重新插入已有父级范围授权。

现有对象先锁定明确枚举对应的底表行，再读取当前权限、范围及版本。授权 revision 共享锁保持至事务提交；历史创建/修改/状态重放同样校验当前对象权限与范围。创建在实际插入及可能的 FK 等待之后重新检查权限和父级范围，然后只授予新对象的创建者范围，不重新补写父级范围。会产生范围授权的创建直接取得 revision 排他锁，避免两个创建由共享锁升级排他锁的相互等待。既有命令幂等摘要和浏览器输入接口保持不变。

真实隔离 PostgreSQL 测试 postgres_master_authority 覆盖：

- 客户、品牌的修改与停用被真实行锁阻塞，期间分别撤销对象范围或管理能力，释放后拒绝，业务字段、状态和版本不变。
- 原创建重放在已撤权时拒绝；重放实际等待对象行锁时再次撤权也拒绝；恢复授权后正常幂等重放。
- 并发更新先提交后，旧版本修改拒绝，合法更新及状态变更成功，状态重放不增加版本。
- 客户创建被父法人真实 FK 锁阻塞，期间撤销法人范围，创建回滚且不恢复该范围。
- 品牌创建等待授权 revision 锁，持锁事务撤销管理权限后，创建回滚，无品牌记录。
- 两笔并发客户创建均成功；法人、业务单元、供应商和仓库创建各产生且仅产生自己的创建者范围。
- 最终仅 12 次获准创建/修改/状态写入产生业务审计，被拒绝的尝试和幂等重放不额外写入业务审计。

负向控制暂时移除最终权限判断，真实撤权用例出现错误执行并使断言失败；已恢复。最终新库 master_authority_creator_final 完整场景通过，Core 24 项单元测试、严格 Clippy、Rust 格式、差异与文件大小检查通过。数据库仅使用本机 55439 独立测试实例，没有线上业务写入。

日志 /tmp/master-authority-{creator-final,negative,restored,unit,clippy-complete,size}.log。运行集成测试必须显式设置 BUSINESS_CORE_MASTER_AUTHORITY_TEST_DATABASE_URL 指向新的隔离数据库；未设置时会跳过，不能算验证。

## 接入仍需完成

- 在不可变意图中绑定实际目标、完整字段、当前版本、父级状态及停用影响；修改必须保留未要求清空的原字段。
- 核对停用影响、父级启用状态与新业务引用之间的并发事务边界，当前权限修复不能替代这些业务校验。
- 为 Core 及 Product 两类资料实现固定准备、签名确认和可回读详情；不能开放任意表名、字段或自授管理员权限。
- 接入 Gateway 委托、Read API 范围交集、MCP、Host、名称定位、补问与详情链接，再完成权限策略配置、部署及真实客户端验收。

线上仍为 CRM 6239a7224 配套版本；本批修复未部署。CRM 的真实聊天/Windows 验收及其余完整业务域仍未完成。

## 2026-09-20 全部基础资料命令预览

新增闭合的 Create / Update / ChangeStatus 命令及两类只读服务入口：POST /v1/agent-core-master-previews、POST /v1/agent-product-master-previews。复用现有 Core 服务认证与用户上下文，不增加 Relay HTTP 面；尚未登记 Gateway/Read API/MCP 写入能力。覆盖法人、业务单元、客户、供应商、仓库，以及计量单位、分类、品牌、商品、SKU、单位换算，共 11 类。

预览锁定实际底表记录与依赖的父级，再复查当前权限/范围。返回完整当前记录、父级状态/版本/更新时间、原命令、实际生效字段、停用影响与 canExecute。没有创建对象 ID 的预览保持 documentId/current 为 null。重复编码/换算、旧版本、额外字段、对当前业务接口实际上不可变字段的修改均拒绝。类型不适用的字段不会被静默忽略；省略客户额度/账期等值产生的 0/30 默认值以及 null 清空在 effectiveFields 明示。换算系数精确返回字符串，拒绝超出 NUMERIC(24,8) 或需要舍入的输入，允许仅多余尾零的等值表示。

停用影响查询改为调用事务的同一连接，并把所有影响计数合并为一条 SQL，以单条语句快照汇总；工作台既有启停操作也复用该实现。预览的 canExecute 只说明当次检查结果，不是执行授权或保证；后续确认仍须在执行事务内重算并绑定快照，处理影响对象并发变化。当前没有持久化意图、审批投票或 Agent 实际写入入口。

真实隔离 PostgreSQL/HTTP 验证：11 类资料分别预览创建、修改、停用；重复预览稳定，父级版本/更新时间变化在子项当前记录完全不变时仍改变父级快照；停用父级出现真实活动子项阻塞。越权范围、旧版本、不可变编号、类型不适用字段、重复编码/换算均拒绝。父级真实行锁等待期间撤销品牌范围，预览在释放后拒绝。HTTP 无凭据 401、范围越权 404、额外外层字段 422，正常结果保持 trace；0.33333333 换算系数按精确字符串返回。预览没有新增业务记录/业务审计或改变 Core 资料版本。

负向控制移除父级 FOR SHARE，真实锁等待断言失败；恢复后完整数据库及 HTTP 场景通过。前批事务权限回归通过、Core 单元测试 25 项、严格 Clippy、格式、差异及文件大小门禁通过。日志 /tmp/master-previews-{restored,negative-lock,authority-regression,unit-final,clippy-complete,size-final}.log。数据库在独立 55439 实例的新库，未调用生产业务写入。运行预览集成测试需同时提供 BUSINESS_CORE_MASTER_PREVIEW_TEST_DATABASE_URL 与 BUSINESS_CORE_DATABASE_URL 指向同一隔离新库、至少 32 字符的 BUSINESS_CORE_SERVICE_CREDENTIAL 及 BUSINESS_WEB_ORIGIN；未配置而跳过不算验收。

本批未部署。下一步把确定的命令/当前记录/父级及影响快照保存为不可变意图，接入执行事务中的快照复核、审批与幂等结果，再实现固定助手工具和详情链接。其他完整业务域、CRM 真实聊天与 Windows 验收保持未完成。

## 2026-09-20 创建与修改的事务内快照复核

Core 与 Product 服务新增 save_guarded，复用原有保存事务和写入逻辑。在目标锁及父级锁内重算完整预览，与传入快照逐项比较；变化返回 StalePreview，旧版本仍返回 VersionConflict，当前权限或范围缺失仍拒绝。创建/修改的幂等摘要绑定命令与快照，并与普通工作台保存摘要隔离。成功后的重放返回原记录和 trace，但继续校验当前权限；拒绝会回滚幂等占位。普通工作台 save 的摘要与调用接口保持兼容。

这是领域一致性入口，不是审批授权。没有新增 HTTP 写入接口或助手工具，没有审批投票和审批状态原子提交。save_guarded 明确拒绝 ChangeStatus；启停尚需完成与并发业务引用的事务保护，不能仅凭预览或旧的影响计数放行。创建/修改快照包含当时的影响信息，但本批不承诺冻结后续并发业务引用。

隔离新库 master_guarded_final 验证 11 类资料各创建、修改一次，共 22 次业务审计；重复执行返回原结果且无新增审计。修改快照后复用幂等键拒绝；错快照拒绝后使用同键及正确快照可成功，证明占位已回滚。旧版本、撤销客户/品牌范围及 guarded 启停均拒绝。客户与 SKU 执行真实等待父级行锁，父级版本变更提交后拒绝旧快照，未增加业务审计。预览与 HTTP 只读断言先于这些执行用例独立完成。

负向控制临时关闭 Core 快照比较，错快照用例错误执行并导致测试失败；已恢复且最终新库完整通过。既有事务权限集成回归通过，Core 25 项单元测试通过，严格 Clippy、格式、差异和文件大小检查通过。日志 /tmp/master-guard-{final,authority,unit,negative,clippy-final,size}.log。本批未部署，未发送聊天消息，未写入生产业务记录。

下一步接入不可变意图与实际审批策略，将审批执行完成标记与 save_guarded 的业务写入原子提交；全局基础资料不得伪造法人范围。完成后再登记 Gateway、Read API、MCP 与 Host 的固定准备/确认能力，并补足启停的并发引用保护。

## 2026-09-20 不可变意图与原子审批执行

新增迁移 0055 和四个固定意图类型：core_master_creation_intent、core_master_update_intent、product_master_creation_intent、product_master_update_intent。意图持久化严格反序列化后的完整命令与预览、创建人、幂等键、trace，30 分钟有效且数据库禁止修改/删除。准备的预览读取与意图插入也使用同一事务。迁移只登记八个 IAM 能力定义及审批类型约束，不自动授权、不配置审批策略；本批没有启停意图。

新增服务认证下的 Core 路径：POST /v1/agent-master-intents/{kind}、GET /v1/agent-approval-previews/master/{kind}/{id}、POST /v1/agent-approvals/master/{kind}/{id}。确认仍使用现有 ChatApprovalInput 的版本、摘要、决定与来源事件/频道绑定。没有新增 Relay HTTP API、界面按钮或生产业务写入。

本类审批复用现有审批请求/不可变投票表，但将投票、基础资料 save_on、幂等结果、创建者新对象范围、业务审计及 executed 状态放在一个事务中；没有先提交 executing 再另开保存事务的间隙。当前策略缺失/停用、角色不符、自审禁止、需要 step-up 均拒绝。人数取请求已记录值和当前策略值中的较大值；每次通过票都重新验证此前赞成者的现有角色、能力、范围与原预览。要求业务单元不同的策略在任一方没有业务单元时拒绝，全局资料不伪造法人或业务单元。

目标 advisory 锁顺序与普通保存一致，再锁实际记录、父级及授权 revision。审批另锁当前策略、人员、角色和实际能力来源；Core 角色权限以及 Core 原本支持导入的无附加义务、unrestricted IAM 直接/角色授权分别校验并锁定。有限期 IAM 来源在全部写入和等待之后再次用数据库时间检查；意图有效期同样在提交前检查。保存与预览内部权限读取改为使用已有事务连接，单连接池也可完成此流程。既有浏览器保存接口和幂等摘要保持兼容。

隔离 HTTP/PostgreSQL 用例 postgres_master_intents 覆盖：

- 11 类资料全部准备、确认创建及修改（另含一条换算用单位，共 23 个初始成功审批/业务写入）；同键准备重放，重复确认不重复写入，两个并发确认只产生一条新记录和一票。
- 全局法人创建没有虚构的 legalEntityId；缺策略拒绝且不产生审批请求。不可变输入、删除、额外字段、错误摘要/版本、过期意图和未开放的启停类型拒绝。
- 自审、角色、step-up、业务单元区分、对象范围拒绝；两人审批中撤销此前投票者角色后拒绝执行，恢复后可继续；策略提升到三人立即生效，随后降低仍保留三人的请求门槛；拒绝票终结请求。
- 客户确认真实等待父法人行锁时，策略改为要求 step-up，释放后拒绝且客户版本/票数不变。
- 在业务插入之后人为使最终审批更新失败，业务记录、范围授权、请求、投票及审计全部回滚；移除故障后可使用同一来源事件重试成功。
- IAM 直接权限及 IAM 角色权限都可按现有 Core 语义授权。有限期 IAM 角色授权在最后审批更新的真实锁等待期间过期，释放后全事务回滚。临时移除提交前检查时该用例错误返回 executed 并使测试失败；检查已恢复。
- 最大连接数为 1 的独立连接池完整准备/确认成功，防止事务中再次借连接造成自等待。

最后集成新库 master_intents_final、基础资料预览/保存回归 master_intents_preview_regression、权限回归 master_intents_authority_regression、CRM 回归 master_intents_crm_regression 均在本机独立 55439 实例。运行新测试需显式配置 BUSINESS_CORE_MASTER_INTENT_TEST_DATABASE_URL 和 BUSINESS_CORE_DATABASE_URL 指向同一个隔离新库，以及服务凭据、BUSINESS_WEB_ORIGIN；未配置而跳过不是验收。日志 /tmp/master-intents-{final,previews,authority,crm,negative,unit,clippy-final,size}.log。

本批是 Core 服务侧闭环，未部署；Gateway 尚不签发这些能力，Read API、MCP、Host、名称定位补问、部分字段保留与详情链接仍需接入。创建目前沿用既有保存语义，由执行审批者获得新对象范围；跨人审批中申请者的结果可见性需在接入回读前明确并验证。启停并发引用保护、CRM 真实聊天/Windows 验收及完整业务清单其他未覆盖环节仍未完成。

最终验证：上述四组隔离数据库集成测试均通过；Core 25 项、Gateway 8 项单元测试通过；两包严格 Clippy、Rust 格式、差异及仓库文件大小门禁通过。生产仍未应用迁移 0055。

## 2026-09-20 Read API 字段合并、范围交集与签名委托

Read API 新增八个固定写入路由名称：prepare/approve_core_master_creation、prepare/approve_core_master_update、prepare/approve_product_master_creation、prepare/approve_product_master_update；以及只读 get_business_master_record。Core 配套只读路径为 /v1/agent-core-master-records/{resource_type}/{id}、/v1/agent-product-master-records/{resource_type}/{id}，要求当前读取或管理权限及实际对象范围，返回完整当前记录/版本与请求 trace。

创建使用闭合的 Core/Product 结构；修改输入固定为 resourceType、documentId、expectedVersion、changes。Read API 从 Core 读取当前记录，检查委托范围与当前版本，再仅合并明确出现的 changes 字段，形成不可变完整命令。不可变编码、父级、所属主体、状态及与类型无关的字段不能通过 changes 修改。registrationNumber/address/barcode 可显式传 null 清空；未出现的字段全部保留。账期、额度、布尔选项与换算系数不会因模型省略而重置。准备之后再次比对 Core 返回的快照，确认从签名委托上下文注入来源事件/频道，返回结果核对目标类型、ID、版本、状态、编码（创建换算时编码由 Core 派生）与 trace。

范围规则不借助空字段放宽授权：创建新客户/供应商/仓库不能用限定于某个既有往来方/仓库 ID 的授权代替；可使用明确父法人/业务单元范围。新法人、品牌、计量单位、分类没有对应的现有对象 ID，因此不能用不相干的法人或品牌范围代替其所需权限。商品/SKU/换算按实际父级推导的品牌检查；既有记录按当前真实维度匹配。相同维度的多个别名拒绝，防止将两个限制值意外并成更宽的写入范围。这些规则应用于新写入路由和完整资料读取，不改既有只返回简要元数据的查找行为。

Gateway 固定能力集合从 88 增至 96；Host 普通回合从 54 增至 58，增加的四项均为准备意图的 create 能力，普通回合没有 approve。四种“确认/拒绝 … v1 摘要”解析均映射到精确意图家族。现有签名、当前身份绑定、来源频道、目标/版本/摘要/决定验证继续生效；没有自动给任何生产用户授权。

真实隔离验证：

- master_adapter_final 新库执行 Read API → 真实 Core HTTP → PostgreSQL，覆盖 11 类资料的创建和修改；另有换算用单位和显式清空 SKU 条码，共 24 次执行、24 票、24 次业务写入审计。
- 只改名称时保留法人登记号、客户额度/账期、供应商账期、仓库地址、商品零成本选项、SKU 条码；换算修改保留使用范围。数据库回读证明客户额度 34567/账期 45、原仓库地址及零成本选项保留。明确 null 清空条码成功，名称保留。
- 范围不符时准备不落意图，确认不执行；旧版本拒绝。准备之后撤销品牌范围，确认拒绝且条码未改；恢复范围后同一确认执行成功。完整资料读取返回当前版本，错误品牌范围拒绝。
- 临时恢复“空维度可绕过限制”的行为，原本应拒绝的受限创建错误返回 200，集成测试失败；已恢复严格匹配，最终新库通过。三个单元用例覆盖字段白名单/明确清空、空维度和重复别名。
- master_gateway_v1 新库验证 58 项普通能力容量，以及四个新增意图的签名确认/拒绝；替换目标、版本、摘要或决定均拒绝。Host 10 项定向测试通过。
- Core/Gateway 单元测试通过；Read API 测试集在更新固定清单计数后通过。该测试集未配置的其他数据库用例会自行跳过，不作为额外数据库验收证据。四包严格 Clippy、格式、差异与文件大小门禁通过。

日志 /tmp/master-adapter-{final,negative,unit,unit-restored,clippy-final,size}.log、/tmp/master-gateway-test.log、/tmp/master-host-test.log。新库仍仅使用独立 55439 PostgreSQL；没有生产写入、聊天消息或客户端替换。

当前 source 已接到 Read API、Gateway 和 Host，MCP 还没有登记这些新工具；Read API 暂不返回虚构的详情 URI。下一步同时补齐 MCP 输入结构、业务字段返回白名单（现有通用过滤会拒绝 address 等合法基础资料字段）、当前记录读取工具、产品/分类/换算的名称定位及真实客户端详情路由，再联调完整工具回合与配套发布。跨人审批创建后的申请者可见性、启停并发保护和其余完整业务范围继续保持未完成。

## 2026-09-20 MCP 工具与结果校验

MCP 新增 get_business_master_record 和 Core/Product 创建、修改各一组准备/确认工具，共九个。输入限定 11 类资源；修改使用严格补丁，省略字段不会变成 null，只有 registrationNumber/address/barcode 支持显式清空。审批工具不接受模型控制的参数，沿用签名来源的目标、版本、摘要和决定。

新增独立返回白名单，允许 warehouse.address 等必要维护字段，限制长度、类型、嵌套、完整信封、资源家族、版本、trace 和审批结果一致性。准备响应重新计算预览 SHA-256，并检查返回确认/拒绝文本。通用敏感字段过滤保持生效。尚无真实详情路由，因此返回引用必须为空，不虚构详情链接。

验证证据：
- 新库 master_mcp_adapter_v1（独立 PostgreSQL 55439）执行 Read API → Core HTTP → PostgreSQL，原有 24 次执行/票/审计断言通过。
- 隔离适配器测试可通过 BUSINESS_MASTER_MCP_FIXTURE_FILE 导出 JSONL 响应；本次 /tmp/master-mcp-corpus-v1.jsonl 的 47 条创建、修改、审批和读取响应全部通过 MCP 校验。MCP 同名环境变量消费语料；未设置的跳过不算端到端证据。
- MCP 23 项测试通过，含修改省略/清空区别、非法字段、错误签名绑定、预览篡改、敏感字段注入、零参数确认和全部会话工具容量。
- 使用新建 debug MCP 与 buzz-agent、模拟模型执行运行时探针：普通会话 100 工具；四类签名确认会话分别 59 工具，均只暴露指定的一个审批工具。这不是实际客户端聊天验收。
- Read API/MCP 严格 Clippy、格式、差异与文件大小门禁通过。日志 /tmp/master-mcp-{adapter,final,clippy-final,size,runtime}.log 与 runtime-* 日志。

本批未部署、未替换客户端、未发送聊天、未创建生产记录。下一步补齐产品/分类/换算名称查找、真实详情路由和字段补问后配套发布。还需检查单位精度省略时的预览默认值、跨人审批后的申请者可见性及启停并发保护；全业务流程目标继续未完成。

## 2026-09-20 产品、分类和换算名称定位

search_business_master_data 扩展为全部 11 类基础资料，新增 product、product_category、uom_conversion，继续使用现有固定工具和 Core 查询路径。迁移 0056 保留原目录字段及全部原有分类，加入单位换算；换算 code/name 由产品与换算单位组成，品牌范围取所属产品，不增加授权。查找返回的 UUID 可继续用于 get_business_master_record，再准备版本绑定的修改。

联调发现通用标识符过滤会拒绝换算名称中的 `/`。基础资料名称现在按有界字面文本处理（最长 128 字符、拒绝空白及控制字符），Core 的绑定参数 strpos 查询保持不变；百分号、下划线不作为通配符，其他查询工具不受影响。

单位精度核对结论：Core 已要求显式传入 0–6，省略会被拒绝，不存在可执行的“预览 null、落库默认值”路径。撤回默认值改动，MCP 字段说明明确要求缺少时询问；增加缺少、负数、边界与超界测试。此前提及该问题待查的条目已完成核查。

验证：新库 master_lookup_final 在独立 55439 PostgreSQL 运行 Read API → Core HTTP → 数据库，24 次原有写入闭环及新增三类名称/代码定位、完整记录读取、分页、百分号字面查询、错误委托品牌、撤销 Core 品牌范围、停用换算过滤全部通过。初次联调失败来自名称过滤及对单位精度默认值的错误假设，已修正，未将失败运行计为通过。查询合同 13 项、MCP 23 项（含重新生成的 47 条响应）、Core 26 项单元测试通过；四包严格 Clippy、格式、差异及文件大小门禁通过。日志 /tmp/master-lookup-{final,unit,core-unit,clippy,size}.log。

未部署或执行真实聊天，迁移 0055/0056 仍待配套发布。下一步完成系统内真实基础资料详情路由及返回链接，并继续跨人审批可见性、启停并发引用保护和其余业务流程。

## 2026-09-20 系统详情页与记录链接

11 类资料使用固定 URI `biz://master-data/{resourceType}/{uuid}`，桌面解析为 `/embed/master-data/{resourceType}/{uuid}`，支持复制引用和业务链接识别；限定类型与 UUID，结构化资源的 metadata、ID、路径必须一致。查询合同同步白名单。Read API 的完整记录读取及审批已执行结果返回该记录链接；尚未执行的准备、待审批、拒绝不返回虚构记录链接。MCP 将链接的类型、ID、标题和 URI 与返回记录严格比对。

企业工作台新增复用现有布局的只读详情页，支持嵌入和独立路径，显示代码、状态、版本、归属及适用字段；换算系数保留完整精度，信用额度使用金额格式函数。未知类型或非法 UUID 不发起读取。Core 在现有浏览器基础资料路径增加 GET，复用完整记录的读取/管理权限与对象范围检查；不增加修改按钮，也不改变确认操作。

验证证据：
- `master_links_final` 隔离 PostgreSQL（55439）：原有 24 次写入闭环与名称定位通过；为同一测试用户建立真实数据库登录会话，11 类 browser GET 全部读取正确记录，无 Cookie 与工作台会话过期均返回 401。
- 47 条最新真实 API 响应通过 MCP 校验；查询合同 14 项和 MCP 23 项测试通过，错目标链接、非法路径等拒绝。
- 桌面链接解析 27 项通过；Desktop TypeScript 检查通过。
- 浏览器 12 项功能测试通过：11 类直接打开指定资料且只发 GET；独立详情路径拒绝访问时显示错误并不泄露记录。这些浏览器测试使用接口夹具，不替代前述真实 API 验证，也不等于已安装客户端聊天验收。
- Web 类型检查、展示格式巡检、构建、四包严格 Clippy、差异和文件大小门禁通过。日志 `/tmp/master-links-{final-db,final-corpus,resolver-final,ui-final,desktop-ts-final,web-check-final,web-final,clippy-complete,size-final}.log`。

本批仍未部署或替换已安装客户端。下一步检查跨人审批创建后的申请者可见性及发布配置，配套发布 Core/Read API/Gateway/MCP/Web/桌面，随后验证真实聊天链接；启停并发保护和剩余完整业务流程仍未完成。

## 2026-09-20 跨人审批后的申请者范围

确认原实现只为实际执行保存的审批人授予新对象范围，申请人可能无法读取创建结果。现在跨人创建达到执行门槛时，在现有审批事务内锁定并重新验证申请人的当前管理权限（含 IAM 权限有效期），重算其父级与对象预览并与不可变快照比较。保存后，仅为申请人授予本次新建的法定主体、业务单元、客户、供应商、仓库或品牌范围；产品、SKU、换算等沿用已有父级范围，不增加其他范围。授权的 granted_by 记录实际审批人，并记录 requester、审批请求与新对象关联审计。

新对象授权、保存、票据与执行完成状态在同一事务提交，最终有效期检查仍在全部写入及等待后执行。申请人已停用、权限撤销、父级范围失效时拒绝执行并回滚；不会恢复旧父级授权。更新操作不增加范围。此次只解决 Core 新对象范围，Gateway/IAM 的既有读取委托仍限制可读对象，不为打开链接自动扩大 IAM 权限。

隔离新库 master_requester_checked（独立 55439）完整 postgres_master_intents 通过：品牌及五类 Core 资料由其他人审批后，申请人与审批人的完整记录读取均成功；授权审计归属正确。申请人管理角色撤销或客户创建父级业务单元范围撤销后，执行拒绝，业务记录、对象范围、审批票和审计数量不增加。原有阈值、旧审批人重验、并发等待、事务回滚和 IAM 到期测试继续通过。Core 严格 Clippy、格式、差异及文件大小门禁通过，日志 /tmp/master-requester-{checked,clippy-final,size}.log。

本批未部署。下一步准备基础资料版本的配套发布、权限配置与回滚方案，再验证真实客户端写入及结果链接；启停并发引用保护和其他业务域的剩余流程继续未完成。

## 2026-09-20 法人资料与商品读取权限分离

生产预检查发现 business_master_data:read 的现有 IAM 授权限定法人，不能用于不带法人维度的商品完整记录。新增固定工具 get_business_product_master_record，使用独立 business_product_master:read，输入只允许六类商品资料。Read API 检查能力与资源家族，拒绝使用商品工具读取客户等 Core 资料；Core 仍检查用户的读取/管理权限及真实品牌范围。既有工具、法人读取授权和严格范围交集不放宽。

迁移 0057 只登记该低风险读取能力，不自动授权。Gateway 范围白名单增至 97，普通 Host 请求 59 项能力（只读 17），MCP 普通会话 101 工具、指定确认会话 60 工具。该变更尚未进入 ddecf9c0e 服务端候选或本地已安装 Host/MCP，发布必须换成包含本批的完整版本。

独立 55439 新库 master_product_read_v1 的真实 Read API/Core/PostgreSQL 闭环通过：六类商品记录通过专用读取工具获得正确记录；保留法人范围时仍拒绝全局资料；商品能力读取客户拒绝；原有 24 次业务写入、名称查找、浏览器会话读取和权限校验继续通过。53 条真实响应通过 MCP 校验。另在 master_product_gateway_retry 新库通过 Gateway 完整签名委托/撤销集成测试；MCP 23 项、Host 10 项、四包严格 Clippy、文件大小门禁通过。使用已安装 buzz-agent 和新 debug MCP 的模拟模型原生回合证实 101/60 工具，不等同真实聊天。

中途本机空间耗尽，失败的编译与未创建成功的测试库未计为通过。通过 Cargo 标准 `cargo clean --profile dev` 仅清理可再生成的开发构建产物，恢复约 25 GB 空间，保留发布二进制、源码、日志和数据库；后续关闭增量缓存重跑成功。先前准备的两份临时源码副本仍保留。日志 /tmp/master-product-read-{db,gateway-retry,mcp-retry,host-retry,clippy-retry,runtime,runtime-approval,size}.log。

下一步按新能力制定副本授权方案，重建最终配套版本并演练迁移 57、暂停回滚及客户端文件。生产没有迁移、扩权或切换。

## 启停引用并发：下一项可复现缺口

基础资料创建/修改已配套上线，当前状态以 e51 发布记录为准；上述“未部署”段落是历史阶段记录。助手启停工具仍未开放。

代码核对发现销售草稿的 `b2/sales.rs::validate_order_master_data` 在同一事务读取客户、业务单元、仓库、SKU 和产品状态，却未在这些读取上持有共享行锁。客户/业务单元等停用入口会锁资料底表并统计未完成订单，但普通 FK 的 key-share 保护不足以阻止非键状态更新。因此需要验证“草稿已读 active、停用检查尚无新订单、随后两方提交”的交错；当前这是由代码推导的竞争风险，尚未通过并发测试证明。

下一步使用真实 PostgreSQL 等待观测复现，先验证停用先提交时新草稿被拒绝、草稿先持锁提交时停用因未完成订单被拒绝，再将协议扩展到采购、库存和其他引用入口。不能仅添加助手 status 工具或仅重复静态影响计数，就宣称启停全流程安全。

## 客户停用与销售草稿竞争：复现与首项修复

新增 `postgres_master_order_status` 真实数据库回归：另一事务先锁住客户并设置 disabled，销售创建请求开始后，通过 pg_blocking_pids 确认真正等待；提交停用后检查创建结果。未修复版本在隔离库 master_order_status_before 中错误创建成功，回归按预期失败。销售客户校验增加 FOR SHARE 后，master_order_status_after/control 中请求在等待后返回 NotFoundOrForbidden，销售订单数为 0；恢复 active 后正常创建成功。该共享校验路径也用于草稿更新，但本次并发测试直接覆盖创建。

完整 postgres_b2 在新库 master_order_status_b2 通过；Core 库及新增测试严格 Clippy 通过，格式和差异检查通过。日志 `/tmp/master-order-status-{before,after,control,b2,clippy}.log`，数据库均在独立 55439。

本批尚未部署，也不开放助手启停。反向交错（订单先持锁，停用后检查）、其他资料类型、采购/库存引用及状态意图接入仍需继续完成，不能由客户创建的单项回归推断全量启停安全。Windows 运行 35471308524 在本轮最后核对时仍执行 Build sidecars，未重复触发。

## 客户停用反向交错验证

回归进一步通过 sales_orders 的事务表锁停住真实草稿插入，并用 pg_blocking_pids 获取该业务事务 PID；随后调用真实 Core change_status，确认它等待销售事务持有的客户共享锁。放行订单插入后，销售成功，停用返回明确的 blocking operational impacts，客户仍 active，订单恰为一笔。

首次反向测试误用固定 expected_version=1，而前面的直接状态切换已触发版本递增，导致版本冲突；改为读取当前版本后，在新隔离库 master_order_status_reverse_v2 通过，证明命中的是业务影响保护。两个方向都在同一回归内执行，严格 Clippy、格式和 diff 检查通过。日志 `/tmp/master-order-status-reverse-v2.log` 与 `/tmp/master-order-status-reverse-clippy.log`。

这补齐客户创建路径的双向证据，其他资料及写入入口仍未补齐，尚未部署或开放助手启停。Windows 同一运行 35471308524 仍在 Build sidecars。

## 销售草稿引用保护扩展

五类资料等待测试已扩展到客户、业务单元、仓库、SKU、产品。未修复版本在 master_order_refs_before 中复现业务单元已停用仍返回成功草稿。业务单元校验改为实际行的 FOR SHARE，仓库/SKU/产品联查增加 FOR SHARE OF w,s,p，直到草稿事务提交才释放。该公共校验也用于草稿更新。

master_order_refs_after 中五类停用先提交场景均在真实行锁等待后返回 NotFoundOrForbidden，无订单残留；客户的订单先提交反向场景继续通过。master_order_refs_b2 的完整 B2 闭环/并发回归与 Core/新增测试严格 Clippy 通过。日志 `/tmp/master-order-refs-{before,after,b2,clippy}.log`。

尚未部署；不能据此宣称所有状态引用都受保护。法人、计量单位、品牌/分类及明细其他归属、采购/库存等入口仍需核对；其余资料的反向业务阻塞与助手状态意图仍待完成。

## 销售基础资料父级状态校验

新增法人、基础计量单位、产品分类、产品品牌的 active 校验和共享锁，持续到销售草稿事务结束；无品牌的产品仍允许使用。原实现未校验这些父级状态，master_order_parents_before 已复现法人停用提交后仍创建草稿。

新库 master_order_parents_after 中九类资料（客户、业务单元、仓库、SKU、产品、法人、计量单位、分类、品牌）的停用等待场景全部拒绝且无订单残留，客户反向交错继续通过。master_order_parents_b2 的完整 B2 回归和严格 Clippy 通过，日志 `/tmp/master-order-parents-{before,after,b2,clippy}.log`。

尚未部署。销售明细覆盖字段的额外归属、采购/库存等入口和状态意图仍需继续核对，不能用本次九类创建引用测试代替所有业务操作验证。Windows 运行 35471308524 最后检查仍在原生 sidecar 编译。

## 采购草稿引用保护

真实 PostgreSQL 回归 master_purchase_status_before 复现供应商在等待期间停用后仍创建采购草稿。采购创建/草稿替换共用校验现已对法人、业务单元、供应商、仓库、SKU、产品、基础计量单位、分类及可选品牌持共享锁并检查 active；联查改为实际行锁，等待后重新判定状态。无品牌的产品保留支持。

postgres_b3 中新增九类实际锁等待用例：停用先提交后草稿拒绝且采购订单数保持 0，随后恢复测试资料状态并继续原采购/收货/成本/应付及并发闭环。master_purchase_status_final 全部通过，严格 Clippy、格式及差异检查通过。日志 `/tmp/master-purchase-status-{before,after,final,clippy}.log`。

本批未部署。采购反向停用业务阻塞、确认及后续库存引用仍需覆盖；不等于助手启停已可用。Windows 同一运行 35471308524 已成功完成 Build sidecars，进入 Build Windows NSIS installer (unsigned)。

## 采购确认引用与预览一致性

新回归在原实现确认阶段复现：供应商处于未提交停用事务时，确认不等待便完成（master_purchase_confirm_before）。确认现在锁住实际法人、供应商、业务单元及明细 SKU/产品/仓库/单位/分类/可选品牌，等待后判断当前状态与原数量金额约束，持锁到确认事务结束。

master_purchase_confirm_final 的真实 B3 流程包含九类创建及九类确认等待：确认被拒绝后仍为 draft/v1，恢复全部资料后正常确认并通过取消清理测试单；原采购/收货/成本/应付并发回归继续通过。确认预览同步检查法人、分类、品牌，九类停用均显示不可确认。严格 Clippy、格式和 diff 检查通过，日志 `/tmp/master-purchase-confirm-{before,after,final,clippy-final}.log`。

尚未部署。库存及收货/出库入口、更多归属覆盖与启停意图接入仍未完成。Windows 运行 35471308524 继续处于 NSIS 构建，未重启；持续观察命令会话 82809。
