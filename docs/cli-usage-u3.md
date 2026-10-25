# U3 逐 CLI 适配进展

**当前状态索引（2026-10-25）：**本文各 U3 节点是冻结历史，不再代表最新全部能力；Reasonix/Kiro/AGY 等后续实现及逐组合边界见 [支持审计](cli-usage-support-audit.md)。历史失败、待测项不删除或追认为通过。

2026-10-24。本文保留 U3-001 的冻结实现与验证，追加 U3-002 的重新检测及 Antigravity 分组适配，以及 U3-003 的 OpenCode V2 官方 DeepSeek 余额适配、U3-004 的真实只读余额诊断、U3-005 的显式钱包入口与会话隔离。首轮节点 U3-001：本节点完成作用域 / 数量保真和目录信任保护，**U3a–U3h 的完整实现与真实验收仍未完成**。只运行本地版本、帮助、离线 schema 生成及离线测试；未查询真实账号额度、调用模型、切换账号或修改凭据。

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
- U3f：官方 DeepSeek Key 的独立只读诊断已返回 HTTP 200 / CNY / available。U3-005 将产品入口明确为 OpenCode 服务的 DeepSeek 钱包，直接绑定该 integration 的唯一活动凭据，不再依赖 model.default 或任取会话。钱包数值与会话 account / task 用量独立，原生端不再用钱包身份阻断或恢复任意 OpenCode 任务。钱包分项 UI 与历史标记已实现；V1、环境-only 连接、完整原生及桌面 UI 实机验收仍待完成，不能宣称 U3f 全部通过。
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

## U3-004 会话来源纠正与真实余额诊断

用户在浏览 Tokage 后选择了 DeepSeek 名称的模型。本轮只读检查发现：

- 从 Tokage 目录与 /tmp 分别执行 `api model.default`，两次返回的 location.directory 都是用户 HOME，模型为 `opencode/flint-40b-free`。因此之前把执行命令的目录当作 API 返回上下文、把 model.default 当作会话选择，判断依据不充分；不再要求用户通过反复切换会话模型来改变这个值。
- `api session.list` 读取到的现有会话模型是 `opencode/deepseek-v4.1-flash-free`，providerID 为 opencode，location 也是用户 HOME。模型名称含 DeepSeek 不代表官方 DeepSeek Key 后端；不能据此认定 Tokage 工作区有一个官方 DeepSeek 会话，也不能把此会话消费算到该官方钱包。
- 尝试用 `--param location=<JSON object string>` 显式传 Tokage 路径时，安装版 CLI 返回 InvalidRequestError（Expected object at location），未接受该对象参数。保留失败，不把当前 CLI 参数写法当成可用的项目绑定方法。

但用户已明确连接并授权测试官方 DeepSeek，验证该钱包的 API 连通性无需切换免费模型会话。本轮以显式 integration=deepseek 的只读诊断验证钱包，不借用服务默认模型作为选择依据：先核对内置 provider、官方 baseURL 和唯一活动 type=key 账号，再单次 GET 固定 `https://api.deepseek.com/user/balance`，前后比对活动凭据 / 账号绑定。Authorization 仅通过 curl stdin；Key 不进命令行、文件或输出。查询禁止重定向，限定超时；失败时不切换 Key、不回退或重试。正常已有网络环境沿用，未改代理、HOME、账号或用户配置。

结果：HTTP 200，is_available=true，返回 CNY，total / granted / topped-up 均为合法十进制字符串；查询前后活动来源一致。余额数值、Key、账号 ID 和原始响应均未保存到仓库或证据；仅归档脱敏结论。没有发送模型请求，未做独立账单零消耗审计。

这只是直接官方 API 诊断，不是 Tokage 的原生授权、代理、缓存或桌面 UI 验收。U3-003 适配仍保留其服务默认源的严格限制，没有改成任取最近会话、按模型名猜测后端，或无条件借用已连接 Key。后续需要从正常产品入口明确传入后端 / 模型 / 项目或会话来源，并贯穿身份复核；用户无需为此继续修改 Key 或反复切模型。U3f 汇总保持未完成。

脱敏证据见 `docs/reliability-evidence/cli-usage/u3-004.json`。本节点仅增加源码边界注释、更新诊断记录；没有改变运行时判断，未为低影响注释重跑模型或原生全量测试。

## U3-005 显式钱包入口与任务来源隔离

修正 U3-003 的默认模型依赖。本节点查询目标固定且公开为 OpenCode 服务中已连接的 DeepSeek 钱包；设置卡片按钮显示“查询 DeepSeek 钱包”，授权说明包含官方只读余额 API。它不是当前会话、Tokage 项目或执行任务的计费来源声明。因此无须用户通过 `/models` 改服务默认模型，也不通过最近会话、模型名、CLI CWD 或环境 Key 推断钱包归属。

- 原生 capability 新增可选 backend；OpenCode 为 deepseek，其余当前来源为空。该字段由原生决定，客户端不能传自定义 endpoint 或账号 ID 选择 Key。
- 来源改为 `provider.get(providerID=deepseek)` 和定向 `auth export deepseek`；仍要求内置 package、确切官方 HTTPS endpoint、无自定义 settings / headers / body 覆盖及唯一活动 type=key 账号。凭据只驻留原生内存，使用现有权限 / 撤销 / 代理 / 超时 / HTTP 状态处理与前后身份复核，不读取或改写 DB，不写 Key 到日志、前端、快照。
- 身份指纹换用 explicit-wallet 域，旧默认模型绑定不能默认为当前可信。查询仍只保存钱包 snapshot；余额没有分母，不进入账号百分比调度。
- 钱包身份不证明任意 CLI turn 的凭据来源。`binds_dispatch` 明确排除具名钱包：OpenCode 的总开关仍生效，但单独钱包换号不再阻断免费模型或其他后端任务，也不恢复整个 OpenCode provider 的 QUOTA_SOFT；认证、预算及任务自身门禁没有旁路。
- Provider store 不再将具名钱包的余额 / 失败 / 身份变化写进普通 account，避免清掉其他来源的会话用量或误报计费。独立 UI 从带 provider=OPENCODE、backend=deepseek 的 typed snapshot 渲染总 / 赠送 / 充值余额，保留原币种和十进制字符串。旧数据、失败和未知来源标为历史；不会画订阅百分比，其他 backend 的数据不会作为 DeepSeek 钱包展示。

新增回归验证固定后端 CLI 调用序列（禁止 model.default、session 或全量凭据导出）、拒绝网关后不访问凭据、活动 Key 轮换、任务绑定政策、store 账号隔离（含失败 / 身份变化）、金额精度、历史标记和跨后端隔离。用例中的 CLI / 余额为离线 fixture，不是服务商或桌面验收证据。

本次没有新增真实请求，沿用 U3-004 的独立 HTTP 连通性结论；完整产品 UI 的原生权限、代理、快照持久性和换号验收尚未执行。U3f 和 U3 汇总保持未完成，自动轮询未开启。

U3-005 验证结果：原生全量 370 通过 / 28 项真实调用或内部 helper 跳过，运行时集成 1 通过；桌面库 22 通过；前端全量 67 文件 / 457 项通过，最终钱包与 store 专项 15 通过（含随后增加的已验证缓存与历史数据区分，以及失败 / 换号隔离）。专项与全量重叠，不相加。类型检查、前端构建、1802 项国际化键检查、测试隔离和差异检查通过。证据见 `docs/reliability-evidence/cli-usage/u3-005.json`。
