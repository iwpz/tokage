# CLI / Provider 支持进度与 TODO

更新日期：2026-10-16。下文带日期的实机结果为历史记录，不自动覆盖当前代码或升级后 CLI。

本轮可靠性范围以 [m2a 支持范围](docs/reliability-m2a.md) 为准：仅选 macOS arm64 的 Claude Code 2.1.296 / `haiku`（官方订阅）及 OpenCode 2.0.29 / `deepseek/deepseek-flash`（官方 API Key），会话 Full Auto + 独立 worktree。两者当前代码真实 CLI 均待复验，自然语言桌面全链路均待 m2b；READY 不是可靠性验收通过。历史“桌面基线”指固定计划，不代表自动规划需求闭环。其他组合、Codex 完整桌面、Aider 实机与 Linux / Windows 保留各自未完成状态。

## 判定口径

- `[x]`：在 Tokage 源码中已实现，或本会话有明确验证记录。勾选旁注明实现与验证范围。
- `[ ]`：待实现、待加强或待验收；不表示厂商 CLI 本身不具备该能力。
- “检测 READY”只表示探测通过。配置文件、Key 存在或模型目录可读，不等于凭据有效，也不等于真实对话通过。
- 不使用主观完成百分比。以“对话已接入 / 有限对话接入 / 仅检测接入”描述成熟度，再逐项追踪。
- 不支持或有意不做的能力放在“边界”中，不将其伪装成待完成需求。
- 实机验收使用用户指定的模型 / 档位；付费调用仅用于明确授权的验收，普通扫描不自动发送模型请求。当前机器状态可能随安装、登录和额度变化。

## 当前优先级

- **OpenCode：已完成当前双版本桌面基线**。1.19.0 / 2.0.29 的认证、角色链、完整桌面 DAG / worktree / 合并，以及取消 / 超时 / 权限拒绝已验证；继续维护版本回归和跨平台验收。
- **P3：Aider**。保留现有适配与修复；暂缓逐家服务商扩展、批量验证和自定义网关界面，避免与其他 CLI 重复建设。

## 总览

- [x] 21 个真实 CLI 注册发现、版本探测和安装引导。
- [x] 9 个 CLI 已注册 headless 对话适配器：Claude Code、Codex、Cursor、Copilot、Antigravity、Aider、OpenCode、Reasonix、Kiro。
- [ ] 12 个 CLI 尚无对话适配器：Grok、Qwen、Kimi、Qoder、Amp、Crush、Goose、Cline、Droid、Auggie、Muse、Trae。
- [x] 本会话实机对话成功：Claude Code 2.1.296、Codex CLI 0.162.0、Cursor CLI 2026.10.01-e373342（仅 auto）、Antigravity CLI 1.3.4（仅 Google / Gemini Flash Low）、Reasonix 2.31.0（仅 DeepSeek Flash）、Copilot CLI 1.0.96（仅 auto + efficiency）。
- [x] 本会话最近实机检测 READY：Claude、Codex、Cursor、Antigravity、Reasonix、Copilot（Reasonix 使用验证凭证，Copilot 使用无聊天请求的 SDK 探测）。
- [x] Cursor 在钥匙串规则修复后，使用订阅账号 auto 完成真实规划 / 编辑 / resume / 审查验收。
- [x] Antigravity 1.3.4 使用当前 Google 账号完成固定 Gemini Flash Low 角色链实机验收。
- [x] Reasonix 补充 DeepSeek Flash 实机对话、文件读取、resume / copy、原生会话及官方 /status 余额刷新验收；2026-10-14 补充 Bash、文件编辑 / Python 断言、resume 和只读审查角色链；2026-10-15 完整桌面 DAG / worktree / 门禁 / 提交合并通过。
- [x] Copilot CLI 1.0.96 使用 auto + efficiency 完成认证、规划、文件编辑、Python 验收、resume、只读审查及原生会话读取。
- [x] OpenCode 1.19.0 / 2.0.29 完成 DeepSeek Flash 真实聊天、编辑、Python 门禁、resume、只读审查。
- [ ] Aider 真实模型验收仍受服务繁忙阻塞，优先级降为 P3。

## 共用验收清单

- [x] provider 注册表与 adapter 注册表分离；无 adapter 的 CLI 不声称可以对话。
- [x] Claude / Codex / 通用 status 探测使用对话环境与适用沙箱；Claude 校验 loggedIn，Cursor / Codex 校验正向登录输出。
- [x] 修复 macOS 钥匙串允许规则被共享禁读列表覆盖的问题；已有沙箱回归测试。
- [x] Gateway 在设置卡片与顶部 CLI 列表显示独立模式标签；故障状态优先显示。
- [ ] OpenCode 和仅检测 provider 的真实认证失败验收仍待补充。Aider 按用户要求检测到配置即视为可用，不主动验证 Key。
- [ ] 验收 Gateway 的有凭据 / 缺凭据 / 失效凭据 / 错误端点；本地配置存在不能代表上游连通与授权正常。
- [ ] 为每个对话适配器验收：登录、模型选择、简单回复、文件编辑、会话恢复、取消、超时、用量、权限拒绝。Cursor 本次已补充核心订阅实机验收与隔离回归，详见下节。
- [x] live E2E 已严格校验返回值、Done 与编辑结果；详见文末固定模型角色链和桌面验收记录。
- [ ] 实机测试“跳过未安装”单独汇总，不将 skip 计作 provider 验收通过。
- [x] Antigravity 新增独立固定模型实机脚本，Cursor 已有独立 auto 脚本。
- [x] 专项实机脚本已覆盖 Aider、OpenCode，OpenCode 支持双版本角色链与桌面验收。
- [ ] 通用 turn_per_cli 仍仅枚举 Claude / Codex / Cursor / Copilot，其他 CLI 使用专项脚本。
- [ ] 每个 provider 分别记录 macOS / Linux / Windows 验收；存在平台实现不代表各平台实机通过。
- [ ] 核对升级后的 CLI 参数与事件格式，更新 fixture 及 AdapterMeta 版本范围。
- [ ] 核对高级能力声明与实际路径：目前 9 个 adapter 均未声明 APPROVAL_CALLBACK、WRITE_SCOPE、COMPACT_HEADLESS、COMPACT_PTY；Claude / Antigravity / Copilot 已声明并实测 USAGE_QUERY；知识包或公共机制存在不能直接视为各 CLI 已验收。

## 已接入对话的 CLI

### Claude Code — `CLAUDE_CODE`

状态：对话已接入；本机会话成功。命令：`claude`。fixture 版本：`2.1.288`；声明兼容范围：`>= 2.1.288, < 3`。

- [x] 登录状态明确解析 loggedIn / authMethod，复用对话沙箱及宿主变量清理；未登录不误报 READY。
- [x] API Key / 自定义 Base URL 配置识别与 Gateway 模式展示。
- [x] JSONL 流式事件、模型选择、resume、原生 fork、工具规则与拒绝事件。
- [x] OS 沙箱、shell wrapper、原生会话读取与终端接管。
- [x] 本机 2.1.296：钥匙串修复后检测 READY，真实 Haiku smoke 返回 Ok(())。
- [x] 2026-10-15：真实 OAuth revoked 负例后，用户重新登录；Haiku 的 Planner → Executor 编辑 / 实际 Python 断言 → 同会话 resume → 原生 fork（新 session ID 且保留历史）→ 只读 Reviewer 全部通过，含原生会话读取。
- [x] 2026-10-15：真实桌面固定计划 → 两级 DAG → 独立 worktree → 验收 / 任务提交 → 集成验收 / 最终合并；只有声明的两个文件变更，测试与 package.json 不变，Git 干净，SQLite 为真实 CLI 的 DONE attempts 与结构化用量。
- [x] 真实结构化 input / output / cache read / cache write 用量归因；桌面两项实际用量记录已独立核对。
- [x] 官方 `/usage` 零模型调用查询：只选择 CLI 已信任且存在的目录，关闭自定义扩展 / MCP / 工具，不自动确认信任、不新增信任记录；使用分段输入并等待完整结果，分别解析会话 5h 与每周 7d，缺失 / 加载 / 越界百分比不伪造成功，未识别的重置日期保留未知，新增 USAGE_QUERY 声明。
- [x] 隔离回归覆盖登录 / 注销 / 失效凭据、取消 / 超时、成功帧后的错误 / 非零退出、权限拒绝与只读窗口磁盘写入；拒绝 / 失败不能发 Done。
- [ ] 真实注销后重扫与再次登录的完整生命周期（隔离 stub 与本次真实重新登录不代替主动注销实验）。
- [ ] 有效 API Key / 自定义 Gateway 的真实连通及授权验收；当前补全范围为既有官方订阅，未提供第三方上游凭据。
- [ ] 实机恶意命令拒绝、取消 / 超时负例与 Linux / Windows；现有失败路径回归不代替各平台实机验收。

边界：无原生审批回调能力声明；Tokage 工具规则 / 拒绝处理不等于 CLI 原生审批回调。

### Codex CLI — `CODEX_CLI`

状态：对话已接入；本机会话成功。命令：`codex`。fixture 版本：`0.160.0`；声明兼容范围：`>= 0.160.0, < 1`。

- [x] login status 正向登录证据；本地模型缓存与模型 effort 分组。
- [x] 结构化事件、模型选择、resume、自带沙箱、拒绝事件、shell wrapper。
- [x] 原生会话读取、终端接管、账号套餐及会话日志用量读取。
- [x] 本机 0.162.0：真实调用返回文本及 Done，检测 READY。
- [x] 2026-10-14：订阅 gpt-6-luna 真实 Planner → Executor 编辑 / Python 断言 → 同会话 resume → 只读 Reviewer，包含真实结构化 token 用量及原生会话读取。
- [ ] Codex 完整桌面任务流水线仍待验收。
- [ ] 实机验证 API Key / 自定义服务端配置与 Gateway 标签一致。
- [ ] 覆盖注销、过期凭据、resume、取消、超时与沙箱拒绝。

边界：Direct 模式明确受 CodexReadOnly 限制；此前编辑提示无法修改是该模式行为。未声明 OS_SANDBOX / TOOL_RULES / NATIVE_FORK / APPROVAL_CALLBACK。

### Cursor CLI — `CURSOR_CLI`

状态：订阅账号 `auto` 实机完成核心角色链验收。命令：`cursor-agent / agent`。fixture 版本：`2026.10.01`；声明兼容范围：`>= 2026.10.01，无上界`。本机版本：`2026.10.01-e373342`。

- [x] 专用 `status --format json` 登录探测；明确区分未登录、已登录与未知 / 网络错误。
- [x] status、about、models 使用对话相同环境和适用沙箱。
- [x] 模型列表与账号读取；API Key 路径必须通过认证模型目录，不能借用钥匙串账号的登录结果。
- [x] 本机真实无效 Key 负例：已有订阅登录时，无效 `CURSOR_API_KEY` 仍判 NEEDS_LOGIN，清空陈旧账号与模型。
- [x] 本地 CLI stub 覆盖有效 Key、失效 Key、网络失败、账号变化与注销输出；不读取或替换用户真实凭据。
- [x] Gateway / 自定义端点识别、模式标签与错误状态：本地 stub 验证；未调用实际第三方端点。
- [x] 结构化事件、模型选择、resume、拒绝事件与 OS 沙箱。
- [x] 项目权限规则及无效配置回归；不可读 / JSON 损坏 / 无效 approvalMode 或 permissions.allow 时阻止 Direct 启动。
- [x] 原生会话读取与终端接管参数：本次真实 auto 会话已读取验证。
- [x] 只用 `auto` 真实调用，之前 Authentication required 问题未再出现；验收请求清除 Key / 自定义端点以使用订阅渠道。
- [x] Planner：真实读取临时测试文件、输出建议，文件保持不变。
- [x] 只读角色改用 `--mode ask`：避免 Cursor 原生 plan 模式触发 CreatePlan，将 Tokage 所需回复转入 CLI 私有计划工具结果；已补充角色参数回归。
- [x] Executor：从只读模式切换时新建 Agent 会话并回放历史，恢复上一轮标记、实际修复加法函数；Python 两组断言通过。
- [x] 同模式 resume：Executor 第二轮保持同一原生 session ID，并正确复述上一轮标记，文件保持不变。
- [x] Reviewer：使用独立 ask 会话只读审查，返回验收标记，文件保持不变。
- [x] 真实角色链严格校验 TurnSucceeded、实际回复、Done 和非零结构化用量；出现 TurnFailed / Error 不得通过。
- [x] Cursor 主对话原生会话持久化 mode；规划 / 执行模式切换或旧记录 mode 未知时新建会话并回放历史，同模式继续 resume。跨角色任务窗口本来就由共用复用策略隔离。
- [x] 原有真实样本解析回归，以及无效 Key 运行时错误分类为 AuthRequired。
- [x] 取消和 idle timeout 隔离进程回归：Cursor 同一沙箱 / adapter 路径下停止进程，不发送错误的 Done；使用本地 fake，不调用模型。
- [ ] 使用用户有效 API Key 做真实 Key 成功调用；本次只有订阅成功、真实无效 Key 和本地成功 stub 的证据。
- [ ] 真实账号切换 / 注销再登录验收；本次回归使用隔离 stub，没有改变本机登录账号。
- [ ] 实际 Gateway 连通及授权验收；本次按用户要求未调用第三方模型池。
- [x] 2026-10-14：真实桌面界面完成持久化计划 → 两级 DAG 调度 → 独立 Git worktree → 任务验收 / 提交 → 集成验收 / 合并，固定订阅 Auto，关闭 Debug Provider；详见文末补充。

边界：工具规则来自 CLI 配置，无 TOOL_RULES / SHELL_WRAP / NATIVE_FORK / APPROVAL_CALLBACK 声明。原生审批回调、付费金额和订阅剩余额度不能由 token 用量或模型调用次数推出。

复验入口：`scripts/run-live-cursor-auto.sh`。只请求 Cursor `auto`，不会 fallback 到其他 provider / 模型；安装、认证、回复、用量或文件验收失败均使脚本失败。fixture / stub / 取消超时测试不消耗模型额度。

本轮发现并修复：原生 plan 模式曾导致审查轮的严格回复验收失败（计划工具保存了验收结果但无对应助手回复）；未将该次失败记为通过。随后还发现 ask 状态会随原生会话持久化，导致跨模式 resume 后无法编辑；已修正主对话会话复用，最终验收按模式切换新建会话，同模式单独验证 resume。

### GitHub Copilot CLI — `COPILOT_CLI`

状态：本机订阅账号完成核心角色链实机验收。命令：`copilot`。本机及新增真实 fixture 版本：`1.0.96`（保留 `1.0.91` 回归 fixture）；声明兼容范围：`>= 1.0.91, < 2`。本轮所有模型调用使用 `--model auto --auto-tier efficiency`，实际路由均为 `gpt-6-luna`。

- [x] 常规扫描通过本机 CLI `--headless --stdio --no-auto-update` 调用官方 SDK `auth.getStatus` + `models.list`；不创建会话、不发送聊天、不消耗模型额度，明确认证成功及模型可读后显示 READY。
- [x] 移除 Copilot 的聊天验证按钮、Tauri / IPC / store 调用链及五分钟验证缓存；重启后直接重新探测。
- [x] 探测复用对话的过滤环境与 OS 沙箱；RPC 超时 / 错误 / 空目录不误报 READY，未认证显示 NEEDS_LOGIN，Gateway 不借用订阅认证。
- [x] 明确指定环境 Token 优先级 COPILOT_GITHUB_TOKEN → GH_TOKEN → GITHUB_TOKEN；SDK 服务需显式 --auth-token-env / --no-auto-login，否则会忽略 CLI 的环境覆盖并使用已登录账号。
- [x] 实机无聊天探测：当前账号 READY、28 个模型；临时无效 COPILOT_GITHUB_TOKEN 在已有 OAuth 登录下仍返回 NEEDS_LOGIN，未修改真实凭据。
- [x] 本地 RPC stub 覆盖认证 / 目录结构、通知帧、环境 Token 优先级及 Gateway；断言只发送 auth.getStatus / models.list。
- [x] 官方 SDK 动态模型目录与 Auto 的 efficiency / balance / intelligence 分组；help config 的离线解析只作为旧版回归，不作为认证依据。
- [x] Gateway 环境检测：`COPILOT_PROVIDER_BASE_URL` 配置显示 gateway，标为非订阅 / 待验证，不借用 GitHub 登录或额度；endpoint 只展示主机名。
- [x] 原生 JSONL 流式文本 / reasoning 去重、模型路由、resume、工具规则、拒绝事件；新增实际 1.0.96 事件 fixture。
- [x] 修复 API 调用计数：按本次事件的唯一 turnId 计数，不把从零开始或续聊后的 turnId 当成次数。
- [x] 缺失成功 exitCode 不派发 Done；shellExecution 非零退出码显示工具失败，不因 success=true 误报成功。
- [x] 补全 COPILOT_PROVIDER_* 凭据 / headers / key command 隔离列表；模型进程保留自身配置，其他 provider 及 shell / MCP 工具环境移除这些值。
- [x] OS 沙箱、shell wrapper、原生会话读取与终端接管参数回归。
- [x] 实机 Planner → Executor → resume → Reviewer：读取临时文件、实际修复 m.py、运行 Python 断言、历史标记恢复、同一 session 续聊、只读审查及 premiumRequests 结构化计费通过。
- [x] 新增 `scripts/run-live-copilot.sh`，固定 Auto + efficiency；明确断言 TurnSucceeded、Done、实际文本、工具操作、文件结果与原生会话历史，不把 skip 视作通过。
- [x] 交互 `/usage` 零模型调用额度刷新：修复临时目录一次性信任、文本 / Enter 分段输入及异步结果等待；本机读取 Plan 29% used、5,900 / 20,000 AIC，查询会话为 0 AIC，声明 USAGE_QUERY。
- [x] 套餐额度显示为“套餐”，不伪造五小时窗口 / reset 时间；缺失真实 Plan 百分比返回不可用，不显示 0%；PTY 使用过滤环境。
- [ ] 每轮 AI credits 的单独结构化归因、legacy premium requests 账号额度与不同周期 / reset 时间验收；当前 Done 只保留 CLI 明确提供的 premiumRequests，不把它当作 AI credits。
- [ ] 实机验收环境 Token 与 keychain / gh 多账号优先级、注销 / 过期 / 无订阅 / 额度耗尽 / 网络失败（当前已实测临时无效 Token 的优先级负例，其他失败使用 stub，未改动真实登录）。
- [ ] Gateway / BYOK 的真实请求与手动验证；订阅 SDK 探测不适用于自定义上游。
- [x] 2026-10-14：真实桌面两级 DAG → 独立 worktree → 验收 / 提交 → 整体验收 / 最终合并，Auto + efficiency，见文末补充。
- [ ] Copilot 实机取消、超时、恶意写入拒绝仍待单独验收。
- [ ] Linux / Windows 实机验收。

边界：没有原生 fork 或审批回调能力声明；不把 resume 当成复制会话。help config 是离线目录，不作为认证证据。登录 / 模型目录可访问不保证每次推理均成功，实际任务还受模型政策、额度和网络影响。接口依据 [官方 SDK 兼容性文档](https://docs.github.com/en/copilot/how-tos/copilot-sdk/troubleshooting/compatibility) 与本机 SDK。

### Antigravity CLI — `ANTIGRAVITY_CLI`

状态：本机 Google 账号完成核心实机验收。命令：`agy`。本机及真实 fixture 版本：`1.3.4`；声明兼容范围：`>= 1.3.4, < 2`。用户指定本轮只验证当前 Google 账号，真实调用仅使用 `gemini-3.8-flash-low`。

- [x] 认证探测与真实对话使用相同环境及沙箱；Google 登录依据官方 `-p /usage --output-format json` 的成功额度响应，模型目录不作为登录证据。
- [x] 静态模型目录在无效 Key 下仍成功的情况已实机确认，并有未登录 / 网络失败 / BYOK 配置状态回归。
- [x] 修复启动参数：`-p`、`--conversation`、`--mode plan / accept-edits`；工作区使用进程 cwd，不再传不存在的 `--workspace` / `--resume` / 工具白名单 / `--secret-env-vars` 参数。
- [x] 录制真实 1.3.4 `init / step_update / result` 事件，替换 adapter 的原合成 fixture；新旧事件兼容解析有测试覆盖。
- [x] 流式文本拼接及终态去重；工具步骤去重、文件修改、权限拒绝、失败 / WAITING / 中断状态不误发 Done。
- [x] Planner / Reviewer 工作区由 OS 沙箱强制只读；`plan` 本身只是规划指令，不作为写权限保证。macOS 实际文件写入被内核拒绝的测试通过。
- [x] Google 实机 Planner → Executor → resume → Reviewer：真实回复、历史标记、文件编辑、Python 验收、同一原生会话恢复、只读审查及结构化 token 用量全部通过。
- [x] 原生会话透视：只读查询 CLI 的 `conversation_summaries.db`，按项目 file URI 过滤，展示标题 / 时间 / 日志位置；已用真实会话验收并声明 NATIVE_SESSIONS。
- [x] 终端接管使用 `agy --conversation <id>`；参数回归及真实 session ID 验收。
- [x] 官方 `/usage` JSON 的 weekly / 5h remaining_fraction 正确转换为 used_percent；4 个额度窗口实机通过，日常探测不发送模型 prompt，声明 USAGE_QUERY。
- [x] BYOK：缺 Key 提示配置；仅配置 Key 不误报 READY。设置页提供明确标注 token 消耗的手动验证按钮，成功后才派发任务。
- [x] Key 验证只保存当前进程内的 SHA-256 指纹；验证绑定 Key / endpoint 指纹，运行时认证失败会撤销验证，重启后需重验；错误返回不携带 Key。
- [x] Gateway 模式、Gemini endpoint 环境传递、与其他 provider 的凭据隔离及已验证 / 未验证状态有本地回归；BYOK 不继承 Google 订阅额度。
- [x] 取消、idle timeout、本机沙箱、完整非实机 native 回归；测试超时配置改为 task-local，避免影响其他并发测试。
- [x] 新增 `scripts/run-live-antigravity.sh`：零 token 登录 / 额度验证与固定 Gemini 模型角色链，无模型 fallback，失败断言不放宽。

验收边界：有效 Gemini API Key、实际自定义 endpoint 与账号切换未做实机测试，遵循用户“仅验证当前 Google 账号”的范围；相关实现由本地 stub 覆盖。2026-10-14 使用 1.3.5 / Google Gemini Flash Low 完成真实桌面 DAG → worktree → commit / merge，并独立检查 Git、验收命令及数据库用量。Linux 只读沙箱分支已实现，当前实机为 macOS；没有可用沙箱的平台会拒绝启动只读角色。

能力边界：撤销未经本机 CLI 支持的 TOOL_RULES / SHELL_WRAP 声明；保留结构化输出、模型选择、resume、read-only、DENIAL_EVENT、OS_SANDBOX，新增 NATIVE_SESSIONS / USAGE_QUERY。普通执行保留 CLI 自身权限规则；仅显式 FullAuto 执行角色使用对应的跳过权限参数。无原生审批回调，API 次数 / 最终上下文尺寸 / USD 费用未报告时保留未知，不根据 token 数伪造。

依据：[官方 headless 文档](https://www.antigravity.google/docs/cli/headless/)、[认证文档](https://www.antigravity.google/docs/cli/install/)、[执行模式说明](https://www.antigravity.google/docs/cli/modes/)；参数、协议及数据目录由本机 1.3.4 交叉核对。

### Aider — `AIDER`

状态：已安装 Aider 0.86.2，适配与本地回归已补齐；固定 DeepSeek Flash 的真实验收被服务端繁忙阻塞。命令：`aider`。已录制 fixture 版本：`0.72.0`；声明兼容范围：`>= 0.50.0, < 2`。

- [x] 多厂商 Key 名探测与按后端查询模型列表。
- [x] --message 非交互调用、文本解析、模型选择及 token / cost 汇总。
- [x] 只读角色 --dry-run、--restore-chat-history 恢复路径与 OS 沙箱。
- [x] 文本 fixture、launch 与解析测试。
- [ ] 安装后验证 DeepSeek 等已配置模型真实调用。
- [x] 按用户要求，检测到 Key 配置即 READY；移除 Verify API Key 按钮、IPC、真实验证请求及验证回执门槛。扫描仅查询本地目录，不发送模型请求。实际认证错误由对话阶段报告。
- [x] 历史文件按工作区与会话隔离，拒绝旧共享 ID 与跨工作区恢复；本地测试通过。
- [ ] 真实模型验证跨轮恢复及新会话不串历史。
- [ ] 补充完整工具拒绝 / 审批行为验证；不将 --yes-always 当审批支持。
- [ ] 验收文本变体、文件编辑、费用归因和任务流水线。

边界：非 JSONL，用量来源为 STDOUT_REGEX；无 STRUCTURED_OUTPUT / DENIAL_EVENT / NATIVE_SESSIONS / NATIVE_FORK / APPROVAL_CALLBACK 声明。不支持按 session ID 终端接管，RESUME 是历史文件恢复。上下文显式附加最多 32 个文本文件、单个 32 KiB、合计 128 KiB，排除隐藏文件、凭据及策略禁止路径；不宣称任意文件浏览或原生 shell 工具支持。只读角色需要可用 OS 沙箱，否则拒绝启动。

### OpenCode — `OPENCODE`

状态：已安装 2.0.29，并临时隔离安装 1.19.0 做双版本回归；两版均通过真实 DeepSeek Flash 角色链。命令：`opencode`。当前声明范围：`>= 1.19.0, < 3`；旧 0.8.4 sample 保留解析回归，新增两版真实 CLI 的本地模拟模型输出 fixture。

- [x] V1：CLI auth list + 按 XDG_DATA_HOME / HOME 定位并校验 auth.json 内容；空文件、无 Key、无效 JSON、过期且无法确认的 OAuth 不凭文件存在判 READY。
- [x] V2：CLI auth list --format json 查询凭据连接，不直接读取或修改 SQLite。模型列表只显示已连接后端。
- [x] 对话使用真实支持的 --agent plan/build；能力探测区分 V1 --pure 与 V2 --standalone，移除不存在的 --mode / --allow-tools / --deny-tools / --workspace / --secret-env-vars 参数。
- [x] 使用进程级内联权限、shell wrapper 与 OS 沙箱；只读角色在工作区禁止写入。V2 独立服务避免工具绕过当前进程沙箱。
- [x] 新旧 JSON 事件兼容；V2 缺 step_finish 时从 CLI session export 核对本轮消息、完成状态与用量，resume 不重复统计历史。
- [x] 两版真实 DeepSeek Flash：聊天、文件编辑、独立 Python 断言、resume 记忆、只读审查通过。
- [x] 真实 CLI + 本地模拟模型协议回归脚本 `scripts/test-opencode-contract.py --v1 <binary> --v2 <binary>`；明确不等同上游模型验收。
- [x] 1.19.0 / 2.0.29 均通过 DeepSeek Flash 完整桌面 DAG / 独立 worktree / 四次门禁 / 任务提交 / 集成 / UI 合并验收。
- [ ] 失效 API Key、网关与其他后端认证的真实验收；扫描 READY 表示 CLI 确认本地已配置，不承诺上游 Key 有效。
- [x] 双版本真实 CLI、生产 supervisor 与 OS 沙箱，在应用数据目录内的真实 Git worktree 中验证取消、空闲超时、总时限与 Shell 拒绝；本地模拟模型，不消耗上游额度。
- [ ] Linux / Windows 双版本实机验收。

边界：无 DENIAL_EVENT / NATIVE_SESSIONS / NATIVE_FORK / APPROVAL_CALLBACK 声明（旧 sample 的 denial 解析保留，但不宣称新版存在独立拒绝帧）。当前禁止模型执行 shell / task / 外部目录工具，验收命令由 Tokage 自行运行；不能将 CLI 的 --auto 当作审批桥接。两版各用原生凭据存储，不迁移或覆盖用户登录信息。托管 worktree 位于私有应用数据目录，关闭 OpenCode 向父目录自动发现项目配置，保留全局与 OPENCODE_CONFIG 显式配置；仓库上下文由 Tokage 提供，不开放应用数据库或其他 worktree。

### Reasonix — `REASONIX`

状态：本机 2.31.0 + 官方 DeepSeek Flash 已通过实际对话与续聊验收。首次未验证时显示 UNKNOWN_AUTH；显式「验证 API Key」发送小请求成功后，验证凭证在 Tokage 私有目录保存 24 小时，重启 / 重新检测仍显示 READY。仅相关密钥、配置 / 网络覆盖变更、凭证过期或认证失败会使验证失效，不因 PWD / TERM / SHLVL / PATH 变化而失效。

- [x] doctor / doctor billing 使用对话环境与适用沙箱；兼容 JSON 后附带 stderr 警告。
- [x] `key_present` 不再充当认证成功；缺密钥 NEEDS_LOGIN，已配置但未验证 UNKNOWN_AUTH；设置页支持显式验证。
- [x] 本机 `deepseek/deepseek-flash` 实际回复、工具文件读取、resume 保留上下文、--copy 创建独立会话、原生会话读取。
- [x] 修复流式片段之间多余换行、最终回复回退、失败写操作误报文件变更、重复工具启动事件。
- [x] 官方 `/status` PTY 实机刷新人民币余额；修复 Enter 字符，不虚构五小时 0% 配额。
- [x] 本地模拟无效凭据 / 成功认证、配置变更失效及 stderr 警告；新增 `scripts/run-live-reasonix.sh`。
- [x] 修复验证状态跨进程丢失与无关 shell 环境变化失效：只持久化 SHA-256 指纹 / 时间 / 版本，无密钥，原子写入且 Unix 权限 0600；过期、未来时间、损坏文件及撤销验证回归通过。
- [x] 实机验证后启动全新进程，只运行 doctor 探测即检测为 READY，不再次发送模型请求；自定义 REASONIX_HOME 的凭据变量绑定与隔离回归。
- [x] 解决 macOS 外层 Seatbelt 与 Reasonix 内部 Bash 沙箱冲突：turn 私有 IPC broker 在宿主侧启动受约束的工具子进程，将 Tokage 与 Reasonix 原生 SBPL 写范围取交集并保留拒绝规则；不关闭沙箱、不修改用户配置 / Key / trust。实际 DeepSeek Flash Bash 输出验收通过。
- [x] 实机 Planner → Executor 文件编辑及实际 Python 两组断言 → 同会话 resume → Reviewer 只读审查通过；临时测试目录无需代写持久化 trust。回归覆盖只读工作区、原生 / 自定义禁读、凭据禁读、broker 不可修改、配置网络拒绝、客户端断开及 turn 取消清理。
- [ ] 实机验证失效 Key、余额不足、网络错误；目前失败认证使用本地模拟，未改动真实密钥。
- [x] 新建临时 Git 目录中验证原生默认模式的未信任拒绝（不写文件、不发 Done）→ 官方 trust --yes → 实际写入 / Python 检查 → 撤销信任；不改动已有用户目录的信任。acceptEdits / FullAuto 是回合授权，不要求持久化 trust。
- [x] 2.31.0 + DeepSeek Flash 实际桌面 DAG → 两个隔离 worktree → 四次门禁 → 任务提交 → 集成 → UI 合并通过；支持原生 Git 指针防删除、符号链接限制与 regex 写拒绝规则，保留 OS 沙箱交集。
- [ ] 验收 /status 网络延迟、超时与其他币种；人民币正常路径已通过。
- [ ] Linux / Windows 实机验收。

边界：默认隐藏、用户主动添加；不代写持久化 trust。原生默认模式需 reasonix trust --dir，Tokage acceptEdits / FullAuto 仅对当前回合授权。当前明确不做 ACP host / 原生审批回调 / Studio 会话接入。配置和密钥处理遵循 [官方文档](https://reasonix.io/docs/)，实测命令与事件以本机 2.31.0 为准。

### Kiro CLI — `KIRO_CLI`

状态：对话已接入。命令：`kiro-cli`。测试包版本 `2.31.1`，兼容范围 `>=2.31.1,<3`；默认 V2，V3 独立协议分支。详见 [调查及实施记录](docs/kiro-cli-support-investigation.md)。

- [x] whoami 正向非空身份校验；认证和模型探测使用对话环境及 OS 沙箱，不发送模型请求。
- [x] headless JSONL、模型选择、文本 / reasoning / 工具 / 文件变化 / 严格终态；退出失败不能被 Done 掩盖。
- [x] 按 ID 恢复；会话 ID 携带 v2/v3 引擎标签，终端接管使用对应引擎。
- [x] 原生会话快照读取、项目筛选；V2/V3 分别解码。
- [x] credits 独立显示和落库；不虚构 input/output tokens、美元或 premium requests。
- [x] 每轮创建并清理独立受控 agent；禁用 MCP / 资源注入 / Shell / 委派，工作区文件工具按角色授权；只读角色要求可用 OS 沙箱。
- [x] V2 真实聊天、编辑、Python 门禁、resume、只读审查；固定 Claude Haiku 4.5。
- [x] V2/V3 桌面 UI 双任务 DAG、独立 worktree、各 4 次门禁、提交与合并；每套 2 条 credits 用量记录落库通过，隔离提交快照亦通过。
- [x] fixture、解析及认证失败回归、取消 / idle / hard timeout / 退出失败回归。
- [x] V3 真实聊天、编辑、Python 门禁、resume、原生会话读取及只读审查；CLI 包版本和引擎版本分开记录。
- [ ] Linux / Windows 实机验收；只读沙箱不可用时明确拒绝执行，不降级为可写。

边界：当前不声明 ACP 外部审批回调、原生 fork、原生压缩、额度查询或 WRITE_SCOPE；Shell 测试命令由 Tokage 门禁执行。V3 检出独立 hooks 时拒绝运行，避免自动执行未隔离脚本；默认 V2 不受该限制。

## 仅检测接入的 CLI

以下 13 个 CLI 均没有 `ProviderAdapter`，即使显示 READY，也不能由 Tokage headless 发起真实对话。安装方式、知识包或沙箱条目不等于对话接入完成。

### Grok Build — `GROK_CLI`

状态：仅检测接入。命令：`grok`。此前本机未安装，尚无本会话实机调用记录。

- [x] 白名单注册、路径 / 版本探测与安装引导。
- [x] 初步认证 / 模型探测代码：auth.json / XAI_API_KEY 存在；models 命令。
- [ ] 验证无效 / 空凭据，避免仅凭存在判 READY。
- [ ] 核验非交互调用方式，新增 adapter、参数映射及输出解析。
- [ ] 补齐模型选择、会话恢复或明确不支持、用量与错误分类。
- [ ] 明确权限与凭据隔离、只读模式及沙箱策略，按实际验证声明能力。
- [ ] 录制输出 fixture、补充解析测试，再完成真实对话 / 编辑 / 取消 / 超时验收。

### Qwen Code — `QWEN_CODE`

状态：仅检测接入。命令：`qwen`。此前本机未安装，尚无本会话实机调用记录。

- [x] 白名单注册、路径 / 版本探测与安装引导。
- [x] 初步认证 / 模型探测代码：DASHSCOPE_API_KEY / BAILIAN_CODING_PLAN_API_KEY / OPENAI_API_KEY 存在。
- [ ] 补充账号 / OAuth / 有效 Key 检测和模型目录。
- [ ] 核验非交互调用方式，新增 adapter、参数映射及输出解析。
- [ ] 补齐模型选择、会话恢复或明确不支持、用量与错误分类。
- [ ] 明确权限与凭据隔离、只读模式及沙箱策略，按实际验证声明能力。
- [ ] 录制输出 fixture、补充解析测试，再完成真实对话 / 编辑 / 取消 / 超时验收。

### Kimi Code — `KIMI_CODE`

状态：仅检测接入。命令：`kimi`。此前本机未安装，尚无本会话实机调用记录。

- [x] 白名单注册、路径 / 版本探测与安装引导。
- [x] 初步认证 / 模型探测代码：.kimi-code/config.toml 存在；provider catalog list --json。
- [ ] 配置存在不等于登录，补充有效认证状态。
- [ ] 核验非交互调用方式，新增 adapter、参数映射及输出解析。
- [ ] 补齐模型选择、会话恢复或明确不支持、用量与错误分类。
- [ ] 明确权限与凭据隔离、只读模式及沙箱策略，按实际验证声明能力。
- [ ] 录制输出 fixture、补充解析测试，再完成真实对话 / 编辑 / 取消 / 超时验收。

### Qoder CLI — `QODER_CLI`

状态：仅检测接入。命令：`qoder / qodercli`。此前本机未安装，尚无本会话实机调用记录。

- [x] 白名单注册、路径 / 版本探测与安装引导。
- [x] 初步认证 / 模型探测代码：status；--list-models。
- [ ] 解析正向身份与机器可读状态，补充输出样本。
- [ ] 核验非交互调用方式，新增 adapter、参数映射及输出解析。
- [ ] 补齐模型选择、会话恢复或明确不支持、用量与错误分类。
- [ ] 明确权限与凭据隔离、只读模式及沙箱策略，按实际验证声明能力。
- [ ] 录制输出 fixture、补充解析测试，再完成真实对话 / 编辑 / 取消 / 超时验收。

### Amp — `AMP`

状态：仅检测接入。命令：`amp`。此前本机未安装，尚无本会话实机调用记录。

- [x] 白名单注册、路径 / 版本探测与安装引导。
- [x] 初步认证 / 模型探测代码：AMP_API_KEY 存在。
- [ ] 支持真实登录状态与模型目录，明确无 Key 时是否使用账号凭据。
- [ ] 核验非交互调用方式，新增 adapter、参数映射及输出解析。
- [ ] 补齐模型选择、会话恢复或明确不支持、用量与错误分类。
- [ ] 明确权限与凭据隔离、只读模式及沙箱策略，按实际验证声明能力。
- [ ] 录制输出 fixture、补充解析测试，再完成真实对话 / 编辑 / 取消 / 超时验收。

### Crush — `CRUSH`

状态：仅检测接入。命令：`crush`。此前本机未安装，尚无本会话实机调用记录。

- [x] 白名单注册、路径 / 版本探测与安装引导。
- [x] 初步认证 / 模型探测代码：crush.json / ANTHROPIC_API_KEY / OPENAI_API_KEY 存在。
- [ ] 核对实际配置路径、后端认证与模型选择。
- [ ] 核验非交互调用方式，新增 adapter、参数映射及输出解析。
- [ ] 补齐模型选择、会话恢复或明确不支持、用量与错误分类。
- [ ] 明确权限与凭据隔离、只读模式及沙箱策略，按实际验证声明能力。
- [ ] 录制输出 fixture、补充解析测试，再完成真实对话 / 编辑 / 取消 / 超时验收。

### Goose — `GOOSE`

状态：仅检测接入。命令：`goose`。此前本机未安装，尚无本会话实机调用记录。

- [x] 白名单注册、路径 / 版本探测与安装引导。
- [x] 初步认证 / 模型探测代码：config.yaml / GOOSE_PROVIDER 存在。
- [ ] GOOSE_PROVIDER 只指明后端，不是凭据；补充实际后端认证及模型目录。
- [ ] 核验非交互调用方式，新增 adapter、参数映射及输出解析。
- [ ] 补齐模型选择、会话恢复或明确不支持、用量与错误分类。
- [ ] 明确权限与凭据隔离、只读模式及沙箱策略，按实际验证声明能力。
- [ ] 录制输出 fixture、补充解析测试，再完成真实对话 / 编辑 / 取消 / 超时验收。

### Cline CLI — `CLINE`

状态：仅检测接入。命令：`cline`。此前本机未安装，尚无本会话实机调用记录。

- [x] 白名单注册、路径 / 版本探测与安装引导。
- [x] 初步认证 / 模型探测代码：.cline/data 目录存在。
- [ ] 数据目录不是登录证据；补充认证命令与模型目录。
- [ ] 核验非交互调用方式，新增 adapter、参数映射及输出解析。
- [ ] 补齐模型选择、会话恢复或明确不支持、用量与错误分类。
- [ ] 明确权限与凭据隔离、只读模式及沙箱策略，按实际验证声明能力。
- [ ] 录制输出 fixture、补充解析测试，再完成真实对话 / 编辑 / 取消 / 超时验收。

### Factory Droid — `DROID`

状态：仅检测接入。命令：`droid`。此前本机未安装，尚无本会话实机调用记录。

- [x] 白名单注册、路径 / 版本探测与安装引导。
- [x] 初步认证 / 模型探测代码：FACTORY_API_KEY 存在。
- [ ] 补充登录状态、Key 有效性与模型目录。
- [ ] 核验非交互调用方式，新增 adapter、参数映射及输出解析。
- [ ] 补齐模型选择、会话恢复或明确不支持、用量与错误分类。
- [ ] 明确权限与凭据隔离、只读模式及沙箱策略，按实际验证声明能力。
- [ ] 录制输出 fixture、补充解析测试，再完成真实对话 / 编辑 / 取消 / 超时验收。

### Augment Auggie — `AUGGIE`

状态：仅检测接入。命令：`auggie`。此前本机未安装，尚无本会话实机调用记录。

- [x] 白名单注册、路径 / 版本探测与安装引导。
- [x] 初步认证 / 模型探测代码：AUGMENT_SESSION_AUTH 存在；models list --json。
- [ ] 验证 session 过期、登录状态和模型目录格式。
- [ ] 核验非交互调用方式，新增 adapter、参数映射及输出解析。
- [ ] 补齐模型选择、会话恢复或明确不支持、用量与错误分类。
- [ ] 明确权限与凭据隔离、只读模式及沙箱策略，按实际验证声明能力。
- [ ] 录制输出 fixture、补充解析测试，再完成真实对话 / 编辑 / 取消 / 超时验收。

### Muse Code — `MUSE_CODE`

状态：仅检测接入。命令：`muse`。此前本机未安装，尚无本会话实机调用记录。

- [x] 白名单注册、路径 / 版本探测与安装引导。
- [x] 初步认证 / 模型探测代码：META_API_KEY 存在。
- [ ] 补充官方认证探测及可用模型目录。
- [ ] 核验非交互调用方式，新增 adapter、参数映射及输出解析。
- [ ] 补齐模型选择、会话恢复或明确不支持、用量与错误分类。
- [ ] 明确权限与凭据隔离、只读模式及沙箱策略，按实际验证声明能力。
- [ ] 录制输出 fixture、补充解析测试，再完成真实对话 / 编辑 / 取消 / 超时验收。

### TraeCode CLI — `TRAE_CLI`

状态：仅检测接入。命令：`traecli`。此前本机未安装，尚无本会话实机调用记录。

- [x] 白名单注册、路径 / 版本探测与安装引导。
- [x] 初步认证 / 模型探测代码：login status。
- [ ] 校验正向登录输出与模型目录，不仅依据退出码。
- [ ] 核验非交互调用方式，新增 adapter、参数映射及输出解析。
- [ ] 补齐模型选择、会话恢复或明确不支持、用量与错误分类。
- [ ] 明确权限与凭据隔离、只读模式及沙箱策略，按实际验证声明能力。
- [ ] 录制输出 fixture、补充解析测试，再完成真实对话 / 编辑 / 取消 / 超时验收。

## 调试 Provider（不计入 21 个真实 CLI）

- [x] 当前工作区已有 Mock / Luna / DeepSeek 调试来源、独立工作目录与来源标记实现及文档。
- [x] Mock 的实现目标是不调用真实模型；Luna / DeepSeek 路由至实际执行 CLI。
- [ ] 对尚未提交的调试 Provider 改动另行完成构建及完整流水线验收。
- [ ] 将 Mock 流程通过与真实厂商 CLI 通过分别记录；不能据 Mock 成功勾选真实 provider 验收。

## 建议执行顺序

- [x] P0：实机 E2E 遇到真实调用 Err / 无 Done / 未完成编辑断言时失败，逐 provider 输出 pass / fail / skip；固定模型基线与桌面脚本汇总失败且返回非零。
- [x] P0：Cursor 修复后 auto 真实调用、resume、编辑、审查复验。
- [x] P0：Claude / Codex 增加登录前后与失效凭据回归（隔离 stub，真实失效 Claude OAuth 另有负例；成功角色链见 2026-10-14 补充）。
- [ ] P1：为其余 5 个已接入 adapter 完成认证和实机验收，重点排除配置存在造成的 READY 误报。
- [ ] P1：覆盖已有 9 个 adapter 的 resume、权限拒绝、用量、编辑及任务流水线。
- [ ] P2：按实际需求逐个新增其余 13 个 adapter；每个完成最小实机验收后再宣称可对话。
- [ ] P3：按厂商能力和产品需求扩展高级审批、压缩、原生会话、跨平台兼容；有意不做的项目需保留边界说明。

## 核对依据与验证方式

以源码为准，开发文档仅作为历史验收证据：

- `src-tauri/crates/tokage-core/src/provider.rs`：21 个真实 provider 身份。
- `src-tauri/crates/tokage-native/src/providers.rs`：白名单、认证、模型与账号探测。
- `src-tauri/crates/tokage-native/src/chat/adapter.rs`：9 个 adapter、能力声明、fixture 版本及兼容范围。
- `src-tauri/crates/tokage-native/src/chat/{claude,codex,cursor,copilot,antigravity,aider,opencode,reasonix}.rs`：启动参数与事件解析。
- `src-tauri/crates/tokage-native/src/{session,usage,install}.rs`：原生会话、账号和安装方式。
- `src-tauri/crates/tokage-native/src/chat/quota.rs`、`src-tauri/resources/knowledge/builtin.json`：额度解析、查询配置及沙箱配置。
- `src-tauri/crates/tokage-native/src/sandbox/{mod,tests}.rs`：钥匙串规则修复与回归测试。
- `scripts/run-live-cli-e2e.sh`、`chat/mod.rs` 的 `live` 模块：实机测试覆盖及断言限制。
- `docs/reasonix-dev-todo.md`：Reasonix 的历史开发验收记录和明确不做的事项。
- `docs/13-debug-provider/README.zh-CN.md`：调试来源边界。

首次整理验证：provider 单元测试 16 项、adapter 单元测试 7 项全部通过；21 个 provider 标识与源码逐一核对，无遗漏或重复。沙箱测试 12 项通过沿用本会话之前记录。Cursor 后续验证：provider 单元测试增至 18 项、Cursor parser / 权限 / 角色参数 6 项、错误分类 6 项、adapter 7 项全部通过；相关前端 67 项（含原生会话模式隔离）与 TypeScript 类型检查通过。实机订阅 auto 角色链通过；真实无效 Key 探测与原生会话读取通过。

不消耗模型额度的代码检查：

```sh
cargo test --manifest-path src-tauri/Cargo.toml -p tokage-native --lib providers::tests
cargo test --manifest-path src-tauri/Cargo.toml -p tokage-native --lib chat::adapter::tests
cargo test --manifest-path src-tauri/Cargo.toml -p tokage-native --lib sandbox::tests
```

实机检测不发送 prompt；实机对话会消耗对应模型额度。检查未安装 CLI 的 skip，不视为调用成功。

## 本轮 Antigravity 验证记录

- [x] 当前 Google 账号：零模型 token 的认证 / 额度查询通过；4 个窗口。
- [x] 固定 Gemini Flash Low：Planner、实际编辑及 Python 验收、同模式 resume 记忆、Reviewer 只读审查通过；无其他模型 fallback。
- [x] Antigravity 相关 Rust 回归：流式协议、错误状态、工具拒绝、quota、认证 stub、原生 SQLite 会话、只读内核规则、取消和超时通过。
- [x] 69 项相关前端测试、TypeScript 类型检查和国际化检查通过。
- [x] 本轮非实机 native 测试 207 项通过；实机 / 手动测试保持 ignore 并单独运行，不以 skip 充作验收。
- [x] 桌面 Rust cargo check、前端生产构建通过。

此前 Cursor 修改已提交为 `29a8f37`；本节记录后续 Antigravity 修改与验收。


## 2026-10-14：第 4 / 5 项基线与桌面验收

第 3 项 Reasonix 沙箱修复已提交为 `2ae5772`。

第 4 项：新增 `scripts/run-live-baseline.sh`，固定 Claude Haiku / Codex gpt-6-luna，不跳过缺失认证、不换 provider。Codex 的规划、真实编辑 / Python 断言、同会话恢复、只读审查、用量与原生会话读取全部通过。Claude 首次真实返回 OAuth revoked；重新登录后已不再返回该认证错误，但当前账号达到 weekly limit（CLI 提示上海时间 14:00 重置），因此当日未标记成功；2026-10-15 的通过记录见下节。

隔离回归覆盖两者的 READY → 注销 → READY → revoked → 无效状态 / 非零退出，清理陈旧账号信息；认证失败发 NEEDS_LOGIN / AuthRequired，不能发 Done / TurnSucceeded。真实 parser + 子进程覆盖取消、idle timeout、成功帧后非零退出 / 后续错误。权限拒绝与只读窗口磁盘写入必须返回失败且不发 Done；额度错误仍发 Quota。任务首次启动、恢复、修复和审批后恢复现传递同一项目 / 会话授权，重新分配携带已允许命令。

第 5 项：`scripts/run-live-desktop.sh` 构建显式 opt-in 的桌面验收版本；普通构建不包含前端驱动，release 不允许启用验收入口。每个 provider 使用独立临时 Git 仓库、应用数据与 WebView 数据，不改真实项目 / 账号配置。实际渲染界面确认持久化计划、启动任务、查看交付、合并回 fixture。后端 turn、gate、worktree、Git、SQLite 均为真实路径；Debug Provider 关闭，禁止换 provider / 模型。

验收包含两个依赖任务（加法修复 → 使用上游加法组合结果）、独立 worktree / 原生会话、依赖顺序、文件范围、任务提交、集成与完整验收、最终合并、干净 Git 状态，以及数据库内实际 CLI 用量与 DONE attempts。计划为用户编辑的固定 fixture，未把自动规划模型调用或独立 Reviewer 任务计入此次桌面验收。

桌面实测发现并修复 macOS 沙箱对应用数据中的活动 worktree 一并拒绝的问题。仅豁免当前 linked worktree 的读写和路径解析必需的父目录元数据；数据库、父目录内容、其他 worktree 与凭据红线仍禁止。内核测试实际执行 Node realpath / 读写并确认数据库、peer 和 app-data 列表不可访问。Linux / Windows 不属于本次桌面实机范围。

- [x] Cursor 2026.10.01-e373342 / Auto：完整链路通过；两项实际用量记录，四条 gate，最终只改 calc.mjs / compose.mjs。
- [x] Antigravity 1.3.5 / Google Gemini Flash Low：同上，真实结构化 token 用量通过。
- [x] Copilot 1.0.97 / Auto + efficiency：完整链路通过；两项 DONE，各自有明确 premiumRequests（合计 2，不等同 AI credits）及结构化 API 调用次数，四条 gate、最终合并与独立核验通过。
- [x] Claude Haiku 成功角色链 / 原生 fork：2026-10-15 额度恢复且重新登录后全部通过，见下节。

证据保存在每次临时目录的 `progress.jsonl`、`result.json`、`verified.json`、`desktop.log` 与 SQLite。`verified.json` 只有独立 Git / 命令 / 数据库检查全部通过才生成。真实外部失败不会自动成为 skip 或成功。

本轮最终检查：前端 162 项通过；原生 235 项通过、19 项显式 ignored（含消耗真实模型的 opt-in 测试）；TypeScript / 生产构建、i18n 和差异空白检查通过。三家桌面 fixture 均生成独立核验的 `verified.json`。


## 2026-10-15：Claude 补全复验（已完成官方订阅路径）

用户确认额度恢复后，重新运行 Claude Haiku 角色链；普通环境与清除导出 Token / Gateway 覆盖后的订阅环境均返回 `OAuth token revoked`。真实桌面任务亦在首项任务认证阶段失败（无 Done、无工具调用、无成功用量），未将此失败记为完成。随后用户完成 `claude auth login`，下列真实验收全部通过。

已为桌面验收入口增加固定 `CLAUDE_CODE:haiku`，清理测试进程内的导出认证 / Gateway 覆盖，不修改真实凭据或用户配置；入口仅允许该固定模型，无 provider fallback。基线测试确认使用官方订阅通道并记录 CLI 版本。构建与认证 / 退出错误 / 取消超时的 6 项隔离基线回归通过；两项真实模型角色链保持 opt-in，不能以 ignored 视为通过。

复验命令：`cargo test --manifest-path src-tauri/Cargo.toml -p tokage-native --lib chat::baseline_live::claude_haiku_role_chain --locked -- --ignored --nocapture`；桌面入口：`scripts/run-live-desktop.sh CLAUDE_CODE`。

最终实机结果（Claude Code 2.1.296，固定 `haiku`，CLI 报告 `claude-haiku-5-5`，无模型或 provider fallback）：

- [x] 严格角色链测试通过：Planner、真实文件编辑 / 实际 Python 两组断言、同会话 resume、原生 fork 保留历史且创建不同 session ID、Reviewer 只读、原生会话读取；每轮都要求真实文本、非零结构化用量和 TurnSucceeded，禁止 Error / TurnFailed。
- [x] 独立桌面 fixture `/private/tmp/tokage-live-desktop-4bz4s_p9`：两项 DONE、两个独立 worktree / 原生会话、四条实际 gate、任务提交及最终合并；`verified.json` 已独立检查 Git、脚本、源文件范围、干净工作区及 SQLite。input 16、output 28,466、cache read 184,628、cache write 44,792；这些是 CLI 原始计量，不能直接当作订阅剩余额度或实际美元扣费。
- [x] 修复 `/usage` 原有临时目录信任阻塞、输入时机 / Enter、加载帧误判及会话 / 周额度混淆。实际查询返回两个窗口；单独 PTY 诊断面板显示查询会话 API 时长 0s、input / output / cache 全为 0，未发送模型 prompt。不接受新的目录信任，不改用户认证配置；没有已信任目录则明确返回不可用。
- [x] 新增完成帧判定回归：等待异步加载、持续动画下有界捕获有效结果；额度测试覆盖 session / week 分离、部分加载、越界数字、既有信任选择与配置保持不变。

完整复验入口：`scripts/run-live-claude.sh`（顺序运行角色链、零模型调用额度查询、真实桌面 DAG）。第三方 Gateway / 有效 API Key、真实主动注销、实机恶意命令负例及非 macOS 平台保持未验收，不计入上述通过范围。桌面计划是固定用户编辑 fixture，自动生成计划及独立 Reviewer 任务的桌面编排另行验收。

最终回归：原生 238 项通过、20 项显式 ignored（真实模型 / 手动测试单独执行）；前端 162 项、i18n 检查通过。普通生产前端与桌面构建恢复后交付。


## 2026-10-15：Aider API Key 配置引导

- [x] 缺 Key 提示增加服务商选择和平台脚本：默认识别 macOS / Linux / Windows，也可手动切换；覆盖当前检测支持的 9 个 API 服务商。
- [x] 脚本只在用户主目录 `.env` 缺少对应项时补入 `YOUR_<变量名>` 占位符，随后打开编辑器；不覆盖已有密钥行或其他配置。真实 Key 在编辑器中填写，不出现在复制脚本内。POSIX 文件权限设为 0600，Windows 新文件使用无 BOM UTF-8。
- [x] 界面提供运行、填写 / 保存、重新扫描的明确步骤；不执行脚本、不修改当前机器的真实配置。检测到配置即标为 READY；不承诺上游 Key 有效。
- [x] 18 组 POSIX 脚本在隔离目录实测：首次添加、重复执行不重复添加、已有密钥与其他内容保留、权限正确；编辑器启动使用 stub。TypeScript、i18n 及生产构建通过。
- [ ] Windows PowerShell 脚本尚未在 Windows 实机执行；Aider 真实 Key、对话与任务链仍待单独验收。


## 2026-10-15：Aider 支持补全与 DeepSeek Flash 验收

以下为此前验收历史；Key 验证按钮与回执机制现已按用户要求移除，以文末调整记录和当前 Aider 清单为准。

用户授权真实模型调用，并明确固定 `deepseek/deepseek-flash`，不切换服务商。

- [x] 扫描区分缺 Key、已配置但未验证与 READY。设置页提供模型选择与显式付费验证按钮；扫描不发模型请求。回执不保存 Key，并限制权限。
- [x] 每个工作区和会话使用独立私有历史与运行配置；不再共用 `aider-session`。禁用自动提交、lint/test、建议 shell、启动脚本与跨模型回退，Git 与验收门禁由 Tokage 管理。
- [x] Planner / Reviewer 使用 ask + dry-run + OS 只读沙箱，Executor 使用 diff；附加文件区分可写范围与只读上下文。
- [x] 解析读文件、编辑、文件变更、缓存用量和多次请求费用；错误或无用量不得产出成功 Done。识别服务繁忙并停止自动重试。
- [x] 添加固定模型的真实角色链与桌面验收入口：`scripts/run-live-aider.sh`。桌面检查两任务 DAG、worktree、门禁、提交、合并与数据库用量归因。
- [ ] 真实聊天、编辑、续聊、会话隔离、只读审查及完整桌面 DAG 仍需 DeepSeek 恢复后通过；脚本存在不代表验收成功。
- [ ] Windows 配置脚本和 Windows/Linux 沙箱仍需对应平台实机验证；其他模型后端、网关地址及项目局部凭据覆盖尚未完成实测。

阻塞证据：原生真实验证多次返回 `litellm.ServiceUnavailableError` / `Service is too busy`；未保存认证成功回执，未将模型目录或进程 exit 0 当作成功。测试中的假 CLI 仅验证状态机和失败处理，不代替真实模型验收。

桌面失败证据：`/private/tmp/tokage-live-desktop-ptockk2z/result.json`，固定模型 `deepseek/deepseek-flash`，验证请求失败，DAG 未启动；不是成功用量或任务链证明。

本轮最终本地验证：原生单测 246 通过 / 21 忽略，前端 27 文件 / 162 测试通过；TypeScript、i18n 检查与前端构建通过。Aider 专项 11 通过 / 1 项真实模型测试默认忽略；显式执行真实测试遇服务繁忙失败。


## 2026-10-15：OpenCode 双版本支持

根因：本机为 2.0.29，新凭据保存在 SQLite；旧检测只寻找 auth.json，导致 Auth unknown。已改为优先 CLI 结构化认证查询，保留 V1 legacy/XDG 路径，结合模型目录过滤；同时修复两版运行参数及新事件格式。

实测：OpenCode 2.0.29 与隔离安装的 1.19.0 均通过生产 run_turn 路径，固定 DeepSeek Flash，完成真实聊天、编辑、独立 Python 门禁、会话恢复和只读审查。V1 验收临时凭据位于 0700 目录 / 0600 文件中，结束后删除；未修改用户原有认证存储。日志分别为 `/tmp/tokage-opencode-live-roles.log`、`/tmp/tokage-opencode-v1-live-roles.log`。

原生回归 252 项通过、23 项忽略（含显式执行的真实模型测试）。此记录不宣称完整桌面 DAG 或跨平台验收通过。


## 2026-10-15：移除 Aider API Key 主动验证

按用户要求，与其他 API Key 类型一致：环境变量、~/.env 或 ~/.aider.conf.yml 中检测到已配置 Key 即标为 READY，不发送验证模型请求。删除设置页验证按钮与专用文案、前后端验证 IPC、回执生成/读取/失效机制及验收脚本中的验证前置步骤；所有已配置后端的模型均可显示。无 Key 仍提示配置，对话中真正的认证/服务错误仍正常报告。

回归覆盖缺配置、占位符、三种配置来源、多后端、Key 变更即用，以及扫描不发推理请求、不产生验证回执。Aider 维持 P3 优先级。


## 2026-10-15：OpenCode 双版本与 Reasonix 完整桌面验收

固定 DeepSeek Flash，无模型回退，独立临时仓库 / 应用数据，真实渲染 UI + Tauri IPC + 生产 CLI。三个验收均为 T1 → T2 两个依赖任务、两个独立 worktree、四次通过门禁、真实任务提交与最终 UI 合并；数据库各保存两条实际用量记录，最终仅修改 calc.mjs / compose.mjs，测试文件保持原样。

- OpenCode 1.19.0：`/private/tmp/tokage-live-desktop-c97ulfli/verified.json`。
- OpenCode 2.0.29：`/private/tmp/tokage-live-desktop-ehfqau9m/verified.json`。
- Reasonix 2.31.0：`/private/tmp/tokage-live-desktop-iwhkyfjm/verified.json`。

修复：macOS Bash 3 的空数组展开导致验收脚本提前退出；失败退出码在普通前端恢复后保留。扫描 / 单项刷新 / 验证 / reload 统一使用验收环境，避免 V1 的临时 XDG 认证被刷新覆盖。V1 临时 auth.json 为 0600，最外层 finally 删除，既有登录不变。

OpenCode 2.x 的本机服务曾因配置发现读取私有 worktree 父目录返回 HTTP 500。托管 worktree 使用官方项目配置发现开关，保留全局与显式配置，未放开父目录数据读取；这也意味着托管 worktree 不自动发现仓库 OpenCode 配置，需要时使用显式 OPENCODE_CONFIG。

Reasonix 的 broker 增加 Git 元数据防删除 / 防符号链接 / regex 拒绝规则的严格保留；规则只用于收紧 native deny，未知语法仍拒绝。实机信任专项使用 CLI 原生默认模式验证拒绝，然后官方 trust --yes 授予临时目录，再实际写入与 Python 检查，结束撤销。正常 DAG 使用回合 FullAuto 授权，不写持久化 trust，并实际运行原生编辑后验证和 Tokage 独立门禁。

复验：

```bash
bash scripts/run-live-desktop.sh OPENCODE REASONIX
TOKAGE_OPENCODE_TEST_BIN=/absolute/path/to/opencode-v1 \
  bash scripts/run-live-desktop.sh OPENCODE
TOKAGE_OPENCODE_V1_BIN=/absolute/path/to/opencode-v1 \
TOKAGE_OPENCODE_V2_BIN=/absolute/path/to/opencode-v2 \
  cargo test --manifest-path src-tauri/Cargo.toml -p tokage-native \
  installed_versions_cancel_timeouts_and_deny_shell --lib -- --ignored --nocapture
bash scripts/run-live-reasonix.sh
```

控制专项日志 `/tmp/tokage-opencode-control-managed.log`：双版本共八项通过。本次原生回归 262 通过、0 失败、26 个显式实机测试忽略（`--test-threads=2`）；默认高并发首轮出现两个超时类失败，降低测试并发后全量通过。Linux / Windows、其他服务商及真实失效 Key 验收仍保留待办，不扩大本轮结论。
