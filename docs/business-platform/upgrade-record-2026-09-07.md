# Buzz 升级验证记录：2026-09-07

## 基线与目标

- Pacioli 主分支基线：`afd833329`（包含 Life 回复、重放及委托去重修复）。
- 原 Buzz 共同祖先：`0720f5380ce8a6c050afac159f8462c06cd51ab5`，桌面版本 `0.5.18`。
- 目标发布：`desktop-v0.5.23`，提交 `b9392d9d78744df365f9276e1ffe8c1baa5ea903`。
- 升级分支：`codex/pacioli-buzz-v0.5.23`。
- 在独立工作树中合并 177 个上游提交；原工作树未提交内容不纳入此次升级提交。

## 适配

- 兼容预演发现 20 个冲突文件；逐项保留上游能力并重新接入 Pacioli 扩展。
- ACP：先解析上游项目权限，再启动扩展授权；保留 Business/Life 回合级临时 MCP、独立会话、禁用标准工具、答复发布及结束回收规则。扩展运行策略适配上游动态基础提示词及线程作用域。
- SDK：Business/Life 答复发布适配新的 `build_message` 表情标签参数，保持原有消息语义。
- 桌面：保留通用扩展 Provider/Layout/Dock 插槽，合入新的未读优先级、指针拖拽和状态展示。
- 打包：Pacioli CLI 包装器继续计算 Business/Life CSP，并调用上游资源隔离包装器；新增测试覆盖两个配置同时生效且位于 Cargo 参数分隔符之前。
- 身份：保留 Pacioli bundle identifier、密钥库及 `.pacioli` / `.pacioli-dev` 数据目录；新 nest 构建身份逻辑使用 Pacioli 的原有目录。
- 移动端：保留 Life 通知去重，使用上游新的统一时间线排序与频道目录状态。
- 依赖：Buzz 使用上游 JWT 10；三个业务身份服务显式保留 JWT 9.3，避免底座更新顺带改变业务鉴权依赖。
- CI：采用上游拆分后的工作流，将 Life MCP sidecar 及 Sherpa 缓存修复迁移至相应子工作流。
- Agent 配置规则：沿用此发布附带的上游规则，没有新增 Pacioli 配置语义。

## 已完成验证

| 检查 | 结果 |
|---|---|
| 完整 `just ci` | 通过（退出码 0，含全仓静态检查、Rust、桌面、原生、Web、移动端测试与构建） |
| `pnpm install --frozen-lockfile` | 通过 |
| Workspace 与 Tauri Rust Clippy / 格式检查 | 通过 |
| ACP 单元测试 | 959/959 通过 |
| Business/Life 六个服务及库单元测试 | 68/68 通过 |
| Buzz DB PostgreSQL 集成测试 | 252 项全部通过；首轮 251 项通过，连接串格式调整后剩余 1 项重跑通过 |
| Life PostgreSQL 安全契约与迁移边界 | 2/2 通过；独立临时数据库已清理 |
| Tauri 原生工作区测试 | 3269 通过，20 项按上游默认配置忽略 |
| Desktop TypeScript | 通过 |
| Desktop 单元测试 | 6587/6587 通过（含新增打包用例） |
| 打包配置专项测试（含新增用例） | 4/4 通过 |
| Desktop lint / 字号 / 公钥显示检查 | 通过（存在上游警告） |
| Desktop E2E 构建 | 通过 |
| Business Dock / IAM / Life Dock Playwright | 16/16 通过 |
| 正式 Desktop 前端及受限功能隔离检查 | 通过，产物选择 OSS 版本 |
| Business Web 测试与构建 | 28/28 通过，构建通过 |
| Web 检查及构建 | 通过 |
| Flutter analyze | 通过 |
| Flutter tests | 2074/2074 通过 |
| Dart 格式检查 | 通过，547 文件无变更 |
| Business 扩展边界 | 通过 |
| Pacioli macOS 构建配置检查 | 通过 |
| Rust 缓存 CI 契约 | 通过 |
| 文件大小门禁 | 通过 |

## 验证与发布状态

完整 `just ci`、原生 Rust 编译与测试、独立 PostgreSQL 契约测试均已通过。未部署数据库迁移、未替换已安装客户端、未更新服务器。上述 mock-bridge 及数据库契约测试不能替代真实 OIDC/IAM/Relay 工作流验收；本次没有执行生产登录或服务器部署验收。

上游主数据库新增迁移推进到 `0044_drop_nip_fi_ledger.sql`；Business/Life 独立服务的迁移在本次上游合并中没有新增。正式升级部署前需按相应数据库的备份与迁移流程执行。

数据库测试运行说明：使用本机 PostgreSQL 与上游 nextest 隔离数据库脚本。首轮唯一失败源于上游测试使用 `strip_prefix("postgres://")`，将管理员连接串从 `postgresql://` 调整为 `postgres://` 后专项通过；没有为此修改产品代码。临时数据库及首轮中断遗留的唯一测试角色均已清理。


## 2026-09-08 原生验收与生产基线补齐

第一版原生候选安装后，已验证原有身份、历史消息与 Business/Life OIDC
登录恢复。Life Agent 的真实消息验收发现网关委托契约不匹配：生产网关已是
`18315cfdf`，原有已安装客户端及 sidecar 还包含 `97af58fe7` 之前的修复，
这些部署过的提交尚未进入 `origin/main`。升级分支因此合并
`97af58fe7`，保留有界查询后预览、绑定凭证续期、私聊收件人恢复及父子行动
等已有行为；没有回退网关或放宽精确确认的单次写入约束。

CI 补充修复均限于验证流程：回复提醒测试显式配置 shell，并新增无 shell
扩展会话不提醒的覆盖；真实 shell 取消测试先响应工具权限请求；假模型取消
测试等待取消确认；数据库故意篡改失败后等待回滚，消除后续 `SKIP LOCKED`
与未结束错误事务的时序依赖。桌面 E2E 分片的安装、构建和测试总预算由
30 分钟调整到 45 分钟，保留所有测试和单项超时。

此阶段完整 Agent 测试 696 项通过；删除存储 PostgreSQL 测试 20 项通过。
已有服务器备份及迁移预演验证主数据库从 32 迁移到 44，生产数据库尚未切换。
本节记录合并时状态；最终安装、远程检查与部署结果以 PR #7 发布记录为准。
