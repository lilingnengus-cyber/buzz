# BizOS 日期时间字段契约

本清单覆盖 Business 查询、异常、行动、草稿和审批工具中已公开的日期时间字段。中文 BizOS 回复按字段语义和值类型呈现，不能仅凭 `Date` 或 `At` 后缀判断；`orderedAt` 是已知例外，其类型是业务日期。业务日期字段不做时区换算；带时区时间戳使用 RFC 3339 解析并转换为 `Asia/Shanghai`，显示 `UTC+8`。

## 业务日期

这些值表示企业实际采用的日历日，保持服务端 `YYYY-MM-DD`，不附加时间或时区，也不做时区换算：

- `acknowledgedDate`
- `businessDate`
- `countDate`
- `dispatchDate`
- `dueBy`
- `dueDate`
- `expectedDeliveryDate`
- `findingBusinessDate`
- `inspectionDate`
- `nextFollowUp`
- `orderDate`
- `orderedAt`
- `paymentDate`
- `receiptDate`
- `requestedDeliveryDate`
- `returnDate`
- `reversalDate`
- `shipmentDate`

## 带时区时间戳

这些值表示运行、审计、状态或生命周期时刻。可解析且带时区的 RFC 3339 值在中文回复中转换为 `Asia/Shanghai`，显示为 `YYYY-MM-DD HH:mm（UTC+8）`：

- `acceptedAt`
- `asOf`
- `cancelledAt`
- `clearedAt`
- `completedAt`
- `createdAt`
- `dataAsOf`
- `defaultDueAt`
- `dismissedAt`
- `dueAt`
- `effectiveFrom`
- `effectiveTo`
- `expiresAt`
- `firstSeenAt`
- `generatedAt`
- `lastSeenAt`
- `occurredAt`
- `postedAt`
- `resolvedAt`
- `reversedAt`
- `reviewAfter`
- `startedAt`
- `updatedAt`

无法可靠解析时保留服务器原值及其原始时区，不猜测。清单外字段只有在值明确为 `YYYY-MM-DD` 时才作为业务日期，只有在值是可解析且带时区的 RFC 3339 时间戳时才换算；含义不明时保留原值。

自动契约测试位于 `crates/buzz-acp/src/business_agent/tests.rs`，要求运行时提示覆盖上述全部字段、日期不换算规则、RFC 3339 判断和上海时区格式。新增 Business 工具字段时，应在同一变更中更新本清单、运行时提示与测试分类。

## 2026-09-23 验证记录

字段扫描覆盖 `business-query-contracts`、`business-action-contracts`、`business-anomaly-contracts`、`business-read-mcp` 及 Web 端已消费的 Business API 字段。扫描特别确认 `orderedAt` 的 Rust 类型为 `NaiveDate`，而 `effectiveFrom`、`effectiveTo` 等字段为 `DateTime<Utc>`；因此分类以类型和业务语义为准。

测试驱动验证先把完整字段集加入契约断言，旧提示因缺少 `acknowledgedDate` 失败；补齐运行时清单和本文档后，12 项 `business_agent::tests` 通过。隔离模型夹具覆盖三个业务日期例外（`orderedAt`、`dueBy`、`nextFollowUp`）及四个时间戳（`createdAt`、`effectiveFrom`、`generatedAt`、`dataAsOf`）：日期均保持 `YYYY-MM-DD`，时间戳均转换为 `UTC+8`。

locked release `buzz-acp` 构建及应用签名校验通过。安装 SHA256 为 `c0e0fb187c99c88eaf76cb7838af9dac1f05cd5c74f7b2ca7248244e21a1f4b4`；上版备份位于本机 `Application Support/com.shiyueshizi.pacioli/backups/bizos-date-time-inventory-20260923/buzz-acp`。最终产物重新签名安装后仅重启 BizOS，新 ACP PID 16370，客户端恢复 Online。本批没有发送新的生产聊天消息或执行业务写入；生产订单查询的日期与数据时点实机行为已在上一批验证。
