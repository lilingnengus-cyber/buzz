# 核销与收付款逆转发布

服务端发布 `e1df57c1d`，迁移 0038 已成功，Gateway、Business Core、Read API、IAM Admin API 四服务健康检查通过。包含核销历史读取、四类不可变逆转意图和零参数签名确认；共 70 个固定工具。

## 已验证

隔离数据库验证两类来源的完整顺序：核销历史查询、核销逆转、余额恢复、收付款逆转、待核销归零，以及原因审计、准备后版本变化、幂等、重复确认、不可变记录与越权过滤。相关单元测试、严格 Clippy、文件大小检查与 70 工具模拟模型运行时探针通过。

线上只读请求验证四类收付款/往来查询、原有销售订单预览，以及核销历史入口对不存在来源的 404 拒绝（响应带 Core Trace ID）。Trace ID：`714afaf7-ce95-4429-94a5-37f054e3d80b`。正向核销历史及逆转操作仍由隔离数据库证明，未在生产创建记录测试。

发布前后销售订单均为 5 笔，收款、付款、核销意图、逆转意图、审批投票均为 0；`SO-202609-000005` 仍是 v1、draft、CNY 200。

## 配置与备份

- 镜像：`shiyue-business-reversal-{gateway,business-core,business-read-api,iam-admin-api}:e1df57c1d`。
- 源目录：`/opt/business-platform/releases/reversal-e1df57c1d`。
- Compose：在原 CRM、fulfillment、settlement 配置后追加 `/opt/business-platform/app/compose.reversal-e1df57c1d.yml`，保留前端与 CRM。
- 初始化四个 reverse 策略，沿用 business_admin、1 人、自审允许；授予现有管理员法人范围内的 8 个意图创建/签名确认能力，未扩大 Core 既有业务权限。仍需完整人类签名指令。
- 数据库备份：`/opt/business-platform/shared/before-reversal-e1df57c1d.dump`（0600）；原镜像保留为 `shiyue-business-before-reversal-*:e1df57c1d`。旧镜像缺少迁移 0038，不可直接回切并假定兼容。
- Mac Host 配套已安装并通过应用签名校验。两条 managed agent 配置更新至 `/Users/aaronli/Library/Application Support/com.shiyueshizi.pacioli/tools/business-agent/reversal-e1df57c1d/business-read-mcp`，模型保持原配置。
- 客户端及配置备份：`/Users/aaronli/Library/Application Support/com.shiyueshizi.pacioli/backups/reversal-70-20260919`。

## 尚未完成

真实客户端会话尚未验证加载新版。本次通过 Computer Use 检查已安装的 `/Applications/Pacioli.app`，工具明确返回 Mac 锁屏、无法自动解锁；已请求用户手动解锁。没有代发聊天消息。

订单取消候选版本与迁移 0039 尚未部署，其他履约逆转、退货、盘点、基础资料、CRM、费用和行动等工作继续计为未完成。本次发布不等于完整业务流程目标已实现。
