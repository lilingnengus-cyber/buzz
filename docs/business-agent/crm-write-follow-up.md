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
