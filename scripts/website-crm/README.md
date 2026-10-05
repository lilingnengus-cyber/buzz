# 官网线索接入

XiaKe 官网保存原始咨询；本任务每 30 秒拉取待同步项，通过现有 Core 服务 API 创建线索。来源固定为官网，创建人和负责人均为配置的企业用户（生产为李灵能）。无新增公网 Core 路由，无企业凭证进入官网。

网站侧实现位于 `/Users/aaronli/Projects/XiaKe/xiake-universe/server/crm-sync.js`。专用凭证只允许读取待同步咨询和确认 CRM 记录 ID；网站先落库再返回成功。上线前的历史记录默认不进入队列。Core 确认前不确认官网队列；超时、网络故障、回执丢失均使用 `xiake-web:<submission UUID>` 原始幂等键重试，不会重复创建。同步不会改动后续业务状态。需求简报及后续补充留在官网后台，通过 CRM 摘要入口查看。

## 部署

- 将 `run.py`、`sync.py` 安装到 `/opt/xiake-crm-sync/`。
- `/etc/xiake-crm-sync.json` 仅 root 可读，内容含 `token` 与 `owner`。token 必须与 Sites 的 `CRM_SYNC_TOKEN` 一致。
- 安装 service/timer 到 `/etc/systemd/system/`，`systemctl daemon-reload`，`systemctl enable --now xiake-crm-sync.timer`。
- 启动器每次从当前 Core 容器取得内部 IP、端口和现有服务凭证，避免容器重建后地址失效；不打印凭证或联系人信息。
- `systemctl show xiake-crm-sync.service -p Result -p ExecMainStatus` 查看最近结果；`journalctl -u xiake-crm-sync.service` 查看数量或错误类型。
- 停止：`systemctl disable --now xiake-crm-sync.timer`。队列保留；恢复后自动继续。
- 检查失败时先核对网站可达性、两端专用凭证、负责人是否启用且具备 crm:manage，再重启 service。不要更换提交幂等键。

验证：`python3 -m unittest discover -s scripts/website-crm -v`。官网目录 `npm test` 覆盖收件、授权、重复、限流和回执。
