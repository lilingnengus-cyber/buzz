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


## 草稿完整预览与绑定执行

新增 draft_preview / draft_preview_on：创建预览不分配编号或保存草稿；替换预览绑定源批次、期望版本、全部旧明细、完整新输入、显式引用订单的版本与归属，以及请求人的完整当前范围。使用 repeatable-read，持有授权版本锁；两页各 100 行组成完整旧明细，超过领域既有 200 行上限拒绝。金额合计采用精确十进制字符串并检查溢出，完整内容生成摘要。effects 明确不分摊、不产生利润事实、不过账，只创建草稿或替换全部行。

新增 apply_draft_preview_on，在调用方事务中锁定源草稿及显式引用订单，重新生成完整预览并精确比较，然后调用已有受控草稿事务入口。此方法不替代人类签名、有效期和审批策略；修改完成后旧版本预览失效，审批层须负责终态及全事务序列化重试，不能仅重试部分写入。

真实 PostgreSQL 55439 的 adjustment_draft_preview 通过：重复预览相同且九类计数/编号序列总值不变，篡改金额拒绝，引用订单版本变化使摘要变化并拒绝旧预览；创建及替换既可整体回滚，也可提交得到 draft 和正确版本。101 行源草稿的预览包含所有旧行，第 101 行变化使旧摘要失效，即使源批次版本未改变也拒绝旧预览。非法范围拒绝。原草稿受控入口的撤权锁等待、审计故障、幂等及版本回归在同一新库通过。

Core all-targets 严格 Clippy、格式/差异及文件大小检查通过，未运行全仓 just ci。证据 /tmp/adjustment-draft-preview-{test,clippy,size}.log。本批仍是 Core 领域入口，没有新增外部路由、迁移、助手工具或部署。下一步将该预览保存为不可变创建/替换意图，接入签名确认、审批投票和固定工具；完整目标仍未完成。


## Core 草稿不可变意图与原子审批

迁移 67 扩展既有不可变意图表、审批请求与委托文档约束，登记 operational_adjustment_creation_intent / operational_adjustment_update_intent 的 create / approve 能力，不自动赋权或建立审批策略。Core 复用已有费用意图路由；创建输入为完整 CreateAdjustmentBatch，替换输入为 batchId、expectedVersion、batch，拒绝未知字段。准备阶段只持久化 30 分钟不可变意图及准备审计，不创建草稿或消耗编号。

复用现有多人审批的当前身份、权限、完整范围覆盖、策略、全部历史投票复查、门槛不降低及最终实际时钟截止检查。创建/修改按各自领域权限检查，无需过账权限；过账仍保留原 profit_adjustment:preview 与 post 权限组合。审批始终重算请求人的预览，独立审批人额外范围不会改变内容。达到门槛后调用绑定预览的草稿事务入口；投票、草稿/明细、编号、审计、outbox、幂等和请求终态原子提交。

响应分别使用 createdDocument、updatedDocument 或原有 postedDocument，避免把保存草稿误称过账；待审批及拒绝时相应结果为空。原过账命令提取到独立模块，外部过账形状保持不变。Core 内部服务入口仍依赖上游验证签名来源，不能把本次 Core 路由验收称为真实签名客户端验收。

真实 PostgreSQL 55439 的 adjustment_draft_intents_final 验证：只读预览零写入、准备不创建草稿、准备幂等、缺策略/禁止自批拒绝、双人创建、并发双票替换只执行一次、重复终态冲突、确认夹带金额拒绝、拒绝不产生草稿、意图不可修改、已过期拒绝、源草稿变化拒绝。最终投票审计触发器故障后，完整草稿/行内容、编号、投票、请求和相关记录均保持原样。

创建和修改两类均用 pg_blocking_pids 证明在最终审计处真实等待，等待至意图过期后释放，业务写入和投票整体回滚。既有费用过账的多人、撤权、并发、审计故障与最终等待过期回归在同一新库通过。Core/Gateway all-targets 严格 Clippy、格式/差异及文件大小检查通过；未运行全仓 just ci。证据 /tmp/adjustment-draft-intents-{final,clippy,size}.log。

本批尚未接入 Gateway 新文档家族的签名命令验证、Read API/MCP 固定工具和 Host 委托，未部署、未发送真实聊天。下一步贯通上述助手链路，再验证完整签名端到端、详情页面和逆转；完整目标仍未完成。


## 草稿 Gateway 与 Host 签名委托

Gateway 固定白名单接入两类草稿意图的 create / approve，当前共 116 项；Host 普通会话只新增两项准备权限，共 69 项，纯只读仍为 18 项。精确的 operational-adjustment-creation-intent / operational-adjustment-update-intent 确认及拒绝命令才申请对应审批权限，继续绑定真实 Nostr 事件、频道、文档、版本、摘要和决定。不自动赋权；迁移沿用 67。

真实 PostgreSQL 55439 的 adjustment_draft_signed_final 验证完整 69 项普通委托保存，129 项仍拒绝；两类草稿均用隔离真实密钥签署确认/拒绝，完成签发、消费和独立 verify_write。变更文档、版本、摘要、决定、缺失绑定及错误家族均拒绝。三类费用意图分别验证过旧/未来签名、签名后篡改、频道错配、跨家族及创建/修改互换、多余文本、裸确认和审批开关关闭，均不签发委托。

Host 11 项定向测试、Gateway 4 项签名命令单元测试、新库委托验收及两包 all-targets 严格 Clippy 通过；格式/差异及文件大小检查通过，未运行全仓 just ci。证据 /tmp/adjustment-draft-signed-{host,unit,final,clippy,size}.log。未代发真实聊天或部署，Read API/MCP 草稿工具仍未接入，因此不能声称客户端已能创建或修改费用草稿。下一步接入固定工具与独立快照/金额/结果校验，再贯通真实签名端到端。


## 草稿 Read API 固定工具与独立结果校验

Read API 增加 prepare/approve_operational_adjustment_creation 与 prepare/approve_operational_adjustment_update，写目录由 94 增为 98。沿用现有 Gateway verify_write、当前 IAM 委托及开关检查；确认输入不接受金额、明细或来源事件，来源绑定取自已验证上下文。准备先读 Core 只读预览，通过独立校验和委托范围检查后，才带预检摘要保存意图；保存返回内容必须与预检一致。

创建/替换输入采用严格字段结构，金额及权重为十进制字符串。独立校验包括合法期间/日期、币种、正数两位金额与合计、费用类型、分摊方式、UUID、最多 200 行和 500 个唯一显式订单引用、固定权重、原因及文本长度。验证完整内层摘要、effects、请求人、范围、全部显式订单版本/归属，替换时验证全部原始行、唯一行 ID/行号及旧金额合计。更新结果必须匹配原批次 ID/编号和版本加一；创建结果必须为 v1 草稿，不能把 posted 当作保存成功。

继续采用完整 Core 范围被委托覆盖的保守规则。供应商受限委托因无供应商归属而拒绝。对于客户、业务单元、仓库或品牌受限委托，当前要求新旧明细显式填写对应归属；同时检查显式引用订单中的客户、业务单元和品牌。缺少显式归属时即使可从其他对象推导，也暂不推导或扩大授权。后续可通过补齐可信归属快照提升这些场景的可用性；本批不声称已支持逐条范围交集。resourceRefs 为空，未生成尚不存在的详情页面链接。

真实 PostgreSQL 55439 的 adjustment_draft_adapter_final 与真实 Core HTTP 服务验证创建/修改各自的成功、拒绝、等待第二人共六种结果，以及准备幂等、六类错误委托在持久化意图前拒绝、明确的受限归属通过校验、缺少品牌归属拒绝、错误摘要拒绝。重新计算内层摘要后的金额、合计、订单归属、effects、旧行金额篡改仍拒绝；伪造 posted 结果拒绝。既有三种过账适配器结果亦通过。

Read API 57 项报告通过；其他依赖环境变量的集成测试可能跳过，本批只将上述显式新库与真实服务作为新增运行证据。首轮仅有工具计数断言仍为 94，改为 98 后新库重验通过。Read API all-targets 严格 Clippy、格式/差异及文件大小检查通过；未运行全仓 just ci。日志 /tmp/adjustment-draft-adapter-{final,clippy,size}.log；六份实际响应 /tmp/adjustment-draft-adapter-final-proof.jsonl，可供下一阶段 MCP 独立校验语料。

未部署、未发送真实聊天，MCP 草稿工具和完整签名端到端验收仍待接入。下一步补齐 MCP 严格输入、确认工具和独立输出校验，再继续详情页面及逆转；完整业务流程目标保持未完成。


## MCP 草稿工具与独立输出校验

MCP 注册 prepare/approve_operational_adjustment_creation、prepare/approve_operational_adjustment_update。准备输入为严格对象和嵌套明细，费用类型/分摊方式使用封闭枚举，金额与权重只能是字符串；两个确认工具不接受任何模型参数。输入先独立检查并标准化，再消费委托；发送标准化内容，返回的 document.input 必须与之相同。标准化保持十进制精度，不使用浮点数。

MCP 保留独立快照校验，验证完整新旧明细、金额、归属、订单集合与版本、内外摘要、effects、精确确认/拒绝命令、当前签名意图及决定。创建/修改分别使用 createdDocument / updatedDocument，核对草稿状态、版本及更新前 ID/编号；pending/rejected 不得返回业务写入结果。金额通用校验仅在副本中补上输入明细继承的批次币种，原始签名内容不变，拒绝明细自行覆盖币种。仍不返回不存在的详情链接。

Host 提示更新为完整同版本读取、保留未修改字段、缺资料补问、显示完整草稿差异与精确文本确认；准备不保存业务草稿，确认保存不代表费用过账，逆转仍未接入。不增加确认按钮。

实际 Read API 六份 Core 响应均通过 MCP 独立校验；错误签名字段、改决定、错误 Trace、虚假状态/版本、未执行却返回结果、敏感字段、币种覆盖、畸形嵌套结构、重新计算内层摘要后的金额/归属篡改均拒绝。新增输入用例拒绝浮点金额、零/负数、超过两位金额及未知 SQL 字段；无效输入在消费委托前返回。MCP 31 项、Host 11 项报告通过；其中本批显式加载费用草稿、过账及查询的实际响应语料，其他环境控制语料不视为新增验证。

实际 MCP stdio 进程目录：普通会话 112 工具，费用创建/修改/过账确认会话各 62 工具，均低于 128 上限，确认会话仅暴露匹配的审批工具。MCP/Host all-targets 严格 Clippy、格式/差异及文件大小检查通过；未运行全仓 just ci。证据 /tmp/adjustment-draft-mcp-{final,host,clippy-final,input-tests,input-clippy,size}.log，目录 /tmp/adjustment-draft-mcp-inventory.json。

本批未部署或代发真实聊天；工具目录及实际响应校验不是完整签名端到端验收。下一步使用真实 Gateway/Core/Read API 和 MCP 进程完成草稿签名执行闭环，再继续详情页面、逆转及配套发布；完整目标仍未完成。


## 费用草稿完整签名服务链路验收

在独立 PostgreSQL 55439 的 adjustment_draft_chain_final 启动真实 Gateway、Core、Read API HTTP 路由，Read API 使用 Gateway 委托验证器；通过已构建 MCP 二进制的真实 stdio 协议调用工具。隔离密钥完成真实绑定挑战及 Nostr 签名，准备与确认分别签发委托，没有替换为 acceptance bypass，也未向真实聊天/Relay 发消息。

验收贯通签名准备创建 → 签名确认 → MCP 按编号查找 → MCP 读取同版本完整详情 → 签名准备替换 → 签名确认。准备前后业务状态不变，创建结果回读为 draft v1、10.01 元，修改回读为 draft v2、20.02 元；不产生费用利润事实。创建和修改的签名拒绝均不保存业务写入，要求两人审批时单票返回 pending，草稿/明细、编号、幂等、outbox 及利润事实不变。重复调用同一确认不能重复执行，数据库投票来源事件精确对应实际签名消息 ID。

创建/修改各覆盖 Gateway HTTP 撤销委托、签名内容中的错误摘要及准备后内容失效（创建引用订单版本变化；修改源批次版本变化）。失败确认后完整草稿/行、请求、投票、编号、幂等及相关业务计数保持不变。原有费用过账成功、拒绝及三类失败签名链路也同时回归通过。

数据库独立回查：草稿创建和修改各有 executed/pending/rejected 一条，过账有 executed/rejected 各一条；MCP 成功审计共 27 条，与预期的 20 次草稿/读取和 7 次既有过账调用一致。Read API 57 项报告通过，其中本条明确配置新库及二进制并实际执行，其他环境控制测试不算新增运行证据。Read API all-targets 严格 Clippy、格式/差异及文件大小检查通过；未运行全仓 just ci。证据 /tmp/adjustment-draft-chain-{build,final,clippy,size}.log。

本次未部署或更新客户端，未进行真实客户端聊天验收。草稿链路已有隔离完整签名服务证据，仍需费用实际详情页面、逆转、配套发布及客户端验收；完整业务流程目标仍未完成。


## 费用逆转只读预览与原子事务基础

新增 reversal_preview / reversal_preview_on，要求当前 profit_adjustment:reverse 权限、完整当前及历史数据范围、posted 状态和精确版本。读取全部已冻结分摊及原利润事实，验证一一对应、金额/币种/法人一致、完整合计与批次明细一致。金额、分摊金额、权重及数量以字符串或空值表示；原事实完整维度、原因、请求人范围、目标订单和原批次纳入摘要。预览不写业务记录，明确保留原事实、不重新分摊、不执行银行退款。

新增 reverse_guarded / reverse_guarded_on。命令摘要包含批次、版本、原因及完整预览，独立幂等命名空间；锁定源批次和排序后的目标订单，资源等待后取得授权版本锁并复查整单权限，再重新计算并精确比较预览。随后同一事务新增抵销事实、更新 reversed 状态、保存原因事件/审计和幂等结果。事务入口不提交，任何错误必须回滚整笔审批事务；外层只对数据库明确中止的序列化/死锁重启完整事务。重放仍检查当前历史范围。

原浏览器 reverse 的持久化代码提取为私有事务方法，原参数、幂等摘要和无原因时的事件/审计结构保持不变；不声称旧浏览器入口已获得新 guarded 路径的全部授权保护。本批领域方法本身也不替代人类签名确认。

真实 PostgreSQL 55439 的 adjustment_reverse_final 完整 B4 验证：重复预览零写入、两个目标合计 10.01 元、空原因/旧版本/篡改事实/改变原因拒绝，错误隔离拒绝，调用方回滚和最终审计触发器故障均恢复完整批次与事实及七类相关状态；正常逆转、同键重放、同键改原因拒绝、撤权后重放拒绝、并发同键只产生一组抵销事实。用 pg_blocking_pids 确认真实批次锁等待，期间撤权后执行失败且业务状态不变。

先给另一客户授权，再将目标订单当前客户全部改为该客户，逆转预览保持原历史事实不变；新增抵销事实逐字段匹配原事实的金额、日期及维度。撤销原历史客户权限后，重放仍拒绝，即使当前客户有权限。原因同时存在于批次事件及审计。Core all-targets 严格 Clippy、格式/差异及文件大小检查通过；未运行全仓 just ci。证据 /tmp/adjustment-reversal-{final,clippy,size}.log。

本批无迁移或新助手工具，未部署、未发送真实聊天。下一步把逆转预览、原因和版本保存到不可变确认意图，接入签名委托与 MCP 工具；费用详情页面、配套发布及真实客户端验收仍待完成，完整目标保持未完成。


## 逆转 Core 不可变确认意图与原子审批

新增 operational_adjustment_reversal_intent，严格输入绑定批次 ID、预期版本和原因，确认快照保存完整原始利润事实及其逆转预览、范围与摘要。复用不可变意图和多人审批事务，最终执行调用 reverse_guarded_on；审批投票、逆转事实、批次状态、原因审计和幂等结果同事务提交。返回 reversedDocument，等待或拒绝时不返回业务执行结果。迁移 0068 只扩展约束和登记 create/approve 能力，不自动赋权或创建审批策略。

真实 PostgreSQL 55439 的 adjustment_reverse_intent 完整费用意图测试通过，包括既有过账与草稿回归，以及逆转纯预览零写入、空原因拒绝、准备幂等、缺策略/自审拒绝、错误摘要和确认夹带原因拒绝、双人审批、首位审批人撤权后拒绝、最终投票审计故障整体回滚、原事实与抵销事实金额净和为零、原因留痕、终态重复确认拒绝。并发两人审批只执行一次，拒绝保持 posted，源版本变化拒绝且相关状态不变。使用真实数据库阻塞验证最终审计等待期间过期后整体回滚，源批次仍为 posted。

Core/Gateway all-targets 严格 Clippy、格式、差异及文件大小检查通过；未运行全仓 just ci。证据 /tmp/adjustment-reversal-intent-{test,check,clippy,size}.log。本阶段尚未部署或发送真实聊天；逆转 Gateway/Host 签名委托、Read API/MCP 固定工具与完整签名链路仍需接入，完整业务流程目标保持未完成。下一步接入逆转签名委托，再贯通助手工具和真实链路验收。


## 逆转 Gateway 与 Host 签名委托

Gateway 白名单增加逆转意图 create/approve，共 118 项；Host 普通会话只增加准备权限，共 70 项，纯只读范围保持 18 项。精确 operational-adjustment-reversal-intent 确认/拒绝命令映射独立审批能力，复用真实 Nostr 事件签名、频道、文档、版本、摘要和决定绑定。迁移沿用 0068，不自动赋权或初始化审批策略。

真实 PostgreSQL 55439 的 adjustment_reversal_signed 验证完整 70 项普通委托持久化、129 项限制拒绝；隔离密钥签署逆转确认及拒绝，完成签发、消费与独立 verify_write，错误文档、版本、摘要、决定、缺失绑定和错误家族均拒绝。四类费用意图均验证过旧/未来签名、签名后篡改、频道错配、跨家族、兄弟意图错配、多余文本、裸确认及审批开关关闭不签发委托。

Host 11 项定向测试、Gateway 4 项单元测试和新库真实签名委托测试通过；两包 all-targets 严格 Clippy、格式、差异和文件大小检查通过。证据 /tmp/adjustment-reversal-signed-{host,unit,db,clippy,size}.log。未运行全仓 just ci，未部署或发送真实聊天。下一步接入 Read API/MCP 逆转准备与零参数确认工具、独立快照校验并验证完整签名服务链路；当前不能声称客户端已能执行费用逆转。


## 逆转 Read API 固定工具与独立校验

Read API 新增 prepare_operational_adjustment_reversal 和 approve_operational_adjustment_reversal，写目录共 100 项。准备只接受批次 ID、当前版本、非空原因，确认继续取已验证签名上下文，不接受新金额或修改原因。准备先验证 Core 纯预览及 IAM 范围，再绑定预检摘要保存意图；准备返回快照必须与预检完全相同。

独立校验内层摘要、请求人、版本、原因、管理口径边界和全部 effects，验证原事实与分摊记录 ID 唯一性、来源关联、批次与法人/币种、订单集合、费用类型、日期、权重、非负两位金额以及总和。当前采用完整 Core 范围必须被委托覆盖的保守规则；供应商受限委托拒绝，客户/业务单元/仓库/品牌受限委托要求每条历史事实具有对应归属。缺失维度不视为拥有权限。执行结果必须为同批次 reversed、版本加一和同 Trace；等待/拒绝时 reversedDocument 必须为空，暂不生成不存在的详情页链接。

真实 PostgreSQL 55439 新库 adjustment_reversal_adapter_final 与真实 Core HTTP 服务覆盖逆转成功、拒绝、等待第二人三种结果，并回读实际批次状态；准备幂等、六类错误 IAM 范围在保存意图前拒绝、明确受限客户/业务单元/品牌正常通过、错误确认摘要拒绝。重新计算内层摘要后的金额、总和、来源关联、客户归属、原因、effects 和目标订单篡改仍拒绝；伪造 posted 执行结果拒绝。既有过账与草稿适配器回归通过。

Read API 报告 57 项通过；其他未提供对应环境变量的集成测试可能跳过，本阶段新增运行证据限于上述显式新库与真实 Core 服务。all-targets 严格 Clippy、格式、差异及文件大小检查通过；未运行全仓 just ci。日志 /tmp/adjustment-reversal-adapter-{final,clippy,size}.log，三份实际响应 /tmp/adjustment-reversal-adapter-final-proof.jsonl。尚未部署或发送真实聊天；下一步接入 MCP 严格输入和独立结果校验，再验证完整签名服务链路，完整目标保持未完成。


## 逆转 MCP 与完整签名服务链路

MCP 新增 prepare_operational_adjustment_reversal 与零参数 approve_operational_adjustment_reversal。严格输入只接受批次、版本、原因，无效输入在消费委托前拒绝；返回 input 必须与请求一致。MCP 独立校验原事实与分摊关系、金额合计、范围、原因、effects、内外摘要与精确确认文本，执行结果绑定签名意图/决定、同批次、reversed 状态及版本加一。Host 提示同步要求先展示历史金额与原因并等待文本确认，保留原事实、追加抵销事实，不宣称银行退款。

加载真实 Read API 三份逆转、六份草稿和三份过账响应运行 MCP 34 项测试通过；覆盖畸形结构、敏感字段、错误 Trace、签名字段/决定错配、重算摘要后事实篡改和虚假执行状态。Host 11 项定向测试通过。实际 stdio 工具目录：普通会话 113 项，四种费用审批会话各 62 项，审批会话只保留对应 approve 工具，均未超过 128 项限制。

新库 adjustment_reversal_chain 在真实 Gateway/Core/Read API HTTP 服务和实际 MCP 子进程间完成签名链路。逆转批准、拒绝、待第二人审批各一例；回读批次状态，成功时原事实与抵销事实金额净和为零。投票来源事件精确匹配人类签名事件，重复确认不再执行；Gateway HTTP 撤销委托、签名错误摘要、源版本变化均拒绝，批次/明细/投票/请求/事实/编号/幂等/事件状态保持不变。既有草稿与过账链路同时通过；数据库独立回读三种逆转请求各 1 条，成功工具审计共 36 条。

三包 all-targets 严格 Clippy、格式、差异、文件大小检查通过；未运行全仓 just ci。证据 /tmp/adjustment-reversal-mcp-{final,host,clippy,size}.log，工具清单 /tmp/adjustment-reversal-mcp-inventory.json，完整链路 /tmp/adjustment-reversal-chain-test.log。未部署或代发真实用户聊天。下一步实现费用详情页面与对应链接，再推进配套发布、Mac/Windows 真实客户端验收；完整业务流程仍未完成。


## 系统费用详情页面与查询回复链接

原费用详情路由实际筛选最多 200 条列表，未读取明细。本阶段改为独立 LinkedAdjustmentDetail，直接按 UUID 请求已有整单授权详情 API，逐页读取全部行，后续页绑定首次版本。加载完整前不展示部分明细；批次/版本/合计/总行数不一致、重复行、异常游标或权限失败均显示错误。展示状态、期间、版本、总额、目标数、全部明细及直接订单链接；嵌入模式保持 /embed 路径，不新增写入/确认按钮。

Read API 查找与详情结果返回与实际批次 UUID 对应的 biz://profit-adjustment 链接，MCP 严格验证链接数量、类型、ID、标题和 URI 与结果一致。桌面已有对应资源路由，验证 UUID 链接进入系统 /embed/profit-adjustments/{id}。写入准备及执行回复暂仍不返回详情链接，需下一阶段同步加入严格结果校验。

Web 构建、类型与金额展示检查通过；Playwright 四项真实浏览器测试覆盖嵌入/普通路由、完整两页及版本绑定、第二页版本变化不展示部分结果、无权限不回退列表。页面测试使用 HTTP fixture，不等于生产业务会话验收。新库 adjustment_detail_links_final 与真实 Core HTTP 读取验证通过，MCP 加载实际新响应验证并拒绝跨批次/错误类型/缺失链接；API 57 项、MCP 34 项报告通过（其他未配置环境变量测试可跳过）。两包 all-targets 严格 Clippy、27 项桌面资源解析、格式/差异与文件大小门禁通过，未运行全仓 just ci。日志 /tmp/adjustment-detail-{build,browser,read-final,mcp,clippy,resolver,web-check,size}.log；实际查询响应 /tmp/adjustment-detail-read-proof.json。首轮旧的空 resourceRefs 断言失败，更新为精确链接断言后使用新库重验通过。

尚未部署或通过真实 Mac/Windows 会话验收。下一步补齐写入回复链接并回归完整链路，再进行配套发布；完整业务目标保持未完成。


## 费用写入执行回复详情链接

创建、修改、过账和逆转四类执行成功回复均返回实际结果批次的 biz://profit-adjustment/{UUID} 详情链接，标题取已验证单据编号。准备只保存意图，待审批/拒绝不声称完成，因此这些结果仍不返回执行链接。MCP 精确检查链接集合的类型、ID、标题和 URI 与已验证执行单据一致，跨批次、错误类型/标题/地址和缺失链接均拒绝。工具说明同步要求仅使用已验证资源链接。

新库 adjustment_write_links_final 的真实 Core HTTP 验证四类费用成功、拒绝、等待审批，并生成 12 份实际响应；MCP 显式加载这些响应，34 项报告通过，新增成功链接四字段篡改及缺失校验。新库 adjustment_write_links_chain 使用真实 Gateway/Core/Read API 和实际 MCP 子进程回归创建、修改、查询、过账、逆转及各类签名负向场景，通过完整链路验证。API 57 项报告通过，其中运行证据以本次显式配置的新库测试为准；两包 all-targets 严格 Clippy、格式、差异及文件大小检查通过，未运行全仓 just ci。首轮旧的空链接断言失败，更新为与 executed 对应的断言后新库重验通过。

证据 /tmp/adjustment-write-links-{api-final,mcp,chain,clippy,size}.log；真实响应 /tmp/adjustment-links-{post,draft,reversal}-final.jsonl。未部署、未代发真实聊天。费用源码流程与详情链接已贯通，下一步核对发布条件并构建配套候选，完成服务端与 Mac/Windows 客户端发布及真实会话验收；完整业务目标仍未完成。
