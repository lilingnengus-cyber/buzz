# 退货整批发布准备（尚未切换）

线上只读核对仍为 `734400865` 四服务、迁移 40。退货候选基线 `f78e87f77` 包含迁移 41–49、109 个固定工具及退货详情/冲销报表修复。新增 22 个 Gateway 能力，授权脚本已准备为 `/tmp/business-returns-grants.sh`，限定既有法人；未执行授权。现有 shipment:reverse / goods_receipt:reverse 策略均为 1 人、自审允许，未修改策略。

基线源码已上传 `/opt/business-platform/releases/returns-f78e87f77`，镜像构建任务已启动，日志 `/tmp/business-returns-build.log`（本机）。构建发现同服务器另有 CRM registers 构建，占用共同 Cargo 缓存；未停止对方任务。此基线不是最终发布候选，不能直接覆盖 CRM 的新页面/API。

本分支整合了 CRM 任务的 `645f50c1e`，包括商机、跟进记录、客户联系人独立页面以及抽出的路由模块；保留退货/期初库存详情、库存和财务写入及迁移 35–49。迁移 34 内容一致，无编号冲突。整合范围只在当前工作区，未修改其他任务工作区。

验证：`returns_crm_merge` 的 CRM 数据库闭环、`returns_merge_b2` 的完整 B2 流程通过；CRM、退货详情、期初库存页面共 9 项 Playwright 功能测试通过；Web 构建、Core 严格 Clippy、格式和文件大小门禁通过。日志 `/tmp/business-returns-{crm-core,merge-b2,crm-ui,crm-build,merge-clippy,merge-size}.log`。

待发布工作：确认共享服务器发布顺序、构建整合后的最终候选、备份及迁移演练、兼容新迁移的回退程序、限定授权、前后业务计数、服务/前端/客户端发布与真实会话验收。旧程序缺少新迁移，不视为可直接回切方案。未发送真实聊天消息或执行生产退货业务写入；客户端仍为原版本。

## 演练库与共享发布状态复核

CRM 任务的最新一轮已完成，线上 Core 已变为 `shiyue-business-core:crm-registers-20260919`，Gateway/Read API/IAM Admin 仍为 stock 版本。当前分支的整合提交为 `7b5b8f49d`，其源码包已上传 `/tmp/business-returns-7b5b8f49d.tar.gz`，整合镜像构建脚本 `/tmp/business-returns-integrated-build.sh` 已上传，尚未启动该次构建。

已从在线库执行只读备份，文件 `/opt/business-platform/shared/returns-rehearsal-7b5b8f49d.dump`，权限受 umask 077 保护；恢复到独立数据库 `returns_rehearsal_7b5b8f49d`。恢复后迁移 40 全成功、销售订单 5、销售退货 0、采购退货 0。生产库未执行迁移、授权或业务变更。演练迁移脚本 `/tmp/business-returns-rehearsal.py` 已上传，仅将连接指向该独立数据库；凭据通过容器标准输入传递，不打印到日志或命令参数。

基线 f78e87f77 构建已越过 CRM 的缓存锁等待，正在编译；必须从实际构建进程/日志确认完成后再执行演练，不能因为观察超时而重启。该基线缺少后合并的 CRM registers 更新，只用于演练，不作为直接覆盖线上 Core 的最终版本。磁盘曾复核剩余约 3.3 GB，继续构建前需关注可用空间，不删除现有回退镜像。

## 基线镜像演练通过

四个 `shiyue-business-returns-*:f78e87f77` 镜像构建完成。独立恢复库迁移到 49，全部成功；销售订单仍 5、销售退货与采购退货仍 0。候选 Core 在仅绑定本机 33120 的临时容器中连接演练库成功启动，两类受权限约束的退货读取成功，Trace ID `10f3d900-ab2f-4c8a-b0c3-8374103f1a2a`。临时 Core 已移除，演练库保留；生产库未升级。日志 `/tmp/business-returns-rehearsal.log`。

最终整合候选 `7b5b8f49d` 四服务镜像构建已启动，日志 `/tmp/business-returns-integrated-build.log`，需继续跟进现有进程，不能重复启动。兼容回退源码已复制到 `/opt/business-platform/releases/returns-rollback-7b5b8f49d`，以线上 CRM 源码为基础补入迁移 41–49，尚未编译或验证。

兼容客户端源码 `/Users/aaronli/Projects/Paqiaoli-buzz-v0.5.23` 已通过补丁应用检查后加入 109 工具所需 Host 范围和签名命令支持，原两文件备份在 `/tmp/business-return109-host-backup`。10 项 Host 测试通过，日志 `/tmp/business-return109-compat-host-tests.log`；未替换已安装应用或运行中助手配置。

## 最终候选构建与隔离验证通过

整合候选 `7b5b8f49d` 的四个服务镜像均已构建完成。在迁移 49 的独立演练库中启动最终 Core 成功，两类退货列表均为空，已有销售订单审批预览可读取；Trace ID `6b7fe8bd-e274-4e5d-b165-7a35925d2779`。临时容器已清理。日志 `/tmp/business-returns-final-canary-rollback.log` 的开头保留验证结果，随后为回退镜像构建日志。

兼容回退四服务镜像正在同一日志下构建；当前未完成验证，不可宣称已经具备可用回退。上线前还需运行 `/tmp/business-returns-migration-compat.py` 检查回退 Gateway 的迁移校验，以及 `/tmp/business-returns-canary.py` 检查回退 Core 在迁移 49 的演练库启动和已有订单读取。

Mac 完整候选应用构建成功，路径 `/Users/aaronli/Projects/Paqiaoli-buzz-v0.5.23/desktop/src-tauri/target/release/bundle/macos/Pacioli.app`。已把新 Host `/tmp/business-host-return109` 写入候选包并重新进行 ad hoc 签名，`codesign --verify --deep --strict` 通过。兼容工程的 22 项资源链接测试通过。签名会改变嵌入二进制的文件哈希，不能直接用签名前后全文件哈希相等作为版本校验。已安装应用与助手配置尚未替换，尚无真实会话验收结果。

已核对线上 Core 的 Compose 标签，最后一层为 `compose.crm-registers-20260919.yml`。候选与回退的四服务及两个迁移服务覆盖文件已准备在 `/tmp/compose.returns-{candidate,rollback}-7b5b8f49d.yml`；使用时必须追加到线上完整 Compose 栈末尾。限定授权脚本的审计 actor 已更新为 `deployment:returns-7b5b8f49d`。生产数据库仍未为此次退货版本迁移或授权。
