# CLI 用量支持审计（U5-008）

日期：2026-10-25。历史桌面基线：`b5d60c5`；G1–G3新增实现以当前提交及下述原生证据为准。这是当前查询能力审计，不是全部对话能力、平台、认证方式或厂商账单精度的认证。

## 总体结论

九个现有对话适配器均已有至少一种受限生产在线查询路径：Claude、Codex、Cursor、Copilot、Antigravity、Aider、OpenCode、Reasonix、Kiro。这里的“有路径”仅覆盖下述身份与来源；本机已有真实管理/UI 证据仍逐提交保留。Codex及Aider新增原生实测，提交版桌面待G5。其他仅检测 CLI 不因注册或 Ready 获得查询支持。

所有专门在线查询默认关闭，须本机独立授权。启动先恢复权限/缓存、检测来源，再按每卡状态调度；普通扫描不清空全体历史，单卡刷新不取消其他卡片。查询权限与正常任务推理权限分开。历史账本、statusline/rollout 缓存、实时账号额度是不同数据，缺额度不等于没登录。

## 逐 CLI 范围与证据

- **Claude Code**：官方订阅，本机既有信任目录、固定安全参数下交互 `/usage`。当前本地 OAuth 来源绑定，不自行实现刷新；过期认证依赖官方 CLI。保留实际 session/weekly/具名模型窗口，无报告就不造数据。此前 24648e9 桌面首轮失败后自然重试取得 5h 窗口；不据此声明所有窗口/套餐通过。第三方 Key/Gateway 不冒充订阅额度。用户报告的“不到24小时又 expired”不应靠延长 Tokage 缓存掩盖；真实刷新原因、长期登录稳定性仍无充分证据。
- **Cursor CLI**：官方订阅；当前 status 正向身份与 PTY `/usage` 证明合并，必须识别内置命令补全，沿用已有信任。6587486 本机 2026.10.22-27dcf5f / legacy Pro+ 的自动查询、自然扫描、停启和重启通过。Included/Auto/API 分项、Disabled 和原始日期保留。没有官方网页账单独立对照；不承诺精确剩余请求/美元，新套餐未知布局会失败。详见 [Cursor 验证](cursor-usage-validation.md)。
- **Copilot CLI**：SDK 管理 RPC 的当前认证账号与 quotaSnapshots，保留 requests 或 copilot_billing_units，unlimited 不编造分母；不是会话花费。24648e9 桌面自动查询及到期续查已有证据；用户在 CLI 内可看 usage，与早期 SDK 未认证分别保留。自定义供应商端点/其他认证组合不视为已验收。
- **Antigravity CLI**：Google OAuth 的本地 CLI 凭据来源；固定管理命令仅接受 SUCCESS / usage / num_turns=0，保留模型/group/bucket 及未知周期。早期“身份未接入”已由后续凭据绑定实现替代；24648e9 桌面自动查询、两轮扫描、到期续查已有证据。API Key 模式、未知认证和跨平台未认证。
- **OpenCode**：V2 内置官方 DeepSeek integration、明确标准 baseURL、唯一活动 CLI 自管凭据，固定官方 balance GET。钱包不依赖当前项目默认模型，也不证明其他会话正在用 DeepSeek。CNY/USD 与 total/granted/topped-up 分开，不相加；钱包来源不阻断或恢复任意模型任务。24648e9 桌面初次网络失败后自动重试、跨卡刷新与扫描已有证据。G2b补充版本检测、V1全局配置/认证和V2无存储账号时的环境来源；本机V2真实查询584ms，V1仅契约测试。免费模型钱包、多活动账号、第三方网关仍拒绝猜测。见 [兼容边界](opencode-wallet-compatibility.md)。
- **Reasonix**：各自 Reasonix 全局配置与当前官方 DeepSeek 连接，独立读取凭据与绑定；不会读 OpenCode 配置或提示检查 OpenCode。固定 balance GET，钱包与会话/论坛登录分开；24648e9 桌面自动查询、刷新不干扰其他卡片及扫描已有证据。非官方 DeepSeek、其他模型供应商和未知配置布局未支持；Key 配置存在不等于实际账户余额可查询。
- **Kiro CLI**：2.34.0 / Google social / Pro+ 本机：固定官方账户 GET 约6.2秒成功，替代每次约93秒的 legacy 启动。保留 Estimated credits、套餐/试用/奖励/超额分池。仅单一 Google/GitHub namespace、商业 us-east-1/eu-central-1 profile；当前实测仅 Google/us-east-1/Pro+。API Key、IDC/企业、多 namespace、其他 region、未知 credit dimension 拒绝猜测。令牌拒绝可由官方固定 `/usage` 处理可能的刷新；G3b按稳定provider/profile识别账号，正常grant旋转不再误作换号；独立凭据版本允许同账号重新登录后解除旧认证阻塞，不清除429冷却。本轮真实过期刷新尚未验收。最终桌面结论见 [Kiro 验证](kiro-usage-validation.md)。
- **Codex CLI**：G1接入固定app-server账户额度管理协议，CLI自管ChatGPT file/default来源，回执accountId与当前CLI来源匹配，普通token旋转不当作换号；自定义profile/provider/keyring等拒绝借用旧文件。原生管理查询3464ms成功，2窗口/1数量，无thread/turn。修正共用UI忽略remaining credits和多桶标签的问题。提交版桌面验收待G5，见 [Codex验证](codex-usage-validation.md)。
- **Aider**：G2a新增明确命名的官方DeepSeek全局连接钱包，通过其自身shell/YAML/dotenv来源及固定官方余额GET，不读取OpenCode或任意项目模型。原生真实查询取得3个币种数量，无CLI聊天；钱包不熔断或恢复任意模型任务。非官方网关/未知插值/相对项目配置拒绝猜测，其他服务商不凭空提供通用余额；提交版桌面由G5验证，见 [Aider验证](aider-wallet-validation.md)。

上述旧组合版本详见 [U3 记录](cli-usage-u3.md)、[U5 桌面记录](reliability-evidence/cli-management-u5/validation.md)；不将旧提交单项通过写成 b5d60c5 对七家全部重新实测。

## 安全承诺同步

已同步中英文 README、00 架构、06 用量、09 R11、11 原生读取边界；界面 SourceNotice 链接指向实际 usage_query 模块，明确“本地读取 + 单独授权的服务商查询”。CLI 管理描述不再声称只有 CLI 调用，也不再声称原生永不读取必要凭据。

凭据只在原生内存和对应厂商的必要认证传输中使用，不进入模型、前端、普通存储或普通日志；不附带代码/对话，不经 Tokage 运营方。授权不导入/同步；关闭取消排队与在途请求、拒绝旧授权迟到结果。来源/路径/代次变更不复用旧账号数据。模型发起 shell 的凭据隔离规则继续生效。

## 保留的验收缺口

- 新 Kiro 提交 b5d60c5 的正常桌面自动查询、在途撤销、跨卡隔离、两轮扫描以及授权开/关各一次重启已于 U5-007 通过；U5-004 按已覆盖本机组合完成。其他 CLI 的旧证据仍归属旧提交，不泛化为本轮全矩阵重测。
- 外部真实换号、Kiro expired bearer 正常刷新成功、各厂商真实 401/429/故障注入仍没有全部组合实机证据；离线回归不代替。
- Codex新增原生实测，桌面尚待G5；Aider新增官方DeepSeek原生实测、桌面待G5；仅检测CLI、其他认证/套餐/平台无查询通过声明。额度与官方账单的延迟/精度仍受服务商限制。
- 临时 WebView 隔离不证明所有前端设置重启持久性。U6 全部发布门槛继续保留未完成；本次不自动发布或推送。
