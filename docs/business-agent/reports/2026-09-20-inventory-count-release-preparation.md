# 盘点代理发布准备（尚未切换生产）

候选源码 `e7c54b8c1`。本记录对应四类盘点意图、121 工具、分页预览与 64 次/900 秒委托配置；不代表全业务目标已完成。

## 构建与环境

线上仍为 `shiyue-business-returns-*:7b5b8f49d`，Compose 层叠到 `compose.returns-candidate-7b5b8f49d.yml`。清理仅限未使用的 `type=regular` 构建缓存，回收约 505 MB，保留 Cargo 缓存、运行容器、卷和已知回退镜像。构建前可用约 3.5 GB；构建脚本低于 1 GiB 时会停止该构建。

源码包 `/tmp/business-counts-e7c54b8c1.tar.gz` SHA-256：`e18c55b651279bd3b440c5b31d22c0d7d61e81c7a6f9f8c912b9bc03283eb7d5`。服务器源码 `/opt/business-platform/releases/counts-e7c54b8c1`，复用服务器 Rust 1.95 单任务构建 Dockerfile。四候选已完成：

| 镜像（标签 e7c54b8c1） | 镜像 ID |
|---|---|
| shiyue-business-counts-gateway | sha256:9926c4934e49e402a5fdc31a640acb9692773372fea4d86826ed15eb0c793a4a |
| shiyue-business-counts-business-core | sha256:ca69794ef1e301166c8bd7c8cfeadb32e7d6fdb0471c40598f312e3187c450d4 |
| shiyue-business-counts-business-read-api | sha256:61a4adc4163f25a05694b055f6d499ddc3a6c4b11124060ebb879ebff29584dc |
| shiyue-business-counts-iam-admin-api | sha256:1784228e24a5304e4558a3ad7f18a00f22c7ee400bbdb3f155371d3e8b9140b5 |

构建日志（服务器）`/tmp/business-counts-candidate-build.log`。

## 数据库副本与候选运行

只读导出生产库至服务器 `/opt/business-platform/shared/counts-rehearsal-e7c54b8c1.dump`，权限 0600；恢复为独立数据库 `counts_rehearsal_e7c54b8c1`。候选迁移 49 → 53 全部成功，5 笔销售订单、0 笔销售退货、0 笔采购退货的计数不变；恢复时盘点任务为 0。未对生产库应用新迁移。

Core 候选以副本连接启动并通过健康检查。销售/采购退货读取正常，既有订单审批预览仍可访问；16 个盘点代理路径对缺少参数/不存在对象的请求正常拒绝。该探测没有创建单据，也不替代此前真实 500 行组件闭环。Trace `13e2ba51-2eb3-4219-8421-c4edbb4c19e9`。日志（服务器）`/tmp/business-counts-{migration-rehearsal,candidate-canary}.log`。

## 保留权限修复的功能回退

不得直接回退到旧 Core 从而恢复已修复的盘点范围问题。`scripts/prepare-business-count-rollback.py` 校验候选路由源文件 SHA-256 后复制源码，只给盘点创建/操作的代理路由加返回 503 的层；权限实现、迁移、其他代理流程和工作台盘点处理均保留。它是暂停盘点代理功能的回退，不是恢复所有旧服务代码，也不自动取消已经冻结的盘点。

源文件摘要 `164562d6b12de72fb7340c0ebdb7d97ab10646d57ae2e26157b5cd465aeaeef1`；暂停版摘要 `c3bcebb8c224f83a91a080034ee6009a5c4ede1b9c4aafc901d5867dc7dab542`。暂停版源码 `/opt/business-platform/releases/counts-paused-e7c54b8c1`。镜像构建与副本演练结果见后续追加。

## Mac 候选

兼容源码 `/Users/aaronli/Projects/Paqiaoli-buzz-v0.5.23` 仅追加盘点 Host 能力、签名类型、分页提示和详情链接。改动前相关文件及 sidecar 备份 `/tmp/business-counts-client-source-before`；保留原有工作区改动。Host 10 项定向测试、链接 23 项测试通过。

- MCP 发布二进制 `/tmp/business-counts-mcp121`，SHA-256 `334e23a64b4eacbe75a9188d9666976cb37d8dbebbf31347e828cdfc796d8f9b`。
- Host 发布二进制 `/tmp/business-counts-host121`，SHA-256 `db4eff45846d80418ad8f48f6a26f3ce1e9b4110247fae27263a14797c9f7703`。
- 应用候选 `/tmp/Pacioli-counts-e7c54b8c1.app`，构建完成后重新进行本地 ad-hoc 签名，`codesign --verify --deep --strict` 通过。原打包结果含无效旧签名，因此不能直接使用原路径。
- 签名后应用 Host SHA-256 `82b85f5dd47366330a0f497aa5ff49396ed62da171152c139b87eaae284a86df`，主程序 SHA-256 `4b4972e69b5e5d12cee09e8d604c9eebb9908291b6dd53b2971d99f69c142962`。

原生 Agent 使用 release MCP 完成 121 个固定工具注册及无业务工具调用的会话。测试脚本改用 production adapter 和不可用的 HTTPS 地址验证注册，避免 release 构建禁止 mock adapter 导致假失败；不访问线上服务。日志 `/tmp/business-counts-{release-runtime,host-compat-test,host-compat-build,links-compat-test,mac-app-build}.log`。尚未替换 `/Applications/Pacioli.app`，未修改真实代理配置、重启助手或发送聊天。

## 发布前待完成

副本核对发现仅有 `inventory_opening:post` / `inventory_opening:reverse` 审批策略，均为 business_admin、1 人、允许发起人、无需跨业务单元；创建/录入使用的 `inventory_opening:create` 策略缺失。新 8 项盘点意图 IAM 权限尚未授予。须准备并验证限定法人授权，以及与既有库存策略一致、保留额外认证条件的创建策略；不得因缺策略而绕过审批。

仍需完成暂停版演练、四服务整体候选验证、授权/策略审阅和配置、生产备份及迁移、服务端/网页/客户端配套切换、实际模型与获准聊天验收。Windows 新客户端覆盖也不能由 Mac 构建证明。全业务写入目标保持未完成。

## 暂停版演练完成

`shiyue-business-counts-paused-business-core:e7c54b8c1` 已构建，镜像 ID `sha256:0f920f13610cfaf59aab2e0bd91311e00c38ad84c9028e4518fbcfd6980a5dde`。在迁移 53 的副本上重跑候选与暂停版：候选的 12 条 POST 路径拒绝缺字段请求（422），4 条不存在对象的 GET 路径拒绝访问；暂停版同样 16 条路径全部为 503。两者的既有订单预览和销售/采购退货读取均正常。

候选复核 Trace `d0941f28-a7c0-4a51-923e-34c3f20512c8`，暂停版 Trace `4f301cc2-54bb-4a5f-8fa6-cfc3f9f30f88`。日志（服务器）`/tmp/business-counts-{candidate-canary-final,paused-build,paused-canary}.log`。测试容器已移除，生产容器保持退货版本。当前服务器可用磁盘约 2.8 GB；不在发布时删除上述候选/暂停版及原退货镜像。

## 四服务启动检查

候选四服务分别启动：Gateway / IAM Admin ready 为 204，Core / Read API health 为 200，均接受正确的 2xx 状态。使用数据库的服务全部指向副本；Read API 不直连数据库，其 HTTPS 上游暂指向不可连接的回环地址，防止探测误连生产。这证明发布二进制可加载对应配置并启动，不是 Gateway → Read API → Core 的完整业务验收。Gateway 试用 64 次/900 秒覆盖配置。日志（服务器）`/tmp/business-counts-services-canary.log`；临时容器和包含凭据的临时环境文件均在 finally 中删除。

## 发布配置已准备

版本固定覆盖文件为 `deploy/business-agent/releases/inventory-counts-e7c54b8c1.yml`，暂停覆盖为同目录 `inventory-counts-paused-e7c54b8c1.yml`。已用服务器当前实际 Compose 文件链分别合并验证，六个服务/迁移任务引用的镜像均存在，Gateway 预算均为 64/900；暂停覆盖只替换 Core 镜像。仅执行 `config` 和镜像检查，未执行 `up`。后续切换须使用 `--no-build`，并先核对镜像 ID，不能因标签丢失而从旧工作目录临时构建。

## 限定授权与审批策略副本验证

新增一次性事务脚本 `deploy/business-agent/releases/inventory-counts-e7c54b8c1-authority.sql`。脚本要求迁移 ≥53、目标 Human 激活且两项父库存授权均在有效期内并限定已审阅法人；复制父授权的数据范围、obligations、起止时间到 8 项固定盘点能力。审批权限仍保留 permission 目录中的 fresh_signed_chat_command。已有任一盘点授权或创建策略时拒绝覆盖，数量不足 8 时整笔回滚。

创建/录入使用的 `inventory_opening:create` 策略复制当前 active 的 `inventory_opening:post` 策略，只调整 action/required_permission；保留角色、人数、自我审批、跨业务单元与额外认证阈值。事务写入部署审计，包含来源策略和每项实际授权范围/条件。副本首次执行审计 Trace `89c33e70-0a18-4f2d-8833-92dcd249e7d7`（首次版本后补强了审计授权快照，最终脚本在回滚事务中再次验证）。

重复执行被拒绝，仍为 8 项授权和 1 条部署审计。在可回滚测试事务中把来源策略设为 2 人、禁止自我审批、要求跨单位、额外认证阈值 12345，并为父授权增加附加条件和 1 小时截止时间；派生策略与 8 项授权逐项保留这些限制。测试最终回滚，未改变原副本配置。日志（服务器）`/tmp/business-counts-authority-check.log`。

候选发布镜像在副本完成创建 → 录入 → 过账，以及第二张盘点创建 → 取消；没有遗留冻结，零差异合成测试的库存数量/价值保持一致。过账盘点 `b34d3e5e-42d1-4c57-aecb-cfc9098fe132`，取消盘点 `06b1de80-c121-4083-8dfe-603a24d7926c`，Trace `ff240da1-58ca-4635-b3a6-af45ff55c6b7`。这是服务凭据下的 Core 组件验收，审批来源事件是隔离测试值，不是真实签名聊天；Gateway 签名链另有前述独立测试。日志（服务器）`/tmp/business-counts-core-workflow.log`。
