# m2g 检查记录

日期：2026-10-19。环境、验收二进制及源码指纹见 [environment.json](environment.json)，逐项结果及证据 SHA-256 见 [manifest.json](manifest.json)。本目录只归档合成需求现场的白名单文件，不含真实凭据、用户会话库、完整临时应用目录或 WebView 数据。

## 本地检查

`npm run verify:reliability -- --output /tmp/tokage-m2g-local-final`：退出码 0，12 项必要检查通过、0 失败、0 跳过。原始退出码、命令与统计见 [local/summary.json](local/summary.json)，逐项日志与 runner.log 在同一目录。

- 前端：39 个文件，309 项通过。
- Rust workspace（含 doc-tests）：591 通过，0 失败，27 ignored，1 filtered。忽略项不是模型验收通过；过滤的是可受环境开关影响的 smoke readiness 测试。
- TypeScript、生产构建、i18n（1637 keys）、IPC（134 条命令，0 个未豁免无生产调用缺口）、CLI 基线 / fixture 指纹和 m2c / m2e 真实 Git 归档核验通过。
- m2f 脚本回归 8 项、m2g 驱动与证据回归 9 项通过。收尾补充拒绝空原生事实的检查后，单独复跑 m2g 的全部 9 项及严格里程碑门禁，仍通过。
- 普通本地入口不发模型请求；本轮额外的真实桌面命令在下节单独记录。GitHub Actions 仍未启用。

## 真实桌面异常路径

对两个确切版本的现有二进制分别执行：

```sh
python3 scripts/run-m2g-crash.py CLAUDE_CODE --allow-paid --cli-path /absolute/path/to/claude-2.1.296
python3 scripts/run-m2g-crash.py OPENCODE --allow-paid --cli-path /absolute/path/to/opencode-2.0.29
```

[Claude](claude-crash/verified-crash.json)、[OpenCode](opencode-crash/verified-crash.json)均通过：真实工具事件后完整桌面 SIGKILL，宿主退出 -9；同数据目录重新启动，旧进程组消失，重复继续不新增 turn receipt，T1 BLOCKED、T2 PENDING，无交付；原始仓库 HEAD / 文件保持不变。SQLite 导出分别保留原 attempt 与未知费用 gap。OpenCode 原生会话可见性为 unknown，没有据此假定请求未发生或免费。

首次 Claude 真实 OAuth revoked 见 [进度](claude-auth-failure/progress.jsonl)，强杀触发前失败，用户重新登录后才另起现场复验。真实认证拒绝的安全阻断通过，不代表首轮强杀场景通过。

首次 OpenCode 强杀后的 [失败结果](opencode-initial/first-result.json)、[启动进程回收日志](opencode-initial/startup.log)和 [只恢复探测](opencode-initial/recovery-probe-result.json)一并保留。验收驱动改为优先使用界面已恢复的流水线后，在全新现场重跑完整链路；没有以原现场的只恢复探测替代最终通过。

## 正常路径、Git 与独立断言

两组正常路径均退出 0：每组 3 个任务完成、4 条实际用量记录、8 次门禁通过，最终只修改三个目标模块。OpenCode 提交 `baac4ba9992ead4125d1b2e7bb2dfb8f5b9bee4c`，Claude 提交 `0dc78cb9f9d7b26001119e9404768b094cca246c`。Claude 累计结构化计量尚待 m3b 校准，不解释为账单或节省。最终结果在各自 `*-natural/` 中单独记录；包括原始需求、自动规划、最终状态、独立检查、路由、门禁、数据库归因、交付补丁、Git 提交日志和 bundle。正常路径与强杀路径使用同一验收构建；本轮未重新执行 m2b 的 310 秒隐藏窗口探测，该历史结果保留其原日期和边界。

从仓库根目录执行以下命令，在全新的临时检出中还原归档提交、核对补丁内容及允许的三个文件，运行独立原始 oracle，不调用模型：

```sh
python3 docs/reliability-evidence/m2g/replay-deliveries.py
python3 scripts/verify-m2g-evidence.py --require-complete
```

失败尝试按失败保留，并记录由哪个新现场通过解决；未测项有独立原因，不能因为整体限定验收通过而计作通过。完整桌面其他强杀阶段、真实远端断网 / 额度耗尽、其他授权模式及平台仍未测，范围解释见 [m2g 记录](../../reliability-m2g.md)。m3 尚未开始。

严格门禁通过：36 条矩阵记录中 26 passed、2 failed（历史尝试，resolvedBy 指向通过的新现场）、8 untested（明确范围限制）。未测不计通过，已解决的历史失败不从归档消失；不存在未解决的必需桌面验收阻塞。
