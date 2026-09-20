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
