# 出库、收货与期初库存逆转发布

服务端版本 `734400865` 已发布，迁移 0040 成功，Gateway、Core、Read API、IAM Admin API 健康检查通过。新增三类来源查询、三类准备意图与三类零参数确认，固定工具集合共 83 项。

## 行为与验证

用户选定来源并提供原因后，准备 30 分钟有效的不可变意图，绑定来源、订单、往来版本及库存数量、成本、预留、隔离与最后流水。展示影响并等待完整签名确认；事务取得锁后重新核对关联状态。有核销、有效退货、后续库存流水或库存约束时拒绝执行。

隔离 Core 闭环覆盖三类正常逆转、影响回读、原因审计、过期和不可变意图、撤权、缺策略、失效摘要、重复确认及真实行锁等待期间库存变化。负向控制证明跳过余额比较会导致错误执行。合同 8 项、Gateway 8 项、Read API 23 项、MCP 13 项、Host 10 项、兼容安装分支 Host 10 项、严格 Clippy、文件大小门禁和 83 工具模拟模型运行时探针通过。

线上只读验证三类库存来源查询成功（当前均为 0 条），不存在的逆转意图返回带 Trace ID 的 404。既有财务查询、销售订单预览与不持久化取消预览通过。Trace ID：`e2bfd71c-061a-49c2-8a09-abc63afe333d`。

发布后销售订单仍为 5 笔，SO-202609-000005 仍为 v1、draft、CNY 200。收款、付款、核销意图、财务逆转意图、取消意图、库存逆转意图与审批投票均为 0。生产环境未执行业务逆转。

## 发布与恢复资料

- 四服务镜像：`shiyue-business-stock-{gateway,business-core,business-read-api,iam-admin-api}:734400865`。
- 源目录：`/opt/business-platform/releases/stock-734400865`；追加 Compose：`/opt/business-platform/app/compose.stock-734400865.yml`。
- 在已有法人范围授予六个准备/确认能力，初始化三项 business_admin、1 人、自审允许策略；每次执行仍要求完整签名确认。
- 数据库备份：`/opt/business-platform/shared/before-stock-734400865.dump`；旧镜像保留为 `shiyue-business-before-stock-*:734400865`。旧程序缺少迁移 0040，不能直接回切并假定兼容。
- Mac Host/MCP 已安装，应用签名、Host 可执行代码段与 MCP 文件摘要通过核对。两条企业助手配置指向固定路径 `tools/business-agent/stock-734400865/business-read-mcp`；实例缺少的 env_vars 从同名定义补齐。
- 客户端和配置备份：`/Users/aaronli/Library/Application Support/com.shiyueshizi.pacioli/backups/stock-83-20260919`。

## 待完成

真实聊天会话加载及签名确认交互尚未验收；最近一次界面检查遇到 Mac 锁屏，本次未发送聊天消息。后续继续退货、盘点、基础资料、CRM、费用与行动等写入。采购退货 dispatch/ack 的供应商范围仍需核对。完整业务流程目标尚未完成。
