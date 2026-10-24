# U3 逐 CLI 适配进展

2026-10-24，节点 U3-001。本节点完成作用域 / 数量保真和目录信任保护，**U3a–U3h 的完整实现与真实验收仍未完成**。只运行本地版本、帮助、离线 schema 生成及离线测试；未查询真实账号额度、调用模型、切换账号或修改凭据。

## 已实现

- Claude `/usage`：当前 session、全模型周额度、具名模型周额度分别保存；模型窗口不作为整个账号的额度。缺失 / 非法窗口不覆盖有效窗口；未知标题不被推断为全账号额度。重复作用域拒绝接受。兼容显示仅保留被接受的窗口。
- Cursor：共享快照保留 Included / Auto / API 独立分项，只有 Included 标为账号范围；不相加。On-Demand Disabled 及原始 reset 标签继续由 details 展示，不换算为 0% 或虚构日期。
- Copilot：保存 Plan 下的绝对 AIC / premium requests 数量，使用十进制字符串，不拿会话消耗作账号余额；不虚构五小时周期或 reset。
- Copilot 查询不再传 `--add-dir` 或自动接受 session-only 目录信任。PTY 遇到已知目录信任提示且调用方未授权接受时立即终止，防止延迟输入的回车意外确认。该路径可能因此在未信任目录失败，失败不能降级聊天或悄悄授权。
- Codex：纯结构化解码器覆盖主 / 次窗口、多桶、旧单桶兼容、可选 credits、缺失字段和未知扩展字段。多桶优先，不重复计数旧单桶；credits 不推算订阅百分比。未接入生产查询传输，能力状态仍为 IDENTITY_PENDING。

## 当前验证与阻塞

本机 PATH 及正常登录 shell：Claude 2.1.288、Codex 0.160.0、Cursor 2026.10.22-27dcf5f、Copilot 1.0.91、Kiro 2.34.0。未找到 agy / opencode / reasonix，不能据此断言未安装；自定义路径待补充。

- U3a：有效 OAuth scope / 系统安全存储 / HTTP 认证链未核实。继续使用既有 CLI 路径；不新增钥匙串读取。缺凭据、429、外部换号及完整 UI 验收待做。
- U3b：已用安装版 `codex app-server generate-json-schema` 离线核对公开协议。`account/read` 可指定 `refreshToken:false`，但不能据此证明整个 app-server 生命周期或 `account/rateLimits/read` 不会刷新共享认证。还需证明有效 backend / workspace 归属和无认证写回，才可启用传输；不复制 Token、不消费 reset credit、不启动 thread / turn。
- U3c：历史独立原生查询证据仍有效于其限定范围；本次仅补共享契约分项。不同套餐、身份变更、401/403、代理及完整桌面 UI 未验收。
- U3d：已删除查询路径隐含信任；现有 PTY 对当前 CLI 的就绪 / 内置命令识别和可信目录选择还需完整验证。实机 SDK 账号身份与额度关联、认证失败及新版兼容未验收。
- U3e：缺可运行 CLI 与可靠官方账号身份来源；保留阻断，不把 models 目录视为身份验证。具名 group / 未知周期保真仍待实现。
- U3f：缺可运行 CLI；V1/V2 有效官方 DeepSeek 后端、凭据优先级和只读来源未证明，HTTP 适配继续关闭。不得把模型名或本地自建后端 Key 发送至官方域名。
- U3g：缺可运行 CLI；还需把正常 `/status` 来源和配置指纹绑定到身份，不能以“本地文件存在”冒充账号证明。U2 已有十进制币种契约并不代表该项通过。
- U3h：当前 2.34.0 `chat --help` 区分 V2/V3，`--trust-workspace` 会允许加载项目 hooks / agents / MCP / workflows；本次未执行 `/usage`，也未设置此参数。历史 2.33.1 V2 查询不能代替当前引擎、扩展副作用和身份验证；能力保持 ADAPTER_PENDING。

## 协议资料

- [Codex 官方 app-server 文档](https://learn.chatgpt.com/docs/app-server)：账号管理、JSONL 协议和 rate limits 方法。公开文档不是安装二进制全部副作用的证明。
- 本机 0.160.0 离线生成 `GetAccountResponse`、`GetAccountRateLimitsResponse`、`InitializeParams` schema；哈希见本节点证据。

## 验证

原生单元测试 360 通过 / 28 项真实调用跳过，运行时集成测试 1 通过，桌面库测试 22 通过；TypeScript 类型检查、测试隔离检查与差异空白检查通过。结果归档在 `docs/reliability-evidence/cli-usage/u3-001.json`。合成样本是解析器 / 权限回归测试，不是真实服务商响应或账单零消耗证据。未完成项保留在主清单；没有恢复自动轮询。
