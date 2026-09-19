# 销售与采购订单剩余量取消发布

服务端版本 `f37e91dc9` 已发布，迁移 0039 成功，Gateway、Core、Read API、IAM Admin API 健康检查通过。新增两类准备意图与两类零参数签名确认，客户端固定工具集合共 74 项。

## 行为

用户选择订单并提供取消原因后，准备 30 分钟有效的不可变意图，绑定当前完整订单及版本、逐行取消量、保留履约量和释放预留量。仅用户发送服务器完整签名确认指令后执行取消。支持全部尚未履约数量，不支持任意选定数量或删除历史。部分履约后关闭剩余量的 completed 状态不能解释为已经全部履约。

## 验证

隔离 Core 闭环覆盖销售/采购草稿取消、部分履约取消、完全履约拒绝，物理库存不变、预留释放、原因审计、幂等准备、同键不同原因冲突、缺策略、确认夹带参数、失效摘要与重复确认。Read API 21 项、MCP 12 项、Gateway 8 项、Host 10 项、严格 Clippy、文件大小门禁及 74 工具模拟模型运行时探针通过。兼容安装分支额外运行 Host 10 项定向测试通过。

线上只读验证既有四类财务查询、销售订单预览及不存在意图拒绝。通过不持久化的取消预览验证 SO-202609-000005：v1、取消数量 2、保留已履约数量 0、释放库存预留量 0。Trace ID `26edcefa-d863-4e4b-98d3-d8af846fcf6b`。

发布后销售订单仍为 5 笔；目标仍为 v1、draft、CNY 200。收款、付款、核销意图、逆转意图、取消意图与审批投票均为 0。没有生产取消或代发聊天。

## 发布与恢复资料

- 镜像：`shiyue-business-cancellation-{gateway,business-core,business-read-api,iam-admin-api}:f37e91dc9`。
- 源目录：`/opt/business-platform/releases/cancellation-f37e91dc9`。
- Compose 追加：`/opt/business-platform/app/compose.cancellation-f37e91dc9.yml`，保留此前 CRM、fulfillment、settlement、reversal overlays。
- 发布前确认当前管理员已有 `sales_order:cancel` 与 `purchase_order:cancel_remaining`；在现有法人范围授予四个意图能力，初始化两项 business_admin、1 人、自审允许策略。每次执行仍须完整签名指令。
- 数据库备份：`/opt/business-platform/shared/before-cancellation-f37e91dc9.dump`（0600）。旧镜像保留为 `shiyue-business-before-cancellation-*:f37e91dc9`；旧程序缺迁移 0039，不能直接回切并假定兼容。
- Mac Host/MCP 已安装并验证应用签名。两条企业助手配置使用固定路径 `/Users/aaronli/Library/Application Support/com.shiyueshizi.pacioli/tools/business-agent/cancellation-f37e91dc9/business-read-mcp`，原模型配置保持。
- 客户端与配置备份：`/Users/aaronli/Library/Application Support/com.shiyueshizi.pacioli/backups/cancellation-74-20260919`。

## 待完成

实际聊天会话是否加载新配置尚未验收；最近一次界面检查明确遇到 Mac 锁屏。后续继续履约逆转、退货、盘点、基础资料、CRM、费用与行动等写入。此次发布不表示完整业务流程目标已经实现。
