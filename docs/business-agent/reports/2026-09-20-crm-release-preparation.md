# CRM 配套发布准备（6239a7224）

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

兼容工作树 /Users/aaronli/Projects/Paqiaoli-buzz-v0.5.23 仅应用 Host、提示词和资源解析器 CRM 增量，保留原有修改，修改前四文件备份 /tmp/business-crm-compat-before-6239a7224。Host 10 项、链接 26 项测试通过，Host release 与 Mac 应用构建通过。候选 /tmp/Pacioli-crm-6239a7224.app 已应用本地 ad-hoc 签名并严格验证；打包的 buzz-acp SHA-256 1a377b8ece1624320c28839a0797eef673d867a013a7edc9c955d34f97677f21。不是正式签名公证安装包，未安装或启动。日志 /tmp/business-crm-{release-runtime-ordinary,release-runtime-approval,host-compat-test,host-compat-build,links-compat-test,mac-app-build}.log。

下一步：完成暂停版演练、Compose 核对、生产新备份及迁移/限定授权、四服务与网页及客户端配套切换，再做获准真实聊天和 Windows 验收。主数据、费用、报表、行动、履约细节与纠错范围仍未完成。
