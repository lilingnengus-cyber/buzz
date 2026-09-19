# 企业 MCP 安装包缺口修复

发布清单核对发现 `business-read-mcp` 未进入 Tauri externalBin、统一 sidecar 暂存脚本或 Windows/macOS/Linux 发布构建命令。此前 Mac 使用应用数据目录中的手动配套文件，不能证明新 Windows 安装包具备企业工具。

现已将企业 MCP 加入两个 Tauri 配置、统一暂存脚本、各平台发布与 canary 构建、Pacioli 本地构建计划以及 CI 编译占位文件。显式传入交叉编译目标时，暂存脚本不再额外探测宿主 Rust 工具链。没有修改企业权限或执行业务写入。

`scripts/test-business-sidecar-bundle.py` 实际运行暂存脚本，覆盖 Windows `.exe` 及 macOS/Linux 文件名：缺少 MCP 时失败且不创建不完整暂存目录；提供 MCP 后复制字节正确，Unix 可执行位正确。该检查接入 Windows CI。现有 Tauri 配置测试增加三平台 Business MCP 清单断言。

本地验证通过：三平台暂存测试、Pacioli dev/production 构建计划检查、Shell 语法、全部 workflow YAML 解析和 diff 检查。Tauri Rust 配置测试尚未本地执行，原生 Windows 编译、NSIS 安装和真实会话验收仍未完成；暂存夹具测试不能替代这些验证。

运行时已核对：managed-agent 启动在 PATH 中加入当前应用目录，Business Host 默认命令为 `business-read-mcp`，现有显式命令配置继续保留。Windows Canary 当前仅允许 block/buzz 仓库 main，不能把本分支推送视为 Windows 安装包已生成。下一步应通过适用的 Windows 构建路径生成包含最新 Host/MCP 的安装包并验收；完整业务目标其余工作继续保留。
