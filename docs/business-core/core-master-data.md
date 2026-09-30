# 核心数据维护

核心数据中心维护业务闭环共同依赖的五类权威记录：法定主体、经营主体、客户、供应商与仓库。法定主体、经营组织树和集团共享主数据独立维护；销售、采购、库存等业务单据分别选择法定主体、经营单元及所需业务对象。

## 维护规则

- 新增、编辑和状态变更均要求 `business_master_data:manage`；读取要求 `business_master_data:read`。法定主体与经营主体按各自范围过滤，客户、供应商和仓库只按对象范围过滤。
- 编码创建后不可修改。客户、供应商和仓库不保存法定主体或经营主体归属；历史单据保留各自原始维度，迁移时会清空旧主数据上的兼容归属字段而不改写业务事实。
- 新对象自动向创建者授予对应范围；父级必须处于启用状态且已在创建者授权范围内。
- 所有写入使用幂等键、对象级事务锁、乐观版本、审计事件和 outbox 记录。
- 主数据不物理删除。停用后禁止用于新业务，历史业务事实仍保留原始关联。

## 停用影响

停用确认弹窗从实时业务事实读取影响，而不是依赖缓存。法定主体和经营主体检查其自身业务事实；客户、供应商检查未完成订单与经营往来；仓库检查库存余额、未完成履约行和进行中的盘点。阻断项大于零时服务拒绝停用，提示项仅用于经营判断。

## 接口

- `GET /api/v1/core-master-data`
- `POST /api/v1/core-master-data`
- `PUT /api/v1/core-master-data/{resourceType}/{id}`
- `GET /api/v1/core-master-data/{resourceType}/{id}/disable-impact`
- `POST /api/v1/core-master-data/{resourceType}/{id}/status`

浏览器写接口沿用 Business Session、CSRF 与 `Idempotency-Key` 边界。服务侧只读接口为 `GET /v1/core-master-data`。
