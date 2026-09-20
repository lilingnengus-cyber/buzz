# 经营费用与利润调整写入

完整目标包含草稿创建/修改、分摊预览、签名确认过账及逆转；本记录的第一批基础设施不代表这些助手工具已上线。

## 不写库的分摊预览

现有 AdjustmentService::preview 会持久化 operational_adjustment_previews，将批次变为 previewed，更新版本、审计及幂等记录。不能直接把它当作助手确认前的只读查询。

新增 allocation_preview / allocation_preview_on，在 repeatable-read 或 serializable 事务中检查当前 profit_adjustment:preview 权限、法人、草稿版本、状态及全部分摊目标范围。公共入口自行开启 repeatable-read；调用方事务入口不提交、不写业务记录，并拒绝 read-committed。授权版本 SHARE 锁持有到事务结束。批次、明细、目标订单版本与归属、金额、分摊权重、尾差次序、当前业务范围和管理边界纳入预览摘要。明细金额、总额及分摊金额为字符串。

原有持久化预览和新只读预览共用 calculate，保留原分摊算法与原浏览器预览 sourceHash/previewHash 结构。新只读预览的摘要绑定完整确认内容，不能与原浏览器预览摘要互换。

## 验证

真实 PostgreSQL 55439 的 adjustment_pure_verified 完整 B4 回归通过。新增用例证明：重复只读预览完全相同；批次保持 draft v1；预览表、审计、幂等、利润事实数量不变；10.01 元精确分摊到两个订单；不合适事务隔离、过期版本、撤销客户范围均拒绝；订单版本变化即使分摊金额不变也改变摘要；随后使用原持久化预览得到相同分摊结果，并正常进入 previewed v2。

首次测试断言错误地要求数据库十进制的字符串固定为两位（实际为 10.010000），已改为按十进制精确值比较，同时保留字符串类型断言；没有改变生产金额精度。日志 /tmp/adjustment-pure-verified.log。Core all-targets 严格 Clippy、格式/差异及文件大小检查通过（/tmp/adjustment-pure-clippy-final.log、/tmp/adjustment-pure-size.log）；未运行全仓 just ci。

## 后续仍需完成

- 创建、修改及查找草稿的受控入口、实际详情链接和名称定位。
- 将确认意图、审批投票、过账事实及审计放入同一事务，签名绑定完整只读预览。现有 post 在事务前取得授权，锁等待后没有新的授权版本锁；只比较预览水位，过账时再读取目标订单维度。必须补齐执行前的当前授权与目标绑定，不能直接将现有 post 暴露成无保护的助手确认工具。
- 过账/逆转幂等、撤权、目标变更、低序号迟提交事实及并发验证；逆转原因与范围预览。
- Gateway、Read API、MCP、Host 固定工具、输入与独立结果校验，以及配套发布和真实客户端验收。

本批无新迁移，未新增助手工具、未部署、未更新安装包或发送真实聊天。完整业务写入目标保持不变。

## 绑定预览的原子过账基础

新增 post_guarded / post_guarded_on。外层入口开启 repeatable-read 并仅对 PostgreSQL 明确中止的 40001/40P01 重启完整事务；事务入口不提交，要求调用方在任意错误后回滚整个审批事务。命令摘要包含 batchId、期望版本、完整只读预览，使用独立 profit_adjustment:post_guarded 幂等命名空间。

执行顺序为当前初步权限/法人检查、幂等锁、草稿行锁、目标订单范围检查与排序 SHARE 行锁、授权版本 SHARE 锁后的当前过账权限与完整目标范围复查，再用只读分摊预览重新计算并精确比较全部确认内容。随后同一事务生成分摊预览、过账利润事实及分摊记录、更新状态、写入事件/审计/outbox 和幂等结果。外层失败不会留下中间 previewed 状态。初始 draft v1 正常经过内部 previewed v2 到 posted v3。

原有浏览器 preview/post 改为调用私有事务方法，再由外层提交，保持原请求摘要、幂等操作名、结果形状和原有行为；本批新保护属于 guarded 入口，不声称已经加固旧浏览器 post 的所有并发权限路径。事务方法不能脱离上层权限校验直接暴露。当前 guarded 方法本身不是人类签名审批，仍需要后续不可变意图、策略、审批人及签名命令检查。

真实 PostgreSQL 55439 的 adjustment_guarded_verified / adjustment_guarded_final 完整 B4 回归通过：调用方成功执行后主动回滚、过账审计故障后的整体回滚、篡改金额与目标版本拒绝、正常 10.01 元过账、同键重放、同键改参数拒绝、撤销客户范围后的重放拒绝及同键并发只产生两条目标分摊。回滚比较预览、分摊、审计、幂等、利润事实、业务事件和 outbox 七类记录数。

用 pg_blocking_pids 确认真实行锁等待：等待草稿期间撤权、等待目标订单期间修改订单版本，释放锁后均拒绝且七类记录数不变。实际保留较小 fact_sequence 的未提交事务，再提交较大序号并预览，最后提交较小序号：最大水位相同但分摊权重不同，旧预览拒绝，新预览可成功过账。

首次审计故障测试误匹配了小写 outbox topic，未触发故障；已改为真实大写审计 operation OPERATIONAL_ADJUSTMENT_POSTED 后重新在新库验证，不把第一次成功执行当作回滚证据。Core all-targets 严格 Clippy、格式/差异、文件大小检查通过，日志 /tmp/adjustment-guarded-{verified,final,clippy-final,size}.log。未运行全仓 just ci。

无需新迁移，未接入签名审批、Gateway/MCP/Host、草稿写入或逆转；未部署或代发聊天。下一步增加不可变确认意图与审批策略，把投票与本次 guarded 过账放入同一事务，再接入固定工具。独立审批人应单独验证当前资格和覆盖范围，重新计算请求人的预览，不能因审批人额外范围而改变分摊目标。

## Core 不可变过账意图与原子审批

迁移 65 新增 30 分钟有效、禁止更新或删除的 operational_adjustment_post_intent，扩展审批文档类型及 profit_adjustment:post 动作白名单。仅登记能力，不自动赋权或建立审批策略。Core 新增只读预览、准备意图、读取审批预览、提交审批四个内部服务入口，统一受经营调整功能开关控制。准备阶段幂等，支持预检摘要绑定；意图绑定批次、版本、请求人及完整原始分摊预览。

审批人必须当前有效、满足策略角色及过账/预览权限，并覆盖快照内请求人的完整六类数据范围（当前是保守的全范围覆盖，并非仅覆盖受影响订单）。独立审批人额外范围不会参与重新分摊；始终重新计算请求人的预览。每次最终审批复查此前审批人，策略提高门槛即时生效，降低不能抹除该请求原先门槛；不允许自批的策略按配置拒绝，尚不支持的金额升级验证策略关闭执行。身份、角色、权限及策略锁持有至事务结束。

投票、guarded 分摊与过账、业务事实、状态、审计、事件及幂等记录在同一 repeatable-read 事务内提交。所有写入和锁等待结束后再用实际时钟核对意图及权限截止时间。已完成或拒绝的意图重复提交返回冲突，不重新过账。审批响应使用 postedDocument；待审批及拒绝时为空。

真实 PostgreSQL 55439 的 adjustment_intents_final 路由验收通过：只读零写入、同键准备重放、意图不可修改、无策略/自批/篡改摘要拒绝、双人审批、首位审批人失效后拒绝、策略降低仍需原人数、最终投票审计故障后八类记录整体回滚、拒绝保持草稿、并发两票只执行一次、功能开关关闭。通过 pg_blocking_pids 证明审批在最终审计处实际等待，等待期间意图过期，释放后全部回滚且批次仍为 draft。adjustment_intents_b4 完整 B4 回归亦通过。初次验收发现遗漏动作白名单，已修复；故障注入响应断言从 500 修正为现有统一错误映射 503 后，在新库重验。日志 /tmp/adjustment-intent-{final,b4}.log。

Core/Gateway all-targets 严格 Clippy、格式/差异及文件大小检查通过；未运行全仓 just ci。当前仅为 Core 源码与隔离验收，尚未接入 Gateway/Read API/MCP/Host 对新意图的聊天签名验证和工具链，不能称为客户端已可用；未部署、未发送真实聊天。下一步贯通签名委托与固定工具，再继续草稿创建/修改、详情和逆转。

## Gateway 与 Host 签名委托接入

Gateway 固定能力清单增加 operational_adjustment_post_intent:create / approve，支持精确的确认及拒绝命令，继续沿用真实 Nostr 签名、来源频道、五分钟时效与 IAM 授权校验。Host 普通会话仅申请准备权限；只有识别出结构化确认或拒绝命令且审批开关开启，才额外申请对应 approve 权限。当前 Gateway 111 项白名单，Host 普通会话 66 项权限；不自动赋权。Read API 与 MCP 的费用固定工具尚未接入，不能据此宣称聊天过账链路可用。

隔离 PostgreSQL 55439 的 adjustment_signed_final 验收：真实绑定密钥签署确认/拒绝，签发、消费及独立再次验证成功；替换文档、版本、摘要、决定，缺失审批内容或换用另一文档家族权限均拒绝。单独覆盖过旧/未来事件、签名后篡改、频道错配、命令家族错配、多余文本、裸“确认”、审批开关关闭，均不签发任何委托。原容量测试的 59 项旧子集升级为实际 66 项普通清单，数据库完整保存，129 项仍拒绝。

Host 10 项针对性测试、Gateway 8 项单元测试及上述真实数据库验收通过；两包 all-targets 严格 Clippy、格式/差异和文件大小检查通过，未运行全仓 just ci。证据 /tmp/adjustment-signed-{final,host-final,unit,clippy-final,size}.log。未部署、未更换客户端、未代发真实聊天。下一步为 Read API 的固定入口、预检范围和独立结果校验，再接 MCP 工具与端到端签名执行验收。

## Read API 固定入口与独立校验

增加 prepare_operational_adjustment_post / approve_operational_adjustment_post 两项固定写入工具（Read API 写入目录 94 项）。沿用服务身份、Gateway verify_write 对精确签名字段的验证、当前 IAM 能力匹配及独立准备/审批开关。将已有审批工具分类提取到目录模块，给原 998 行 writes.rs 留出空间；既有工具语义不变。

准备只接收 batchId / expectedVersion，审批只接收 documentId / expectedVersion / previewHash / decision，拒绝客户端注入来源事件或金额。先读取 Core 纯预览，再独立验证请求人与输入、内层摘要、批次状态和版本、六类快照范围、目标归属、币种、正数两位金额、分摊明细总和及零未分摊余额；检查完整请求人范围处于委托范围后，使用预检摘要保存意图。审批前重新验证预览及范围，审批后验证请求、投票门槛、决定、批次编号和版本加二、posted 状态及 traceId。品牌受限委托拒绝无品牌归属目标。本批不添加尚未完成验收的详情链接。

真实 Core HTTP 服务与 PostgreSQL 55439 的 adjustment_adapter_verified 验收通过：从实际开账、订单、发货和利润投影生成源数据，验证过账成功、拒绝、多人门槛下 pending 三种结果；后两者保持草稿。同一准备请求重放返回同一意图；法人、客户、业务单元、品牌及仓库委托错配不会新增意图；错误确认摘要拒绝；篡改金额或客户后即使重算内层摘要也被语义校验拒绝。三组真实准备/审批输出保存于 /tmp/adjustment-adapter-proof.jsonl，供后续 MCP 独立验证使用。

Read API 测试命令报告 55 项通过，其中本批数据库验收明确设置独立库并实际执行；其他依赖专用数据库环境变量的既有测试可能提前返回，不将该数字声称为全量数据库验收。严格 all-targets Clippy、格式/差异与文件大小检查通过，证据 /tmp/adjustment-adapter-{verified,clippy-final,size}.log；未运行全仓 just ci。当前没有部署或真实聊天，MCP 固定工具/返回校验、Host 提示词以及 Gateway→Read API→Core 端到端联调仍待完成。完整业务写入目标不缩减。

## MCP 固定工具与 Host 提示词

MCP 新增 prepare_operational_adjustment_post（仅 batchId、expectedVersion）和 approve_operational_adjustment_post（无模型参数）。审批内容从本轮签名委托取得；普通会话隐藏审批工具，匹配审批会话隐藏准备/创建/修改工具。Host 提示词明确展示金额、币种、分摊目标、范围及管理核算边界，只输出服务返回的原样确认/拒绝命令，不增加按钮；pending/rejected 不得报告已过账，不生成虚构详情链接。草稿查找、创建/修改与逆转仍需继续实现。

MCP 独立校验准备信封、摘要、命令及请求人，审批时校验签名绑定字段、原请求人预览、门槛、执行状态、批次编号、版本加二、traceId 和空资源链接。金额使用已有工作区 rust_decimal 做精确小数及合计校验。初次真实结果测试发现通用金额校验要求每个 amount 对象有 currency，而 Core 分摊目标继承批次币种；改为仅在校验副本中补入继承币种，原结果及签名摘要不变，拒绝目标私自覆盖币种。继续检查敏感字段；畸形对象结构返回失败，不通过可变 JSON 索引触发 panic。

实际使用上一批 /tmp/adjustment-adapter-proof.jsonl 三组真实 Core/Read API 返回验证 executed、pending、rejected；测试篡改单据/版本/摘要/决定/类型、结果编号/状态/版本/trace、金额/客户（重算摘要后仍拒绝）、伪造链接、敏感字段及畸形结构。MCP 测试 27 项通过（本批真实结果测试显式提供文件，其他可选数据库产物测试可能跳过）；Host 针对性测试 11 项通过。两个包严格 all-targets Clippy、格式/差异及文件大小检查通过，未运行全仓 just ci。证据 /tmp/adjustment-mcp-{safe-final,host,clippy-final,size}.log。

实际 debug MCP 进程完成 stdio initialize/tools-list：普通会话 108 项，费用审批会话 60 项，准备参数严格两个字段，审批无参数；证据 /tmp/adjustment-mcp-inventory.json。该探针仅检验进程协议与目录，没有调用生产服务。源码链路的各段已接入，但尚未完成单次真实签名消息贯穿 Gateway→MCP→Read API→Core 的端到端联调，未部署、未更新安装包、未发送真实聊天。下一步完成隔离端到端验收，然后推进费用草稿与逆转及配套发布。

## 隔离签名执行链路验收

新增 Read API 的 adjustment_chain_tests，使用真实 Gateway / Core / Read API HTTP 路由、Gateway 签名与当前委托复核器，以及实际 business-read-mcp 二进制的 stdio initialize/tools-call。测试身份经真实绑定 challenge 和签名验证绑定，只在隔离数据库授予准备/审批 IAM 能力；Core 明确配置单人允许自批测试策略。源数据由真实开账、订单、发货、利润投影和费用草稿服务生成。没有以 AcceptanceTest verifier 或模拟 API 返回替代执行链路。测试配置的 OIDC JWT verifier 没有走外部登录，本轮也没有通过 relay 或客户端代发消息。

准备消息签名→Gateway 签发委托→MCP 消费→Read API 再验证→Core 保存意图；随后对返回的原样确认/拒绝命令重新签名并签发新委托，再贯穿 MCP/Read API/Core。验证确认仅产生一条对应费用事实，拒绝保持草稿，同一审批会话重复调用不重复执行；数据库审批 source_buzz_event_id 精确等于签名事件 ID，成功 MCP 审计共七条（两个正常准备、确认/拒绝及三个负例准备）。

完整负例包括通过 Gateway HTTP 撤销委托后调用、签名命令摘要与意图不一致、准备后批次版本变化；三者均不创建审批请求、不产生费用事实，保持 draft。MCP 子进程设 kill_on_drop，协议调用有 20 秒超时并正常终止。共享业务夹具移到 test_fixture，避免同一源模块重复加载。

PostgreSQL 55439 新库 adjustment_chain_final 验收实际通过。Read API 测试命令的 56 项通过中，本批链路用专用环境变量实际执行；其他需要独立数据库环境变量的测试可能提前返回，不声称全部数据库场景重验。严格 all-targets Clippy、格式/差异和文件大小检查通过，未运行全仓 just ci。日志 /tmp/adjustment-chain-{final,clippy-final,size}.log。运行前必须先构建当前 MCP：cargo build -p business-read-mcp，并显式设置 BUSINESS_ADJUSTMENT_CHAIN_MCP_BINARY 绝对路径及 BUSINESS_ADJUSTMENT_CHAIN_TEST_DATABASE_URL、正常 Core 配置。

费用过账签名链路已有隔离端到端证据；仍未部署、未更新安装包或完成 Windows/真实聊天验收。下一步补全费用草稿查找/详情、创建/修改和逆转，再做配套权限、发布与实际客户端验证；完整企业业务写入目标继续保持。

## Core 费用详情与整单读取范围

新增 AdjustmentService::detail、服务 GET /v1/profit-adjustments/{id} 及浏览器 GET /api/v1/profit-adjustments/{id}，保留已有 PUT 修改路由。当前 profit_adjustment:read 权限及授权修订锁在 repeatable-read 事务内读取；全部明细先做权限检查，之后才截取返回页。默认 20 条，上限 100 条，后续页必须提供第一页版本，版本变化返回冲突。返回批次、当前页明细、总额十进制字符串、目标订单总数、请求人当前范围及分页元数据；不返回无限展开的目标 ID 列表。

检查全部明细的客户/品牌/业务单元/仓库显式引用，直接订单、订单列表与固定权重引用，未冻结草稿当前聚合目标，以及已过账分摊目标。订单当前归属、订单行仓库、历史利润事实中的法人/客户/品牌/业务单元/仓库也必须在当前范围内；今日订单迁移到可见客户不能绕开历史事实范围。已过账/逆转按存量分摊目标校验，不用今天新增订单重算冻结结果。此读取路径不会持久化分摊预览或改变批次状态。

PostgreSQL 55439 独立库 adjustment_detail_verified 的真实路由验收通过：两行分页与总额 30.03、缺失后续页版本拒绝、版本变化冲突、仅第二页引用的品牌撤权后第一页也拒绝、已过账详情可读、订单移至新授权客户后撤销历史客户仍拒绝、功能关闭返回 503；读取前后分摊预览、审计及利润事实计数不变。初次测试夹具新客户遗漏必须的 credit_currency，补齐 CNY 后在新库重验。Core all-targets 严格 Clippy、格式/差异和文件大小检查通过，日志 /tmp/adjustment-detail-{verified,clippy-final,size}.log；未运行全仓 just ci。

当前只完成 Core 详情基础，尚未接入助手 Read API/MCP 或浏览器详情 UI，未部署。原浏览器列表仍仅按法人过滤，本批没有把它直接暴露给助手，也不声称该旧列表已完成整单范围加固。下一步加入同等范围检查的分页查找及助手详情读取，再衔接草稿创建/修改和逆转。

## Core 整单授权分页查找

增加 GET /v1/profit-adjustments 与 AdjustmentService::search，支持编号字面量（不把 % / _ 当作通配符）、法人、YYYY-MM 期间及精确状态筛选，每页默认 20、上限 100。查找和详情共用 detail_on 的整单授权逻辑，在同一个 repeatable-read 事务及同一授权快照内完成；不为每个候选开启独立事务。按不可变创建时间和 UUID 降序分页，避免单纯更新批次把记录移到另一页。

内部每次扫描 64 个法人范围候选，跳过完整权限检查失败的批次，直到收集 limit+1 个可见批次或扫描结束。只输出返回页最后一个可见 ID 作为 nextAfterId，不暴露隐藏扫描位置或隐藏总数；继续请求先重新核对锚点整单权限，不存在、已撤权或不可见锚点统一拒绝。跨页不保留数据库快照，因此每页显示当前状态与版本，不声称历史快照一致性。设置单条 SQL 三秒及整体八秒上限，超限整次失败并要求缩小筛选，不返回伪完整结果。

PostgreSQL 55439 独立库 adjustment_search_final 的真实服务路由验收通过：65 个不可见批次排在 3 个可见批次之前，仍能跨内部扫描窗口返回两页（2+1），顺序、hasMore、nextAfterId 正确且无隐藏总数；隐藏/随机锚点拒绝；编号百分号按字面量处理；非法分页/期间/状态/字段拒绝；未授权法人拒绝；撤销客户范围后结果为空且旧锚点拒绝；查找功能关闭返回 503。保留并重跑前一批详情、整单范围与历史事实保护测试，查询前后预览、审计及利润事实计数不变。

Core all-targets 严格 Clippy、格式/差异、文件大小检查通过，日志 /tmp/adjustment-search-{final,clippy-final,size}.log；未运行全仓 just ci。本批未部署，助手 Read API/MCP 的查找和详情工具仍未接入，旧浏览器列表也未迁移到该新查找契约。下一步增加受委托约束的助手读取工具，再继续草稿创建/修改和逆转。


## 助手费用查找与当前版本明细

新增 search_operational_adjustments / get_operational_adjustment，共用严格输入合同。查找支持编号字面量、法人、管理期间、状态及可见记录游标，每页最多 20 条；明细每页最多 10 行，后续页必须带首次读取的版本。返回金额字符串及后续草稿编辑需要的字段，版本变化返回冲突。尚无实际详情页面，因此明确 detailLinkAvailable=false、resourceRefs 为空，不生成虚假链接。

迁移 66 仅登记 profit_adjustment:read，不自动赋权。Gateway 白名单 112 项，Host 普通委托 67 项（纯只读 18 项），Read API 读目录 47 项、写目录 94 项。实际 MCP stdio 工具目录验证普通回合 110 项、费用过账确认回合 62 项；确认回合仍只暴露对应审批工具。Host 提示要求读全同版本明细、消歧后再继续，不把备注当作执行指令。

Read API 独立验证 Core 返回的完整范围被当前委托覆盖。这是保守的全范围覆盖，并非逐条交集筛选：较窄委托可能拒绝整个请求。供应商受限委托因无费用供应商归属而拒绝；客户、业务单元、仓库或品牌受限而无目标订单时亦拒绝。Core 增加当前及历史无品牌目标标记，品牌受限委托不能将空品牌视为已授权。

真实 PostgreSQL 55439 的 adjustment_read 通过 Core HTTP 适配器测试：查询及详情、六类委托范围拒绝、版本冲突、缺少后续页版本、空品牌目标拒绝，审计/持久化预览/利润事实数量不变。其实际返回值输入 MCP 独立校验，篡改 ID、金额、币种、日期、版本或分页以及超出响应预算均拒绝。adjustment_read_gateway_final 验证完整 67 项普通委托；adjustment_read_core_final 回归整单明细授权及分页查找。

查询合同 15 项、Host 11 项、MCP 28 项、Read API 57 项报告通过；其中环境变量控制的其他集成测试可能跳过，本批只将上述显式新库和实际响应语料计为新增运行证据。六个相关包 all-targets 严格 Clippy 通过，真实 MCP 进程目录探针通过；未运行全仓 just ci。日志 /tmp/adjustment-read-{contract,host,mcp,test,gateway,core,clippy}.log，目录 /tmp/adjustment-read-inventory.json。

本批未部署、未更新安装包、未发送真实聊天，尚未将新读取接入完整签名端到端验收。下一步补齐费用草稿创建/修改的不可变意图、签名确认与幂等保护，并完成真实详情页面及逆转；完整业务流程目标仍未完成。


## 草稿写入的受控事务入口

增加 create_guarded_on / replace_draft_guarded_on，要求调用方提供 repeatable-read 或 serializable 事务，并在任意错误后回滚完整事务。新入口不提交，且本身不是签名确认 API。原浏览器 create / replace_draft 的持久化逻辑提取到私有方法，保持旧参数和幂等语义；不能据此认为旧浏览器路径已获得新入口的全部权限保护。

新入口先验证输入，将所有显式订单引用（directSalesOrderId、salesOrderIds、fixedWeights）按 ID 排序并锁定；引用必须处于当前全部业务范围内，且与草稿法人、币种相符。资源等待结束后取得授权版本 SHARE 锁并重新检查权限。修改还锁定原草稿，通过整单明细授权检查全部旧行及旧目标，防止用替换删除越权明细。动态分摊尚不在此阶段计算或冻结，后续确认意图仍需绑定明确预览及执行边界。

使用独立 guarded 幂等命名空间，修改摘要包含 batchId、expectedVersion 和完整替换内容，拒绝同键跨草稿重放；派生内部键调用原持久化方法。重放仍检查当前范围。所有草稿、明细、事件、审计、outbox 和幂等记录加入同一事务。

真实 PostgreSQL 55439 的 adjustment_draft_verified 证明：错误隔离拒绝，创建/修改成功后调用方回滚，创建/修改幂等重放，旧版本与同键跨草稿拒绝，未知显式目标拒绝，旧行越权不能通过替换移除，撤权后重放拒绝。用 pg_blocking_pids 确认真实目标订单/草稿行锁等待，在等待期间撤销客户范围，释放后两种写入均失败。审计触发器注入创建/修改故障，回滚后七类记录计数不变。首次测试使用过短幂等键，修正测试数据后在全新库重验，未放宽生产键规范。

adjustment_draft_b4 完整既有 B4 回归通过，Core all-targets 严格 Clippy 通过。证据 /tmp/adjustment-draft-{verified,b4,clippy}.log；未运行全仓 just ci。本批没有新增迁移、对外路由或助手工具，未部署和发送真实聊天。下一步基于这些事务入口增加草稿创建/替换的不可变确认意图、完整内容预览、签名委托与固定工具；完整业务流程目标保持未完成。
