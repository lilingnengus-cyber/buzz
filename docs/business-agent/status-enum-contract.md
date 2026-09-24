# BizOS 状态与枚举显示契约

本契约统一中文 BizOS 回复中的状态与枚举呈现。状态语义取决于字段和资源类型；同一个原始值可能在不同业务域表示不同阶段，不能只按字符串做全局替换。

## 基本规则

- 只翻译当前已验证工具结果实际返回的字段和值，不根据日期、金额、数量或关联记录推断状态。
- 不能把未完成状态说成已完成。`pending`、`partial`、`blocked`、`draft`、`counting` 等都不表示业务操作已经执行。
- 未知枚举、清单外值或缺少字段上下文时，保留服务器原值，并用自然中文说明“该状态含义未定义”；不擅自拆分下划线、逐词直译或映射到最相近状态。
- 状态为 null、缺失或 unavailable 时显示“—”“缺失”或服务端原因，不显示成“正常”“成功”或“已完成”。

## 单据与履约

| 字段或资源 | 原值 | 中文 |
| --- | --- | --- |
| 单据生命周期 | `draft` | 草稿 |
| 单据生命周期 | `approved` | 已批准 |
| 单据生命周期 | `confirmed` | 已确认 |
| 单据生命周期 | `processing` | 处理中 |
| 单据生命周期 | `partially_shipped` | 部分出库 |
| 单据生命周期 | `shipped` | 已出库 |
| 单据生命周期 | `partially_received` | 部分到货 |
| 单据生命周期 | `fully_received` | 已到齐 |
| 单据生命周期 | `completed` | 已完成 |
| 单据生命周期 | `cancelled` | 已取消 |
| 库存／费用生命周期 | `posted` | 已过账 |
| 库存／费用生命周期 | `reversed` | 已冲销 |
| 应收应付 | `open` | 未结 |
| 应收应付 | `partially_settled` | 部分结清 |
| 应收应付 | `settled` | 已结清 |
| 核销 | `partially_allocated` | 部分核销 |
| 核销 | `fully_allocated` | 已核销 |
| 销售履约 `fulfillmentStatus` | `unreserved` | 未预留库存 |
| 销售冻结 `holdStatus` | `none` | 正常 |
| 销售冻结 `holdStatus` | `manual_review_hold` | 人工暂停 |
| 采购到货 | `not_started` | 未开始 |
| 采购到货 | `partially_received` | 部分到货 |
| 采购到货 | `fully_received` | 已到齐 |
| 发运状态 | `not_dispatched` | 待发出 |
| 发运状态 | `dispatched` | 已发出 |
| 发运状态 | `supplier_acknowledged` | 供应商已签收 |

关键示例：`unreserved` → “未预留库存”，不能原样暴露给中文用户，也不能说成“库存不足”或“已预留”。

库存盘点状态：`counting` 为“盘点中”，`counted` 为“已盘点、待过账”，`posted` 为“已过账”，`cancelled` 为“已取消”。补货单 `converted` 为“已转采购”。

## 必须结合上下文的值

- 审批结果中的 `pending` 显示“等待审批”；退货的 `inspectionStatus` 或 `workflowStatus` 中的 `pending` 显示“待质检”。二者不能混用。
- 数据质量的 `blocked` 显示“数据受阻”；工作项状态的 `blocked` 显示“工作受阻”。预览中的阻塞项应直接陈述服务端原因，不能仅用笼统状态替代。
- 读取工具顶层状态的 `partial` 显示“部分结果”，并说明分页、警告或缺失范围；数据质量的 `partial` 显示“部分完整”。
- `completed` 在普通工作项或单据生命周期中为“已完成”；退货质检工作流中为“质检完成”。
- `rejected` 在审批结果中为“已拒绝”；只有结果明确 `executed: true` 才能说“已执行”。等待审批不等于已保存、已确认或已过账。

## 行动与异常

- 条件状态：`active`“生效中”、`cleared`“已解除”。
- 复核状态：`unreviewed`“未复核”、`acknowledged`“已知悉”、`in_progress`“处理中”、`resolved`“已解决”、`dismissed`“已忽略”、`reopened`“已重新打开”。
- 建议状态：`suggested`“待确认”、`accepted`“已接受”、`dismissed`“已忽略”、`expired`“已过期”、`superseded`“已被替代”。
- 工作项：`open`“待处理”、`in_progress`“处理中”、`blocked`“工作受阻”、`ready_for_review`“待复核”、`completed`“已完成”、`cancelled`“已取消”、`reopened`“已重新打开”。
- 异常顶层状态：`ok`“正常”、`partial`“部分结果”、`missing_context`“上下文缺失”、`not_found_or_forbidden`“未找到或无权访问”、`upstream_unavailable`“上游服务不可用”、`data_quality_blocked`“数据质量受阻”。

严重程度使用“提示、低、中、高、严重”表达 `info`、`low`、`medium`、`high`、`critical`。置信度使用“低、中、高”，必须标注为置信度，不能与严重程度混淆。

自动契约测试位于 `crates/buzz-acp/src/business_agent/tests.rs`，固定上下文敏感映射、未知值保留规则以及未完成状态的表达边界。新增状态字段时，应同时记录字段、资源类型、允许值及每个值的业务含义。

## 2026-09-24 验收记录

- 状态盘点覆盖 `business-query-contracts`、`business-anomaly-contracts`、`business-action-contracts` 及 Business Web 的 API 类型和 `statusLabel` 映射。
- 契约测试先因本文档不存在而编译失败；补齐提示与文档后，`buzz-acp` 的 15 个 Business Agent 契约测试全部通过。
- 离线同模型冲突夹具同时包含审批 `pending`、退货质检 `pending`、数据质量 `blocked`、工作项 `blocked`、读取结果 `partial`、数据质量 `partial`、盘点 `counted` 及未知值 `awaiting_orbit`。回复分别显示“等待审批”“待质检”“数据受阻”“工作受阻”“部分结果”“部分完整”“已盘点、待过账”，并保留未知原值、说明含义未定义。
- 已安装二进制 SHA-256：`f0b467d5966551438a7ca598a7f6a9c6f9c8eab0e03c526561396ec0e0f5eda6`；BizOS 进程 PID `45383`，启动时间 `2026-09-24 11:18:37 +0800`；客户端显示在线，应用深度签名验证通过。升级前备份位于 `~/Library/Application Support/com.shiyueshizi.pacioli/backups/bizos-status-enum-20260924/buzz-acp`。
- 本轮未发送新的聊天验收消息，未调用 Business 工具，没有业务写入，也未进行 Windows 安装验收。
