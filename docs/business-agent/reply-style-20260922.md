# BizOS 中文回复优化（2026-09-22）

调整 host contract 的呈现规则：结果优先、中文状态、按业务影响精简草稿边界说明、详情链接嵌入结果、查询记录和追踪号置于末尾、具体且最多一个下一步。确认和拒绝指令原样独立代码块展示，仍需当前签名指令匹配；没有放宽写入权限或审批边界。冲突不能推断之前成功，也不引导重新创建重复单据。

验证：11 项 business_agent::tests 通过；locked release buzz-acp 构建通过；git diff --check 通过。隔离 codex-acp / gpt-5.6-terra 测试使用两组合成夹具、无 Business MCP 连接，成功修改回复使用中文草稿与可点击编号，冲突回复未误报成功。不是生产聊天验收，未发送新的业务消息。

已安装 ACP SHA256：132bd7bb382d5b5979e7bd0a9f8932ee00de2e47d875933c6ee65f0b3d250446。原 ACP 备份在本机 Application Support/com.shiyueshizi.pacioli/backups/bizos-reply-20260922/buzz-acp。应用签名校验通过。

客户端可读取助手管理界面，但多次菜单操作没有展开，坐标点击返回 noWindowsAvailable；尚未验证运行中的 BizOS 已重启加载新组件。下一次重启 BizOS 后需核对新会话回复。未重启整个 Pacioli，以保留当前 LifeOS 会话观察条件。


## 22:45–22:46 实际加载与只读回复验证

通过 BizOS 详情页 Restart agent 成功重启，UI 显示 Restarted 拾玥_BizOS 并恢复 Online。实际新 ACP PID 93676、启动时间 22:45:35，路径 /Applications/Pacioli.app/Contents/MacOS/buzz-acp，SHA256 与上述安装值一致。

发送已获准的“查询最近五笔销售订单”，事件 ba8161acc269fe6e655572b1c22e4b7fbc7b5f39a710a038fbe4e9913c9c364e，返回 Trace 148f45f2-cfa1-4ecc-8bc9-6406ecac59b5。实际回复以“已查到最近五笔销售订单，均为草稿、未预留库存”开头，用五条可点击单号列出金额和日期，末尾保留查询记录与中文“追踪号”，没有输出工具名或重复写入边界清单。真实只读回复已验证，未再次执行写入或确认预览测试。查询回复没有额外给出下一步建议，也未显式列出数据时点；后者仍是呈现规则的待观察项，不应声称模型对全部格式要求均严格遵循。
