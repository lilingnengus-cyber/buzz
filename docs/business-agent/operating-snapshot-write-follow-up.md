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
