# CLI Compliance Statement

[简体中文](cli-compliance.zh-CN.md)

Reviewed on November 27, 2026. This statement covers the nine CLI integrations in the [native adapter registry](../src-tauri/crates/tokage-native/src/chat/adapter.rs). Tokage is an independent tool. The provider's license, account terms, regional restrictions, organization policies, and model access requirements apply to each request. Availability depends on the installed CLI and its authentication state.

Tokage runs CLIs that users install or specify. Users authorize the workspace and the CLIs that can run in it. Model requests can send prompts, context, and project content read by tools to the configured provider. Tokage handles routine detection, explicit authentication checks, model requests, and opt-in online usage queries separately. The [CLI Usage Support Audit](cli-usage-support-audit.md) lists account requirements and the credentials used for online queries.

## Anthropic Claude Code

Tokage runs the installed `claude -p` and reads its structured output. It checks sign-in with `claude auth status`. The separate subscription usage query uses Claude Code's built-in `/usage` and leaves authentication to Claude Code.

Claude Code binary integrations are subject to Anthropic's commercial terms and its requirements for the official binary, direct user authentication, and billing. Anthropic restricts third-party Claude.ai sign-in, routing requests through consumer subscriptions on behalf of users, and resale. Third-party service orchestration should use authorized Anthropic API or enterprise inference credentials. Local sign-in detection reports the CLI's status. [Claude Code programmatic usage](https://code.claude.com/docs/en/headless) · [Anthropic legal and authentication requirements](https://code.claude.com/docs/en/legal-and-compliance)

## OpenAI Codex CLI

Tokage uses the official `codex exec --json` and native resume command. Streaming planning uses the official `codex app-server`. Codex manages sign-in, and separate quota checks use specific app-server account methods. Ordinary Direct work with Codex is read-only. Writes require explicit Full-auto authorization and an isolated worktree. Account permissions, billing, and applicable terms depend on the chosen authentication method. [Codex non-interactive mode](https://learn.chatgpt.com/docs/non-interactive-mode) · [Codex App Server](https://learn.chatgpt.com/docs/app-server) · [OpenAI Terms of Use](https://openai.com/policies/terms-of-use/)

## Cursor CLI

Tokage runs Cursor Agent's official print mode and parses its streaming output. Cursor manages sign-in and billing. Tokage checks for settings that allow commands to run without review and rechecks them before Direct work. The opt-in quota query uses Cursor CLI's built-in `/usage` and displays results for sources it can identify. Cursor account, organization, and plan policies apply. [Cursor CLI documentation](https://cursor.com/docs/cli/overview) · [Cursor Terms of Service](https://cursor.com/terms-of-service)

## GitHub Copilot CLI

Tokage runs the official `copilot -p` programmatic entry point. Copilot manages sign-in, models, and available permissions. Separate usage queries use authorized Copilot SDK management methods. Model access, seats, and billing are subject to the user's GitHub account, Copilot plan, and organization policies. [Copilot CLI programmatic usage](https://docs.github.com/en/copilot/how-tos/copilot-cli/automate-copilot-cli/run-cli-programmatically) · [GitHub Copilot terms](https://docs.github.com/en/site-policy/github-terms/github-terms-for-additional-products-and-features#github-copilot)

## Google Antigravity CLI

Tokage launches the official `agy acp` and communicates over stdio JSON-RPC using its own client identifier. Supported versions start at 1.5.1 and stop before 2.0. Sign in using AGY's own login flow, then verify sign-in in CLI management. Google sign-in verification opens an ACP session to list models without sending an inference request.

Personal Google subscriptions are available in desktop Direct work when the user sends the message directly. AGY manages native sign-in and conversation history. Personal sign-in is unavailable for planning, task dispatch, background context generation, Full-auto, and public headless entry points. Planning and automation require a Gemini API key, which Tokage explicitly selects as the ACP authentication method. Key verification sends one small model request. A configured third-party endpoint remains subject to its own terms and the upstream provider's terms. Edits and tool access follow the active sandbox and AGY mode. Tokage declines tool requests that need additional confirmation from the ACP client. Users can handle those requests in the native AGY terminal. Online Antigravity subscription quota queries are disabled. [Google ACP documentation](https://antigravity.google/docs/ide/extensions/acp/) · [Google FAQ](https://antigravity.google/docs/faq) · [Antigravity terms](https://antigravity.google/terms) · [Gemini API terms](https://ai.google.dev/gemini-api/terms)

## Aider

Tokage runs the installed `aider`, supplies the prompt through a temporary message file, and disables Aider's automatic commits. The model connection uses Aider's configured provider and API credentials the user is authorized to use. The opt-in DeepSeek wallet query uses the official balance API and requires an identifiable global connection to DeepSeek. [Aider scripting](https://aider.chat/docs/scripting.html) · [Aider API keys](https://aider.chat/docs/config/api-keys.html) · [DeepSeek balance API](https://api-docs.deepseek.com/api/get-user-balance)

## OpenCode

Tokage launches `opencode run --format json`. OpenCode manages the model connection, authentication, and sessions. The configured model provider and gateway terms apply, including each provider's rules for account sign-in and subscriptions. The opt-in online wallet query is limited to identifiable connections to the official DeepSeek endpoint. [OpenCode CLI](https://opencode.ai/docs/cli/) · [OpenCode CLI providers](https://opencode.ai/docs/cli/providers/) · [DeepSeek balance API](https://api-docs.deepseek.com/api/get-user-balance)

## Reasonix

Tokage invokes `reasonix run --output-format stream-json` with Reasonix's model configuration and verified credentials. Explicit model verification consumes a small amount of usage. The opt-in balance query is limited to identifiable connections to the official DeepSeek endpoint. Other endpoints' terms govern authentication, model access, and billing. [Reasonix documentation](https://reasonix.io/docs/) · [DeepSeek's Reasonix integration guide](https://api-docs.deepseek.com/quick_start/agent_integrations/reasonix/) · [DeepSeek balance API](https://api-docs.deepseek.com/api/get-user-balance)

## AWS Kiro CLI

Tokage runs the official local `kiro-cli chat --no-interactive`. Kiro manages authentication. The opt-in subscription quota query uses Kiro's built-in `/usage` and identifies the account from local identity metadata. Sign-in and automation are subject to the user's Kiro account, plan, and organization policies. [Kiro headless mode](https://kiro.dev/docs/cli/headless/) · [AWS Service Terms](https://aws.amazon.com/service-terms/)
