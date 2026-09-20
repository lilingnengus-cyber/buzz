# 销售订单暂停与恢复写入

状态：Core 受保护执行、不可变意图及原子审批接口已通过隔离回归；Gateway/Read API/MCP 尚未接入，助手尚未开放，未部署。完整业务流程目标不因此完成。

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
