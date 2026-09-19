# CRM 配套发布准备（6239a7224）

> 最新状态：四服务、迁移 54、限定授权、网页及 Mac 配套文件已更新；客户端重载、真实聊天和 Windows 验收未完成。下面的发布前状态保留为过程记录，以文末实际结果为准。

状态：候选四服务构建、副本迁移与授权演练、CRM Core 闭环、Mac 候选打包已完成。暂停版正在构建，尚未演练；未切换生产、安装客户端或发送聊天。完整业务写入目标未完成。

## 服务端候选

源码为提交 6239a7224 的 git archive，SHA-256 b5aa423fbee52fad7f07637d24df26fbe0723e08db195edc9d0c77e58b4344da。服务器源目录 /opt/business-platform/releases/crm-6239a7224；四个镜像标签均为 6239a7224。

| 镜像仓库 | 镜像 ID（sha256） |
| --- | --- |
| shiyue-business-crm-gateway | 033047540204da690a786d879ac59569bbca0d46a57e8cbbcb29b568ffac7f9b |
| shiyue-business-crm-business-core | b47bbab834ab0945f57f53414652b7d24e034c7ed37cae9f11db74c9502d93d6 |
| shiyue-business-crm-business-read-api | c39ac72b6f73e52558bb701ded09619edf3d4c9d01434d24c8011fd6771631c3 |
| shiyue-business-crm-iam-admin-api | a5be32ea556c1b39f8a48ecadd8433d9b6d08849e226c38d7d1f26083e001a7a |

克隆生产数据库 crm_rehearsal_6239a7224，备份 /opt/business-platform/shared/crm-rehearsal-6239a7224.dump；候选 Gateway 实际迁移从 53 到 54。四服务使用副本启动，Read API 的外部服务地址指向不可用地址，仅用于健康检查，不能把该检查称作全链路业务验收。

Core 候选验证 12 个 CRM Agent 路由已存在，既有订单预览及销售/采购退货读取正常。副本创建、修改和跟进实际执行得到版本 1/2/3，商机 6085aa8f-639f-4bea-9ee7-21e4e12c6527、Trace 22c27b84-6877-4b9d-a6bd-1759465b12b0。副本中仅新增 1 个测试商机和 1 条跟进，销售订单仍为 5。来源事件为合成值；真实签名验证见前批独立 Gateway 测试，尚无真实聊天证明。

服务器日志：/tmp/business-crm-{candidate-build,migration-rehearsal,services-canary,candidate-canary,core-workflow}.log。

## 权限及审批策略

生产只读核对：目标用户现有 business_admin 角色拥有 crm:read/crm:manage；法人、业务单元、客户分别限定为既有默认实体。IAM 尚无 CRM 授权，crm:manage 审批策略缺失。

部署脚本 [crm-6239a7224-authority.sql](../../../deploy/business-agent/releases/crm-6239a7224-authority.sql) 仅为当前已具有 Core CRM 权限的用户新增 7 项能力（crm:read 和三个意图的 create/approve）。以已审阅 sales_order:approve 的法人限定及授权条件为上界，再收窄到明确业务单元和客户；拒绝未审阅的父范围结构。Core 仍独立核查实时权限，未关联客户的线索仍允许 customer=null。

crm:manage 审批策略复制 sales_order:confirm 的角色、审批人数、自我审批、跨业务单元及额外认证条件。此为明确的部署策略配置，不是 migration 自动授权。副本首次执行审计 Trace 5bbc7223-0c4c-423e-b669-2f28ba7b3d93。重复执行拒绝；在回滚事务中强化父策略为 2 人、禁自我审批、跨单位、额外认证阈值 12345，并加入授权义务及 1 小时截止时间，派生策略及 7 项授权均保留。最终 7 项授权、1 条部署审计。尚未在生产执行。

服务器日志：/tmp/business-crm-authority-{apply,check}.log。

## 回退与客户端

[暂停源码准备脚本](../../../scripts/prepare-business-crm-rollback.py) 严格校验输入路由 SHA，唯一改动为将 crm::routes() 包裹为返回 503 的中间件；保留迁移 54、CRM 事务权限修复、工作台 CRM 及其他业务域。本地验证只修改一个文件，错误输入哈希被拒绝。暂停源目录 /opt/business-platform/releases/crm-paused-6239a7224，构建日志 /tmp/business-crm-paused-build.log，启动进程 PID 706968；后续必须检查实际进程及镜像，不能凭 PID 文件推断仍在运行。此记录时未完成构建或路由演练。禁止直接回退至缺少 CRM 事务权限修复的旧 Core。

候选及暂停 Compose 覆盖在 deploy/business-agent/releases/crm-6239a7224.yml 与 crm-paused-6239a7224.yml；尚未执行生产 Compose 合并/切换。发布时必须核实完整文件链及镜像 ID，再使用 --no-build。

发布版 MCP SHA-256 68ba5c94da4d6860acc5bd417516e5ca0df40f124b4d3c0b64265d5052d36a74。原生 runtime 验证普通回合 95、指定确认回合 58 个工具，不是同会话加载全部 129。Host/MCP 必须配套安装以传递 BUSINESS_AGENT_APPROVAL_SCOPE。

兼容工作树 /Users/aaronli/Projects/Paqiaoli-buzz-v0.5.23 仅应用 Host、提示词和资源解析器 CRM 增量，保留原有修改，修改前四文件备份 /tmp/business-crm-compat-before-6239a7224。Host 10 项、链接 25 项测试通过，Host release 与 Mac 应用构建通过。候选 /tmp/Pacioli-crm-6239a7224.app 已应用本地 ad-hoc 签名并严格验证；打包的 buzz-acp SHA-256 1a377b8ece1624320c28839a0797eef673d867a013a7edc9c955d34f97677f21。不是正式签名公证安装包，未安装或启动。日志 /tmp/business-crm-{release-runtime-ordinary,release-runtime-approval,host-compat-test,host-compat-build,links-compat-test,mac-app-build}.log。

下一步：完成暂停版演练、Compose 核对、生产新备份及迁移/限定授权、四服务与网页及客户端配套切换，再做获准真实聊天和 Windows 验收。主数据、费用、报表、行动、履约细节与纠错范围仍未完成。

## 实际发布与生产核对

暂停镜像 shiyue-business-crm-paused-business-core:6239a7224 构建完成，镜像 ID sha256:ee11c470e33f310126a3154ee6abc7bb675bdb8399144129af8164e1ec2de5f5。副本验证 12 个 CRM Agent 路由均返回 503，既有订单预览、销售/采购退货读取正常，Trace e6a06ecf-4600-48f1-9670-51122b6530fd。服务器日志 /tmp/business-crm-paused-canary.log。实际 Compose 文件链追加候选/暂停覆盖合并检查通过，迁移任务及四服务镜像均与上述固定 ID 对应，委托预算 64 次/900 秒。

生产备份 /opt/business-platform/shared/before-crm-6239a7224.dump，0600，808645 字节。迁移成功到 54；限定授权事务创建 7 项授权与 CRM 审批策略，生产审计 Trace 309ab844-752b-4e67-8aff-7ce6fab34d54。策略与来源 sales_order:confirm 一致：business_admin、1 人、允许本人、不要求跨单位、无额外认证阈值。随后 --no-build 切换四服务至 6239a7224，全部健康。覆盖文件 /opt/business-platform/app/compose.crm-6239a7224.yml；回退覆盖同目录 compose.crm-paused-6239a7224.yml。服务器发布日志 /tmp/business-crm-deploy.log。

生产只读验证 Trace 15d6c308-2567-43a6-bbb9-b2421f51ca4e：现有商机列表 1 条，盘点及盘点选项 0，销售/采购退货 0，原订单预览正常。SQL 核对迁移 54、7 项授权及审计中 7 个授权快照；CRM 意图 0、商机 1、隔离测试标题商机 0、销售订单仍为 5；原订单 7706b2ff-395f-422c-8794-73619618c304 保持 draft、v1、gross_amount 200。服务器 /tmp/business-crm-live-reads.log、本地 /tmp/business-crm-production-proof.log。未创建生产业务记录。

网页已原子切换为 /opt/business-platform/shared/business-web-154f02bfd-e95318bf8c91，之前的静态目录和回退指针保留。入口 assets/index-Cb_7VJm3.js，SHA-256 f497884e963702722f884bc18b40cc9f00fb8e314c5085430aa492886acf0ab0，公开资源哈希及 Core/IAM 健康检查通过。日志 /tmp/business-crm-web-release.log。详情路由的两项 Playwright 功能验证见 CRM 读取批次证据。

/Applications/Pacioli.app 已替换并验证签名，两个企业助手配置仍使用 gpt-5.5，新 MCP 固定在 ~/Library/Application Support/com.shiyueshizi.pacioli/tools/business-agent/crm-6239a7224/business-read-mcp。配置/完整应用备份位于同一应用数据目录 backups/crm-6239a7224，/Applications/Pacioli-before-crm.app 也保留。新安装 Host/MCP 哈希与候选一致，CRM 提示词已追加；没有重启应用。安装日志 /tmp/business-crm-install-client.log。

验收脚本增加 BUSINESS_AGENT_TEST_BINARY，以实际已安装 buzz-agent 和 MCP（不是工作树 debug agent）分别加载普通 95/确认 58 个工具，模拟模型回合通过。日志 /tmp/business-crm-installed-runtime-{ordinary,approval}.log；不代表真实聊天或 Host 会话重载。系统会话状态仍为 CGSSessionScreenIsLocked=true，未绕过锁屏或读取密钥。已请求手动解锁及代发精确只读 CRM 验收消息的授权；未收到前不发送。

剩余：客户端重载及真实只读/获准写入聊天、Windows 配套版本与验收；完整业务目标其余域继续保留。
