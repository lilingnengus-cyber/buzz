# 经营报表快照写入接入

目标是让企业助手在明确周期、日期、币种及当前权限范围后创建不可变经营快照。当前只完成现有 Core 执行基础修复，尚无助手写入工具、不可变审批意图或配套发布。

## 2026-09-20 事务与重放修复

原 generate_operating_snapshot 先在外层事务占用幂等键，内部另开事务保存快照和审计；外层完成失败时内部已提交，且已有幂等响应直接返回，未复核当前权限。

现使用 generate_operating_snapshot_on 共用同一事务保存快照、审计、幂等结果。生成与定时生成均持授权修订锁并重新读取当前权限；手动幂等重放也核对生成权限及快照 scope_hash 与当前有效范围摘要一致。生成明细查询沿用此事务连接。定时入口保留独立事务包装，不改变外部 API。

实际 PostgreSQL 55439 隔离库 operating_snapshot_atomic_verified 的完整 postgres_b4 回归通过。新增 helper 验证有效重放、撤销生成权限拒绝、创建独立基准后撤销法人范围拒绝；数据库触发器在完成幂等响应时注入明确错误，快照/审计/幂等记录均无残留，移除故障后同键成功创建。原有利润投影、报表、快照、订阅及异常生命周期断言仍通过。测试把授权变更放在原有断言后，避免范围摘要变化干扰原有历史读取。

严格 Clippy、cargo fmt、文件大小和差异检查通过。日志 /tmp/operating-snapshot-atomic-verified.log、/tmp/operating-snapshot-clippy.log、/tmp/operating-snapshot-size.log。尚未进行并发撤权/数据变化测试，不应把串行撤权测试描述为并发覆盖。

## 后续必要工作

- 度量查询与 data_quality 目前不是完整的同一数据库时间点快照；明确一致性及读取连接策略，验证并发数据变化与授权锁等待。
- 核对管理月报快照与此经营日/周快照的产品边界，一并覆盖用户需要的现有报表写入操作。
- 固定输入、范围/币种/周期查找及缺字段补问、影响预览、不可变意图、签名确认、当前权限与结果回读。
- Gateway/Read API/MCP/Host、详情链接、隔离闭环、配套发布和真实客户端验收。

这批源码晚于订单暂停 c186ddf01 候选，不在正在构建的 Windows 或已验证 Linux/Mac 暂停候选中。生产仍为 e51，未改写生产业务。

## 2026-09-20 同一时间点读取与实际并发验证

手动及定时经营快照在事务开始、任何查询之前设置 REPEATABLE READ。数据质量聚合移入 quality.rs 的 data_quality_on，共用生成事务连接，因此指标、对账、投影质量和快照保存使用同一数据库快照；独立 data_quality 调用也使用自己的可重复读事务。避免快照事务中另借连接读取质量数据。

新增实际 PostgreSQL 并发 helper：外部事务对 purchase_orders 持 ACCESS EXCLUSIVE 锁，启动快照后用 pg_blocking_pids 确认它已等待，再提交 inventory_balances 的变化并释放锁。生成快照仍保留较早时间点的库存金额和质量状态。临时将生成事务降为 READ COMMITTED 后，测试准确报错“snapshot metrics must not mix in a later committed balance”；恢复代码后在全新库 operating_snapshot_consistency_restored 完整 B4 回归通过。负向库独立，未删除或复用生产数据。

日志 /tmp/operating-snapshot-consistency.log、/tmp/operating-snapshot-consistency-negative.log、/tmp/operating-snapshot-consistency-restored.log；严格 Clippy、格式、文件大小和差异检查通过。并发授权撤销、同键竞争的序列化失败处理、月报接口以及 Agent 意图/确认/发布仍待完成，不能将本次时间点一致性覆盖扩展为所有并发场景。

## 2026-09-20 并发重复请求与等待期间撤权

手动及定时生成增加有界事务重试：只对 PostgreSQL 40001（序列化冲突）和 40P01（死锁）重开整个事务，最多重试两次；每次重新锁定授权修订并读取当前权限。业务输入错误、范围拒绝和其他数据库错误直接返回，不做泛化重试。

operating_snapshot_concurrency（55439）完整 B4 回归通过。新 helper 用实际 advisory transaction lock 阻塞首次请求完成，用 pg_blocking_pids 确认两个数据库等待者后放行：同键竞争和不同键/同一期竞争均返回同一快照，只有一条生成审计，不同键后到者 created=false。另用实际授权修订行锁阻塞生成，在持锁事务内撤销生成权限再提交，生成重试后明确返回 NotFoundOrForbidden，快照/审计/幂等均无残留。

日志 /tmp/operating-snapshot-concurrency.log。助手接入、管理月报快照和配套发布仍未完成；当前 c186ddf01 发布候选不含这些后续报表修复。

## 2026-09-20 月度管理快照权限边界

月报生成现在先在实际写入事务内锁定授权修订、读取当前生成权限及范围；幂等重放再检查原快照全部五类范围是否仍可见。legal_entity_ids 规范化排序去重，避免同集合不同顺序生成不同 scope_hash。旧快照替代必须同时满足当前可见范围，以及同报表类型、管理期间、币种和快照范围；否则在写入之前拒绝。读取与重放共享严格范围解析，非法 UUID/非字符串不再被 filter_map 静默忽略。

management_snapshot_authority（55439）完整 B4 回归通过：空法人参数代表当前范围，撤销法人后旧 key 仍不能返回历史快照；不可见旧快照与错误期间替代被拒绝，快照/审计/幂等无残留；实际锁住授权修订，pg_blocking_pids 确认生成等待后撤销权限，放行后拒绝且无成功幂等记录。该批只完成月报权限基础，尚未为助手开放写入。

日志 /tmp/management-snapshot-authority.log；后续仍须核对月报冻结一致性、并发同一期竞争与前驱链，再统一接入经营日/周与管理月报意图、确认及结果链接。Windows 35482058899 最近查询仍在构建，生产保持 e51。

## 月报同一期并发生成

月报事务在任何读取前启用 REPEATABLE READ，生成过程中范围、事实水位、金额与质量检查使用同一数据库快照。与经营日/周报共用 snapshot_transaction::retry，只有已中止的序列化冲突/死锁最多重试两次，每次重新校验授权。快照唯一键冲突走 ON CONFLICT，避免竞争请求直接暴露重复键错误。

management_snapshot_concurrency（55439）完整 B4 回归通过。实际 advisory lock 阻塞首个请求完成，观察两个等待者后放行：同 key 和不同 key/同报表期间两组竞争最终返回同一 ID，各只有一条 MANAGEMENT_REPORT_SNAPSHOT_GENERATED 审计；月报等待期间撤权回归继续通过。严格 Clippy、格式、文件大小及差异检查通过。日志 /tmp/management-snapshot-concurrency{,-clippy,-size}.log。

仍需处理一个独立边界：profit_facts 的序列号分配顺序不保证事务提交顺序，较低序列号的事实晚提交时，max watermark 可能不变，但金额/事实数量已经变化。现有月报唯一键及 existing 查询只按 watermark 去重；必须在助手开放前补齐内容摘要去重及该并发场景，不能把本次同一期请求竞争测试当作该场景的覆盖。

## 2026-09-20 晚提交事实与内容身份

迁移 0060 将管理快照唯一键扩展为原六项加 source_hash，旧快照保持不可变；existing 查询及 ON CONFLICT 同时按摘要匹配。相同最大序号但金额或事实数量变化时可以生成新的内容版本，相同内容仍去重。

management_snapshot_late_fact（55439）完整 B4 回归通过。测试在未提交事务中先插入较低 fact_sequence 的 7 元事实，再在另一事务提交较高序号的 11 元事实并生成第一份月报；提交较低序号事务后生成替代月报。两份 watermark 都是较高序号，source_hash 不同，旧/新金额分别 11/18 元，前驱链接正确；新请求再次生成复用第二份，旧 idempotency key 仍返回第一份不可变结果。

使用实际重建 gateway --migrate-only 对已有 59 数据的独立副本 management_snapshot_upgrade 升至 60，原 management_report_snapshots 全行摘要前后一致（5ca7e5d082a4efb838b1583fe347b7f9）。严格 Clippy、格式、文件大小及差异检查通过。日志 /tmp/management-snapshot-late-fact.log、/tmp/management-snapshot-upgrade.log、/tmp/management-snapshot-late-{clippy,size}.log。

迁移 60 必须与使用新冲突键的 Core 配套发布；不能把仅支持旧冲突键的报表源码作为迁移后的回退方案。当前 c186ddf01 暂停候选仍固定迁移 59，不受本批源码影响；本批未部署。质量状态单独变化时是否产生新内容版本、完整前驱链和助手意图/确认仍需明确及实现。

## 月报只读预览与受保护生成

新增 Core snapshot_preview 和 generate_snapshot_guarded。范围/前驱验证及内容聚合抽取到 reporting/snapshot_preview.rs，普通生成与预览共用同一套计算。预览明确输入、完整范围、来源序号/摘要、金额/事实数量、数据质量、创建或复用效果及非财务法定报表边界；复用时显示实际历史快照 ID/编号/版本、前驱、生成时间、数据截止时间及冻结时的质量状态。预览不分配编号、不写快照/审计/幂等。

受保护生成的幂等哈希绑定 input 和完整 preview；事务内重新生成预览，任何内容/范围/来源/复用状态变化均返回 StalePreview，失败不留幂等占位。成功重放继续校验当前权限/原快照范围，但返回原不可变结果，避免因为创建后 existing 状态变化拒绝合法重试。普通生成保留原请求哈希。

management_snapshot_preview_final（55439）完整 B4 回归通过：预览前后四项记录/编号计数不变；摘要篡改、加入新事实后的旧预览拒绝且无残留；新预览成功生成，原预览同键成功重放，改变预览同键返回 IdempotencyConflict；新的复用预览返回原快照，不新增快照，含历史时间信息。日志 /tmp/management-snapshot-preview-final.log。尚未增加 HTTP/Agent 意图、审批策略或 MCP 工具，不能据此声称聊天写入已开放；经营日/周报相同接口仍需接入。

## 月报加入审批外层事务

提取 generate_snapshot_on：由调用方持有并提交事务，内部快照、编号、审计和幂等写入均留在该事务中。原普通/受保护生成继续使用原有重试包装及独立事务；新入口显式检查 transaction_isolation，只接受 repeatable read 或 serializable，调用方负责重试整个外层事务。

独立数据库 management_snapshot_outer_tx（55439）完整 B4 回归通过。新测试实际生成后撤销外层事务，确认快照、审计、幂等和编号计数全部恢复；默认 read committed 调用被拒绝且无残留。日志 /tmp/snapshot-outer-test.log。此项仅提供事务接口，尚未实现报表审批投票或 Agent 工具，未部署。

## 月报意图与 Core 原子审批

新增迁移 0061：business_agent_report_snapshot_intents 保存不可修改/删除的输入、完整预览和 30 分钟有效期；扩展审批请求/委托类型约束，登记 create/approve IAM capability，不自动授权或创建审批策略。当前只开放 management_profit_statement，profitability_by_dimension 计算尚未实现独立维度语义，不能冒充已支持。

Core 新增 agent-report-snapshot-previews、agent-report-snapshot-intents、agent-approval-previews/report-snapshots、agent-approvals/report-snapshots 路由。准备绑定可选 preflight hash，同键只复用相同且仍有效的意图。审批沿用严格文字命令输入、当前策略、身份/角色/权限 witness、所有历史投票者重新验证及最终墙钟过期检查；整个事务使用 REPEATABLE READ，审批票、快照和执行状态统一提交。snapshot_preview_on 支持同一外层事务。

真实 Router + PostgreSQL 独立库 report_snapshot_intents_verified（55439）通过：只读预览无报表、准备重放、意图不可更新、无策略拒绝、摘要篡改拒绝、另一个确认已生成报表后的旧意图拒绝、成功只生成一份、重复确认拒绝、拒绝票不生成。审计触发器在报表创建后故意失败，快照/请求/投票均为零，再移除故障原意图成功。数据库故障沿用 API 的 503 映射。日志 /tmp/report-intent-test-verified.log。

仍未部署，未接通 Gateway/Read API/MCP/Host 文本命令或真实聊天。新外层事务的序列化冲突目前安全返回数据库错误，尚需增加整个审批事务的有界重试及并发/等候过期、多审批者撤权专项验证。经营日/周报尚未接入；完整报表业务流程未完成。

迁移 61 后完整 B4 流程与订单暂停/解除审批回归分别在 report_intent_b4_regression、report_intent_hold_regression 独立数据库通过；严格 Clippy、格式、文件大小及差异检查通过。未运行全仓 just ci。日志 /tmp/report-intent-{b4,hold,clippy,size}.log。

## 月报审批竞争与等候后授权

准备意图和执行审批现使用整个事务的有界重试：仅 PostgreSQL 40001/40P01 重试，最多两次，每次重新读取意图、预览、权限、策略、已有投票和有效期。业务冲突、过期、普通数据库故障不重试。此项补齐上一节记录的审批外层序列化冲突处理。

report_intent_races_full（55439）真实 Router/数据库回归通过。使用 advisory lock 和 pg_stat_activity 中实际阻塞者确保请求重叠：
- 同键准备同时到达，最终同一意图 ID，仅一条准备审计。
- 两位审批人同时确认，第二位在第一位未提交时竞争请求行，最终 2 票/1 请求/1 新快照；首票返回 pending。
- 首票后撤销该投票者客户范围，第二票冲突且计数不变；恢复范围后可完成。
- 快照已在事务内生成后，最终审计被真实锁阻塞直至意图过期；放行后拒绝，报表/请求/票计数全部不变。
- 授权 revision 被锁时发起确认，等待期间撤销生成权限，放行后重新校验返回 404；无新增报表/票/请求。

日志 /tmp/report-intent-races-full.log；严格 Clippy、格式、文件大小、差异检查通过（/tmp/report-intent-races-{clippy,size}.log）。没有部署或发送真实聊天消息。下一步接通 Gateway、Read API、MCP 和 Host，保留文字确认与真实详情链接；日报/周报和其他尚缺业务领域仍在完整目标范围内。

## Gateway 与 Read API 月报写入适配

Gateway 登记 management_report_snapshot_intent:create/approve，解析固定 management-report-snapshot-intent 签名确认语法；普通写入委托不能获得 approve 权限。Read API 增加 prepare_management_report_snapshot / approve_management_report_snapshot 固定工具，准备先读取 Core 预览并核验全部范围，再带预览摘要写入意图；审批再次检查当前委托范围、意图 ID/版本/摘要，确认结果验证实际快照与 trace。返回复算过摘要的 preview，供后续 MCP 校验；创建成功返回 biz://management-report/<UUID>，沿用已有客户端详情解析。准备新快照不返回尚不存在的详情链接，复用已有快照则返回原链接。

当前 Core 计算包含未分配品牌/仓库的事实，且没有供应商过滤。适配器因此拒绝带品牌、仓库或供应商限制的委托，不能声称这三个维度的受限报表已支持；后续须补齐准确过滤语义后开放。法人、客户、业务单元限制必须覆盖预览中的整个聚合范围，不能仅验证用户输入。

真实 HTTP Core + PostgreSQL 独立库 report_snapshot_adapter_verified（55439）验收：六类不匹配/不支持范围的准备均拒绝且意图计数为零；成功准备、非法范围/摘要校验、确认前客户范围复核、实际生成及详情链接；已有快照的再次准备/审批复用原 ID，快照数保持 1。日志 /tmp/report-adapter-test-final.log。Read API 库测试 51 项通过（数据库环境未设时数据库用例跳过，不能计作 51 项真实数据库验收）；Gateway Agent 单测 4 项通过。尚未接入 MCP/Host，未部署或发送真实聊天消息。

Gateway/Read API 严格 Clippy、格式、文件大小和差异检查通过；日志 /tmp/report-adapter-clippy-final.log、/tmp/report-adapter-size.log。未运行全仓 just ci。

## MCP 与 Host 月报工具接入

MCP 增加固定 prepare_management_report_snapshot / approve_management_report_snapshot。准备只接受严格的月报输入，审批零业务参数，从当前已签名委托取意图 ID、版本、摘要和决定。Host 普通委托新增 create（共 64 个 scope）；严格文字确认才选对应 approve scope。工具说明要求明确月份/币种、展示范围/金额/质量/创建或复用效果并等待用户，不把准备当作已生成。

MCP 独立复算 scopeHash、sourceHash、previewHash，检查固定报表类型、结构、执行状态、票数、当前签名意图、trace 和实际 biz://management-report/<UUID> 链接。新意图准备不得伪造未生成快照的链接；复用必须匹配已有快照 ID/编号/版本。拒绝或等待审批结果不能携带已生成记录。

真实 Core/Read API/数据库 report_mcp_proof（55439）导出四份新建/复用准备及确认返回值 /tmp/report-mcp-proof-96ece.jsonl；MCP 25 项测试通过，包含这四份实际结果及摘要、命令、链接、结果字段、签名意图、pending/rejected 反例（/tmp/report-mcp-proof-test.log）。Host 命令/委托测试 10 项通过（/tmp/report-host-test.log）。

本地实际 buzz-agent + business-read-mcp 二进制、模拟模型端点运行：普通会话 106 个固定工具；月报确认会话 60 个，只含对应审批写入工具，两种 prompt 均完成。日志 /tmp/report-native-{ordinary,approval}.log。此探针验证实际工具可见性，不代表真实用户聊天、生产登录或 Windows 运行已验收。

仍未部署；旧 c186ddf01 安装包不含本批报表功能。品牌/仓库/供应商受限报表过滤、日报/周报，以及整目标其余业务领域仍需继续；上线还需配套迁移 60/61、授权/策略、暂停回退与真实客户端验证。

本批严格 Clippy（MCP、Read API、Host 全 targets）、格式、文件大小及差异检查通过；日志 /tmp/report-mcp-clippy-final.log、/tmp/report-mcp-size.log。未运行全仓 just ci。

## 显式月报维度筛选与快照读取范围

月报输入新增可选 filters（customerIds、businessUnitIds、brandIds、warehouseIds）。每个提供的列表必须非空且完全属于当前 Core 授权范围；Scope 中排序去重。指定品牌/仓库时排除未分配该维度的事实，分别写入 includeUnassignedBrand=false / includeUnassignedWarehouse=false，参与 scopeHash、sourceHash、预览和签名绑定。金额、水位和质量查询使用该筛选口径。不提供 filters 的请求序列化不增加字段，仍使用原五字段 scope 和原幂等哈希；旧快照保持不可变，不能用不同口径快照作为替代前驱。

Read API/MCP 支持明确排除未分配事实的品牌、仓库受限委托，Suppliers 仍不支持：profit_facts 没有可直接用于正确供应商归属的字段，不能伪造过滤。get_management_report_snapshot 同时补齐整个存储范围的委托检查：受限委托不能读取范围更宽或包含未分配事实的旧快照；明确筛选的快照可读取。

实际 PostgreSQL report_dimension_filters（55439）完整 B4 回归：同一组 3/5/7/11 元事实在完整/品牌/仓库/两者组合下分别为 26/8/10/3 元，快照身份不同；空筛选/越权 ID 拒绝，不同口径替代拒绝，旧请求重放原 ID 和原 26 元内容。report_filter_adapter 实际 Core HTTP + Read API：品牌/仓库受限准备及确认成功，旧宽范围快照读取 404，筛选后快照详情读取成功。导出 6 份真实结果到 /tmp/report-filter-mcp-proof.jsonl，MCP 25 项测试通过，包含新筛选返回值；report_filters_approval_regression 完整审批竞争/撤权/过期/回滚回归通过。

日志 /tmp/report-filter-{test,adapter,mcp,approval}.log。三服务严格 Clippy、格式、文件大小和差异检查通过（/tmp/report-filter-{clippy,size}.log）。本批无需新迁移，未部署；生产及旧安装包仍不包含这些源码更新。下一步继续日报/周报写入链路，供应商归属与其他剩余完整业务流程仍未完成。

## 日报/周报纯预览与受保护生成基础

提取 trends/snapshot_preview.rs 共用计算，新增 operating_snapshot_preview(_on)、generate_operating_snapshot_guarded(_on)。新预览包含输入、排他 periodEnd、ownerUserId、完整六维范围/当前 scopeHash、指标、质量、来源摘要、创建或复用效果及已有快照时间。已有快照直接取冻结 payload/质量，不用实时指标冒充历史结果；新预览没有波动的采样时间字段。预览不写快照/审计/幂等。

受保护请求哈希绑定完整输入与预览，事务内复算后比较；变化返回 StalePreview，不残留幂等占位。成功重放保留原结果并重新检查当前授权和原快照可见性。普通生成/定时生成继续共享计算，普通请求哈希保持原样。外层事务接口检查 repeatable read/serializable，由调用方决定提交或回滚。日期加法改为 checked_add_signed，极端日期返回输入错误。

operating_preview_guarded（55439）完整 B4 回归通过，新增日报、周报两组实测：预览计数不变、摘要篡改拒绝、库存变动导致旧预览拒绝、外层生成后回滚全部记录、低隔离级别拒绝、正确生成内容与预览一致、同键重放、同键改预览冲突、冻结后修改实时库存不改变已有预览、新键复用同 ID。极端日期无 panic。已有生成授权、相同/不同 key 并发、指标一致快照、月报筛选测试继续通过。

日志 /tmp/operating-preview-guarded.log；严格 Clippy、格式、文件大小及差异检查通过（/tmp/operating-preview-{clippy,size}.log）。未部署、未增加日报/周报聊天工具。

接入审批前仍须解决：operating scopeHash 当前绑定用户与授权 revision，不能直接照搬月报用不同审批人复算同一预览；事件时间统计仍使用数据库时区的 date 转换，utcOffsetMinutes 只用于完成周期检查/调度，旧快照唯一键也不包含 offset。必须明确并实现准确时区/历史身份语义，再开放完整日报/周报写入链路。

## 日报/周报固定偏移时间口径

迁移 0062 新增可空 utc_offset_minutes。旧快照不猜测/回填时区，使用单独 legacy 唯一索引；新快照唯一键包括 offset，Core INSERT/查找与索引配套调整。预览和 sourceHash 明确 periodStartUtc / periodEndUtc（结束不含）及固定偏移；日期型订单/业务事实仍按业务日期统计，事件时间使用显式 UTC 参数，不再依赖 PostgreSQL 会话时区。SLA 统计截断到周期末，不能把周期结束后才发生的超时提前计入。

趋势只比较同 cadence/currency/scope 下相同已知 offset 的较早周期，返回 comparisonSnapshotId；旧时区未知记录不比较。网页增加每行 UTC±HH:MM/时区未知标签和无可比基线说明，无新增按钮。类型与展示检查通过，未进行浏览器视觉验收。

operating_timezone_verified（55439）完整 B4 实测通过：覆盖偏移周期起点/终点、周期前发生但周期内解决、未来 SLA 超时，日报 UTC+8 与 UTC 分别 4/2 个新事件、4/2 个解决事件，周报分别 6/3 个新事件、7/4 个解决事件；切换事务时区 America/Los_Angeles 后预览一致。不同时区 ID/摘要不同，趋势只引用同偏移的前一期，legacy_unknown 不参与比较，极端日期溢出返回错误。

从此前真实 61 库 operating_preview_guarded 克隆 operating_timezone_upgrade，直接执行迁移 62 SQL（非应用 migrator 进程）：8 行旧快照的全字段摘要去掉新增列后仍为 758d22afbae559b2ebb23611d4f8cd72，新增 offset 全为 NULL。新库完整测试通过正常 PgStore migrator 应用到 62。日志 /tmp/operating-timezone-{verified,upgrade,clippy,web,size}.log；严格 Clippy、Rust 格式、文件大小、差异及前端 TypeScript/展示巡检通过，未运行全仓 just ci。

此迁移必须与新 Core 配套，旧 Core 的四字段 ON CONFLICT 不适用于迁移后的索引；当前生产/旧 c186 安装包未更动。本批未部署。接下来仍须接入以请求人为报表所有者、独立校验审批人范围的不可变意图/多人审批，以及 Gateway/Read API/MCP 和实际客户端验收；历史 scopeHash 含授权 revision 的范围身份问题仍需明确处理。
