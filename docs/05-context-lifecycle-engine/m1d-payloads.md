# M1.d：当前会话 COMPACT 载荷

2026-10-26。完成 d1/d2 与 d3 的执行意图、草稿构造；d3 的实际下一轮生效仍需 M1.e 执行路径验收。继续保持只读预览，不自动采纳或发送，不记录已压缩或节省。

## 来源和身份

现有 `resolve_context_inputs_cmd` 返回可选 `compactIntent`，沿用真实输入解析，不新增执行 IPC。Scope 绑定当前 session、完成 turn、window、provider、model、role、task 和 revision。Provider 来自当前请求的已选窗口身份，补齐 CODEX_CLI、CURSOR_CLI、COPILOT_CLI 正式标识；不写死 CLAUDE_CODE。

Runtime 主会话使用当前计划目标，缺计划时使用原始需求；任务窗只使用对应 task.goal。契约保留当前计划内真实 ID、标题和正文；PlanContract 没有物理文件路径，contractPaths 保持空，不编造路径。仅摘录所选窗口当前失败、当前任务错误和失败 Gate，不混入其他任务或恢复前的历史错误。缺任务、缺契约、同任务有未结束 turn 时不构造载荷。

旧会话只在消息的原生窗口所属会话、provider/model、mode 与当前 thread 一致且没有在途消息时构造；使用实际首条用户需求作为目标。缺可证明归属时不猜测、不从标题生成目标。错误限于最近成功之后的当前窗口，摘录去重并限制长度。

保留信息缺失或过期时单独返回不可用原因，仍保留有效的 token、成本与冷却预览。Rust 校验身份一致性、字段长度、错误/契约数量及整体保留数据上限。保留正文作为 JSON 数据，并明确不构成额外命令或授权。

## 方法分流与下一轮草稿

- NATIVE_SLASH_COMMAND：记录 command，保留内容进行单行 JSON 转义；transportReady=false，草稿构造器拒绝执行。原生命令语法候选不是当前已启用或实测的传输能力。
- INSTRUCTION_PROMPT：生成同窗口前缀草稿；用户原文独立保留，不改写用户消息。
- SYNTHESIZED_FRESH_BRIEF：生成保留简报草稿，requireNewWindow=true；不能把 resume 解释为新窗。Aider 不再把丢弃内容的 /clear 当作保留式压缩。

`compose_compact_turn_draft` 同时检查完整 scope 和保留数据相等；目标、错误、契约改变即失效，即使旧协议没有 revision。草稿仍 readOnly；不写全局 pending、不写动作状态、不调用模型、不更改权限或模型选择。

M1.e 必须在采纳和发送前重新读取权威状态，沿用正常冻结的模型、参数和授权，通过正常流程排队/开窗，并区分已排队和已执行。不能把预览中的 observed model 用作静默切换模型的依据。当前没有下一轮实际应用证据，因此 d3 保持未勾选，M1 整体验收未完成。

## 验证

- 上下文库 39 项通过，含正式 provider 路由、当前状态保留、原生命令转义与禁用、同窗/新窗草稿、过期身份和保留内容拒绝、未知 provider 和超限数据。
- 前端 79 文件 / 512 项通过，含任务/主会话目标、嵌入契约、当前失败 Gate、历史错误排除和窗口归属。
- 生产构建（含 TypeScript）、桌面 crate check、i18n 通过。

测试隔离与 IPC 接线检查在提交前执行。没有真实模型、CLI 原生 compact 或桌面采纳验收；没有修改 .tokage、凭据或运行中的应用。实际执行和节省分别留给 M1.e、M1.f/g。
