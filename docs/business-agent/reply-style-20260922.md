# BizOS 中文回复优化（2026-09-22）

调整 host contract 的呈现规则：结果优先、中文状态、按业务影响精简草稿边界说明、详情链接嵌入结果、查询记录和追踪号置于末尾、具体且最多一个下一步。确认和拒绝指令原样独立代码块展示，仍需当前签名指令匹配；没有放宽写入权限或审批边界。冲突不能推断之前成功，也不引导重新创建重复单据。

验证：11 项 business_agent::tests 通过；locked release buzz-acp 构建通过；git diff --check 通过。隔离 codex-acp / gpt-5.6-terra 测试使用两组合成夹具、无 Business MCP 连接，成功修改回复使用中文草稿与可点击编号，冲突回复未误报成功。不是生产聊天验收，未发送新的业务消息。

已安装 ACP SHA256：132bd7bb382d5b5979e7bd0a9f8932ee00de2e47d875933c6ee65f0b3d250446。原 ACP 备份在本机 Application Support/com.shiyueshizi.pacioli/backups/bizos-reply-20260922/buzz-acp。应用签名校验通过。

客户端可读取助手管理界面，但多次菜单操作没有展开，坐标点击返回 noWindowsAvailable；尚未验证运行中的 BizOS 已重启加载新组件。下一次重启 BizOS 后需核对新会话回复。未重启整个 Pacioli，以保留当前 LifeOS 会话观察条件。


## 22:45–22:46 实际加载与只读回复验证

通过 BizOS 详情页 Restart agent 成功重启，UI 显示 Restarted 拾玥_BizOS 并恢复 Online。实际新 ACP PID 93676、启动时间 22:45:35，路径 /Applications/Pacioli.app/Contents/MacOS/buzz-acp，SHA256 与上述安装值一致。

发送已获准的“查询最近五笔销售订单”，事件 ba8161acc269fe6e655572b1c22e4b7fbc7b5f39a710a038fbe4e9913c9c364e，返回 Trace 148f45f2-cfa1-4ecc-8bc9-6406ecac59b5。实际回复以“已查到最近五笔销售订单，均为草稿、未预留库存”开头，用五条可点击单号列出金额和日期，末尾保留查询记录与中文“追踪号”，没有输出工具名或重复写入边界清单。真实只读回复已验证，未再次执行写入或确认预览测试。查询回复没有额外给出下一步建议，也未显式列出数据时点；后者仍是呈现规则的待观察项，不应声称模型对全部格式要求均严格遵循。

## 22:59–23:05 预览及失败回复回归

首次真实修改预览（事件 210ce75da546c942db4093c1c4533c2bb2a44fe71e0015c9d5bb9df4cc7fdfbd，追踪号 530c2c0b-0f65-42a6-bb6f-981f0138ec43）展示了修改差异、当前版本及独立可复制确认/拒绝代码块，但错误使用“仅影响管理报表”。已收紧规则：草稿及其预览不影响管理报表或总账，只有实际过账/逆转产生管理利润事实时才能描述相应影响。

旧确认指令首次回归（事件 16a79ce361f2968b2eb4b614bdc9bf36eb08b073cf511fc3277e0b311e8c46bb，追踪号 e0363eea-bdfb-40af-b188-e5b2493f5df1）的审计仅有授权、回复、撤销委托，没有业务工具调用。回复凭历史宣称旧预览已执行，因此该次不算失败流程验收通过。补充规则要求完整确认指令调用本轮匹配无参数工具，并禁止从历史推断当前状态。

修订后再次通过 11 项 business_agent::tests、locked release 构建、签名校验；安装 SHA256 93cfd9c92d7f1c7479fa2629b2aa599aa2e30b3f972cb479286c0b1d2256afe7。上版备份位于本机 Application Support/com.shiyueshizi.pacioli/backups/bizos-reply-preview-20260922/buzz-acp。仅重启 BizOS，新 ACP PID 10162 于 23:04:31 启动，父进程仍为 89180。

失败复测事件 6c2f09f39eb7fe498865e7a03927588bd6978ce3d03c61fecc17a4c6af780ec5，追踪号 7a4e996f-d6e9-4650-aac0-a3814f22dbb0。服务端审计确认 approve_operational_adjustment_update 被调用并失败，随后正常发送回复并撤销委托。客户端实际回复：“本次未执行，未找到或无权访问该旧确认指令对应的记录。”并保留追踪号及下一步建议，没有虚报成功或要求重新登录。失败原因使用工具的合并表述，不能由此进一步认定具体是权限还是失效。

生产只读核对：ADJ-202609-000001 仍为 draft、version 2、金额 1.00，原备注未追加“回复格式验收”，分摊数 0，posted_at 为空。本次只生成预览，未批准新的修改意图。未运行全量 just ci 或 Windows 安装验收。

23:06 预览复测事件 c0a80f9f0acf15d531f0f317998b687423b5133cefec3e0f442fa3ea1d96a72d，追踪号 ee24ebe0-06d5-44fe-940a-ab49978a45a3。审计确认依次读取费用详情并成功 prepare_operational_adjustment_update，随后回复及撤销委托，没有 approve 调用。新回复提供可点击单号、备注追加差异、金额和保留字段说明，明确“本次仅为预览，尚未修改草稿，也未分摊、过账或付款”，不再误称已影响管理报表；确认和拒绝仍为各自独立代码块。再次查询数据库仍为草稿第 2 版、无分摊、未过账。格式仍有待改进：正文未单独显示原草稿版本（确认命令中的 v1 是意图版本），不应把本次验证扩展为全部格式要求已严格满足。

## 23:22–23:29 数据时点与版本区分

回复合同新增两项字段映射：批量查询必须展示 Business 工具顶层 `asOf`（嵌套业务数据的 `dataAsOf` 同样显示为“数据时点”）；修改预览必须在正文展示读取结果中的当前单据版本，并与确认指令的意图版本区分。字段缺失时不得猜测。

首次真实查询复测事件 9d3ab9bae3ca71101dafb8f65729e2bbd9bae918eb900ebe3d04d8af2a198ac0，追踪号 cd00b029-b73a-4542-8a2d-8597922dfebc，仍未显示数据时点。会话工具原始返回确认字段实际为顶层 `asOf`，不是最初规则误写的 `dataAsOf`；修正映射后重新构建、签名并仅重启 BizOS。

修正后的真实查询事件 df80c1ddb19c0664870fa4b925acd51f42dfc2d9bf2299f14d3de9b9501c74e6，追踪号 cf2bec67-9483-4695-bcff-f61f7d05c29d。客户端回复显示“数据时点：2026-09-22 15:28 UTC”，保留五笔订单、查询记录及追踪号；审计确认只调用 `search_sales_orders`。

真实修改预览事件 659d332f1e1cfe5509d84fb3a1ed75649d6c49abbaf36f8ce53a5833fa8ad45c，追踪号 b4e0a509-4733-4be9-b0b3-b354e509820d。客户端回复正文显示“当前单据版本 v2”，确认与拒绝指令显示意图版本 `v1`，并明确尚未修改草稿。审计确认只调用读取及准备工具，没有 approve 调用。

最终安装 ACP SHA256：a98b0a997f326b0065af81ab7931c6f3e24d50aa7e5a595d2057ea7a723fede7。上版备份位于本机 Application Support/com.shiyueshizi.pacioli/backups/bizos-asof-field-20260922/buzz-acp；新 ACP PID 30109 于 23:27:21 启动，Pacioli 父进程保持不变。11 项 business_agent::tests 与 locked release 构建通过。未运行全量 just ci 或 Windows 安装验收。

## 2026-09-23 数据时点本地化

中文 BizOS 回复现在将可解析的 RFC 3339 数据时点转换为 `Asia/Shanghai`，统一显示为“YYYY-MM-DD HH:mm（UTC+8）”。无法可靠解析时保留服务器原值和原始时区，不猜测转换结果；工具未提供时仍不得自行生成数据时点。

隔离模型回归先以 `asOf=2026-09-22T15:06:10Z` 复现旧回复“数据时点：2026-09-22T15:06:10Z”，再验证新合同输出“数据时点：2026-09-22 23:06（UTC+8）”。11 项 `business_agent::tests` 与 locked release `buzz-acp` 构建通过，`git diff --check` 通过。

仅重启 BizOS 后，新 ACP PID 76201 于 17:53:18 启动，安装 SHA256 为 `0df1d76084ba47a12d1eb457704d62f91b5408ac049ac095047e01c69241636b`；上版备份位于本机 `Application Support/com.shiyueshizi.pacioli/backups/bizos-shanghai-time-20260923/buzz-acp`，应用签名校验通过。

真实只读查询事件 `3dc972eeb3eaf3d80921fe3c1e46db1b528c031ec1263c8e05081dbe4e372a2f`，追踪号 `e02f8075-f496-4da5-8f78-918b5374dde4`。客户端回复显示“数据时点：2026-09-23 17:55（UTC+8）”，并返回五笔订单。服务端审计确认唯一业务工具为 `search_sales_orders`，成功返回 5 条；其余事件仅为授权、委托、回复和撤销委托，没有业务写入。未运行全量 `just ci` 或 Windows 安装验收。

## 2026-09-23 业务日期与时间戳规则

中文 BizOS 回复进一步区分日期语义：`orderDate`、`businessDate`、`receiptDate`、`paymentDate`、`dueDate` 等业务日期保持服务端 `YYYY-MM-DD`，不附加时间或时区，也不做时区换算；`asOf`、`dataAsOf`、`createdAt`、`updatedAt`、`occurredAt`、`postedAt`、`reversedAt` 等带时区的运行、审计和状态时间戳转换为 `Asia/Shanghai`，并显示 `UTC+8`。无法可靠解析时保留服务器原值及原始时区。

先增加提示契约测试并确认旧规范失败于缺少“业务日期字段”规则；补充规则后，12 项 `business_agent::tests` 通过。隔离模型夹具同时包含 `orderDate=2026-09-19`、`createdAt=2026-09-19T01:02:03Z`、`dataAsOf=2026-09-22T15:06:10Z` 和 `occurredAt=2026-09-22T15:06:12Z`，实际回复分别显示订单日期 `2026-09-19`、创建时间 `2026-09-19 09:02（UTC+8）`、数据时点及查询时间 `2026-09-22 23:06（UTC+8）`。

安装 ACP SHA256 为 `7d8d78b194d4c5ebd75a785496be44a99eae41ed25407de1c71c973bcaa1925f`；上版备份位于本机 `Application Support/com.shiyueshizi.pacioli/backups/bizos-date-time-rules-20260923/buzz-acp`。签名校验通过，仅重启 BizOS，新 ACP PID 76632 于 22:12:17 启动。

真实只读查询事件 `2c89977ac729c064201612d51f52a88b40c00360e314c40efc3e8e017426791f`，追踪号 `81a7abad-d5a0-4d39-9691-5d9cf5459b31`。客户端回复中五笔订单日期保持纯日期，数据时点显示“2026-09-23 22:12（UTC+8）”。服务端审计确认唯一业务工具为 `search_sales_orders`，成功返回 5 条；没有业务写入。未运行全量 `just ci` 或 Windows 安装验收。
