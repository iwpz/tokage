# CLI / Provider 支持进度与 TODO

更新日期：2026-10-13。范围：当前工作区源码，含尚未提交的修改；不是厂商 CLI 最新功能清单。

## 判定口径

- `[x]`：在 Tokage 源码中已实现，或本会话有明确验证记录。勾选旁注明实现与验证范围。
- `[ ]`：待实现、待加强或待验收；不表示厂商 CLI 本身不具备该能力。
- “检测 READY”只表示探测通过。配置文件、Key 存在或模型目录可读，不等于凭据有效，也不等于真实对话通过。
- 不使用主观完成百分比。以“对话已接入 / 有限对话接入 / 仅检测接入”描述成熟度，再逐项追踪。
- 不支持或有意不做的能力放在“边界”中，不将其伪装成待完成需求。
- 实机验收使用用户指定的模型 / 档位；付费调用仅用于明确授权的验收，普通扫描不自动发送模型请求。当前机器状态可能随安装、登录和额度变化。

## 总览

- [x] 21 个真实 CLI 注册发现、版本探测和安装引导。
- [x] 8 个 CLI 已注册 headless 对话适配器：Claude Code、Codex、Cursor、Copilot、Antigravity、Aider、OpenCode、Reasonix。
- [ ] 13 个 CLI 尚无对话适配器：Grok、Kiro、Qwen、Kimi、Qoder、Amp、Crush、Goose、Cline、Droid、Auggie、Muse、Trae。
- [x] 本会话实机对话成功：Claude Code 2.1.296、Codex CLI 0.162.0、Cursor CLI 2026.10.01-e373342（仅 auto）、Antigravity CLI 1.3.4（仅 Google / Gemini Flash Low）、Reasonix 2.31.0（仅 DeepSeek Flash）、Copilot CLI 1.0.96（仅 auto + efficiency）。
- [x] 本会话最近实机检测 READY：Claude、Codex、Cursor、Antigravity、Reasonix、Copilot（后两者经过明确的模型验证）。
- [x] Cursor 在钥匙串规则修复后，使用订阅账号 auto 完成真实规划 / 编辑 / resume / 审查验收。
- [x] Antigravity 1.3.4 使用当前 Google 账号完成固定 Gemini Flash Low 角色链实机验收。
- [x] Reasonix 补充 DeepSeek Flash 实机对话、文件读取、resume / copy、原生会话及官方 /status 余额刷新验收；命令执行与完整任务流水线仍待完成。
- [x] Copilot CLI 1.0.96 使用 auto + efficiency 完成认证、规划、文件编辑、Python 验收、resume、只读审查及原生会话读取。
- [ ] Aider、OpenCode 补充安装后的实机验收；此前本机未安装。

## 共用验收清单

- [x] provider 注册表与 adapter 注册表分离；无 adapter 的 CLI 不声称可以对话。
- [x] Claude / Codex / 通用 status 探测使用对话环境与适用沙箱；Claude 校验 loggedIn，Cursor / Codex 校验正向登录输出。
- [x] 修复 macOS 钥匙串允许规则被共享禁读列表覆盖的问题；已有沙箱回归测试。
- [x] Gateway 在设置卡片与顶部 CLI 列表显示独立模式标签；故障状态优先显示。
- [ ] 将同等严格的认证验证扩展至 Aider、OpenCode 和仅检测 provider；区分未配置、未知、已配置、认证失败。
- [ ] 验收 Gateway 的有凭据 / 缺凭据 / 失效凭据 / 错误端点；本地配置存在不能代表上游连通与授权正常。
- [ ] 为每个对话适配器验收：登录、模型选择、简单回复、文件编辑、会话恢复、取消、超时、用量、权限拒绝。Cursor 本次已补充核心订阅实机验收与隔离回归，详见下节。
- [ ] 增强 live E2E 断言：turn_per_cli 当前打印 Err 后仍可能整体通过；smoke 当前 Thread 或 Done 即可满足断言。应校验返回值成功、实际文本及需要时的文件变化。
- [ ] 实机测试“跳过未安装”单独汇总，不将 skip 计作 provider 验收通过。
- [x] Antigravity 新增独立固定模型实机脚本，Cursor 已有独立 auto 脚本。
- [ ] 扩展实机脚本覆盖 Aider、OpenCode；通用 turn_per_cli 仍仅枚举 Claude / Codex / Cursor / Copilot。
- [ ] 每个 provider 分别记录 macOS / Linux / Windows 验收；存在平台实现不代表各平台实机通过。
- [ ] 核对升级后的 CLI 参数与事件格式，更新 fixture 及 AdapterMeta 版本范围。
- [ ] 核对高级能力声明与实际路径：目前 8 个 adapter 均未声明 APPROVAL_CALLBACK、WRITE_SCOPE、COMPACT_HEADLESS、COMPACT_PTY；Antigravity / Copilot 已声明并实测 USAGE_QUERY；知识包或公共机制存在不能直接视为各 CLI 已验收。

## 已接入对话的 CLI

### Claude Code — `CLAUDE_CODE`

状态：对话已接入；本机会话成功。命令：`claude`。fixture 版本：`2.1.288`；声明兼容范围：`>= 2.1.288, < 3`。

- [x] 登录状态明确解析 loggedIn / authMethod，复用对话沙箱及宿主变量清理；未登录不误报 READY。
- [x] API Key / 自定义 Base URL 配置识别与 Gateway 模式展示。
- [x] JSONL 流式事件、模型选择、resume、原生 fork、工具规则与拒绝事件。
- [x] OS 沙箱、shell wrapper、原生会话读取与终端接管。
- [x] 本机 2.1.296：钥匙串修复后检测 READY，真实 Haiku smoke 返回 Ok(())。
- [ ] 实机验证注销后重扫、凭据失效、登录后重扫三条路径。
- [ ] 实机验收 Gateway、resume / fork、文件编辑、拒绝工具及完整任务流水线。
- [ ] 实机验收额度查询与 token / cache 用量归因。

边界：无原生审批回调能力声明；Tokage 工具规则 / 拒绝处理不等于 CLI 原生审批回调。

### Codex CLI — `CODEX_CLI`

状态：对话已接入；本机会话成功。命令：`codex`。fixture 版本：`0.160.0`；声明兼容范围：`>= 0.160.0, < 1`。

- [x] login status 正向登录证据；本地模型缓存与模型 effort 分组。
- [x] 结构化事件、模型选择、resume、自带沙箱、拒绝事件、shell wrapper。
- [x] 原生会话读取、终端接管、账号套餐及会话日志用量读取。
- [x] 本机 0.162.0：真实调用返回文本及 Done，检测 READY。
- [ ] 实机验收执行角色文件编辑、任务验收及完整流水线。
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
- [ ] 从桌面界面完成持久化计划 → DAG 调度 → Git worktree / 提交 / 合并的全流程；本次实机覆盖 run_turn 后端角色链，调度由既有前端回归测试覆盖，不能视为完整桌面实机验收。

边界：工具规则来自 CLI 配置，无 TOOL_RULES / SHELL_WRAP / NATIVE_FORK / APPROVAL_CALLBACK 声明。原生审批回调、付费金额和订阅剩余额度不能由 token 用量或模型调用次数推出。

复验入口：`scripts/run-live-cursor-auto.sh`。只请求 Cursor `auto`，不会 fallback 到其他 provider / 模型；安装、认证、回复、用量或文件验收失败均使脚本失败。fixture / stub / 取消超时测试不消耗模型额度。

本轮发现并修复：原生 plan 模式曾导致审查轮的严格回复验收失败（计划工具保存了验收结果但无对应助手回复）；未将该次失败记为通过。随后还发现 ask 状态会随原生会话持久化，导致跨模式 resume 后无法编辑；已修正主对话会话复用，最终验收按模式切换新建会话，同模式单独验证 resume。

### GitHub Copilot CLI — `COPILOT_CLI`

状态：本机订阅账号完成核心角色链实机验收。命令：`copilot`。本机及新增真实 fixture 版本：`1.0.96`（保留 `1.0.91` 回归 fixture）；声明兼容范围：`>= 1.0.91, < 2`。本轮所有模型调用使用 `--model auto --auto-tier efficiency`，实际路由均为 `gpt-6-luna`。

- [x] 环境 Token / keychain / 本地配置 / GitHub CLI 凭据来源探测；仅凭据存在显示 UNKNOWN_AUTH，不误报 READY。
- [x] 设置页提供明确标注 AI credits 消耗的连接验证按钮；通过同一过滤环境 / OS 沙箱发送最小 Auto + efficiency 请求，必须同时获得正确回复与成功终态。
- [x] 验证结果只保存在当前进程内，绑定配置 / 环境 SHA-256 指纹及 keychain 项目元数据，五分钟有效；配置变化或运行时认证失败撤销验证，不读取 keychain 密钥值。
- [x] 本地 stub 覆盖配置存在但未验证、模型失败、验证成功、Token 变化、Gateway 模式、错误输出不泄露凭据；不修改真实账号或凭据。
- [x] 官方 `help config` 模型目录与 Auto 的 efficiency / balance / intelligence 分组。
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
- [ ] 实机验收环境 Token 与 keychain / gh 多账号优先级、注销 / 过期 / 无订阅 / 额度耗尽 / 网络失败（当前失败回归使用 stub，未改动真实登录）。
- [ ] Gateway / BYOK 的真实请求与手动验证；Auto + efficiency 的订阅验证不适用于自定义上游。
- [ ] 实机验收取消、超时、恶意写入拒绝、Tokage 完整任务 DAG 流水线；本次角色链为直接运行生产 turn 路径。
- [ ] Linux / Windows 实机验收。

边界：没有原生 fork 或审批回调能力声明；不把 resume 当成复制会话。help config 是离线目录，不作为认证证据。五分钟验证过期后重新检查会回到待验证状态，不自动消费模型额度。

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

验收边界：有效 Gemini API Key、实际自定义 endpoint 与账号切换未做实机测试，遵循用户“仅验证当前 Google 账号”的范围；相关实现由本地 stub 覆盖。后端真实角色链与已有调度回归已验证，完整桌面 DAG → worktree → commit / merge 的界面操作仍属于共用流水线验收。Linux 只读沙箱分支已实现，当前实机为 macOS；没有可用沙箱的平台会拒绝启动只读角色。

能力边界：撤销未经本机 CLI 支持的 TOOL_RULES / SHELL_WRAP 声明；保留结构化输出、模型选择、resume、read-only、DENIAL_EVENT、OS_SANDBOX，新增 NATIVE_SESSIONS / USAGE_QUERY。普通执行保留 CLI 自身权限规则；仅显式 FullAuto 执行角色使用对应的跳过权限参数。无原生审批回调，API 次数 / 最终上下文尺寸 / USD 费用未报告时保留未知，不根据 token 数伪造。

依据：[官方 headless 文档](https://www.antigravity.google/docs/cli/headless/)、[认证文档](https://www.antigravity.google/docs/cli/install/)、[执行模式说明](https://www.antigravity.google/docs/cli/modes/)；参数、协议及数据目录由本机 1.3.4 交叉核对。

### Aider — `AIDER`

状态：有限对话接入；本机未安装。命令：`aider`。fixture 版本：`0.72.0`；声明兼容范围：`>= 0.50.0, < 2`。

- [x] 多厂商 Key 名探测与按后端查询模型列表。
- [x] --message 非交互调用、文本解析、模型选择及 token / cost 汇总。
- [x] 只读角色 --dry-run、--restore-chat-history 恢复路径与 OS 沙箱。
- [x] 文本 fixture、launch 与解析测试。
- [ ] 安装后验证 DeepSeek 等已配置模型真实调用。
- [ ] 加强失效 Key 验证；模型目录离线可用不能证明 Key 有效。
- [ ] 验证 --restore-chat-history 的目录隔离，避免不同会话串历史。
- [ ] 补充完整工具拒绝 / 审批行为验证；不将 --yes-always 当审批支持。
- [ ] 验收文本变体、文件编辑、费用归因和任务流水线。

边界：非 JSONL，无 STRUCTURED_OUTPUT / DENIAL_EVENT / NATIVE_SESSIONS 声明；不支持按 session ID 终端接管，RESUME 是历史文件恢复。

### OpenCode — `OPENCODE`

状态：对话已接入；本机未安装。命令：`opencode`。fixture 版本：`0.8.4`；声明兼容范围：`>= 0.1.0, < 2`。

- [x] auth.json 存在探测、models 命令与登录引导。
- [x] 结构化事件、模型选择、resume、工具规则与拒绝事件。
- [x] OS 沙箱、shell wrapper、终端接管与 fixture 测试。
- [ ] 安装并验证真实对话、resume、编辑和任务流水线。
- [ ] 替换仅凭 auth.json 存在判 READY 的方式；验证空文件 / 陈旧文件 / 无效凭据。
- [ ] 核对 CLI 版本变化、模型目录、用量与权限模式。
- [ ] 如需原生会话透视，新增读取器与能力声明。

边界：尚无 NATIVE_SESSIONS / NATIVE_FORK / APPROVAL_CALLBACK 声明。

### Reasonix — `REASONIX`

状态：本机 2.31.0 + 官方 DeepSeek Flash 已通过实际对话与续聊验收。常规扫描仅检查凭据配置，显示 UNKNOWN_AUTH；显式「验证 API Key」发送小请求成功后，本进程内显示 READY。密钥、配置、环境变更或认证失败会使验证失效。

- [x] doctor / doctor billing 使用对话环境与适用沙箱；兼容 JSON 后附带 stderr 警告。
- [x] `key_present` 不再充当认证成功；缺密钥 NEEDS_LOGIN，已配置但未验证 UNKNOWN_AUTH；设置页支持显式验证。
- [x] 本机 `deepseek/deepseek-flash` 实际回复、工具文件读取、resume 保留上下文、--copy 创建独立会话、原生会话读取。
- [x] 修复流式片段之间多余换行、最终回复回退、失败写操作误报文件变更、重复工具启动事件。
- [x] 官方 `/status` PTY 实机刷新人民币余额；修复 Enter 字符，不虚构五小时 0% 配额。
- [x] 本地模拟无效凭据 / 成功认证、配置变更失效及 stderr 警告；新增 `scripts/run-live-reasonix.sh`。
- [ ] 解决 macOS 外层 Seatbelt 与 Reasonix 内部 Bash 沙箱冲突；当前对话 / 文件读取成功，命令执行仍受限。保留现有保护，不自动修改用户 sandbox 配置。
- [ ] 实机验证失效 Key、余额不足、网络错误；目前失败认证使用本地模拟，未改动真实密钥。
- [ ] 验收目录 trust、文件编辑、shell 验证及完整任务流水线；Tokage 不代写目录信任。
- [ ] 验收 /status 网络延迟、超时与其他币种；人民币正常路径已通过。
- [ ] Linux / Windows 实机验收。

边界：默认隐藏、用户主动添加；无等价 trust flag，依赖 reasonix trust --dir。当前明确不做 ACP host / 原生审批回调 / Studio 会话接入。配置和密钥处理遵循 [官方文档](https://reasonix.io/docs/)，实测命令与事件以本机 2.31.0 为准。

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

### Kiro CLI — `KIRO_CLI`

状态：仅检测接入。命令：`kiro-cli`。此前本机未安装，尚无本会话实机调用记录。

- [x] 白名单注册、路径 / 版本探测与安装引导。
- [x] 初步认证 / 模型探测代码：专用 whoami JSON 账号解析；chat --list-models JSON。
- [ ] 严格验证 whoami 正向身份字段与退出码、超时，统一对话环境。
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

- [ ] P0：让实机 E2E 遇到真实调用 Err 时失败，并提供逐 provider 的 pass / fail / skip。
- [x] P0：Cursor 修复后 auto 真实调用、resume、编辑、审查复验。
- [ ] P0：Claude / Codex 增加登录前后与失效凭据回归。
- [ ] P1：为其余 5 个已接入 adapter 完成认证和实机验收，重点排除配置存在造成的 READY 误报。
- [ ] P1：覆盖已有 8 个 adapter 的 resume、权限拒绝、用量、编辑及任务流水线。
- [ ] P2：按实际需求逐个新增其余 13 个 adapter；每个完成最小实机验收后再宣称可对话。
- [ ] P3：按厂商能力和产品需求扩展高级审批、压缩、原生会话、跨平台兼容；有意不做的项目需保留边界说明。

## 核对依据与验证方式

以源码为准，开发文档仅作为历史验收证据：

- `src-tauri/crates/tokage-core/src/provider.rs`：21 个真实 provider 身份。
- `src-tauri/crates/tokage-native/src/providers.rs`：白名单、认证、模型与账号探测。
- `src-tauri/crates/tokage-native/src/chat/adapter.rs`：8 个 adapter、能力声明、fixture 版本及兼容范围。
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
