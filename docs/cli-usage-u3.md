# U3 逐 CLI 适配进展

2026-10-24。本文保留 U3-001 的冻结实现与验证，追加 U3-002 的重新检测及 Antigravity 分组适配，以及 U3-003 的 OpenCode V2 官方 DeepSeek 余额适配。首轮节点 U3-001：本节点完成作用域 / 数量保真和目录信任保护，**U3a–U3h 的完整实现与真实验收仍未完成**。只运行本地版本、帮助、离线 schema 生成及离线测试；未查询真实账号额度、调用模型、切换账号或修改凭据。

## 已实现

- Claude `/usage`：当前 session、全模型周额度、具名模型周额度分别保存；模型窗口不作为整个账号的额度。缺失 / 非法窗口不覆盖有效窗口；未知标题不被推断为全账号额度。重复作用域拒绝接受。兼容显示仅保留被接受的窗口。
- Cursor：共享快照保留 Included / Auto / API 独立分项，只有 Included 标为账号范围；不相加。On-Demand Disabled 及原始 reset 标签继续由 details 展示，不换算为 0% 或虚构日期。
- Copilot：保存 Plan 下的绝对 AIC / premium requests 数量，使用十进制字符串，不拿会话消耗作账号余额；不虚构五小时周期或 reset。
- Copilot 查询不再传 `--add-dir` 或自动接受 session-only 目录信任。PTY 遇到已知目录信任提示且调用方未授权接受时立即终止，防止延迟输入的回车意外确认。该路径可能因此在未信任目录失败，失败不能降级聊天或悄悄授权。
- Codex：纯结构化解码器覆盖主 / 次窗口、多桶、旧单桶兼容、可选 credits、缺失字段和未知扩展字段。多桶优先，不重复计数旧单桶；credits 不推算订阅百分比。未接入生产查询传输，能力状态仍为 IDENTITY_PENDING。

## 当前验证与阻塞

U3-001 的冻结环境为 Claude 2.1.288、Codex 0.160.0、Cursor 2026.10.22-27dcf5f、Copilot 1.0.91、Kiro 2.34.0，当时未找到其余三个 CLI。用户安装并登录后，U3-002 已在正常登录 shell 重新找到 Antigravity 1.4.7、OpenCode 2.1.4、Reasonix 2.50.0；安装路径阻塞已解除，认证与用量结论分别见下文。

- U3a：有效 OAuth scope / 系统安全存储 / HTTP 认证链未核实。继续使用既有 CLI 路径；不新增钥匙串读取。缺凭据、429、外部换号及完整 UI 验收待做。
- U3b：已用安装版 `codex app-server generate-json-schema` 离线核对公开协议。`account/read` 可指定 `refreshToken:false`，但不能据此证明整个 app-server 生命周期或 `account/rateLimits/read` 不会刷新共享认证。还需证明有效 backend / workspace 归属和无认证写回，才可启用传输；不复制 Token、不消费 reset credit、不启动 thread / turn。
- U3c：历史独立原生查询证据仍有效于其限定范围；本次仅补共享契约分项。不同套餐、身份变更、401/403、代理及完整桌面 UI 未验收。
- U3d：已删除查询路径隐含信任；现有 PTY 对当前 CLI 的就绪 / 内置命令识别和可信目录选择还需完整验证。实机 SDK 账号身份与额度关联、认证失败及新版兼容未验收。
- U3e：Antigravity 1.4.7 的直接管理诊断返回 SUCCESS / command.name=usage / num_turns=0，包含两个 groups。已接入具名 group / bucket id、未知周期保真、0/1 边界和无效 reset 处理；未证明可靠的账号身份来源，生产能力仍为 IDENTITY_PENDING。不把 models 目录视为身份验证。
- U3f：用户已连接 DeepSeek，OpenCode 2.1.4 的 auth list 已返回该 integration；定向读取发现一个 active / type=key 账号。provider.get 确认官方 https://api.deepseek.com 与内置 DeepSeek package。但在 /tmp 观察到的 model.default 仍为 opencode/flint-40b-free，尚不能将免费模型归属为 DeepSeek。V2 默认模型的官方后端与活动 Key 校验、HTTP 请求及十进制余额解析已接入共享服务；当前源不满足条件时拒绝查询。真实 DeepSeek 余额及完整 UI 尚待验证；V1、环境-only 连接和自定义 package 暂不回退支持。
- U3g：Reasonix 2.50.0 的 doctor --json 检测到 deepseek / kind=anthropic / base_url_host=api.deepseek.com / key_present=true / is_default=false。仅证明 CLI 看到了 Key 配置，不代表有效认证、默认模型归属或余额已验证。其帮助明确 login / whoami / logout 是 reasonix.io 论坛与问题跟进身份，不能借此证明模型认证。还需把正常 `/status` 来源和配置指纹绑定到生效模型身份；未自动触发小额推理验证。
- U3h：当前 2.34.0 `chat --help` 区分 V2/V3，`--trust-workspace` 会允许加载项目 hooks / agents / MCP / workflows；本次未执行 `/usage`，也未设置此参数。历史 2.33.1 V2 查询不能代替当前引擎、扩展副作用和身份验证；能力保持 ADAPTER_PENDING。

## 协议资料

- [Codex 官方 app-server 文档](https://learn.chatgpt.com/docs/app-server)：账号管理、JSONL 协议和 rate limits 方法。公开文档不是安装二进制全部副作用的证明。
- 本机 0.160.0 离线生成 `GetAccountResponse`、`GetAccountRateLimitsResponse`、`InitializeParams` schema；哈希见本节点证据。

## 验证

原生单元测试 360 通过 / 28 项真实调用跳过，运行时集成测试 1 通过，桌面库测试 22 通过；TypeScript 类型检查、测试隔离检查与差异空白检查通过。结果归档在 `docs/reliability-evidence/cli-usage/u3-001.json`。合成样本是解析器 / 权限回归测试，不是真实服务商响应或账单零消耗证据。未完成项保留在主清单；没有恢复自动轮询。

## U3-002 安装复检与 Antigravity 分组

本轮直接 CLI 管理诊断用于核实安装与公开命令形状，未通过 Tokage 桌面调用链，因此不是桌面权限、代理或账单验收。未发送普通模型任务；Antigravity 的观察结果仅证明返回的管理回执为零轮，不能推导独立账单零消耗。预先登记的命令、25 秒截止与进程组终止条件，以及脱敏结果见 `docs/reliability-evidence/cli-usage/u3-002.json`。

Antigravity 原生产解析器改为复用统一的分组解码器，避免旧解析路径把不同模型同周期合并。没有新建认证读取路径，没有提前开放查询能力。缺少身份绑定时，用户手动登录和一次 `/usage` 成功仍不足以恢复自动查询。

U3-002 验证：Antigravity 专项 18 通过 / 2 项真实调用跳过，共享用量专项 26 通过；两组测试有重叠，不能相加。最终 group / bucket / period 标识调整重跑 Antigravity 专项通过。隔离检查与差异检查通过。真实 CLI 诊断使用普通安装及登录 shell；没有运行 Tokage 桌面验收。

## U3-003 OpenCode V2 官方 DeepSeek

用户连接 DeepSeek 后重新排查。本机公开 CLI 接口确认：

- `auth list --format json` 已有 DeepSeek 连接，不再是 U3-002 时的空数组。
- `api provider.get --param providerID=deepseek` 返回内置 `@opencode/ai/providers/deepseek`，integrationID / id 都是 deepseek，settings.baseURL 为 `https://api.deepseek.com`。
- `api model.default` 在临时目录仍指向 `opencode/flint-40b-free`，因此本轮没有请求 DeepSeek 余额或运行模型。已请用户通过正常 OpenCode `/models` 选择待测 DeepSeek 模型，并确认工作区；不修改用户模型、默认配置或账号。
- 官方 V2 文档确认 SQLite 由 OpenCode 管理，已保存账号优先于环境连接。本适配使用 CLI 的只读 `auth export deepseek` 定向读取该 integration 返回的账号列表，只使用唯一 active、type=key 的账号；不导出其他 integration、不打开认证数据库、不把凭据写入文件、前端、日志或证据。本轮只打印结构及 active/type/key-present 布尔判定，没有打印或保存 Key 值。

服务在查询前后重读有效身份；Key / 活动账号变化会使旧结果失效。身份指纹采用本机盐 HMAC，只包含不可逆绑定。当前仅支持 CLI 临时目录可证明的默认模型来源，不声称覆盖任意项目层、Tokage 单次模型覆盖或 session 专属模型设置；这些上下文需要后续显式接入，不能从已连接模型名前缀猜测。

只有模型与 provider 都属于内置 DeepSeek、两者 endpoint 都为严格匹配的官方 HTTPS 地址、且没有自定义 settings / headers / body 覆盖时，才读取并使用活动 Key。其他模型、网关、别名、凭据冲突、无账号、旧 CLI 或协议变化均拒绝；不回退环境 Key、旧 auth.json 或另一账号。先检查模型后读取凭据，避免对免费模型触发凭据读取。新 HTTP 路径复用已有原生授权、撤销取消、代理准备、固定地址、禁止重定向、超时、401/403/429 处理及前后身份核对。

响应保存各币种 total / granted / topped-up 三个独立数量，保留原十进制精度；缺字段、数字型金额、重复币种或非法金额拒绝接受，不覆盖旧成功快照。`is_available` 保存为可选 available，不伪造百分比或五小时额度。旧 schema 2 快照缺 available 时仍兼容。OpenCode 能力改为 HTTP / IDENTITY_REQUIRED，实际查询仍受权限和上述身份校验约束；自动轮询保持关闭。完整数量界面展示属于 U4，不能以本节点的原生适配替代桌面验收。

资料：[OpenCode V2 账号与优先级](https://dev.opencode.ai/v2/docs/cli/providers)、[提供商与 endpoint 覆盖](https://dev.opencode.ai/v2/docs/providers/)、[公开管理 API](https://dev.opencode.ai/v2/docs/api)、[DeepSeek 官方余额契约](https://api-docs.deepseek.com/api/get-user-balance/)。

U3-003 验证：原生全量 368 通过 / 28 项真实调用跳过，运行时集成 1 通过；最终来源校验重构及新增“免费模型不读取凭据”进程回归后，共享用量专项 31 通过。桌面库 22、前端 Provider store 10 通过，类型、隔离和差异检查通过。以上测试有重叠，不相加；尚无本节点真实余额或桌面 UI 通过证据。
