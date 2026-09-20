# 基础资料启停原生运行时验证

源码基线 c70f1c86c。本机使用当前源码实际构建 buzz-agent 与 business-read-mcp（debug profile），更新固定工具预期后，通过 scripts/business-agent-runtime-acceptance.mjs 启动真实原生进程、MCP stdio 握手和模拟模型会话。

三个配置均完成 prompt，实际模型可见工具与固定预期逐项一致：

| 配置 | 可见工具 | 确认隔离 |
| --- | --- | --- |
| 普通会话 | 103 | 无 approve 工具 |
| core_master_status_intent:approve | 60 | 只有匹配 Core 启停确认 |
| product_master_status_intent:approve | 60 | 只有匹配商品启停确认 |

三种配置均不超过 128 工具。日志 /tmp/status-native-build.log、/tmp/status-native-ordinary.log、/tmp/status-native-core-approval.log、/tmp/status-native-product-approval.log。模拟模型不连接用户私聊，不代表真实模型业务推理或真实客户端验收，也没有写入生产业务记录。

原生文件 SHA-256：

- target/debug/buzz-agent：d96093a3e04f7eaccbc9ca893d5cc2669745520e7e57ef18e03897488773fdc3
- target/debug/business-read-mcp：00a1ca50f14b25ff8d57a728e7c8a8ccf139f1bcd0dff75b4ace62bafbbc323e

发布前只读核对：生产四服务仍使用 e51a84b9c，旧暂停镜像/覆盖仍属该版本。根分区可用 1,546,014,720 字节，约 1.44 GiB，低于已有 1.5 GiB 构建启动阈值，因此未在生产机发起新构建。未删除任何镜像、缓存、卷或数据。Docker 报告的可回收缓存不等于已经获准删除或无需保留。

后续发布需准备迁移 58 的候选与匹配暂停镜像，在隔离副本验证四项受限能力、现有策略约束、普通读取及完整启停链路，再切换服务与 Host/MCP。可先研究异地构建后传送镜像，避免增加生产编译峰值；仍须验证解包/启动余量。新法人创建权限仍未配置。Windows 候选源码仍为 6c8ad6492，未包含本次启停，不能作为启停 Windows 验收证据。

完整业务覆盖仍未完成；本报告仅证明当前原生 Agent/MCP 的工具发现、会话容量和确认工具隔离。

## 异地候选构建已启动

新增 Dockerfile.candidate，在 Rust 1.95/bookworm 中一次构建 Gateway、Core、Read API、IAM 四服务，保留每个服务的标准入口和非 root 运行用户。GitHub Linux runner 导出共享层镜像归档、源码 SHA、镜像 ID/平台/入口清单及 SHA256SUMS，保存为七天 artifact；不推送部署标签，不操作生产服务。

新 workflow 尚未存在默认分支，gh workflow run 返回 404；随后增加精确仓库/集成分支约束，以及仅两个候选构建文件变更触发的 push 路径。未修改默认分支。YAML 解析与差异检查通过。

运行 35479487131，job 105994575986，源码 4796f86e38a2355b37a9eb836d74e8b8d4285b8b。最后核对 status=in_progress，正在 Build and export four service images。须继续检查同一运行，不因观察超时重复启动。产物、镜像加载、匹配暂停镜像、副本验收和生产部署均尚未完成。

## 启停授权脚本隔离演练

新增 releases/master-status-authority.sql，要求迁移 58、活动身份、两项当前主资料审批策略，以及恰好四项有效的 Core/Product update create/approve 来源授权。脚本按能力逐项复制 data_scope、obligations、valid_from、valid_until，仅写入对应 status 授权及审计；不修改现有审批策略、Core 权限或对象范围。目标已有授权时直接失败，不覆盖。

本机隔离库 status_authority_rehearsal（55439，从隔离 master_status_intents_atomic 克隆）使用固定演练身份、有限期且带附加限制的四项来源授权。脚本成功插入四项；数据库断言四个限制字段与来源逐项完全相同；重复执行被 status grants already exist 拒绝。日志 /tmp/status-authority-rehearsal.log。未在生产执行，尚需在生产副本核验实际来源授权及审批策略。

再次检查同一构建 35479487131 仍 in_progress，未重复触发。候选产物、暂停方案验证及发布继续未完成。
