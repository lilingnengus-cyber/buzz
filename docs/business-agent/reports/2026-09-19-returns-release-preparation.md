# 退货整批发布准备（尚未切换）

线上只读核对仍为 `734400865` 四服务、迁移 40。退货候选基线 `f78e87f77` 包含迁移 41–49、109 个固定工具及退货详情/冲销报表修复。新增 22 个 Gateway 能力，授权脚本已准备为 `/tmp/business-returns-grants.sh`，限定既有法人；未执行授权。现有 shipment:reverse / goods_receipt:reverse 策略均为 1 人、自审允许，未修改策略。

基线源码已上传 `/opt/business-platform/releases/returns-f78e87f77`，镜像构建任务已启动，日志 `/tmp/business-returns-build.log`（本机）。构建发现同服务器另有 CRM registers 构建，占用共同 Cargo 缓存；未停止对方任务。此基线不是最终发布候选，不能直接覆盖 CRM 的新页面/API。

本分支整合了 CRM 任务的 `645f50c1e`，包括商机、跟进记录、客户联系人独立页面以及抽出的路由模块；保留退货/期初库存详情、库存和财务写入及迁移 35–49。迁移 34 内容一致，无编号冲突。整合范围只在当前工作区，未修改其他任务工作区。

验证：`returns_crm_merge` 的 CRM 数据库闭环、`returns_merge_b2` 的完整 B2 流程通过；CRM、退货详情、期初库存页面共 9 项 Playwright 功能测试通过；Web 构建、Core 严格 Clippy、格式和文件大小门禁通过。日志 `/tmp/business-returns-{crm-core,merge-b2,crm-ui,crm-build,merge-clippy,merge-size}.log`。

待发布工作：确认共享服务器发布顺序、构建整合后的最终候选、备份及迁移演练、兼容新迁移的回退程序、限定授权、前后业务计数、服务/前端/客户端发布与真实会话验收。旧程序缺少新迁移，不视为可直接回切方案。未发送真实聊天消息或执行生产退货业务写入；客户端仍为原版本。
