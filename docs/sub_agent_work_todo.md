# Sub-Agent 流水线真实执行任务清单 (sub_agent_work_todo.md)

返回 [TODO.md](./TODO.md) · 创建于 2026-10-06

本文档对照系统编排与执行引擎设计，定义将前端设计稿确认后的「子代理并行流水线」与底层各 CLI（Claude Code、Copilot、Aider、Codex 等）真正串联、并实现**实时持久化/断点续传**与**无尽超时支持**的完整开发计划。

---

## 使用约定

- 每个单项条目支持通过 `- [ ]` 与 `- [x]` 打勾追踪状态。
- 里程碑内全部条目完成且通过验证后，在里程碑标题末尾标注 ✓。
- 每完成一个子里程碑（如 m1a、m1b 等），及时勾选对应任务并保留可验证的测试或提交。

---

## 里程碑总览

- **m1: 超时守护改造（支持无尽/超长等待）✓**
  - m1a: Rust 后端 `TimeoutTier` 扩展与无尽/超长超时支持 ✓
  - m1b: 前端 IPC 类型与 ChatRequest 超时参数适配 ✓
  - m1c: 超时守护单元测试与验证 ✓
- **m2: 流水线状态实时持久化与断点续传底座**
  - m2a: Pipeline 本地持久化存储协议设计 (`.tokage/execution/<sessionId>.json`)
  - m2b: Rust/前端持久化读写与原子落盘
  - m2c: 异常中断与冷启动断点续传恢复逻辑
- **m3: 任务分发器与 CLI 真实调用串联 (Task Dispatcher & Runner)**
  - m3a: 任务模型与 Provider/CLI 反向解析映射
  - m3b: 任务简报 (Task Brief) 组装与 Prompt 注入
  - m3c: 子代理真实进程调起 (`startChatTurn` 并行驱动)
  - m3d: 独立 Worktree / 目录工作空间隔离与并发控制
- **m4: 实时日志捕获、验收关卡与 DAG 推进**
  - m4a: 多子代理事件流分流与任务状态/日志实时追加
  - m4b: 自动化验收关卡命令执行 (Gate Command Execution)
  - m4c: 关卡结果判定、报告解析与下游依赖任务拓扑触发
- **m5: 界面交互、看板实时联动与端到端联调**
  - m5a: 对话流卡片与执行看板实时状态同步
  - m5b: 暂停 / 继续 / 单任务重跑与手动接管交互
  - m5c: 全链路真实 CLI 端到端执行与断点续跑集成验证

---

## m1: 超时守护改造（支持无尽/超长等待）✓

子代理编写代码或深度排查时耗时较长，系统不得因为固定的 3~5 分钟硬超时将子代理强行杀死，必须支持无尽等待（或超长超时）。

- [x] **m1a: Rust 后端 `TimeoutTier` 扩展与无尽/超长超时支持**
  - [x] 在 `src-tauri/crates/tokage-native/src/chat/mod.rs` 中为 `TimeoutTier` 增加 `Infinite`（或 `Unlimited`）枚举项。
  - [x] 在 `TimeoutLimits` 中对 `Infinite` 档位配置超长硬超时（如 7 天 / 604800s，或直接在监督循环中禁用 hard/idle 超时计时器）。
  - [x] 确保进程存活期间仅依靠真正的进程退出信号或用户主动取消（Cancel）结束，不被无输出或长时间思考强杀。
- [x] **m1b: 前端 IPC 类型与 ChatRequest 超时参数适配**
  - [x] 在 `src/lib/ipc.ts` 中更新 `ChatRequest.timeoutTier` 类型定义，加入 `"INFINITE"`。
  - [x] 确保子代理执行请求默认注入 `timeoutTier: "INFINITE"`。
- [x] **m1c: 超时守护单元测试与验证**
  - [x] 编写/更新 `tokage-native` 的超时单元测试，验证 `Infinite` 档位下长时间执行不触发 `Command timed out` 错误。

---

## m2: 流水线状态实时持久化与断点续传底座

子代理流水线需要支持高可靠性。在执行过程中，各个子代理的状态变化、执行日志、产物信息必须实时持久化到磁盘，支持重启后继续执行未完成的任务。

- [ ] **m2a: Pipeline 本地持久化存储协议设计**
  - [ ] 定义 `.tokage/execution/<sessionId>.json` 数据结构，包含：`sessionId`、`status`、`tasks`（包含各自 `state`、`assigned_model`、`turn_id`、`started_at`、`ended_at`）、`taskLogs`、`gateResults`。
  - [ ] 兼容 `tokage-storage` SQLite 中的 `tasks` 表与 `task_reports` 表。
- [ ] **m2b: Rust / 前端持久化读写与原子落盘**
  - [ ] 封装流水线状态的持久化 API：`save_pipeline_state` 与 `load_pipeline_state`。
  - [ ] 每次任务状态跃迁（`PENDING` -> `RUNNING` -> `VERIFYING` -> `DONE`）或收到关键日志时，实时且安全地追加/刷盘。
- [ ] **m2c: 异常中断与冷启动断点续传恢复逻辑**
  - [ ] 客户端启动或切入已有会话时，检测是否存在正在运行或未完成的 Pipeline。
  - [ ] 若此前进程意外退出（应用关闭或崩溃），自动将处于 `RUNNING` 的悬挂任务标记为 `INTERRUPTED`，并提供「继续执行 / 从断点重试」操作。

---

## m3: 任务分发器与 CLI 真实调用串联 (Task Dispatcher & Runner)

将任务计划真正派发给底层各个 CLI 进程。

- [ ] **m3a: 任务模型与 Provider/CLI 反向解析映射**
  - [ ] 根据 `task.assigned_model`（如 `claude-3-7-sonnet`、`opus`、`deepseek-coder` 等）解析出底层具体的 `ProviderId`（`CLAUDE_CODE` / `COPILOT_CLI` / `AIDER` / `CODEX_CLI` 等）与模型规格。
  - [ ] 当指定 CLI 处于未登录或熔断状态时，实现优雅提示或回退备选 CLI。
- [ ] **m3b: 任务简报 (Task Brief) 组装与 Prompt 注入**
  - [ ] 调用 `assemble_task_brief`（或 Rust `assemble_task_brief_cmd`），将任务目标、约束契约（Contracts）、写入范围白名单（`write_scope`）、上游前置任务的交付报告结构化组装为标准 Sub-Agent Prompt。
  - [ ] 注入固定头部规则，要求子代理专注在指定的 Scope 内修改，并生成标准总结。
- [ ] **m3c: 子代理真实进程调起 (`startChatTurn` 并行驱动)**
  - [ ] 实现 `TaskRunner` 模块：为每个 Ready 任务生成唯一的 `turnId`。
  - [ ] 调起 `startChatTurn`，传入组装好的 Brief Prompt、对应 Provider 与 Model、以及 `timeoutTier: "INFINITE"`。
  - [ ] 正式建立子代理进程与流水线任务的绑定关系。
- [ ] **m3d: 独立 Worktree / 目录工作空间隔离与并发控制**
  - [ ] 支持为并行任务分配独立的 Worktree 或受控工作目录，避免文件修改冲突。
  - [ ] 遵守用户设置的最大并行子代理数（Concurrency Limit）。

---

## m4: 实时日志捕获、验收关卡与 DAG 推进

捕获执行过程，完成自动化验收，驱动后续任务。

- [ ] **m4a: 多子代理事件流分流与任务状态/日志实时追加**
  - [ ] 在全局 `chat-event` 监听器中，通过 `turnId` 将输出文本、工具步骤（`toolStart`/`toolEnd`）、文件变更（`fileChange`）准确分流至对应任务的日志流中。
  - [ ] 实时更新前端看板展示，并触发持久化落盘。
- [ ] **m4b: 自动化验收关卡命令执行 (Gate Command Execution)**
  - [ ] 当子代理任务的 CLI 轮次返回 `Done` 时，任务状态流转为 `VERIFYING`。
  - [ ] 若任务配置了 `acceptance.commands`，调用 `execute_gate_command_cmd` 在工作目录下执行测试/验证命令。
  - [ ] 收集关卡输出结果与退出码。
- [ ] **m4c: 关卡结果判定、报告解析与下游依赖任务拓扑触发**
  - [ ] 若验收命令通过（或无验收命令），标记任务为 `DONE`，生成结构化任务报告 `TaskReport`（包含修改文件统计与总结）。
  - [ ] 触发 DAG 调度器计算后续依赖已满足的下游任务，将下游任务由 `PENDING` 置为 `READY` 并自动启动调度执行。
  - [ ] 若验收未通过，标记任务为 `BLOCKED` 并保留失败证据，供用户查看或触发修复。

---

## m5: 界面交互、看板实时联动与端到端联调

完善用户交互体验，支持全自动执行过程中的监控与干预。

- [ ] **m5a: 对话流卡片与执行看板实时状态同步**
  - [ ] 对话流底部的执行卡片与 Pipeline 页面实时联动展示各个子代理的当前动作、当前工具调用与最新日志。
  - [ ] 增加每个任务的执行耗时与分配模型徽章展示。
- [ ] **m5b: 暂停 / 继续 / 单任务重跑与手动接管交互**
  - [ ] 支持用户在看板上一键「暂停流水线」，挂起尚未启动的任务。
  - [ ] 支持对已失败或阻塞的单项任务点击「重新执行」，支持修改分配模型后重试。
- [ ] **m5c: 全链路真实 CLI 端到端执行与断点续跑集成验证**
  - [ ] 测试由两个具备依赖关系的真实任务（如 T1 创建基础模块，T2 编写业务并测试）构成的技术计划。
  - [ ] 验证 T1 调起真实 CLI 完成代码写入，通过关卡后自动触发 T2。
  - [ ] 验证执行过程中关闭应用并重新打开，任务进度与日志完整保留，且支持无缝断点续传。
