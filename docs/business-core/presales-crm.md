# 简洁售前 CRM

入口：企业工作台「业务闭环 → 售前 CRM」，页面 `/#crm`，嵌入页 `/embed/crm`。

企业客户售前采用一张商机列表和详情页。阶段为新线索、沟通中、报价中、已成交、已流失。可以填写潜在客户公司和联系人，或关联有权限访问的已有客户。每次跟进保留沟通内容、作者、时间、当时阶段和下一步；下次跟进日期支持「今天及逾期」筛选。预计金额可为空；已成交/已流失不进入待跟进筛选。成交后可转到现有销售订单模块手工录单。

这是 Business Core 的业务扩展，复用其既有 HttpOnly 会话、同源/CSRF 检查、操作限流和 PostgreSQL。没有新增 Relay HTTP API，也没有浏览器本地数据冒充持久化。当前不包含聊天 Agent 工具、自动建销售订单、报价单、营销自动化或提醒推送。

## 数据与访问

迁移 `0034_presales_crm.sql` 创建 `crm_opportunities`、`crm_followups`；不修改订单、库存、应收。`crm:read` / `crm:manage` 授予现有 Business Core 的 `business_admin`、`s1_operator` 角色；IAM 目录登记能力但不自动授予 IAM 主体。访问仍必须命中法人主体、业务单元以及已关联客户的当前数据范围。新商机默认由当前账号负责；初版同范围的有权用户共享访问，没有另设负责人分配体系。

主体创建后固定。关联客户必须归属同一法人和业务单元。主数据停用后不能创建/保存引用它的新内容，但旧商机和跟进历史仍可在授权范围内查看。所有修改使用现有命令幂等表、版本检查、审计与 outbox，同一事务保存跟进和下一步。浏览器在不确定网络结果后的相同内容重试复用幂等键；改变内容会生成新键。没有删除入口。

列表先过滤权限，再做文本/阶段/跟进日期筛选和 50 条分页。搜索把 `%` 等字符作为字面量。跟进历史每页 100 条，可查看更早记录。下次跟进按浏览器本地日历日期比较，避免 UTC 跨天偏移；这是待办筛选，不会自动发送提醒。

## 验证

- `pnpm --dir apps/business-web check`、`test`、`build`。
- `cd apps/business-web && pnpm exec playwright test --config=playwright.visual.config.ts crm.functional.spec.ts`：新建、已有客户选择、金额、阶段跟进、刷新、筛选、窄屏及错误恢复。
- `cargo clippy -p business-core --all-targets -- -D warnings`。
- 使用独立临时 PostgreSQL，设置 `BUSINESS_CORE_CRM_TEST_DATABASE_URL` 后运行 `cargo test -p business-core --test postgres_crm`：真实迁移、持久化、重复请求、版本冲突、权限撤销、范围隔离、零订单副作用。设置 `BUSINESS_CORE_DATABASE_URL`、32 字符以上 `BUSINESS_CORE_SERVICE_CREDENTIAL` 和 `BUSINESS_WEB_ORIGIN` 时额外检查浏览器路由拒绝未登录访问。

生产只验证入口与读取，不自动创建测试商机；端到端写入使用本地隔离测试库和浏览器 fixtures。生产源码以已部署 master-search 版本为基线选择性移植 CRM，避免把尚未上线的聊天审批功能混入本轮。新迁移生效后，回滚旧 Core 时须使用包含新迁移目录的兼容构建，不能直接恢复缺失迁移版本的旧镜像。

## 2026-09-19 发布验收

代码提交 `1e43e3dcd`、`0118dadf9`，个人 origin 分支 `codex/presales-crm`。Core 镜像 `shiyue-business-core:crm-20260919`，附加部署文件 `/opt/business-platform/app/compose.crm-20260919.yml`，源码 `/opt/business-platform/releases/crm-20260919`。迁移 34 success，服务健康。网页原子发布 `business-web-0118dadf9-a336c4cd5a93`，入口 JS SHA256 `3e6025b5e7cf3700e13b15280713907339c65a5b0697635f083091a2ecf9d9ee`，静态资源及 IAM/Core 检查通过。

通过已安装的 `/Applications/Pacioli.app` 刷新 Business Dock，当前 authentik Default Admin 正常登录。点击新菜单「售前 CRM」打开 `/#crm`，列表正常读取空态，新建表单显示五阶段、默认法人主体、默认业务单元、客户公司/联系人/金额/下一步/日期字段。未保存表单；现有客户关联回填由本地 Playwright 验证，未将原生下拉菜单的自动化限制误报为线上选择成功。生产核对 CRM 商机与跟进均为 0 条，已有销售订单为 5 张，本轮未新增销售订单。

最终检查：31 项网页单元测试、2 项 CRM Playwright 场景、真实 PostgreSQL 闭环与无会话浏览器路由拒绝检查、网页类型/金额展示检查和构建、Core 全目标严格 Clippy、Rust fmt 均通过。本轮未运行仓库全量 just ci，未创建 PR。
