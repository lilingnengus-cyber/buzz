# 企业 MCP 安装包缺口修复

发布清单核对发现 `business-read-mcp` 未进入 Tauri externalBin、统一 sidecar 暂存脚本或 Windows/macOS/Linux 发布构建命令。此前 Mac 使用应用数据目录中的手动配套文件，不能证明新 Windows 安装包具备企业工具。

现已将企业 MCP 加入两个 Tauri 配置、统一暂存脚本、各平台发布与 canary 构建、Pacioli 本地构建计划以及 CI 编译占位文件。显式传入交叉编译目标时，暂存脚本不再额外探测宿主 Rust 工具链。没有修改企业权限或执行业务写入。

`scripts/test-business-sidecar-bundle.py` 实际运行暂存脚本，覆盖 Windows `.exe` 及 macOS/Linux 文件名：缺少 MCP 时失败且不创建不完整暂存目录；提供 MCP 后复制字节正确，Unix 可执行位正确。该检查接入 Windows CI。现有 Tauri 配置测试增加三平台 Business MCP 清单断言。

本地验证通过：三平台暂存测试、Pacioli dev/production 构建计划检查、Shell 语法、全部 workflow YAML 解析和 diff 检查。Tauri Rust 配置测试尚未本地执行，原生 Windows 编译、NSIS 安装和真实会话验收仍未完成；暂存夹具测试不能替代这些验证。

运行时已核对：managed-agent 启动在 PATH 中加入当前应用目录，Business Host 默认命令为 `business-read-mcp`，现有显式命令配置继续保留。Windows Canary 当前仅允许 block/buzz 仓库 main，不能把本分支推送视为 Windows 安装包已生成。下一步应通过适用的 Windows 构建路径生成包含最新 Host/MCP 的安装包并验收；完整业务目标其余工作继续保留。

## 原生 Windows 候选已启动

提交 `6c8ad6492` 为 Windows Canary 增加精确仓库 `lilingnengus-cyber/buzz` 与精确集成分支 `codex/life-write-intent-compiler` 的来源校验；仍保留原 main 入口。非 main 分支不保存 Cargo/pnpm 发布缓存。产物只上传短期 Actions artifact，不创建发布标签、Release 或自动更新。

已触发运行 `35471308524`，对应源码 `6c8ad6492ce10079d7365fe35dbfa1b8a000ec0f`。实际 job `105972663733` 已进入 in_progress，来源校验成功、checkout 进行中。须继续检查同一运行，不能因观察超时重启。原生编译、安装包生成及 Windows 客户端验收仍未证明完成。

## Windows 候选构建完成

同一运行 35471308524 已 completed/success，Build sidecars、Build Windows NSIS installer (unsigned)、Upload Windows canary installer 均成功。Artifact 10594095609，名称 buzz-windows-canary-6c8ad6492ce10079d7365fe35dbfa1b8a000ec0f，归档大小 60,611,486 字节。

已下载 Pacioli_0.5.19-test.2_x64-setup.exe，文件识别为 Windows NSIS 自解压安装程序，SHA-256 为 bd00dbacb9fdcd6ac5227428ad2403e647ed1d922161e26eea5b87827be6087e。本地路径 /tmp/pacioli-windows-master-35471308524/Pacioli_0.5.19-test.2_x64-setup.exe。该候选不签名、不自动更新。

源码固定为 6c8ad6492，之后新增的原生 Agent/MCP 101/60 探针步骤不在本次运行中。当前仅完成构建、下载及文件类型/校验值核对；没有在 Windows 安装、检查安装后文件或完成真实会话验收。后续 Core 收货状态修复也未部署，不能把本安装包成功视为完整业务目标完成。
