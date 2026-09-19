# 基础资料配套发布准备（ddecf9c0e）

状态：候选四服务构建已启动；本地 MCP 发布版及工具容量验证通过。未迁移生产、配置新权限、切换服务或替换客户端。完整业务目标仍未完成。

## 源码、构建和回退

候选为已推送提交 ddecf9c0e 的 git archive；本地与服务器路径 /tmp/business-master-ddecf9c0e.tar.gz，SHA-256 68e14648d9b83ed36ea170282eb0a5c876b0c16d29a88cb260a0c8c3dbdf947a。服务器源目录 /opt/business-platform/releases/master-ddecf9c0e。复用上一版经验证的 business-platform.Dockerfile（Rust 1.95、单构建作业、四个服务），不使用旧的 Rust 1.88 agent Dockerfile。

服务器 /tmp/business-master-build.sh 正在依次构建 gateway/business-core/business-read-api/iam-admin-api，目标标签 shiyue-business-master-{service}:ddecf9c0e。启动后已实际检查进程 1026998 与 Docker build 子进程 1027030；后续须重新检查进程、日志和镜像，不能依据此记录或 PID 文件推断仍在运行。日志 /tmp/business-master-build.log。首次构建前可用 2.2 GB，提取后约 2.0 GB；每个镜像前要求至少 1.5 GiB，构建期间低于 1 GiB 则停止。未清理生产卷、镜像或构建缓存。

候选 Compose 覆盖 [master-ddecf9c0e.yml](../../../deploy/business-agent/releases/master-ddecf9c0e.yml) 沿用 64 次/900 秒委托配置，须追加到当前实际 Compose 链，不能单独使用。暂停覆盖 [master-paused-ddecf9c0e.yml](../../../deploy/business-agent/releases/master-paused-ddecf9c0e.yml) 仅替换 Core 为保留当前权限修复的暂停镜像；该暂停镜像尚未构建和验证。

[暂停源码脚本](../../../scripts/prepare-business-master-rollback.py) 校验 document_approval.rs 的 SHA，再将 master::routes() 包裹为返回 503 的中间件，保留普通读取及工作台人工页面。已在完整候选源码副本执行，逐文件比较证明只改这一文件；错误 SHA 被拒绝且未创建目标目录。输入路由 SHA 76af97d58974b476a92ae236fd53997d03f06b63d4559444c138a63d2048363d，暂停路由 SHA 2ed59ab6edf3d5892abdfd86144492e1ce4c5dddd21c349e27be24ecb494987f。本地暂停源 /tmp/business-master-paused-ddecf9c0e；尚无暂停运行时证据。

## 线上只读核对

线上四服务仍为 CRM 6239a7224，SQLx 最新迁移 54；候选需迁移 55、56。目标用户既有 Core 权限包含 business_master_data:read/manage 和 business_product_master:read/manage。两类 master manage 审批策略不存在；现有 sales_order:confirm 策略为 business_admin、1 人、允许本人、不要求跨单位、无额外认证阈值。

当前 IAM business_master_data:read 仅限定法人 ea9d9cef-5408-4f86-a34c-afe4604f1754，无其他义务或到期。全局产品资料的完整记录读取不含法人维度，当前严格范围交集会拒绝该读取，因此不能简单照搬法人限定策略或声称产品完整链路可用。发布前需完成可审阅的授权方案及副本演练：保留实际 Core 对象范围约束，明确全局资料和法人资料的 IAM 读取/创建/修改权限边界，不在生产静默扩权。此次没有修改任何权限、审批策略或业务记录。

## 客户端候选

MCP 发布版备份 /tmp/business-master-client-ddecf9c0e/business-read-mcp，SHA-256 596afe4544bb756dc9f717f1417a8b2e0c99de5ab1f6f8c822672d661d7b5c71；旁边 JSON 记录来源及未安装状态。使用 /Applications/Pacioli.app/Contents/MacOS/buzz-agent 与该候选 MCP 执行模拟模型原生回合，普通会话 100 工具、指定确认会话 59 工具通过。日志 /tmp/business-master-installed-agent-{ordinary,approval}.log。这不是安装、Host 重载或真实聊天验收。

下一步：先确认候选构建实际完成并记录镜像 ID；演练迁移 54→56、限定授权和暂停回滚；准备兼容 Host/桌面链接解析及 Web 配套文件，再切换并进行获准的真实客户端验收。启停保护与其他业务域继续保持未完成。

## 演练候选构建及迁移结果

ddecf9c0e 四服务构建已正常完成，原构建进程结束；镜像 ID 如下。因新增独立商品读取能力及迁移 57，该版本仅用于演练，不作为最终上线候选。

| 服务 | 镜像 ID（sha256） |
| --- | --- |
| gateway | 96ad3da6fdf4ad8c7a019638a2a9bd4668453c577da10ecf3a4e6722ad997758 |
| business-core | b0412cfb512b0f7fba2d80f2428c17839c16b19c475ac487be7f81cd3330a46a |
| business-read-api | d8e1314e99a49ebf43ea92ad724dd85894079eec8c27b99cc84794f28a07d807 |
| iam-admin-api | 4c8e1e7de212ed6f1b304654b883ed685f891d3d19992625d1d6f5ea80ed4a74 |

新增生产副本 master_rehearsal_ddecf9c0e，备份 /opt/business-platform/shared/master-rehearsal-ddecf9c0e.dump（0600、813078 字节）。候选 Gateway 的实际 --migrate-only 成功将副本从 54 升至 56，全部 migration success；订单 5、销售/采购退货各 0、商机 1。创建副本脚本最后查询误用了商机表名而报错，副本恢复本身已成功；随后只重做正确的只读核对并执行迁移，没有重复创建副本或恢复。日志 /tmp/business-master-rehearsal-{prepare,migrate}.log。服务器剩余约 1.9 GB，未删除旧镜像或卷。

独立商品读取方案已在源码接入，保留现有法人读取授权；尚需配置精确新权限并在副本演练。包含该方案的 Gateway 新库签名测试、真实 Core 读取闭环与 101/60 工具原生回合通过，详见基础资料跟进文档。旧的 100/59 发布 MCP、暂停源码及 ddecf9c0e 镜像不能混作最终配套版本。暂停运行时、最终迁移 57、生产切换、客户端兼容打包和真实聊天仍未完成。
