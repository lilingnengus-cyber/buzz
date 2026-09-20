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
