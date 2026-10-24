# U0：在线用量查询范围与入口冻结

日期：2026-10-24。审计源码：`6dd19c7`。节点：`U0`。本节点只冻结范围、数据源选择与权限覆盖清单，不实现 U1–U6，不新增查询支持声明。

## 结论与证据等级

首轮平台固定为 macOS；Linux / Windows 未验收。范围为 8 个已有对话历史的 CLI，不包括 Aider。当前代码有 21 个发现注册项、9 个对话 adapter、4 个 `USAGE_QUERY` 声明（Claude、Cursor、Copilot、Antigravity），设置卡片手动查询名单为 6 个（另含 Codex、Reasonix）。这些数字分别表示发现、对话、能力声明和 UI 入口，不能互换。

本次只执行 PATH 查找与 `--version`（每项截止 8 秒）、源码审计和公开文档查阅。没有调用 detect_all / detect_one、额度命令、模型、认证探测、钥匙串或真实凭据文件。没有改 HOME、账号、信任、代理、Git 身份或 `.tokage/`。版本输出不是登录状态证明，也不是查询验收。

证据采用四种口径：源码确认、历史实测、官方协议依据、待验证候选。历史实测不追认为本次新权限验收。合成契约样本见 [U0 证据](reliability-evidence/cli-usage/u0.json)，只用虚构额度与标签，不包含真实账号数据，不作为厂商响应 fixture 或解析测试通过证据。

## 本次版本观察

当前执行 shell 的 PATH：Claude 2.1.288；Codex 0.160.0；agent / cursor-agent 均为 2026.10.22-27dcf5f；Copilot 1.0.91；Kiro 2.34.0。agy、opencode、reasonix 未在此 PATH 找到，不能推断未安装：App 登录 shell、自定义 CLI 路径及历史隔离 checkout 可不同。本次没有重新捕获登录 shell，也没有扫描用户安装目录。

历史查询依据分别保留原版本：Copilot 1.0.96、Antigravity 1.3.4、Reasonix 2.31.0、Kiro 2.33.1；Codex 0.162.0 的近期桌面交付不能替代当前 0.160.0 的额度协议验收。升级 / 降级版本需在 U3 重新验证，不更新 adapter.tested 来伪造通过。

## 逐 CLI 冻结的数据源

### Claude Code / CLAUDE_CODE

认证范围：官方订阅。首个可复用来源为现有交互 `/usage`，认证由 CLI 持有；本地来源为 CLAUDE_CONFIG_DIR（否则 ~/.claude）下约定 statusline 缓存。源码 `usage::claude_settings` 用于识别 API Key、base URL / 云认证模式，不能把套餐识别当作可复用 OAuth 凭据。

目标数据：实际提供的五小时、周及模型窗口百分比、reset；缺字段保持未知。现有 PTY 使用已有信任目录、safe-mode、空 tools / MCP；仍需在 U3 核对当前版本启动副作用、错误和身份关联。官方交互文档仅作交互机制参考；本节点未从该页单独证实当前 /usage 的完整契约，更不构成账单级零消耗证明。

`https://api.anthropic.com/api/oauth/usage` 仅为候选内部兼容接口。有效 OAuth 来源、系统安全存储、scope、headers 及身份优先级均未在本节点验证；不授权新增钥匙串读取，不将 API Key / Gateway 借用订阅。U3 可先保留可靠 CLI 路径，HTTP 未核实前不上线。CLI 失败不能转普通聊天。

### Codex CLI / CODEX_CLI

认证范围：ChatGPT 订阅；API Key / Gateway 不接订阅额度。现有本地来源为 CODEX_HOME（否则 ~/.codex）的 sessions rollout rate_limits；`codex_account` 当前读取 auth.json 的认证模式 / 套餐，尚未覆盖所有系统认证存储，不宜扩展为通用密钥解析器。

优先研究 CLI app-server 的 `account/rateLimits/read`：官方协议 / 源码提供结构化账号额度读取依据，可避免 Tokage 另持 Token。仅 initialize + 账号 / 额度读取，不创建 thread / turn；启动刷新认证、退出清理及本机版本兼容性在 U3 验证后才启用。字段保留多 limit ID、主次窗口、reset、响应实际提供的 credits；不从时长推断模型映射。

现有 `/status` PTY 是实现路径但未声明 USAGE_QUERY，不等于真实额度通过。`https://chatgpt.com/backend-api/wham/usage` 保留候选，不作为 U0 已核实直连接口；认证 headers / workspace 与过期处理待核实，不能绕过 CLI 认证失败或写回共享认证。

### Cursor CLI / CURSOR_CLI

认证范围：官方订阅。冻结现有 `query_cursor_quota`：无初始 prompt 启动 agent，在已有信任目录进入交互 UI，只有确认内置 `/usage` 补全才发送 Enter；拒绝 Key / Gateway，不自动信任。CLI 自持凭据和服务商请求，Tokage 不读浏览器 Cookie 或 Token；复用 Cursor 当前代理准备，失败不降级直连。

数据：套餐、Included / Auto / API 各自百分比、On-Demand 状态 / 文本、reset 原文。各项不相加，Disabled 不画成 0%；缺年份日期不转精确 UTC。源码与同版本真实原生查询已有证据，见 [Cursor 验证](cursor-usage-validation.md)；桌面 UI、身份变化、失败及代理组合仍待 U3/U6。历史资料中的 HTTP usage-summary 不加入当前选择。

### GitHub Copilot CLI / COPILOT_CLI

认证范围：官方 GitHub 订阅。使用现有 `/usage`，CLI 持有认证；环境优先级源码确认为 COPILOT_GITHUB_TOKEN → GH_TOKEN → GITHUB_TOKEN，再由正常 CLI 解析官方登录。某个显式 Token 被拒不得借用另一个账号。Gateway / BYOK 不接 GitHub 订阅。

数据：套餐已用百分比、AI credits 已用 / 上限；legacy premium requests 单列，缺 reset 保持未知。历史 1.0.96 实测有 5,900 / 20,000 AIC 和查询会话 0 AIC；本次 PATH 1.0.91 不追认同样通过。现有解析未完整保留绝对 credits，U3 补齐。

启动参数已限制工具 / 内置 MCP，但源码存在临时目录 accept_folder_once；U3 必须核对信任行为，不将开启额度权限扩大为信任授权。SDK 是否有适合的非聊天查询接口待核实；api.github.com/copilot_internal/user 为候选内部接口，本轮不冻结其凭据读取或字段契约。

### Antigravity CLI / ANTIGRAVITY_CLI

认证范围：Google 官方登录。冻结 `agy -p /usage --output-format json`，CLI 自持登录。虽使用 -p，现有解析强制 status=SUCCESS、command.name=usage、num_turns=0；任一不符拒绝额度，不能把普通模型响应当管理命令。历史版本 1.3.4；本次 PATH 不可用，未新实测。

数据：groups / buckets 中模型分组、remaining_fraction、window、reset_time；已用为 (1-remaining)*100。现有 parser 只留五小时 / 周，丢失组标识，U2/U3 必须保留 scope，未知周期不假定。API Key / Gateway 不借用 Google 额度。无需新增直接 Google HTTP 认证链；实际 CLI 网络目标由 CLI 管理，Tokage 不声称已验证逐请求域名 / 公网 IP。

### OpenCode / OPENCODE

认证范围：只纳入官方 DeepSeek endpoint 与实际生效的该后端 Key；历史对话版本 1.19.0 / 2.0.29，不等于已接余额。目前没有专用在线额度实现或能力声明；本次 PATH 不可用。

冻结可实施接口：GET https://api.deepseek.com/user/balance，Bearer 认证；目标只能为官方 HTTPS 主机与固定路径。官方文档确认 is_available、balance_infos[].currency / total_balance / granted_balance / topped_up_balance；金额字符串按币种保留，不推算百分比、次数或 token。余额接口不是生成接口。

阻塞：OpenCode V1/V2 当前有效 Key 的来源 / 覆盖顺序、配置路径、endpoint 与 provider 绑定尚未通过实机验证；只见模型名前缀不足以请求。U3 验证前不上线，不读写解密后的共享数据库，不索取额外凭据。其他后端不支持在线查询但保留本地任务用量。

### Reasonix / REASONIX

认证范围：Reasonix 官方钱包，不因模型名 deepseek/... 视作用户 DeepSeek 直连。冻结现有 `/status`，CLI 自持认证；历史 2.31.0 有人民币余额记录，本次 PATH 不可用。目标为 CLI 的钱包管理输出，未核实独立 HTTP endpoint，不臆造余额 API。

数据：金额文本及币种；没有分母 / 周期则不画百分比或 reset。当前 `execute_slash_quota` 先写独立余额缓存，再解析额度窗口，可能余额更新但 quota 返回错误 / 旧值；U2/U3 需将余额与本次状态统一返回。配置目录与外部身份有效来源待验证。doctor 状态与显式 API Key 小额推理验证保留原边界，额度权限不能触发验证推理。

### Kiro CLI / KIRO_CLI

认证范围：CLI 当前官方账号，CLI 自持 AWS / 登录凭据；不新增 Tokage AWS 认证。优先 `kiro-cli chat --no-interactive /usage` 管理路径，历史 2.33.1 默认 V2 已实测，见 [Kiro 调查](kiro-cli-support-investigation.md)。本次版本 2.34.0，V2/V3 是否同源、项目 agent / hooks / MCP 初始化、副作用及新版查询性质均未验证；不能在此基础上声明 USAGE_QUERY。

数据：Estimated 套餐、已用 / 总 credits、百分比、reset 原文；购买提示不等于已有 bonus 数量，不换算美元 / token，不伪造五小时。若不能证明管理命令不进入推理或不加载不受控扩展，则保持 blocked，不启用自动查询，也不尝试普通 prompt 兜底。

## 权限覆盖清单（U1 的冻结输入）

所有主动入口必须在任何隐式扫描和子进程 / HTTP 之前经过原生权限判定。单独在 Tauri query_provider_quota 加开关不足。扫描应拆出本地 / 正常认证能力探测和专门在线额度，后者只走统一服务。原生公共函数与 runtime / CLI 不得成为无门禁旁路。

- Q01 手动卡片：CliSettings → providers.refreshQuota → ipc.queryProviderQuota → query_provider_quota。当前无后端权限，6 个硬编码对象；失败可能返回旧缓存。U1 阻断联网；U2 返回本次状态，U4 改本地按钮与能力元数据。
- Q02 自动刷新：providers.load 后 scheduleQuotaPolling、设置 activeQuotaQuery / meltThreshold / debugProvider 变化重新调度。当前只跳过 NOT_INSTALLED，不按启用 / 认证 / 能力过滤；立即 tick，闲置十分钟暂停，2/5 分钟间隔。U1 等待权限恢复并取消 / 废弃迟到结果；U2 改单位和统一发布；新调度采用设计的 3/10 分钟。
- Q03 启动与全量重扫：providers.load → refresh → refresh_providers → detect_each → Antigravity probe → query_antigravity_quota。额度开关关闭仍可能查询。U1 禁止扫描隐含专门额度请求，保持其他正常认证 / 模型目录边界。
- Q04 单卡重扫：refreshOne → refresh_provider → detect_one。Antigravity probe 会查额度；Reasonix READY 后额外 /status。U1 删除隐含额度，开启后也由统一服务显式调度。
- Q05 空 Provider 缓存：AppState.providers_or_scan → detect_all。调用方包括 query_provider_quota、verify_antigravity_api_key、verify_reasonix_api_key 和普通 turn 启动。请求其他 CLI 也可能先全扫触发 Antigravity；须在扫描层消除隐含额度，不能等扫描后检查目标。
- Q06 共享 runtime：service::run_native → detect_one，每次真实 planner / executor / reviewer 启动可能间接查 Antigravity / Reasonix。正常模型请求仍可运行，但不得附带专门额度查询；权限关闭不能禁掉模型输出自身的被动用量。
- Q07 正式 CLI：cli::dispatch 的 providers list → detect_all；共享 runtime 执行同 Q06。没有专用用量命令也存在扫描旁路。CLI 没有用户显式开启来源时默认拒绝专门额度查询；不得复用 App 前端布尔值或偷用其他数据目录权限。
- Q08 显式 API Key 验证：用户确认的模型验证与本权限分离；验证前 providers_or_scan、验证后 detect_one 都应遵守 Q03–Q05。额度权限不能反向授权 verify_* 推理。
- Q09 恢复 / 重连：当前没有独立原生在线额度恢复调度器；UI 重载会重走 load，缓存为空会重走 Q05，恢复任务可能重走 Q06。U1 先恢复本机权限再安排查询；导入 / 旧设置 true 不授予权限，运行中关闭后旧授权结果不得发布。
- Q10 被动来源：usage.rs 本地 statusline / rollout / 缓存、正常 CLI stream Done / quota 信号、任务账本与 project_utilization_now。继续允许读取；读取不得隐式扫描 / 联网。来源为 Estimated 的投影不能标为厂商查询；Provider 单键缓存、过期 reset 伪零和 1% 单位问题交 U2/U4。
- Q11 原生辅助函数：quota.rs 的 query_cursor_quota、query_antigravity_quota、execute_slash_quota 和 execute_subcommand_quota。最后一个目前无生产调用方，公开能力依然需在 U1/U2 设计中避免旁路；取消权限 / 身份代际在返回及任何 cache write 前核对，包含 Reasonix 内部余额写入。

审计范围覆盖 src、src-tauri、cli 的生产引用；测试 / 实机脚本只作为历史或测试调用方，不能自动执行。通用 get_provider_utilization_now 是本地投影，不应为了读利用率触发查询。无新增 U0 测试驱动。

## U1 进入条件与后续阻塞

U0 已冻结“允许哪些组合、采用何种来源、哪些路径必须拦截”。未知凭据、版本和副作用明确为对应 U3 发布阻塞，不要求 U0 读取真实密钥或完成各家实机查询。U1 可先用测试文件中的合成数据完成默认关闭、原生持久化、代际与扫描拆分；后续适配须按来源验证，不能因 U0 完成而勾选 U3/U6。

原生权限应由桌面与共享查询服务共同遵守；底层扫描 API 必须默认不查额度，runtime / CLI 不得默认开启。普通认证与模型目录请求不受额度开关直接禁止，但不能以认证为名继续使用兼具额度抓取的路径。Antigravity READY 过去依赖 /usage，拆分时需提供独立可信认证证据或降为待确认，不能无依据保留 READY。

无账户切换、重试付费、全局在线权限实现或模型验收。可靠性第十项继续暂停。

## 公开依据与验证

- [DeepSeek 官方余额接口](https://api-docs.deepseek.com/api/get-user-balance/)：2026-10-24 查阅，GET 管理资源与字段依据。
- [Codex 官方账号处理源码](https://github.com/openai/codex/blob/main/codex-rs/app-server/src/request_processors/account_processor.rs)：account/rateLimits/read 候选协议依据；main 为可变版本，U3 须固定兼容版本，不据此宣称本机通过。
- [Claude 官方交互文档](https://code.claude.com/docs/en/interactive-mode)：管理命令性质参考，实际版本行为以源码 / 实测为准。
- 本仓库 quota.rs、usage.rs、providers.rs、adapter.rs、src-tauri/src/lib.rs、runtime/service.rs、cli/src/lib.rs、providers Store、CLI / Other Settings 的精确哈希在证据 JSON；证据无个人凭据。

校验内容：源码数字与入口复核、8 个 Provider / 合成样本完整性、相对文档链接、JSON 结构、隔离静态检查和 git diff --check。本节点没有生产代码改动，不运行模型或无关全量构建。验证结果见证据 JSON 的 validation。
