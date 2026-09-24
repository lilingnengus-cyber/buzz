# BizOS 数量与比率显示契约

本契约统一中文 BizOS 回复中的数量、比率和百分比呈现。工具返回的精确十进制字符串是业务依据，展示格式不能用于反算、修改预览或确认摘要。

## 数量规则

- 数量使用千分位。工具返回单位的 `precisionScale`（0–6）时，按该精度显示；没有精度元数据时与 Business Web 一致显示两位小数。
- 示例：带单位的数量显示为 `1,234.50 件`；没有单位时只显示 `1,234.50`。单位名称、代码或符号必须来自当前工具结果，不能猜测单位。
- 普通数量不加正号。只有明确表达库存或数量变化时才显示符号，例如 `+12.50`、`-2.00`。
- 真实零数量明确显示为 `0.00`，或按已知 `precisionScale` 显示对应位数。
- null、缺失值或 unavailable 显示为“—”“缺失”或服务端原因，不能显示为 0.00。
- 使用正常舍入，不截断；格式化前的精确十进制字符串仍是计算、预览和确认依据。

## 百分比规则

- 以下字段由服务端以 0–1 比率传输，展示时乘以 100 并添加 `%`：`taxRate`、`contributionMarginRate`、`managementOperatingMarginRate`、`fulfillmentRate`、`receiptRate`、`paymentRate`、`onTimeRate`、`qualityAcceptanceRate`、`salesReturnRate`、`purchaseReturnRate`。
- 百分比最多保留两位小数并去除末尾零，使用正常舍入：`0.13 → 13%`、`0.075 → 7.5%`、`0.12345 → 12.35%`。
- 真实零比率显示为 `0%`。null、缺失值或 unavailable 显示为“—”“缺失”或服务端原因，不能显示为 0%。
- `turnoverRate` 表示库存周转次数，按工具单位显示为“次”，不乘以 100。字段名含 `rate` 但业务语义不明时，保留原值和工具单位，不自行转换成百分比。

## 已覆盖数量字段

- 通用数量：`quantity`、`totalQuantity`、`sourceQuantity`、`returnedQuantity`、`returnableQuantity`
- 订单与履约：`orderedQuantity`、`requiredQuantity`、`shippedQuantity`、`receivedQuantity`、`cancelledQuantity`、`openQuantity`、`reservationOpenQuantity`、`draftAllocatedQuantity`、`shippableQuantity`、`receivableQuantity`、`orderRemainingQuantity`、`expectedReservedQuantity`、`shortageQuantity`
- 库存：`onHandQuantity`、`reservedQuantity`、`quarantinedQuantity`、`availableQuantity`、`currentOnHandQuantity`、`projectedOnHandQuantity`、`snapshotOnHandQuantity`、`snapshotReservedQuantity`、`snapshotQuarantinedQuantity`、`actualOnHandQuantity`、`varianceQuantity`
- 补货与采购：`minimumOrderQuantity`、`inboundQuantity`、`openRequisitionQuantity`、`projectedQuantity`、`suggestedQuantity`
- 异常事实：`inTransitQty` 及工具返回的其他明确数量字段

金额字段继续遵循 `money-field-contract.md`。计数、天数、版本、序号和百分比不是数量，不套用数量格式。新增字段时必须先确认业务语义、单位来源以及它是 0–1 比率、百分数、次数还是普通十进制值，再更新本清单和契约测试。

自动契约测试位于 `crates/buzz-acp/src/business_agent/tests.rs`，固定精度、分组、符号、零值、缺失值、单位来源、百分比换算及非百分比 rate 边界。

## 2026-09-24 验收记录

- 字段清单核对范围包括 `business-query-contracts`、`business-anomaly-contracts`、`business-action-contracts` 和 `apps/business-web/src/api.ts`。Business Web 的数量格式以两位小数为缺省值；经营概览中的百分比字段按 0–1 比率传输。
- 契约测试先因文档不存在而编译失败；补齐运行时提示和本文档后，`buzz-acp` 的 14 个 Business Agent 契约测试全部通过。
- 离线同模型夹具覆盖数量、变化量、真实零、缺失数量、税率、履约率、利润率、零比率、缺失比率和周转次数，输出 `1,234.50 件`、`+12.50 件`、`0.00 件`、`13%`、`7.5%`、`12.35%`、`0%` 与 `2.5 次`；缺失项没有显示成零。
- 已安装二进制 SHA-256：`285f660593d2899b505f847ab2335bb4433ca898d80b5ef9384e09fd45988758`；BizOS 进程 PID `42288`，启动时间 `2026-09-24 11:13:03 +0800`；客户端显示在线，应用深度签名验证通过。升级前备份位于 `~/Library/Application Support/com.shiyueshizi.pacioli/backups/bizos-quantity-rate-20260924/buzz-acp`。
- 本轮未发送新的聊天验收消息，未调用 Business 工具，没有业务写入，也未进行 Windows 安装验收。
