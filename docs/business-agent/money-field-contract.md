# BizOS 金额显示契约

本契约统一中文 BizOS 回复中的金额呈现。Business 工具以精确十进制字符串和 ISO 4217 三字母币种代码传递金额；格式化只用于展示，不能改变计算、预览摘要或确认依据。

## 显示规则

- 币种代码位于金额前，例如 `CNY 1,234.50`。不使用 `¥`、`$` 等可能歧义的符号。
- 金额显示千分位和两位小数，使用正常舍入，不截断。负数写作 `CNY -1,700.00`。
- 真实零金额明确显示为 `CNY 0.00`。
- null、缺失、成本不可得或 unavailable 显示为“—”“缺失”或服务端原因，不能显示为零。
- 工具没有给出币种时不猜测。不同币种逐组显示，不能跨币种合计。
- `expectedAmountMinor`、`creditLimitMinor` 等最小货币单位字段不能直接作为主币金额。仅根据工具定义和配对币种换算；当前 CNY 规则为 100 分 = `CNY 1.00`。币种或换算规则不明时保留原值并标注“最小货币单位”。
- 数量、比率、税率、利润率和百分比不是金额，不添加币种代码。

## 已覆盖字段

标准 `{ amount, currency }` Money 对象及以下金额字段使用本规则：

- 收入与订单：`revenue`、`netRevenue`、`shippedRevenue`、`orderAmount`、`salesOrderAmount`、`purchaseOrderAmount`、`shippedSalesAmount`、`receivedPurchaseAmount`
- 单据金额：`grossAmount`、`subtotalAmount`、`netAmount`、`taxAmount`、`discountAmount`、`originalAmount`、`settledAmount`、`openAmount`、`allocatedAmount`、`unallocatedAmount`、`unappliedAmount`、`totalAmount`
- 收付款与账龄：`shippedAmount`、`invoicedAmount`、`receivedAmount`、`paidAmount`、`outstandingAmount`、`overdueAmount`
- 成本与价格：`unitPrice`、`unitCost`、`averageUnitCost`、`currentAverageUnitCost`、`projectedAverageUnitCost`、`provisionalUnitCost`、`productCost`、`totalCost`、`expectedCostAmount`、`inventoryCostAmount`、`provisionalInventoryCost`、`issuedProductCost`、`surplusUnitCost`
- 库存价值：`inventoryAmount`、`inventoryValue`、`inventoryValueAsOfGeneration`、`currentInventoryValue`、`projectedInventoryValue`、`endingInventoryValue`、`varianceValue`
- 利润与费用：`grossProfit`、`contributionProfit`、`managementOperatingProfit`、`freight`、`commission`、`discount`、`customerRebate`、`supplierRebate`、`platformFee`、`otherDirectCost`
- 履约、退货与预览：`salesAmount`、`expectedReceivableAmount`、`expectedInventoryCost`、`expectedTaxAmount`、`expectedPayableAmount`、`salesReturnAmount`、`purchaseReturnAmount`、`returnLossAmount`、`scrapCostAmount`
- 异常影响：`impact`、`impactByCurrency`
- 最小货币单位：`expectedAmountMinor`、`creditLimitMinor`

`contributionMarginRate`、`managementOperatingMarginRate`、`taxRate`、各种 `Quantity` 字段及 `sequenceValue` 不属于金额字段。

自动契约测试位于 `crates/buzz-acp/src/business_agent/tests.rs`。它要求运行时提示固定币种、分组、小数精度、零值、缺失值、负数、最小货币单位和多币种边界。新增 Business 金额字段时，应在同一变更中更新本清单和相应模型回归夹具。

## 2026-09-23 验收记录

- 字段清单核对范围：`business-query-contracts`、`business-anomaly-contracts`、`business-action-contracts`、`business-read-mcp` 和 `apps/business-web/src/api.ts`。Business Web 的现行权威格式同样使用币种代码、千分位和两位小数。
- 契约测试先在缺少 `ISO 4217` 规则时失败；补齐运行时提示与本文档后，`buzz-acp` 的 13 个 Business Agent 契约测试全部通过。
- 离线模型夹具同时覆盖正数、真实零、负数、缺失值、多币种和最小货币单位，得到 `CNY 1,234.50`、`CNY 0.00`、`CNY -1,700.00`、`USD 25.00`，且未把缺失成本显示为零。
- 已安装二进制 SHA-256：`366d4ff1f95f0bd8913c50d96db8e901ccde21999525f515d00ceabc4d7613eb`；进程 PID `38399`，启动时间 `2026-09-23 23:10:51 +0800`；应用深度签名验证通过。升级前备份位于 `~/Library/Application Support/com.shiyueshizi.pacioli/backups/bizos-money-contract-20260923/buzz-acp`。
- 真实客户端只读查询事件 `9044b453745675bbacec6b6635090780414ab433361c2f6a32410039856171ce` 返回五笔销售订单，金额为 `CNY 200.00`、`CNY 1.00`、`CNY 1.00`、`CNY 0.00`、`CNY 1.00`，验证币种、两位小数与显式零值。响应事件为 `1d8dad486b224b7d087d6624b584df9511977580d43719db93e59ec960a12438`，追踪号为 `de83662f-b8ad-4f82-8da2-1b050f6344fc`。
- 服务端审计仅记录 `search_sales_orders` 成功读取 5 条结果，没有业务写入。本轮未进行 Windows 安装验收。
