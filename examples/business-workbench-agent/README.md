# 企业工作台代理助手

这是独立于 LifeOS 和只读 `business-query` 的 Persona。开放查询、客户新增与六类单据草稿；不包含付款执行或通用主数据修改。

## 接入条件

使用独立 Agent 身份、会话与进程，遵循 `docs/business-agent/operations.md` 的专用 Host 配置：

- `BUSINESS_AGENT_READ_ENABLED=true`
- `BUSINESS_AGENT_DRAFT_WRITE_ENABLED=true`，仅在当前用户已获相应 create 权限时使用。
- `BUSINESS_CHAT_APPROVAL_ENABLED=false`
- `BUSINESS_ACTION_ENABLED=false`
- `LIFE_EXTENSION_ENABLED=false`、`LIFE_AGENT_READ_ENABLED=false`、`LIFE_AGENT_WRITE_ENABLED=false`、`LIFE_CHAT_HIGH_RISK_WRITE_ENABLED=false`（不注入 LifeOS 凭据）。
- `BUZZ_ACP_HEARTBEAT_INTERVAL=0`

Gateway/API URL 和服务凭据通过现有部署的秘密配置注入，不写进 Persona。使用 `buzz-agent` 的专用运行时，按现有运维文档配置模型提供方。不要把 Persona 导入普通通用工具运行时后就视为隔离完成。

服务端仍负责签名事件校验、身份绑定、当前权限交集、短期委托、幂等与审计。Persona 不是权限控制。

## 验收

1. 用户绑定企业身份后，查询本人有权访问的订单，核对详情链接。
2. 无相应权限的查询不泄露记录；缺必填字段的草稿请求先询问。
3. 在明确标识的测试业务范围创建销售订单草稿，核对状态、字段、Trace 与详情页。
4. 重放同一事件不重复创建；委托过期或撤销后调用失败。
5. LifeOS 退出登录不影响企业授权；企业 Agent 不获得 LifeOS 工具。

聊天“确认”绑定操作版本与预览的接入是后续工作。现有审批协议要求服务器验证的 `/approve ...` 命令，不能仅靠提示词将任意“确认”视为业务审批。
