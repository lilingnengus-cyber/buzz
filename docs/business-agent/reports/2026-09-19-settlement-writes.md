# 收付款确认、核销意图与单据定位发布

## 发布范围

服务版本 `07e4a083b`，包含客户收款/供应商付款记录确认、不可变应收/应付核销意图及签名确认、四类来源/目标单据的受限分页查找。MCP 候选及已安装版本均为 60 个固定工具；Host 提示另含 `05611f5b8` 的语义修正。核销不发起银行交易。

## 验证

- Core 隔离 PostgreSQL 闭环：两类收付款确认、两类核销、金额与版本绑定、幂等、撤权、非法确认字段、行锁等待期间版本冲突、重建意图后精确核销、重复确认拒绝；四类查询的编号/ID/版本/余额及分页。
- 查询合同 8 项、Read API 20 项、MCP 12 项、Gateway 8 项、Host 定向 10 项通过。相关服务严格 Clippy 与 `just file-size-check` 通过。
- `buzz-agent` 与模拟模型运行时探针证明 60 工具集合；不能据此宣称真实聊天验收已通过。
- 线上四类金融单据查找及原销售订单预览只读请求成功。Trace ID：`2632f043-d003-4bb4-9831-1735b8a49919`。
- 发布后业务数据：销售订单 5；收款/付款/核销意图/审批投票均 0。`SO-202609-000005` 仍为 draft、v1、CNY 200。

## 部署

四个服务镜像为 `shiyue-business-settlement-{gateway,business-core,business-read-api,iam-admin-api}:07e4a083b`，源目录 `/opt/business-platform/releases/settlement-07e4a083b`。Compose 在原有 CRM 与 fulfillment 配置后追加 `/opt/business-platform/app/compose.settlement-07e4a083b.yml`，保留已部署 CRM 与前端。四服务健康检查通过。

数据库已应用 0036/0037。为既有业务管理员初始化收款确认、付款确认、应收核销和应付核销四个策略（1 人、允许本人、无自动执行）；授予该用户现有法人范围内的 12 个配套 IAM 能力。策略不会绕过完整签名确认，未新增实际业务单据。

备份：`/opt/business-platform/shared/before-settlement-07e4a083b.dump`（0600）；旧服务镜像保留为 `shiyue-business-before-settlement-*:07e4a083b`。旧镜像缺少新迁移，不能直接回切并假定兼容。

客户端使用原有 macOS 兼容源码编译 Host，替换已安装 App 的 `buzz-acp`，保持其他客户端功能；应用签名校验通过。两个 managed agent 配置改用：
`/Users/aaronli/Library/Application Support/com.shiyueshizi.pacioli/tools/business-agent/settlement-07e4a083b/business-read-mcp`。

客户端与配置备份在：
`/Users/aaronli/Library/Application Support/com.shiyueshizi.pacioli/backups/settlement-60-20260919`。

## 尚未完成

尚未执行真实客户端聊天验收，不能证明运行中的会话已加载新配置；本次未代发任何聊天消息。完整业务写入仍缺核销/收付款逆转、订单取消、退货、盘点、基础资料、CRM、费用及行动等清单项目，详见完整流程覆盖表。该发布不是整体目标完成证据。
