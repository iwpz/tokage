# 调试 Provider

完整的双语演示项目与七任务剧本见[演示模式](../14-demo-mode/README.zh-CN.md)。

调试 Provider 接管执行来源，同时保留用户选择的 provider、模型、匹配结果与任务图。它替代旧的 `devShadowModel`，对主对话、规划、计划格式修复、执行任务、测试任务、审查与任务修复使用同一入口。

## 使用

重新构建并重启桌面开发实例后，在 **设置 → 规划与模型 → 调试 Provider** 选择输出来源。仅前端热更新时，真实替代输出会先检查后端是否支持调试路由，避免旧后端忽略新配置而调用原选模型。


- **关闭**：使用所选模型的真实 CLI。
- **Mock 输出**：不调用模型；使用预定义回复、计划与任务报告，实际模型用量为零。
- **Luna 输出**：全部模型请求交给设置中的执行 CLI 和模型。默认 `CODEX_CLI / gpt-6-luna`。
- **DeepSeek 输出**：全部模型请求交给设置中的执行 CLI 和模型。默认 `AIDER / deepseek/deepseek-v4-pro`；按本机 CLI 的实际目录修改模型 ID。

Luna 与 DeepSeek 真实消耗账号额度。需要在 CLI 管理中安装、登录并授权对应的执行 CLI。目标不可用时报告错误，不回退到 Luna、Haiku 或所选的昂贵模型。浏览器预览不执行真实模型请求。

开启调试后，模型选择器同时提供知识库中的模型目录与已经探测到的模型；无需安装每个被模拟的 CLI。真实 CLI 管理始终展示实际安装、登录和授权状态。模型禁用设置继续生效。

Mock 提供四个场景：完整流程成功、首次验收失败后修复成功、执行失败、等待 30 秒的慢速响应。流式输出间隔可调，直接对话回复可自定义；结构化计划与任务报告使用固定模板。先运行一次 Plan，再审阅并执行计划，可以验证完整的 T1 → T2 依赖流程。执行失败场景仍能产生计划，然后在执行阶段失败。

## 运行边界

桌面 Mock 为每个项目准备一个独立的 Git 调试仓库，位于应用数据目录的 `debug-workspaces/`。原工程不会接收模拟文件、计划或 Git 修改。设置中的“打开目录”可查看这个仓库。

规划解析、计划校验、模型匹配、调度、修复策略、任务报告解析、文件预览、worktree、提交和合并使用现有实现。Mock 只提供外部反馈：模型输出、文件内容和验收结果。验收命令不会在 Mock 仓库中真正执行，返回值明确带有模拟标记。模拟过程仍需要本机 Git。

调试来源通过 `debugRoute` 事件显式传递并写入消息。请求中的模型是所选模型；实际来源是 `DEBUG_PROVIDER/mock` 或配置的执行 CLI/模型。历史记录用独立的 `debug_output` 字段标记 `mock`、`luna`、`deepseek`，并保存原选 provider/模型，不根据模型名猜测。普通 Luna 会话不标为调试会话。

Mock 不写入真实用量账本，不改变真实 provider 的健康状态。调试任务结果不参与原模型的成功率学习。真实替代模型的用量按实际 CLI/模型归因。Mock 生成的会话、计划和报告保留用于调试，不代表原工程已经完成任务。

原生 session ID 带有调试来源、实际 CLI、实际模型的指纹。切换来源或模型时，不把旧 session ID 传给新的 CLI；主对话携带历史供新窗口继续。Mock 没有真实 CLI 的终端接管命令；真实替代模型的接管命令使用实际 CLI 和原生 session ID。

## 实现

- `src/lib/debugProviderTypes.ts`：持久化配置、来源与默认值。
- `src/lib/debugProvider.ts`、`src/lib/ipc.ts`：统一请求配置、虚拟目录覆盖、工作目录重定向与来源记录。
- `src-tauri/src/debug_provider.rs`：独立调试仓库、有效知识库目录与按任务保存的验收反馈。
- `tokage-native::chat::debug`：严格替代路由、Mock 流式执行、文件生成、取消与超时。
- `tokage-native::chat::TurnRegistry`：真实和 Mock 共用轮次所有权及唯一的结束通知。
- `src-tauri/resources/debug/plan.md`：桌面与浏览器共用的预定义计划文本。

数据库通过 V10 增加调试来源与原选模型字段，执行模式仍为 `HEADLESS / PTY`。设置版本升级为 6。旧 Luna、DeepSeek 选择迁移到对应输出来源；旧关闭状态仍为关闭。执行来源不再由主对话或格式修复入口分别改写。

## 验证

```sh
npm run typecheck
npm test
npm run build
npm run check:i18n
cd src-tauri
cargo test -p tokage-native --lib chat::debug
cargo test -p tokage -p tokage-storage -p tokage-core --lib
```

测试覆盖 Unicode 流式拼接、零用量、唯一结束通知、取消、超时、失败、旧设置迁移、配置目标与信任检查、原生协议解析、来源和 session ID 隔离、真实 worktree 与报告解析，以及任务验收失败后的反馈。替代模型的协议测试使用本地脚本，不调用真实模型。

Mock 用于验证 Tokage 的流程与状态，不验证真实模型的能力或厂商 CLI 的最新兼容性。
