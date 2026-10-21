# sh1：共享编排基础决策与映射

下文为 sh1 完成时记录；后续实现见 [运行服务说明](shared-orchestration-runtime.md)。

2026-10-21。服务固定为 `tokage-runtime`。sh1 不接管任何旧运行；sh2 实现业务处理器，sh3 依次接入 CLI、App。

## 调用点和回归清单

- 需求/规划：App `components/planning/usePlanSession.ts`、`PlanReview.tsx`、`lib/planApprove.ts`、`replanInvalidation.ts`；CLI `runs.rs::dispatch/set_plan`。复用 planning parser/validator；回归 planApprove、replanInvalidation、planning/plan、planning crate acceptance、CLI commands/workflow。
- 模型分配：App `TaskModelPicker.tsx`、`taskRunner::resolveTaskProviderAndModel/buildPickAssignments`，matching crate assignment；CLI Run.provider/model 固定。服务保留显式模型锁，App 自动分配不降为 CLI 单模型。
- DAG/并发：`PlanReview`、`PipelineView`、`taskRunner::dispatchReadyTasks/progressDownstreamTasks`，orchestration scheduler/lifecycle；CLI execute::pick 串行。服务统一调度，CLI 设槽数 1；App 冻结正常配置。
- 预算/熔断：taskRunner::maybeCheckPlanBudget/onProviderMeltedEvent/onProviderRecoveredEvent，stores/providers，storage budget/health；CLI 当前无对应能力。共享服务拥有预留、释放和未知用量阻断，CLI 不得静默禁用。
- 修复/补充/人工接手：taskRunner::applyRepairLadder/supplementTask/completeTakeoverAndRerunGates/confirmContractDeviation；orchestration repair；CLI 仅显式 retry。保留 App 同窗口修复→换模型→阻断的有界阶梯；CLI 显式配置 manual。
- 审批/turn/取消：taskRunner::resolvePipelineTaskApproval/resumePipelineTaskAfterApproval/handleSubAgentChatEvent/cancelTask/interruptExecution，lib/unifiedApprovals、native chat/turns；CLI turn::run_turn、runs dispatch。所有权、审批窗口和停止确认归服务；按钮/信号仅发送意图。
- 上下文：taskRunner::enforceWindowContinuity/buildSubAgentPrompt/continueTask，context crate；CLI turn 无 App 完整复用。默认保留 App context 策略；不支持时阻断并披露。
- 用量：taskRunner::recordAttemptStart/recordAttemptEnd/recordOutcomeForTask，storage usage_receipts、native usage；CLI turn events。服务按 receipt 去重，requested/observed 分开，未知模型/费用为 null。
- 恢复：execution.ts checkpoint/flush/restore，taskRunner::verifyRecoveredTask/resumeGitDelivery，native recovery/file_lease/git；CLI state Store 和 execute。服务恢复先对账；lease 释放只证明 owner 退出，不能证明原生子进程结束。
- 接受/集成：taskRunner::processSubAgentCompletion/finishWorktreeTask/runGatesForTask，native scope/Git journal、orchestration gate；CLI execute 和 delivery::accept。DONE 必须证据成立，不能信任旧 completed_tasks。
- 最终审查及三种交付：taskRunner::maybeStartPlanDelivery/deliverMergeToCheckout/deliverExportPatch/deliverKeepBranch，CLI delivery::accept/merge 和 runs export/keep。服务绑定接受快照和目标身份，入口只负责确认/路径选择。

现有重点回归：taskRunner.test.ts、execution.persistence.test.ts、unifiedApprovals.test.ts、nativeSessionResume.test.ts、contextPlanDefaults.test.ts、scopeGuard.test.ts、taskState.test.ts、gitDelivery.test.ts、artifactIdentity.test.ts、modelContract.test.ts；Rust orchestration acceptance/gate、native git m2c/m2d/m2e 与 runtime_defaults；CLI commands/workflow 13 项命令回归保留。sh1 仅增加基础契约/事务测试，不能据此声称这些业务已迁移。

## 固定的基础契约

runId 为不透明稳定标识，不复用整数 planId；来源映射保留 sourceKind/sourceId。App mainSessionId + planId 构成来源键，attemptId/turnId 原样保留于后续证据。CLI 旧 id 仅作为来源，不覆盖新 runId。一个 run 固定 engine/schema，不双写推进。

配置无隐式 Default：入口必须提交来源、策略版本、模型分配、权限/信任、范围、目标、并发、预算、超时、修复、上下文、命令策略、自定义包和协作者设置。规范 JSON 的 SHA256 冻结配置。未知预算默认阻断；FullAuto 不从旧快照推导。更改配置需新授权，sh1 不支持执行。

状态修改只接受有类型命令，无 SetState。schema v1；未知命令/字段反序列化拒绝，未知版本只报错不自动降级。相同 commandId + 完整请求返回原 durable receipt；同 ID 不同请求冲突；revision 先在事务中核验。sh1 支持 CreateRun、CancelRun 草稿取消；其他命令作为契约保留并明确 Unsupported，不产生副作用。运行中取消不能通过基础转换直接变成 stopped。

事件永久保留至显式归档整个运行；不对活跃运行截断。序号等于每运行 revision；游标为 exclusive afterSequence，分页重放。原始流式 transcript 后续另建表，不将秘密或任意原始 Provider 输出放基础事件。

## 仓储与迁移决策

runtime 定义 Repository trait，SQLite 适配直接复用 tokage-storage 的 writer 和 migration/备份机制（storage 不依赖 runtime，runtime 依赖 storage，无循环）。独立新表，不改旧权威表。owner 使用 native FileLease，路径取仓储规范绝对目录和 runId 哈希；持有句柄覆盖整个执行生命周期。接管后必须 Resume 对账，sh1 不开放执行。

旧记录只显式导入为只读历史，完整原文及 SHA256 存档；绝不补审批、用量或接受证据。CLI schemaVersion=1、App 导出有明确来源 envelope，未知格式拒绝。先备份原输入，再事务登记唯一来源/哈希；相同来源内容变化拒绝。fixture 验证失败重试不重复导入，原文件不修改。

备份恢复：关闭写入者；保存 DB/WAL 一致副本（SQLite backup 或 VACUUM INTO），输入原文先归档。Storage::open 在新 migration 前自动备份。迁移失败保留备份和错误，不接管运行；用兼容版本在新目录打开副本核对 quick_check、schema、事件及来源哈希。不得将旧二进制指向迁移后的 DB。回退只停止创建新运行，旧活动引擎继续收尾至完成或显式安全迁移，无按日期强行切换。新服务运行保留兼容读取，不能回退为旧写入。


## 实现边界与复核入口

`tokage-runtime/src/lib.rs` 是版本化 DTO、错误、配置哈希及纯转换；`repository.rs` 通过 `Storage::atomic` 复用现有 writer/savepoint/durable reply。V14 只新增 runtime_runs/events/commands/history；没有修改旧 App/CLI 状态。两个入口 Cargo manifest 声明相同 path dependency，目前不派发服务执行命令。

现有原生适配入口集中在 `src-tauri/src/lib.rs` 的 Tauri commands：规划/分配、任务状态、预算、Provider health、attempt/report/gate、chat turn、scope、Git journal 和最终交付。`src/lib/ipc.ts` 是传输封装；`health_sub.rs` 是健康订阅，`policy.rs` 是权限适配。sh3 逐一将这些高层写入口改为服务命令；原生 Git/Provider 工具保留。App `stores/execution.ts` 的快照、旧 storage orchestration 表和 native 文件 journal 当前仍各自负责原流程，不与 runtime 双写。

仓储每个实例绑定一个 run，持有 OS lease；只读历史以 `history` 返回原文并复核哈希。App 导入采用显式的 `app-pipeline-v1` envelope（mainSessionId/planId/pipeline），不是自动识别任意 DB；将旧 SQLite/PipelineState 导出成该 envelope 的入口工作属于 sh3。CLI v1 JSON 直接归档。fixture 为旧结构的离线样本，DELIVERED/DONE 声明故意保留原文，不授予执行或交付资格。

尚未实现的执行命令明确返回 Unsupported；不会启动模型、Git 或审批窗口。仅无任务草稿可取消至 Stopped；Executing 不能使用此转换。lease 不代替原生进程对账；sh2 接入异步副作用时仍须补充 launch receipt、停止及恢复处理。


## sh1 验证记录（2026-10-21）

- runtime：3 项契约 + 6 项仓储测试通过，覆盖版本/未知命令拒绝、revision、配置篡改、幂等、owner 排他、事件分页重放、事务故障回滚、只读历史导入、损坏状态、真实 V1–V13 schema 升级冲突及独立副本恢复。失败 fixture 不进入产品导入链。
- storage：现有 37 项测试通过。桌面 workspace `cargo check --locked --lib -j 2` 与 CLI workspace `cargo check --locked -j 2` 通过，两个 lockfile 更新。
- CLI `cargo test --locked -j 2 -- --test-threads=2`：2 项 commands + 11 项 workflow 通过；均为公开入口离线 fixture，不计真实模型验收。
- 扩展 native suite：首次 310 通过、1 失败、28 忽略；失败为 `m2d_git_hook_timeout_reaps_tree_and_preserves_staged_work` 超时后找不到 PID 文件（m2d_tests.rs:21）。同一源码单独复跑该项通过（2.55 秒），未改测试/超时或业务代码，不能将首次整套结果称为全绿。跨进程 owner 冲突、SIGKILL 释放和重复描述符释放回归均通过。
- `python3 scripts/check-test-isolation.py`、`git diff --check` 通过。未改 `.tokage/`、个人配置或原有活动运行；未运行模型、桌面 UI 或真实交付验收。10/20 保持暂停。

上述基础验证不证明 sh2 执行、审批、预算和交付处理器已实现；sh3 接入前仍须按本映射保留旧活动运行收尾。
