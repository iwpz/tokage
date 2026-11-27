# CLI 合规声明

[English](cli-compliance.md)

核对日期 2026-11-27。本文覆盖 [当前原生执行注册表](../src-tauri/crates/tokage-native/src/chat/adapter.rs) 中的九个 CLI。Tokage 是独立工具。各服务商的许可、账号条款、地区限制、组织政策和模型授权仍适用于对应请求。技术可用性以本地 CLI 和认证状态为准，服务商文档中的接入条件仍需满足。

Tokage 通过用户安装或指定的 CLI 执行任务。项目和 CLI 需要获得工作区信任。模型请求可能将提示词、上下文和工具读取到的项目内容发送给配置的服务商。常规检测、显式身份验证、模型请求和单独授权的在线用量查询分别处理。在线查询的身份来源、必要凭据使用和适用账号见 [CLI 用量支持审计](cli-usage-support-audit.md)。

## Anthropic Claude Code

Tokage 调用用户安装的 `claude -p` 并读取其结构化输出。登录状态来自 `claude auth status`。订阅用量的独立查询经 CLI 自带的 `/usage`，由 Claude Code 处理认证。

使用 Claude Code binary 集成时，请遵守 Anthropic 的商业条款、官方 binary 及用户直接认证和计费要求。Anthropic 对第三方提供 Claude.ai 登录、代用户路由消费订阅请求和转售分别设有限制。第三方服务编排应使用获授权的 Anthropic API 或适用的企业推理凭据。Tokage 的本地 CLI 登录检测只说明 CLI 的状态。[Claude Code 无界面运行](https://code.claude.com/docs/en/headless) · [Anthropic 法律与认证要求](https://code.claude.com/docs/en/legal-and-compliance)

## OpenAI Codex CLI

Tokage 调用官方 `codex exec --json` 及其原生续接命令，规划流式输出使用官方 `codex app-server`。登录由 CLI 管理。专门额度查询使用限定的 app-server 账户管理方法。普通 Codex 直聊为只读，写入要求明确的 Full-auto 和隔离 worktree。账号权限、ChatGPT 或 API 计费及适用条款随用户选择的认证方式生效。[Codex 非交互模式](https://learn.chatgpt.com/docs/non-interactive-mode) · [Codex App Server](https://learn.chatgpt.com/docs/app-server) · [OpenAI 条款](https://openai.com/policies/terms-of-use/)

## Cursor CLI

Tokage 调用本地官方 Cursor Agent print mode 并解析流式输出。Cursor 管理登录和计费。Tokage 会检查可能导致命令未经批准执行的配置，并在直聊启动前复核。经独立授权的账号额度查询使用 CLI 自带 `/usage`，只对可明确绑定的来源显示结果。用户仍需遵守其 Cursor 账号、组织和套餐政策。[Cursor CLI 文档](https://cursor.com/docs/cli/overview) · [Cursor 服务条款](https://cursor.com/terms-of-service)

## GitHub Copilot CLI

Tokage 使用官方 `copilot -p` 的程序化入口。Copilot 自己管理登录、模型和可用权限，专门用量查询走授权的 Copilot SDK 管理接口。使用者应遵守所选 GitHub 账号、Copilot 套餐和组织策略涉及的模型、席位和计费条件。[Copilot CLI 程序化调用](https://docs.github.com/en/copilot/how-tos/copilot-cli/automate-copilot-cli/run-cli-programmatically) · [GitHub Copilot 适用条款](https://docs.github.com/en/site-policy/github-terms/github-terms-for-additional-products-and-features#github-copilot)

## Google Antigravity CLI

Tokage 用官方 `agy acp` 与 Antigravity 通过 stdio JSON-RPC 通信，握手使用 Tokage 自己的客户端标识。支持的版本范围为 1.5.1 至 2.0 之前。在支持的版本上，先使用 AGY 自己的登录流程，然后在「CLI 管理」显式验证。Google 登录验证只打开一个 ACP 会话读取模型，不发送推理请求。

个人 Google 订阅仅用于桌面「直接工作」，由用户直接发送消息，AGY 保管原生登录及历史。个人登录路径排除了计划、任务派发、后台上下文生成、Full-auto 和公开无界面入口。计划和自动化路径必须使用 Gemini API Key，ACP 明确请求该认证方法。API Key 验证会执行一次小型请求，配置的第三方端点仍适用其自身及上游条款。编辑和工具权限按当前沙箱及 AGY 模式执行。需要 ACP 客户端进一步确认的工具请求会被拒绝，可在 AGY 原生终端处理。Tokage 当前停用了 Antigravity 在线订阅额度查询。[Google ACP 文档](https://antigravity.google/docs/ide/extensions/acp/) · [Google FAQ](https://antigravity.google/docs/faq) · [Antigravity 条款](https://antigravity.google/terms) · [Gemini API 条款](https://ai.google.dev/gemini-api/terms)

## Aider

Tokage 启动用户安装的 `aider`，通过临时消息文件传输入并禁用 Aider 的自动提交。模型连接使用 Aider 的供应商配置和用户有权使用的 API 凭据，受实际服务商条款约束。经独立授权的 DeepSeek 钱包查询仅适用于可识别的官方全局 DeepSeek 连接，经 DeepSeek 的专用余额 API 查询。[Aider 脚本入口](https://aider.chat/docs/scripting.html) · [Aider API Key](https://aider.chat/docs/config/api-keys.html) · [DeepSeek 余额 API](https://api-docs.deepseek.com/api/get-user-balance)

## OpenCode

Tokage 启动 `opencode run --format json`，由 OpenCode 处理模型连接、会话和认证。实际模型服务商与网关的条款随 OpenCode 配置生效，其中的账户登录和订阅可用性由各服务商决定。在线钱包目前限定于独立授权且可明确识别的官方 DeepSeek 连接。[OpenCode CLI](https://opencode.ai/docs/cli/) · [OpenCode 供应商](https://opencode.ai/docs/cli/providers/) · [DeepSeek 余额 API](https://api-docs.deepseek.com/api/get-user-balance)

## Reasonix

Tokage 调用 `reasonix run --output-format stream-json`，使用 Reasonix 自己的模型配置及校验过的凭据。显式模型验证消耗少量用量。独立授权的余额查询限定于明确绑定的官方 DeepSeek 连接。其他端点的鉴权、模型使用权和计费遵循该供应商条款。[Reasonix 文档](https://reasonix.io/docs/) · [DeepSeek 的 Reasonix 集成说明](https://api-docs.deepseek.com/quick_start/agent_integrations/reasonix/) · [DeepSeek 余额 API](https://api-docs.deepseek.com/api/get-user-balance)

## AWS Kiro CLI

Tokage 调用本地官方 `kiro-cli chat --no-interactive`，由 Kiro 维护认证。独立授权的订阅额度查询调用 CLI 的 `/usage`，账户绑定使用本地身份元数据。用户应按所用 Kiro 账户、套餐和组织策略使用相应的登录与自动化方式。[Kiro 无界面文档](https://kiro.dev/docs/cli/headless/) · [AWS 服务条款](https://aws.amazon.com/service-terms/)
