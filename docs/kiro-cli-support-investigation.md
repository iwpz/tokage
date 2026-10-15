# Kiro CLI 支持度调查

日期：2026-10-15。本机 macOS，`kiro-cli 2.31.1`；Tokage 当前工作区源码。本文保留初始调查，并在文末记录同日完成的适配与验收；当前已新增对话适配器。

## 结论与建议

Kiro 值得列为下一批 P1 接入候选：已有结构化输出、按 ID 恢复、模型选择及 ACP 双向协议，V3 还声明分叉、历史导出与压缩能力。接入价值主要在 Kiro 账号、credits、会话及权限体系，而非重复实现通用模型列表。

建议先实现固定 V2 的最小对话闭环，再按能力协商增加 V3。CLI 包版本与引擎版本必须分别记录；同一个 2.31.1 可运行 `--v2` 和 `--v3`，两者不是同一事件或权限协议。

长期优先评估 ACP：双向 JSON-RPC 可以承接审批与取消。单向 headless JSONL 适合首轮适配与回归，但不能因为输出载荷是 ACP 就声称已支持外部审批回调。是否分两阶段实施，应结合 Tokage 新增双向 transport 的成本决定。

## 本次实测

使用临时目录，不发送项目源码。固定 `claude-haiku-4.5`，不启用 `--trust-all-tools`。共 3 次真实模型请求，无工具调用事件：

1. V2 首轮：返回 `TOKAGE_KIRO_791`；exit 0，`runFinished.status=success`、`stopReason=end_turn`。
2. V2 `--resume-id`：新提示不包含原标记，成功回忆原标记；相同 sessionId，exit 0。
3. V3 首轮：返回 `TOKAGE_KIRO_V3_791`；exit 0，成功完成，配置事件确认所选模型为 Haiku。

CLI 报告用量分别为 0.0058918981、0.0046026112、0.0276800633 credits，合计约 **0.038175 credits**。这不是美元金额，也不是 token 数。

V2 测试采用临时 `.kiro/agents/tokage-probe.json`：`tools=[]`、`allowedTools=[]`、`resources=[]`、`mcpServers={}`、`includeMcpJson=false`。V3 使用内置配置，提示禁止工具并传入空 trust-tools；实际无工具调用，但不据此宣称工具从配置层完全隔离。

原始证据位于本机临时目录（非仓库、非长期 fixture）：

`/private/var/folders/8n/5_wv5ynd46s07bw1l1dsk73w0000gn/T/tokage-kiro-investigation-q4we1atj`

文件：`v2.stdout.jsonl`、`v2-resume.stdout.jsonl`、`v3.stdout.jsonl`、对应 stderr，以及两个 `*-acp-init.jsonl`。未修改用户全局配置或凭据。

## 已核验的 CLI 接口

- 非交互：`chat --v2 --no-interactive --output-format stream-json --model <id> --agent <name> --trust-tools= <prompt>`。
- V3：相同 headless 入口，显式 `--v3`；模型参数在实测中有效。
- `--format json` 用于模型和会话列表；对话输出应使用 `--output-format stream-json`，不能混用。
- 会话列表：`chat --list-sessions --format json`，实测返回按 cwd 包装的数组，含 `sessionId/source/title/updatedAt/messageCount/complete`。
- CLI 还公开 `--resume`、`--resume-id`、`--resume-picker`、`--delete-session`、`--session-source`，本次仅恢复 ID 和列表有实测成功记录。
- `chat --help` 公开 `--model`、`--effort`、`--trust-tools`、`--require-mcp-startup`。所选 Haiku 的 reasoning support 实测为 unavailable，不能向所有模型统一暴露 effort 档位。
- `acp --help` 公开引擎选择和 V3 `--auth-method cli|client`；当前登录可完成 V2/V3 对话，不需要本次额外配置 API Key。官方 headless 文档以 KIRO_API_KEY 为主，不能据此否定本机登录实测。

## 输出、完成与计费

V2 headless 实测事件：`runStarted`、`metadata`、`sessionUpdate`、`runFinished`。文本在 `data.update.content.text`，会话 ID 在 `data.sessionId`。

V2 用量为 `metadata.data.meteringUsage[].{value,unit}`；V3 为 `sessionUpdate.data.update._meta.kiro` 中 `kind=turn_completion` 的 `promptTurnSummaries[].{usage,unit}`。V3 还包含模式、模型、上下文占比等配置／状态事件，需要过滤，不能全部当正文。

本次两种引擎均未报告精确 input/output token 计数。上下文占比和 breakdown 不是逐轮计费 token。Tokage 应单独保存 credits；若当前用量结构仅有 token / USD，应先扩展结构，不能用倍率换算出虚构 token 或金额。

完成判定应结合进程状态、最终 `runFinished.status/stopReason` 与取消／超时状态；缺少终态、refusal、错误和截断不得直接当正常完成。`finalText` 会重复流式正文，不能再追加一次。

## ACP 能力：握手实测声明，未全部执行

本次两个引擎均真实发送 `initialize`，没有发送模型 prompt：

- V2：`loadSession=true`、图像输入、HTTP MCP；未声明 `sessionCapabilities.fork`。
- V3：`loadSession=true`、图像／embeddedContext、HTTP/SSE MCP；声明 `sessionCapabilities.list/close/delete/fork`，分叉还声明 messageId 定位。
- V3 扩展包含 `_kiro/session/context`、`compact`、`export`、`history`；以及 workflow、memory 等。只有协商返回的扩展才可调用。
- 本机 V3 握手列表没有 `_kiro/account/getUsage`，即使最新文档提到也不能直接启用额度查询。

官方 ACP 文档有外部权限请求及取消路径；本次未触发审批、执行 fork/compact/export，也未验证 V3 resume。它们属于下一阶段验收，不应直接设置 Tokage 的 APPROVAL_CALLBACK、NATIVE_FORK、COMPACT_HEADLESS 或 USAGE_QUERY 标志。

## 权限与平台边界

V2 JSON agent 的 `tools/allowedTools/toolsSettings` 与 V3 permissions 配置需要不同实现。空 `--trust-tools=` 不等价于禁用所有内置只读能力；更不能覆盖用户配置中已有的信任规则。

Tokage 接入时应创建受控 agent，隔离 workspace/global MCP、hooks、resources 与默认配置，分别验收只读和执行者。工具审批建议经 ACP 承接，再接现有命令门禁；外层 OS 沙箱仍需实测，不能用内置 plan 模式替代。

官方 headless 文档说明 V3 的配置型沙箱可能在激活失败时继续运行；如使用原生沙箱需核实安装版本支持并采用失败即退出路径，本机帮助未直接列出 `--sandbox`，本次未验证该参数。

本次只验收 macOS。官方目前列出 macOS、Linux、Windows 11 PowerShell 支持；Linux/Windows 的路径、进程组取消、凭据读取及沙箱都需要独立验收。

当前模型列表为 Kiro 服务提供的 9 个选项，包括 auto、Claude、DeepSeek、MiniMax、GLM、Qwen。未找到并验证通用 OpenAI-compatible base URL / 任意 BYOK 接口，不应把 Kiro 当作 Aider 式任意后端代理。

## 接入前差距与实施顺序（初始调查）

当前 `providers.rs::probe_kiro` 已做 whoami / 模型列表并发探测；`chat/adapter.rs` 没有 Kiro adapter，因而尚不能在 Tokage 发起对话。探测中只凭 whoami 退出 0 即 READY，即便身份字段为空也会通过，需补严格正向身份检查及失败测试。

1. **P1 基础接入**：V2 启动、模型选择、文本／会话／终态／credits 映射；真实输出脱敏 fixture、解析及失败回归；保持扫描无模型请求。
2. **P1 任务安全闭环**：受控 agent 与环境隔离、只读／工作区编辑、权限拒绝、取消、超时、工作树角色链、桌面 DAG 和 Git 门禁。当前均未通过本次验收。
3. **P2 V3 与双向能力**：独立协议分支、ACP 审批和 cancel、V3 resume；按声明启用 fork/history/export/compact；逐项实测后再标能力。
4. **P2 平台与账户异常**：Linux/Windows，失效登录、额度耗尽、无效模型、MCP 启动失败、网络中断及版本升级回归。

## 官方资料

- [Headless mode](https://kiro.dev/docs/cli/headless/)：结构化输出和 headless 权限／沙箱行为。
- [ACP](https://kiro.dev/docs/cli/acp/)：双向协议入口及会话生命周期。
- [V3 ACP migration](https://kiro.dev/docs/cli/v3/acp-migration/)：V2/V3 协议、权限与可协商扩展差异。
- [Agent configuration](https://kiro.dev/docs/custom-agents/configuration-reference/)：V2 legacy 字段与 V3 permissions 差异。
- [Installation](https://kiro.dev/docs/getting-started/installation/)：当前平台范围。

资料与安装版存在演进差异时，以本机帮助、握手和本次实际事件为当前验收依据。


## 同日实施与验收

已新增 Kiro adapter，CLI 包版本范围 `>=2.31.1,<3`，默认固定 V2；不随用户默认引擎自动漂移。需要新会话使用 V3 时，可在登录 shell 环境配置 `TOKAGE_KIRO_ENGINE=v3` 后重新扫描环境；设置 `v2` 或移除恢复默认。该变量仅选择引擎，不改变模型。已保存会话 ID 的引擎标签优先，恢复时不会跨引擎误用 ID。

V2 和 V3 均通过 Tokage 正式 `run_turn` 路径：固定 Haiku 4.5 聊天、m.py 文件读取／编辑、独立 Python 断言、按 ID 恢复记忆、只读审查。V3 同时验收了原生会话快照读取。V2 首次角色链通过记录 `/tmp/tokage-kiro-v2-live.log`，V3 通过记录 `/tmp/tokage-kiro-v3-live.log`。

桌面 UI 的 V2 双任务验收也通过：T1 修复加法，T2 依赖 T1 并组合结果；独立 worktree、4 次门禁、任务提交及最终合并，仅修改 calc.mjs / compose.mjs。2 条真实 Kiro credits 用量持久化到独立临时数据库，无模型或 provider 回退。证据：`/private/tmp/tokage-live-desktop-142t_9ls/verified.json`。

每轮在用户 Kiro agents 目录建立唯一的临时 profile，结束／取消／超时后清理，不修改现有 agent。V2/V3 分别设置文件工具与权限；不暴露 Shell、委派、MCP 和自动资源。V3 工作区读权限显式设置，写权限限定当前范围，空信任列表不替代策略配置。只读窗口额外由 OS 沙箱限制工作区写入。V3 的独立 hooks 发现范围包括 HOME 与工作区祖先；存在 hooks 时拒绝该引擎，避免脱离 Tokage 门禁执行脚本。

补齐 Kiro 自己的 macOS `~/Library/Application Support/kiro-cli`、Linux `~/.local/share/kiro-cli` 认证存储访问：V3 刷新登录依赖这些目录。其他厂商凭据仍保持隔离。增加回归覆盖：严格登录身份、错误／refusal／截断／缺失终态、重复用量快照、流式文本不重复、提供商环境隔离、取消／idle／hard timeout／非零退出，以及沙箱下禁止工作区写入与读取其他凭据。

新增共享 nullable `credits` 字段，贯通 Done、TurnUsage、前端聊天、SQLite usage_events 与读取接口。旧数据库通过 V11 migration 加列，旧用量记录保持兼容。credits 不转换成 token、premium requests、美元或估算美元。

手工复验入口：`bash scripts/run-live-kiro.sh`，会产生少量真实 credits 消耗，先分别验证 V2/V3 角色链，再分别跑 V2/V3 桌面 DAG；普通测试与扫描不会运行这些付费调用。

当前能力标志为 STRUCTURED_OUTPUT、RESUME、MODEL_SELECT、READ_ONLY_MODE、TOOL_RULES、OS_SANDBOX、NATIVE_SESSIONS。ACP 审批回调、原生 fork / compact、独立额度查询仍未接入，不声明能力；Shell 验收命令由 Tokage 门禁执行。仅 macOS 有本次实机验收；Linux / Windows 尚需平台验收，缺少只读沙箱时拒绝对应窗口。

最终验证：工作区原生回归 260 项、存储 28 项、前端任务 42 项通过，前后端构建和 i18n 检查通过。另对只包含本次提交内容的隔离快照执行 Kiro 专项回归，10 项通过、1 项真实付费测试默认忽略；隔离快照全量原生测试也通过 238 项；提交独立性检查不依赖其他尚未提交的 provider 修改。

最终隔离提交快照的桌面验收也全部通过：V2 证据 `/private/tmp/tokage-live-desktop-qp920mfb/verified.json`；V3 证据 `/private/tmp/tokage-live-desktop-p66o_fsh/verified.json`。V3 验收显式固定 engine=v3，并校验所有实际会话 ID 的引擎标签；两套均为 2 个任务、4 次门禁、2 条真实 credits 用量记录。V3 支持 Tokage 将文件规范化为 `file/**` 的写范围格式，同时包含文件本身，防止目录 glob 导致文件操作被误拒绝。
