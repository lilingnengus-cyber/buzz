# LifeOS 会话恢复：定时只读核查

## 2026-09-22 15:57 Asia/Shanghai

结论：发现到期前建立的新会话，但因 Mac 锁屏，无法完成页面可用性验收，也不能排除人工触发。**不宣称真实自动续期完整通过。**

- 核查前确认 Pacioli 已运行，PID 89180，启动于 2026-09-22 14:57:56；该进程未在基准之后重启。
- 主程序 SHA256 与预期一致：e7d612803a3de2a26f42fe01a000b54f584829bf266d2f228fb6779193fa63c8。
- 基准 workbench：d0c8548c-3146-4daf-b345-deacc04129ea，创建 14:58:16.285555，到期 15:54:23。
- 新 workbench：f315d9c0-df30-4bdc-8f88-604c3171553f，创建 15:52:54.141947，到期 16:52:27，status=active、revoked_at=NULL。
- 新 embed：226e17c1-59d5-4157-a5b9-6c4e08626f76，创建 15:52:55.414768，到期 16:52:27，status=active、revoked_at=NULL。
- 旧 workbench/embed 数据库 status 仍为 active，但 expires_at 已过；不能仅凭 status 判定仍有效。
- Computer Use 读取现有应用返回 Mac 已锁定、无法自动解锁；未取得到期后页面证据。未点击登录、重连或刷新，未修改业务记录或会话到期时间；数据库仅读取 id、created_at、expires_at、status、revoked_at，未读取令牌或哈希。

新会话建立时间符合到期前续期预期，但上述只读字段无法证明触发来源，且缺少页面可用性证据。本次自动化 lifeos 已按要求暂停。下一步需用户解锁 Mac 并保持 Pacioli 现状，再只读核对 LifeOS 页面；若会话再次到期或期间发生人工登录，需要重新建立基准。
