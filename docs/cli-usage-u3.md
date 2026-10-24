# U3 逐 CLI 适配进展

2026-10-24。本文保留 U3-001 的冻结实现与验证，追加 U3-002 的重新检测及 Antigravity 分组适配。首轮节点 U3-001：本节点完成作用域 / 数量保真和目录信任保护，**U3a–U3h 的完整实现与真实验收仍未完成**。只运行本地版本、帮助、离线 schema 生成及离线测试；未查询真实账号额度、调用模型、切换账号或修改凭据。

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
- U3f：OpenCode 2.1.4 在正常登录 shell、临时目录下执行 auth list --format json 返回空数组，不能据此把所有工作区或环境认证都判为注销。debug config 报告的全局配置目录仅见 service.json，未见配置 model / provider；当前有效官方 DeepSeek 后端和凭据优先级未证明，HTTP 适配继续关闭。未执行会输出秘密的 auth export，也不读取或解密认证数据库。
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
