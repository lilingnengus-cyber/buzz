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

Host 提示补充基础资料查找、专用商品读取、字段保留及显式清空、计量精度询问、不可变字段、签名确认及实际链接规则，并修正顶部审批例外范围。10 项 Host 定向测试通过，提交 `ee7b26b81`。兼容源码已加入该段提示，Host 发布版重建完成，已替换 `/tmp/Pacioli-master-e51a84b9c.app` 内 Host 并重新 ad-hoc 签名；严格深度签名校验通过。当前包内 Host SHA-256 为 `c9b75dbaf19d863c1e64f4b8df4182db38297ed4db454a84af203c356036f3a0`，取代前述旧候选哈希。尚未安装。

下一步是配套生产迁移/授权/服务/Web 切换及真实客户端验收。基础资料启停、新法人授权与其他业务域仍未完成。

## 实际配套发布

生产四服务已从 CRM 6239a7224 切换为上述 e51a84b9c 镜像，3100/3110 readiness 与 3120/3130 health 全部通过。实际 Compose 链从当前容器标签获取，再追加 `/opt/business-platform/app/compose.master-e51a84b9c.yml`。暂停覆盖为同目录 `compose.master-paused-e51a84b9c.yml`，引用已验证的暂停镜像。执行脚本 `/tmp/business-master-e51-deploy.py` 校验候选镜像 ID、旧线上标签、授权 SQL SHA 和磁盘阈值；日志 `/tmp/business-master-e51-deploy.log`。

发布前备份 `/opt/business-platform/shared/before-master-e51a84b9c.dump`（0600，813079 字节）。生产迁移最新为 57 且全部成功，新增 9 项限定用途能力及 2 条保留原限制的审批策略，授权审计 trace `308245b5-4999-4a4f-89c6-201cd5e18bf5`。原法人读取授权不变；创建新法人的授权仍未配置。

Web 已原子切换至 `/opt/business-platform/shared/business-web-a281e3423-1b660e9cf319`，前版 `/opt/business-platform/shared/business-web-154f02bfd-e95318bf8c91` 已保留。入口 `assets/index-ChD_1Fhb.js`，SHA-256 `aade4af64a75f1568cbdf408a50b51b11c32be740431169f17402b70998a2622`；发布脚本的资源与服务检查通过。日志 `/tmp/business-master-e51-web-release.log`。

Mac `/Applications/Pacioli.app` 已安装当前签名候选，严格深度签名验证通过；旧应用保留 `/Applications/Pacioli-before-master.app`，完整备份位于应用数据目录 `backups/master-e51a84b9c`。两条企业助手配置保留 gpt-5.5 模型，使用版本固定路径 `tools/business-agent/master-e51a84b9c/business-read-mcp` 并追加基础资料提示。配置备份权限为 0600。日志 `/tmp/business-master-e51-install-client.log`。

已安装的 Agent/MCP 模拟模型原生回合验证普通 101、指定确认 60 个固定工具，日志 `/tmp/business-master-installed-runtime{,-approval}.log`。Mac 屏幕锁定，未重启或重载运行中的客户端，未发送聊天；安装不等于真实会话已加载。

生产只读检查证明订单预览、退货、盘点查询和四类基础资料详情可用。销售订单仍为 5，原单 `7706b2ff-395f-422c-8794-73619618c304` 仍为 draft/v1/gross 200，基础资料意图为 0。首次核对脚本把销售状态列写成 status 而失败，随后按实际 schema 改为 lifecycle_status 并完成检查；没有为修复检查修改业务数据。未创建生产基础资料测试记录。

后续仍需真实聊天与 Windows 配套验收；新法人授权、基础资料启停并发保护，以及完整业务覆盖文档中其余业务域保持未完成。
