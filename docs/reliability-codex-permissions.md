# m2g-r2：Codex 工作区执行权限闭环

日期：2026-10-19。用户明确要求修复权限子节点；本轮固定 Codex CLI 0.162.0 / `gpt-6-luna`，不回退 Haiku，不修改登录与全局配置。此前 Luna 失败归档保持原样。

## 问题与授权语义

原实现用 `session_allowed` 是否非空决定 Codex 整个工作区能否写入，但桌面命令审批只能为 Claude 保存可兑现的命令授权。同时，Full-auto 资格只接受 `OS_SANDBOX`，Codex 的原生沙箱无法参与，导致执行者只读、审批也不能恢复。

现在以明确的会话 Full-auto 授权表达工作区执行许可，与逐条命令审批分开：

- 普通 Direct、无授权与授权失效后为 `read-only`。命令白名单不再扩大 Codex 的整个工作区权限。
- 只有允许写入的 Executor / Tester 且具有 Full-auto 授权时，启用 `workspace-write`。Planner / Reviewer 始终只读，包括原生会话恢复。
- 新增独立的 `NATIVE_WORKSPACE_SANDBOX` 能力，接通设置、主聊天、任务派发、后端授权预览与启动校验。仅有 `OWN_SANDBOX` 不代表支持全自动。
- Codex 仍不声明 `OS_SANDBOX`，不会套用 Tokage 的外层沙箱。后端拒绝在主 checkout 或非 Git 目录执行 Codex Full-auto，保留独立 worktree 与每轮快照要求；Windows 仍不开放 Full-auto。
- 每轮及 resume 显式固定 `approval_policy="never"`，不提供沙箱外升级；忽略 execpolicy rules，清空额外 writable roots，禁用工作区工具网络及默认临时目录写入例外。不使用 bypass 或 `danger-full-access`。

使用方式：在会话中明确开启带有效期的全自动，保留确认与独立 worktree 创建流程，再派发执行。没有逐条审批通道的 Codex `exec` 不会因点击某条命令的“允许一次 / 会话允许”自动获得工作区写权限；这类决定仍保留人工处理提示。

原生沙箱与 Tokage 外层沙箱的读取、凭据保护能力不同；本轮不声称 Codex 具备 Tokage 的全部凭据隔离或命令事前红线拦截。确认文案已明确能力差异。验收只使用临时文件，不访问真实凭据。官方语义参考：[Codex security](https://developers.openai.com/codex/security/)，参数同时核对本机 `codex exec --help` 与 `exec resume --help`。现有 0.160.0 parser fixture 是旧协议样本；本轮新增执行参数与权限行为的实机验证范围为 0.162.0 / macOS arm64。

## 验证记录

- [x] 启动回归覆盖角色、授权模式、命令白名单及新开 / resume 组合；普通命令授权不能变成工作区授权。
- [x] 前端回归覆盖能力声明、无授权、明确授权与撤销；已有 TTL 过期降级回归保留。
- [x] 后端实际预检拒绝主 checkout 与非 Git 目录，不启动 CLI。
- [x] Luna 真实探针：空命令白名单下，独立 worktree 写入与 Python 断言成功，工作区外临时文件写入被内核拒绝且原内容不变。
- [x] 同一原生会话恢复后可继续写入；撤销授权后再次恢复，即使白名单仍有 `python3`，实际写入仍被拒绝。
- [x] 更新后的 Luna 角色链最终通过：只读 Planner → 实际编辑与 Python 断言 → 同一原生会话恢复 → 只读 Reviewer；每轮使用独立 turn ID。
- [x] 本地统一检查完成：12 项通过、0 失败、0 跳过；前端 41 文件 / 322 测试、Rust 594 测试通过。Rust 的 28 个显式 opt-in 测试默认忽略；本轮指定的 Luna 实机项目另行记录，不将忽略项当作实机通过。
- [x] 完整自然语言桌面交付与双会话输出回归完成：3 个任务 DONE、8 项门禁通过、独立审查通过、合并完成，4 条真实结构化 usage；没有人工修改业务代码 / 计划 / 验收或切换模型。

首次权限探针因测试命令的嵌套引号错误未形成目标文件而失败。修正测试命令的路径传递方式，不削弱断言，重跑三轮全部通过。另一项角色链复验的 Planner 已通过，Executor 请求遇到 `Selected model is at capacity`，作为外部服务失败保留，不描述为权限错误或通过。

本轮是权限整改与回归，不计入 m3a 的 20 个真实需求。m3a-001 仍未派发。

证据目录：[codex-permissions](reliability-evidence/codex-permissions/)。

完整桌面 session：`eb3725a4-999a-4f71-aaff-23e8549b162a`，交付提交 `1f15ec2d44f069decf2fae4d1ef868fe6457971d`；该提交属于隔离 invoice 验收仓库，不是 Tokage 主仓库提交。`verified.json`、SQLite 一致性备份、实际交付文件与 diff、会话检查、门禁和环境均已归档。

本地首轮发现旧测试将 Codex 当作不支持沙箱的提供者，改为无此能力的 Grok 样本，保持“未知 / 不支持能力必须被拒绝”的原断言。角色链恢复还暴露了测试复用同一 turn ID 的问题：Full-auto 快照正确拒绝重用已结束轮次。已为各轮分配独立 ID 并改善错误诊断，不放松快照保护；全部失败日志保留。

最终角色链日志为 `role-chain-final.log`，三轮全部通过的权限探针为 `permission-probe-retry.log`。恢复普通前端与 `custom-protocol` 原生构建，不保留 `live-acceptance` 入口；指纹见 `facts.json`。归档文件完整性见 `sha256.json`。
