# 基础资料发布候选 e51a84b9c

状态：四服务镜像、Mac 候选包及 MCP 发布版完成；生产副本迁移与授权演练通过。尚未切换生产、安装客户端或完成真实聊天验收，完整业务流程目标仍未完成。

## 服务端与副本

源码归档 SHA-256：b28db097c16eb632da2818c344336d130f2d98f44022c9befa481132f7d7bc62。服务器源目录 `/opt/business-platform/releases/master-e51a84b9c`，构建日志 `/tmp/business-master-e51-build.log`。构建进程已结束，四镜像实际存在：

| 服务 | 镜像 ID（sha256） |
| --- | --- |
| gateway | a598fa24aad14883baaec4f53be0890d5959f6b96624a617402b1d6cde2337df |
| business-core | 8d63acf564edffcb64aa435bb4a7d32256077727387cd7419d2acde1bb2b6f48 |
| business-read-api | 9be1de37c789cc04ef1460b548ba51ae126099f3d6b53e8ab4f33bf03b8fc9f8 |
| iam-admin-api | eaa0aa4bc4152cd316f19239336c04636772665567fea427958906d1774426b2 |

使用实际 Gateway 镜像将既有生产副本 `master_rehearsal_ddecf9c0e` 从迁移 56 升至 57，全部 migration success；订单仍为 5，销售及采购退货均为 0。日志 `/tmp/business-master-e51-rehearsal-migrate.log`。没有重新恢复副本或迁移生产库。

授权脚本在该副本成功写入 9 项固定能力及 2 条审批策略，审计 trace 为 `d7d246a8-313d-4a73-baa1-516ad1e334a9`。再次执行在事务内拒绝覆盖，退出码 3，错误为 `master-data grants already exist; inspect instead of overwriting`。

脚本保留法人资料的现有法人范围，商品能力由 Core 继续校验品牌等对象范围；不修改原基础资料读取授权，不新增 Core 权限或对象范围。创建新法人的授权尚未配置，仍属于完整目标的待完成项。

本地隔离库 `master_authority_e51` 验证了审批人数、禁自审、跨业务单元、额外认证阈值、授权义务及一小时有效期的继承；事务回滚后不保留强化测试配置。禁用用户会被拒绝。对应日志 `/tmp/master-authority-{strengthened,final-apply,disabled}.log`。

## 客户端候选

兼容目录 `/Users/aaronli/Projects/Paqiaoli-buzz-v0.5.23` 仅应用 Host 和基础资料链接解析的配套改动，原文件及 sidecar 已备份到 `/tmp/business-master-compat-before-e51a84b9c`。Host 与链接测试通过，Tauri app 构建完成。

候选 `/tmp/Pacioli-master-e51a84b9c.app` 已完成 ad-hoc 签名，`codesign --verify --deep --strict` 通过。包内 buzz-acp SHA-256 为 `9b282856fb1ccb672b895fdbf2fa1021e6e4779e885241664c608979c91e468d`。未替换 `/Applications/Pacioli.app`。

MCP 候选 `/tmp/business-master-client-e51a84b9c/business-read-mcp`，SHA-256 为 `3e9d7317d0c863c37c81d6a62b8475b1998f84dc999795775a774e80113ed06f`。使用已安装 buzz-agent 和候选 MCP 的模拟模型原生回合完成，普通会话 101 工具、指定确认会话 60 工具，均仅暴露固定业务工具。日志 `/tmp/business-master-e51-runtime-{ordinary,approval}.log`。这不代表真实聊天或线上授权链验收。

## 后续发布条件

尚需构建并运行验证 e51 暂停镜像、准备 Web 配套、完成新能力的副本运行时检查，再进行配套生产切换和真实客户端验收。服务器根分区构建后约剩 1.6 GB，继续构建须遵守已有磁盘阈值，不能直接删除生产数据或旧镜像。当前 Compose 文件均为候选覆盖，不能独立启动；暂停覆盖引用的镜像尚未构建。
