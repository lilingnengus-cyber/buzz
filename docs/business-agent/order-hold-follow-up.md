# 销售订单暂停与恢复写入

状态：Core 受保护执行、不可变意图及原子审批接口已通过隔离回归；Gateway/Read API、MCP/Host 已接入并完成隔离及原生进程验证；尚未部署和真实聊天验收。完整业务流程目标不因此完成。

## 已完成

将既有 set_hold 移入 sales/hold.rs，保留工作台调用签名及有效请求的幂等哈希。新增 hold_preview 与 set_hold_guarded，快照绑定订单 ID/版本/当前生命周期与暂停状态、法人/客户/业务单元、操作和原因，明确暂停阻止创建及确认出库、不释放库存预留。不可执行状态或空白原因的预览 canExecute=false，实际执行仍拒绝。

受保护幂等摘要同时包含目标订单、操作、输入及完整预览。set_hold_on 可加入后续审批事务，待审批接入后统一提交。获取实际订单行锁后重新检查当前权限及三项范围，并持授权修订共享锁直至提交。幂等回放也检查当前权限；旧哈希跨订单复用被拒绝。工作台的恢复流程继续走相同业务逻辑。

## 验证证据

55439 隔离库 sales_hold_wait_final 的 postgres_b2 完整库存/销售/收付款/并发回归通过。新 helper support/sales_hold.rs 验证篡改摘要拒绝、成功暂停、同键回放版本不变、旧快照拒绝、同键不同快照冲突、撤权后的回放拒绝。

并发用例用实际销售订单行锁阻塞执行，通过 pg_blocking_pids 确认等待，再撤销 place_hold 权限、释放阻塞。执行被拒绝，订单仍 none/v2；恢复权限后正常暂停，原出库阻止及恢复后出库路径通过。测试只写独立库。日志 /tmp/sales-hold-wait-final.log、/tmp/sales-hold-wait-clippy.log；严格 Clippy、cargo fmt 检查、差异检查通过。

## 仍需完成

- 保存不可变暂停/恢复意图、时效、预览与签名确认；意图、投票和业务写入在同一事务提交。
- 当前创建者/审批者权限、对象范围、审批策略和实际等待期间撤权/状态变更保护。
- Gateway 能力、Read API 输入/范围适配、MCP 固定工具、Host 确认隔离、结果回读与订单详情链接。
- 严格确认参数绑定、撤权和并发失败不残留投票/意图执行状态的实际接口回归。
- 服务及客户端配套发布，获准的真实聊天与 Windows 验收。

当前候选 4796f86e3 不含本项新源码；不能以已构建主资料启停镜像作为订单暂停/恢复发布证据。

## 2026-09-20 不可变意图与原子审批

新增迁移 0059：business_agent_order_hold_intents 保存 sales_order_hold_intent 与 sales_order_release_hold_intent，30 分钟有效、禁止更新/删除；登记四项 create/approve 能力，approve 必须 fresh_signed_chat_command。不自动授予能力或创建审批策略。

新增 Core dry preview、prepare、stored preview、approve 四组路由。输入仅允许来源 ID、版本和原因，确认仅使用 ChatApprovalInput，不能另传操作/原因。准备时预览、保存意图和审计同事务；投票、订单修改、事件/审计、请求状态同事务提交。请求人、当前及先前审批者均核对当前身份、权限和精确订单范围；权限期限及意图期限在所有写入/等待后再次验证。

审批策略在事务内锁定，支持自审限制和多人门槛；后续降低策略门槛不会降低已有请求要求。权限证据函数提取到 document_approval/permission_witness.rs，供主资料与订单暂停共用；既有主资料实现未改变业务行为。

真实 PostgreSQL HTTP 回归 order_hold_intents_rejection（55439）通过：dry preview 不落意图、严格输入、幂等准备、同键改变原因冲突、不可变约束、缺策略无投票、篡改确认参数拒绝、来源版本变化拒绝、暂停/恢复真实落库、重复确认冲突、库存现存/预留保持 10/8。实际行锁等待中使意图过期，放行后请求/投票/订单全部回滚；双人审批重新检查创建者与先前审批者撤权，失败不追加投票或改订单；拒绝恢复后保持暂停状态并禁止再确认。

共享权限函数移动后的 postgres_master_intents 在 master_hold_witness_regression 通过。严格 Clippy、格式和差异检查通过。日志 /tmp/order-hold-intents-rejection.log、/tmp/order-hold-intents-clippy-final.log、/tmp/master-hold-witness-regression.log。均为服务端隔离测试，未验证聊天签名链，也未写入生产。

上述“仍需完成”中的持久化意图与 Core 原子投票基础已经完成；等待期间更完整的策略/权限期限测试、Gateway/Read API/MCP/Host 接入、部署与客户端验收仍未完成。生产与已构建 4796f86e3 候选均未包含迁移 0059。

## 2026-09-20 网关与 Read API 接入

Gateway 固定能力由 101 增为 105，新增两类 hold 意图 create/approve；结构化确认和拒绝分别绑定精确家族、意图 ID、版本及摘要。Read API 写入工具由 84 增为 88，新增 prepare/approve_sales_order_hold 与 prepare/approve_sales_order_release_hold，沿用既有写入开关和独立委托校验。

适配器只接受来源订单 ID、版本、原因；确认不允许夹带操作、原因或来源事件身份。Core 预览增加订单头品牌及所有明细的仓库/业务单元/品牌范围。Read API 先逐项检查整个订单范围，再把已检查预览的 SHA-256 放入内部 x-business-preflight-hash；Core 在锁住订单、重新生成快照后比较摘要，只有完全一致才保存意图。确认前再次校验委托范围、绑定快照和摘要；执行结果校验实际订单 ID、目标状态、递增版本、trace，并返回系统销售订单详情链接。

order_hold_adapter_shared（55439）实际 Core HTTP + PostgreSQL 验证通过：两仓库、两行订单，只授权第一仓库时准备返回 403 且意图数不增加；确认同样返回 403。全部范围授权后暂停和恢复成功。预查后改变订单版本，旧摘要保存返回 409，意图数仍为零；错误摘要、操作/状态篡改也被拒绝。真实响应样本 /tmp/order-hold-mcp-corpus-lines.jsonl 共四条，可供后续 MCP 消费验证。

Gateway 定向四项测试通过；Read API 49 项通过，其中本次 order hold fixture 使用了上述真实库，其他未配置可选数据库的 fixture 不作为实际集成覆盖。Core postgres_order_hold_intents 在 order_hold_core_lines 重跑通过。严格 Clippy（lib/tests）、cargo fmt、差异及仓库文件大小检查通过。日志 /tmp/hold-gateway-tests.log、/tmp/order-hold-adapter-shared.log、/tmp/order-hold-core-lines.log、/tmp/order-hold-adapter-clippy-fixed.log、/tmp/order-hold-file-size.log。

同时按既有职责拆分原超过 1000 行的 Read API 主文件为 tool_catalog、core_reads、analytics_results，主入口现不足 500 行；既有读取/分析测试通过。两类测试共用同一 B2 种子模块，避免重复编译模块。

MCP 固定工具、Host 确认隔离、原生进程/真实聊天、配套部署与 Windows 仍未完成。本批未部署，生产缓存清理授权仍未收到，未执行清理。

## 2026-09-20 MCP/Host 与原生进程验证

新增 prepare_sales_order_hold、prepare_sales_order_release_hold，以及两个无业务参数的 approve 工具。准备输入固定 UUID、正版本和用户原因，工具描述与 Host 提示明确暂停保留库存预留、恢复不代表出库；只报告 executed=true 为实际完成，不增加确认按钮。Host 普通请求范围由 61 增至 63，仅增加两项 create；审批范围只从当前人类签名的精确结构化指令提取。

MCP 新增独立 order_hold_result 验证器：固定字段、预览家族/操作/状态/行范围、完整预览哈希、确认/拒绝命令、目标订单详情链接及响应限额。Read API 确认结果附带已验证 preview 和 previewHash，MCP 再与受信任委托中的签名摘要比较，逐项验证执行订单 ID、目标状态和版本递增，防止同时替换结果订单与链接。pending/rejected 不能带业务执行结果。

从真实 Core + Read API 重跑获得四条响应 /tmp/order-hold-mcp-corpus-verified.jsonl（隔离库 order_hold_mcp_verified，55439）。MCP 实际消费全部四条并验证摘要、参数、状态、版本、链接、审批门槛和目标替换拒绝；模拟 pending/rejected 分支同样验证无执行效果。MCP 24 项、Host 定向 10 项、Read API 本项两项实际集成测试通过，严格 Clippy、格式及文件大小检查通过。其他需要可选语料的测试未设置语料时不能计作实际语料验证。

重新构建 buzz-agent/business-read-mcp 后，原生 stdio + 模拟模型探针通过三个配置：普通会话 105 工具；sales_order_hold_intent:approve 与 sales_order_release_hold_intent:approve 各 60 工具，均只包含固定允许集合且完成 prompt。可见性回归验证确认工具零业务参数、普通会话无 approve、确认会话无 prepare/create/update，均低于 128 工具上限。此项不是用户真实聊天或真实模型推理验收。

原生 SHA-256：buzz-agent d96093a3e04f7eaccbc9ca893d5cc2669745520e7e57ef18e03897488773fdc3；business-read-mcp ea762200c2f18a6282ea47ff4b638ae60e4dc5c2d69346991db9aca72c862715。

日志 /tmp/order-hold-mcp-corpus-api.log、/tmp/order-hold-mcp-final.log、/tmp/order-hold-host-tests.log、/tmp/order-hold-final-clippy.log、/tmp/order-hold-mcp-final-clippy.log、/tmp/order-hold-native-{build,ordinary,pause,resume}.log。尚未安装这些原生文件；新 MCP 的确认验证要求本批配套 Read API 返回预览证明，不能单独替换线上旧 MCP。

后续仍需配套 Linux/Windows 候选、迁移/受限授权及暂停回退演练、生产和客户端发布、获准真实聊天与 Windows 实机验收。已有 4796f86e3 镜像不含本批订单暂停/恢复，旧主资料暂停方案也不覆盖订单暂停路由。生产缓存清理问题仍等待此前明确授权请求的答复；本轮未清理或切换生产。

## 配套候选构建已启动（尚未完成）

来源 c186ddf01119acfb545c56dee7b77304874e282b。Linux Business service candidate 运行 35482054779 已启动，Windows Canary 运行 35482058899 已排队；必须读取这些具体运行的最终结果，不能把已启动视为构建成功。两者来源 SHA 已核对一致。

Linux 流程现在同批导出四项服务及 writes-paused Core 镜像。暂停镜像从同一提交的 git archive 生成，仅对 master/order_hold Agent 路由加 503 层，保留迁移和既有工作台行为；manifest 与镜像清单同归档计算校验和。准备脚本继续支持旧版仅 master 模式。临时目录验证旧/新模式的准确路由替换、其他路由保留、来源不变、摘要对应，以及来源摘要不匹配时拒绝生成，均通过。镜像编译和实际 HTTP 暂停验证仍待完成。

这次未切换生产、未安装客户端、未清理服务器缓存。后续需下载校验、隔离迁移和暂停恢复演练，再进行配套发布及实际客户端验收。

## 受限授权脚本隔离演练

新增 order-hold-authority.sql，仅为既有销售暂停/恢复操作员增加四项 IAM 意图权限。要求迁移 59、活跃双层身份、原销售审批授权仅限已核对法人、既有 Core 暂停/恢复权限及法人/业务单元/客户范围；新授权进一步限定业务单元和客户。保留来源 obligations、有效期，审批策略复制现有销售确认的角色/人数/自审/跨业务单元/金额条件，不覆盖已有授权或策略，并记录审计。

本地 55439 新建 order_hold_authority_rehearsal（克隆隔离测试库），仅把部署脚本中四个生产 UUID 替换为 fixture 对应 UUID 后演练；未使用生产数据库。实际生成四项授权，obligations/valid_from/valid_until/法人范围一致；两项策略均保留双人、禁止自审、跨业务单元及 10000 金额阈值。再次执行明确拒绝已存在授权。审计 trace 8e4d9f9c-7c2b-49c9-8052-00758c036abd。此证据尚不能证明生产账号满足全部前置条件，生产克隆演练仍需完成。

新增 order-hold-c186ddf01.yml 和 order-hold-paused-c186ddf01.yml，固定候选来源全 SHA，配套迁移及四服务与双入口暂停镜像。尚未在生产合并或启动。

## 生产数据副本迁移与授权验证

只读检查生产确认：迁移仍为 57；目标账号 business_admin 已有 sales_order:place_hold/release_hold；sales_order:approve IAM 授权仅限预期法人；原销售确认策略为 business_admin、单人、允许自审。新 hold 策略尚不存在。

将新鲜生产 pg_dump 恢复到本地 55439 独立库 order_hold_production_rehearsal，使用本批重建 gateway --migrate-only 成功执行 57→59，再原样运行 master-status-authority.sql 和 order-hold-authority.sql（本次没有替换 UUID）。最终核对八项新增授权、两项复制策略、零 hold 意图。审计 trace 分别 b87a0137-33b0-4314-a1e2-159c9a0269da、f163aa90-85b4-40a3-862b-005d57186c57。

备份 /tmp/order-hold-production-rehearsal.dump 和 .sql 权限 0600。远端 custom dump 版本不被本地 PG16 pg_restore 支持，因此改用 plain SQL，仅移除 PG16 不支持的 SET transaction_timeout = 0；恢复及迁移均成功。这证明生产数据和 SQL 迁移兼容性，不代替生产同版本镜像运行验收。日志 /tmp/order-hold-production-{restore,migrate,authority}.log。生产未改写、缓存未清理。Linux 35482054779 和 Windows 35482058899 最新查询均仍 in_progress。

## 授权失败与等待期间过期保护

授权脚本对来源有效期改用 clock_timestamp，并在新增策略/授权后再次检查来源授权到期；同时锁定四项目标 permission 行，避免校验过程中目标停用或签名义务被并发修改。失败全部回滚。

order_hold_authority_negative 为生产数据副本的独立克隆。六项负向测试均拒绝且新增授权/策略数量保持 0：enterprise user 停用、来源授权已过期、来源授权扩大为 unrestricted、目标权限停用、approve 缺失 fresh_signed_chat_command、准备中来源授权到期（200ms 有效期，校验后等待 400ms）。正常路径通过后主动 ROLLBACK，未发布到生产。本批只改部署 SQL，无需重建正在运行的候选镜像。

## Linux 候选完成与本地进程运行

运行 35482054779 已 success。产物 /tmp/business-hold-candidate-35482054779/business-candidate-c186ddf01119acfb545c56dee7b77304874e282b/，包含四服务和 writes-paused 镜像；全部校验和、来源全 SHA、linux/amd64、65532:65532 用户、entrypoint 和 revision 标签已逐项核对。暂停 manifest 的 pausedModules 精确为 master/order_hold。

business-images.tar.gz 68,461,921 bytes，SHA256 22d56a501f15adf748949f79e2d09692225c44c5bbe6ddc84acf8433ce500651；tar 内文件总计 187,467,617 bytes。尚未加载生产服务器，不能将归档验证当作容器运行验收。Windows 35482058899 此时仍在构建。

重新编译并以临时本地端口启动 Core，连接 order_hold_production_rehearsal：/health 200、现有 /v1/sales-orders 成功、原生产草稿订单的 hold dry preview 明确 canExecute=false。测试后终止了本次进程，无生产调用或业务写入。日志 /tmp/order-hold-core-runtime-build.log、/tmp/order-hold-core-runtime.log。此为 macOS 原生 Core 运行，不替代 Linux 镜像及暂停回退测试。
