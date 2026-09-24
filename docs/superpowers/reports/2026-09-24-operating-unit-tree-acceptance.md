# BizOS 法定主体与经营组织树生产验收

日期：2026-09-24（Asia/Shanghai）

## 结果

法定主体与经营组织已经解耦上线。经营组织以单根树表示，经营单元可以持续向下分解；CRM 等业务表单分别选择法定主体和经营单元。

生产运行版本：

- Business Core：`e9d2cab5feac70f9e2fa7343b92ddfa88bc334e8`
- Business Web：`e9d2cab5feac70f9e2fa7343b92ddfa88bc334e8`
- Business Core 镜像：`shiyue-business-candidate-business-core:e9d2cab5feac70f9e2fa7343b92ddfa88bc334e8`
- Business Web 目录：`/opt/business-platform/shared/business-web-e9d2cab5f-f5ee7b98af39`

## 发布与数据保护

- 发布前备份：`/opt/business-platform/backups/bizos-before-operating-tree-20260923T152004Z.dump`
- 追加迁移：`0069_operating_unit_tree.sql`、`0070_independent_operating_unit_directory.sql`
- `_sqlx_migrations` 中 69、70 均成功。
- 迁移后存在 1 个活动根节点、0 个孤儿节点。
- 迁移前后销售订单整行哈希一致：`2bf980668f97b273dd9ccba5cf967b24`。
- 迁移前后采购订单整行哈希一致：`d41d8cd98f00b204e9800998ecf8427e`。
- 销售订单保持 5 笔、含税总额 `203.000000`；采购订单保持 0 笔、总额 `0`。
- Core 切换后 `/health` 返回 `{"service":"business-core","stage":"S1","status":"ok"}`。
- 公网首页和发布资产均返回 HTTP 200。

## 自动化验证

- `cargo fmt --all -- --check`
- `cargo check --offline -p business-core`
- `cargo clippy --offline -p business-core --all-targets -- -D warnings`
- PostgreSQL：`postgres_operating_units` 5/5，通过 `postgres_b1`、`postgres_b2`、`postgres_b3`、`postgres_crm`；业务记录归属扩展后再次通过 `postgres_b2`、`postgres_b3`
- Business Web：34 项测试通过，`tsc --noEmit` 通过，Vite 生产构建通过

## Native 生产验收

在 Pacioli 的 Business Dock 中使用生产账号 `authentik Default Admin` 完成：

1. 打开“核心数据”，确认“法定主体”和“经营组织树”并列显示。
2. 通过 Native 表单依次创建四层路径：
   - `BU_CN_01` 默认业务单元
   - `ACC_DIV_20260924` 验收事业部 20260924
   - `ACC_REGION_20260924` 验收区域 20260924
   - `ACC_TEAM_20260924` 验收团队 20260924
3. 页面显示下级计数 `3 / 2 / 1 / 0`，数据库递归查询显示深度 `0 / 1 / 2 / 3`。
4. 打开“新建商机”，确认“法定主体”和“业务单元”是两个独立字段；经营单元下拉包含根、事业部、区域和团队。
5. 取消未保存的商机表单，没有新增商机或业务单据。
6. 刷新 Native 页面后，四层路径仍完整显示。
7. 客户、供应商和仓库台账使用“法定主体 / 经营单元”并列归属块，不再以箭头表达父子关系；客户记录在 Native 生产页面完成验证。
8. 销售订单详情使用“法定主体 / 经营单元”并列归属块，旧的单独“法定主体 ID”字段已移除；在 `SO-202609-000005` 的 Native 生产详情完成验证。
9. 库存台账生产空态正常；当前生产库存为 0 个仓库商品组合，因此行级归属展示由 PostgreSQL B2 测试验证。采购订单生产为 0 笔，其详情归属由 PostgreSQL B3 测试和前端测试验证。

三次经营单元创建将 `business_core_audit_events` 从 38 增加到 41。

## 后续兼容边界

`business_units.legal_entity_id` 仍以非空兼容列保留一个发布周期，运行时兼容写入也仍保留。所有读模型和业务校验已经不再把该列当作经营组织归属。删除该列前，需要先改写兼容写入并完成一轮生产审计；本次发布不执行破坏性收缩迁移。
