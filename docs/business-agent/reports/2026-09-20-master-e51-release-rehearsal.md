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

## 发布镜像运行时补充验证

e51 Core 候选实际连接 `master_rehearsal_ddecf9c0e`，通过隔离容器及仅回环端口 33120 运行。订单预览、两种退货读取，以及法人、业务单元、客户、SKU 的完整记录读取均成功。四类 master intent 各三个路由共 12 项检查通过：缺少幂等键的准备请求返回 400，不存在意图预览返回 404，缺少审批字段返回 422。日志 `/tmp/business-master-e51-canary.log`。

随后在同一副本真实准备并确认创建一个计量单位，重放准备请求得到同一意图，执行结果为 executed=true，新增对象 `054f3674-0f84-4af1-8222-10631effa194`，完整详情可读。日志 `/tmp/business-master-e51-write-canary.log`，执行脚本 `/tmp/business-master-e51-write-canary.py`。仅副本发生测试写入；本次直接测试内部 Core 服务接口，使用明确标记的合成来源 ID，不是签名 Gateway 或真实聊天验收。

暂停镜像采用流式源码构建，避免另存一份服务器源码目录。暂停路由文件输入和输出 SHA 与先前已审核版本一致；构建开始前检查至少 1.5 GiB，运行中低于 1 GiB 自动终止。日志 `/tmp/business-master-e51-paused-build.log`。构建及暂停路由运行结果须以随后实际检查为准。

尚需构建并运行验证 e51 暂停镜像、准备 Web 配套、完成新能力的副本运行时检查，再进行配套生产切换和真实客户端验收。服务器根分区构建后约剩 1.6 GB，继续构建须遵守已有磁盘阈值，不能直接删除生产数据或旧镜像。当前 Compose 文件均为候选覆盖，不能独立启动；暂停覆盖引用的镜像尚未构建。

## 暂停回退与配套检查完成

暂停镜像 `shiyue-business-master-paused-business-core:e51a84b9c` 构建成功，镜像 ID 为 `sha256:621f4da73c80e50c3c3b548b39d2cb3e865d88d0291069fda550cc1b59c696de`。实际连接副本验证全部 12 个基础资料意图接口返回 503；订单预览、销售/采购退货读取、法人/业务单元/客户/SKU 完整详情仍正常。日志 `/tmp/business-master-e51-paused-canary.log`。构建后服务器剩余约 1.07 GiB；未删除旧镜像、缓存或生产卷。

四服务各自连接隔离副本的启动检查通过：Gateway/IAM readiness 204，Core/Read API health 200。上游地址在启动检查中替换为不可达本地地址，避免连接生产服务；这项检查仅证明启动健康，不能作为跨服务签名链验收。日志 `/tmp/business-master-e51-services-canary.log`。

Web 配套 `tsc --noEmit && vite build` 通过，入口 `index-ChD_1Fhb.js`、样式 `index-15V03VvM.css`，尚未发布。日志 `/tmp/business-master-e51-web-build.log`。

Host 提示补充基础资料查找、专用商品读取、字段保留及显式清空、计量精度询问、不可变字段、签名确认及实际链接规则，并修正顶部审批例外范围。10 项 Host 定向测试通过，提交 `ee7b26b81`。兼容源码已加入该段提示，Host 发布版正在重建；之前签名的 Mac 候选仍需替换新 Host 并重新签名，不能把旧包当作已包含本次提示修复。

下一步是完成客户端候选更新、配套生产迁移/授权/服务/Web 切换及真实客户端验收。基础资料启停、新法人授权与其他业务域仍未完成。
