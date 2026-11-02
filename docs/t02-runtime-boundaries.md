# T02：执行入口、权威状态与旧引擎退役边界

核查日期：2026-10-31（Asia/Tokyo）。源码基线 `7d202df`。本节点完成代码入口与现有回归核查、只读清点方案及后续任务归属；未读取用户个人应用库、未执行旧运行恢复、未删除执行代码、未启动 App/模型。个人旧运行是否已清空仍为 B02，不能据本报告关闭 L04。

## 1. 结论与适用范围

新 App/CLI 托管运行的权威是 tokage-runtime；旧运行只在既有 legacy 身份下收尾；普通单轮聊天不是共享流水线；Mock/演示不是另一条真实验收路径。源码中没有发现本次所追踪的正常新建运行会在读取错误后自动回退为真实 legacy 执行。该结论限定于下列已核查入口和回归，不是对所有 IPC 的形式化安全证明。

退役条件目前不具备：没有用户所有应用/CLI 数据目录、旧活动运行、原生进程和 Git 副作用已完整对账的证据。本次不尝试打开 owner 或启动 App 来“顺便清理”。

两个后续重点：

- 共享任务调度已经有复用决策和部分真实参数，T09 不应重建已有功能；普通主会话的 COMPACT/FRESH 执行尚未贯通共享任务窗，T06 才负责该缺口。
- 共享运行内部 usage/接受证据与旧全局质量/费用看板不是同一条数据链。T03/T04 需先解决生产汇总和去重边界，不能只修改旧 frontend runner 的统计。

## 2. App 共享运行

入口：[`dispatchAsync`](../src/lib/dispatch.ts) 在 Tauri、非 demo、非 mock 且分流为 plan 时调用 [`createRuntimePlan`](../src/lib/runtimeDispatch.ts)。普通计划导入以及旧草稿批准的共享导入也走同一服务；[`PlanReview`](../src/components/planning/PlanReview.tsx) 保留已有旧 pipeline 的收尾分支，不在活动运行中换引擎。

- **业务 owner**：[`runtime_submit`](../src-tauri/src/runtime.rs) 管理的 NativeRuntime，生命周期不依赖 WebView；[`owned.rs`](../src-tauri/crates/tokage-runtime/src/owned.rs) 持有 jobs、仓储和取消/工作失败状态。
- **持久 owner**：[`SqliteRepository::open`](../src-tauri/crates/tokage-runtime/src/repository.rs) 使用规范化 `<data>/runtime-v1` 下按 run ID digest 命名的 OS FileLease。revision、幂等 command、快照/事件/turn transcript 在事务中推进；没有公开覆盖任意状态接口。
- **投影**：[`useRuntime`](../src/stores/runtime.ts) 只读取 snapshot/events/transcript，服务命令由 `runtimeSubmit` 提交。`main_sessions`/侧栏显示状态和运行 Store 是投影，不可写出服务的 DONE/Delivered。
- **审批/预算/恢复**：服务持有冻结 EffectiveRunConfig、approval 身份、预留、provider health、修复及 scope/gate/交付证据；ResumeRun 对账，不等于自动重跑成功。前端配置和 Full Auto 显示不能代替服务授权。
- **读取**：`runtime_snapshot/list/events/transcript` 用只读 Reader。关联、视图重新挂载和分页重放不获取执行 owner，不派发模型；读取失败展示错误，不降级为旧视图执行。

实际数据：`<data>/runtime-v1/tokage.db` 的 runtime_runs/runtime_commands/runtime_events/runtime_turn_events/runtime_history。原生 backend 使用相邻 `<data>/runtime-v1.native`，与数据库根是两个目录。

## 3. CLI 共享运行与历史 JSON

[`cli/src/runs.rs`](../cli/src/runs.rs) 的公开 create/plan/import-plan/approve/execute/retry/cancel/review/resume 和三种交付提交同一 runtime 契约。CLI 进程持有服务 owner；只读观察用 Reader，不能因输出 JSON 或有 `--confirm` 绕过服务规则。

- [`Store::new`](../cli/src/state.rs) 只决定 data root；默认与 App 的 data root 不同。只有显式选择同一目录才有共同运行/锁命名空间，不把两个独立目录下同名 ID 当成同一运行。
- `run list` 返回共享运行及 legacyDirectory 信息，**不自动枚举或接受旧 JSON**。`legacy-show` 只读取已知 schema-v1、ID 一致且非 symlink 的旧 `runs/<id>/run.json`。
- `import-history` 是需要备份且会写历史仓储的显式归档操作，不属于只读清点，不执行旧运行，也不补出接受证据。普通 Markdown 计划导入须新 ID 并重新批准。
- [`migration::admit_new`](../src-tauri/crates/tokage-runtime/src/migration.rs) 拒绝复用旧 CLI 历史、旧 App plans/pipeline 身份；损坏/未知 schema 不自动升级为可执行状态。

## 4. 普通主会话：不要与 legacy 流水线混淆

[`dispatch`](../src/lib/dispatch.ts)/普通 direct send → [`useChat`](../src/stores/chat.ts) → Tauri `run_turn` → native TurnRegistry。同步 dispatch 保留聊天用途，不能据共享规划路径的存在称所有聊天都由 runtime 接管。当前普通会话有自己的队列、native thread、审批、取消和上下文动作。

- 单轮 owner 为聊天 Store 的消息/队列加原生 TurnRegistry；后端 reserve turn ID、durable launch receipt、进程登记防止同 turn 重复启动。App 生命周期持有 `<data>/run/.owner.lock`，防止同数据目录第二个 App 启动时回收第一实例的活进程。
- `run_turn` 拒绝共享运行 ID/共享原生 cwd；pipeline 请求还检查既有 legacy 身份。普通聊天不能写共享运行接受/交付状态。
- [`CompactNextTurn`](../src/components/workspace/CompactNextTurn.tsx) 在 [`Workspace`](../src/components/Workspace.tsx) 中仅对非 runtime 普通会话开放；chat 的准备/发送链可消费 COMPACT/FRESH。共享运行展示的 context 输入目前是只读预览，不能据预览显示推断已执行压缩。
- 普通聊天用量由 App Bus 订阅写入 `<data>/tokage.db` 的 usage/receipt 路径。它没有共享任务 DAG 的全局预算预留与交付状态；不把一次普通回复视为任务已接受。

`legacyContextInputRequest` 的“legacy”在这里包含普通主会话适配，不等同“即将删除的 legacyTaskRunner”。**L04 不可按名字批量删除这些仍用于正常主会话的模块。**

## 5. 旧 App 流水线：只收尾、不创建新真实旧运行

[`taskRunner.ts`](../src/lib/taskRunner.ts) 是兼容路由；真实旧实现仍位于 [`legacyTaskRunner.ts`](../src/lib/legacyTaskRunner.ts)。已有旧 pipeline 的状态/调度由 `useExecution` 与旧 runner 管理，快照持久化在项目/会话目录 `.tokage/execution/<session>.json`，关联旧 DB plans/tasks/attempts、reports/gates 和用量。

- 正常真实托管路径先查询 runtime 身份；共享运行的旧 dispatch/integrate/accept/delivery 被拒绝，明确 cancel/retry/approval 意图改交服务。查询失败不会自动选旧执行。
- `legacyCancelKnown` 保留已知旧 Store 的同步取消入口；部分 listener/工具函数仍直接再导出 legacy。不能以 TS wrapper 为唯一安全边界。
- [`legacy_guard`](../src-tauri/src/legacy_guard.rs) 在后端检查共享 run ID、共享目录/保留分支、旧 pipeline schema/ID；metadata 写调用先使用进程内锁，再取与 runtime 相同的 run lease。`legacy_write` 只准既有 pipeline 或受管理的 Mock workspace。
- App-wide `.owner.lock` 是实例级生命周期锁；legacy metadata lease 是调用期锁；runtime lease 是 run owner 生命周期锁。三者职责不同。不同 data namespace 没有共同锁，不承诺跨不同配置实例指向同一旧仓库时也有全局唯一 owner；B02 清点必须列出所有已选 data root，冲突或来源不明即阻塞退役。
- `raw_pipeline` 是不恢复/不改写的读取；**`load_pipeline_state_cmd` 不是纯读**：它在确认原生空闲后调用会将旧 RUNNING 改写为 interrupted 的恢复器。只读清点不得使用这个命令，也不调用 `useExecution.loadPipelineFromDisk`。
- 旧报告、接受、提交集成和 delivery journal 仍是旧运行恢复的依据；不能因 Git clean、用户关闭窗口或历史显示 DONE 就抹去它们。

## 6. Mock、Demo 与付费替代来源

[`demoIdentity`](../src/lib/demoIdentity.ts)、debugProvider mock 和浏览器演示保留独立展示/模拟路径。native Mock 只在受管理工作区运行，使用自己的 mock turn 生命周期，不执行真实 CLI。模拟成功不写成真实模型质量/用量。

前端兼容路由允许这些明确模拟分支进入 legacy；后端的共享身份/cwd/保留引用保护仍需保留。不能用“当前选了 Mock”把既有共享真实运行变回旧 owner。

共享新自动执行在 `createRuntimePlan` 明确拒绝 inplace 及 luna/deepseek 付费 surrogate。普通主会话可按正常 debugRoute 记录实际来源，不能把其实际用量归给原选模型。L04 退役时保留普通聊天、显式演示和来源标识，不保留第二套新真实任务编排。

## 7. 能力差异与后续施工归属

### 7.1 上下文：T06–T12

[`execution::choose_context`](../src-tauri/crates/tokage-runtime/src/execution.rs) 已为共享任务调度选择 DONE peer，要求同 assignment、RESUME、已清理和未被占用等条件；构造知识包价格、窗口、TTL、缓存倍率和上轮 usage，保存 ContextDecision.inputs。auto 才消费可复用建议；off 新窗，suggest 不自动复用。保留既有修复/continuation resume 语义，不把 resume 都算成优化复用。

仍使用 `..Default::default()` 的 brief/baseline/预计轮次/增长/minSaving 等字段及 fallback usage 需 T09 核查；它不是完全没有真实输入。前端 [`findReusableWindow`](../src/lib/windowReuse.ts) 的固定参数用于兼容旧执行，修改它不会自动改善新 runtime。

共享普通预览的 plan_context 与实际任务发送尚无完整 COMPACT/FRESH auto/suggest 队列、撤销和报告承载链；T06/T07 应在 runtime command/owner 下接线。T08 的显式任务入口不能绕过共享服务。T10 核查实际 cwd/worktree/角色/授权与策略变更，不能只比较模型字符串；T11/T12 的审计与校准必须以服务接受证据为来源。

### 7.2 质量、用量与看板：T03/T04

共享 [`NativePlanner`](../src-tauri/crates/tokage-runtime/src/service.rs) 使用自己的 `Bus::new` 和 TurnRegistry，结构化 usage 经 [`TurnRecord`](../src-tauri/crates/tokage-runtime/src/turn.rs) 与 repository 的 runtime_turn_events/runtime_runs 保存；共享预算/未知费用门槛使用这些内部记录。

App `get_usage_dashboard` 读取旧 StorageState 的连接；[`dashboard.rs`](../src-tauri/src/dashboard.rs) 聚合 usage_events/task_outcomes/savings_events。当前检查 runtime/transport/Store 源码未发现把共享运行的 usage/outcome/savings 幂等发布到该全局统计路径的桥接；旧 `recordOutcomeForTask` 则在 legacyTaskRunner。

结论限定为**源码链路缺口**，没有通过读取用户账本测量实际漏计金额。T03/T04 应选择明确的一条方案：从服务证据只读聚合，或发布有版本/可重放去重的派生统计；派生表不能反向成为执行权威，不按两份来源重复计费。CLI 默认私有 data root 的数据是否进入 App 全局看板也要显式定义，不能隐式扫个人目录。

### 7.3 恢复与健康：T05/B02

共享 recovery 检查 turn launch receipt、process/gate registry、Git pending intent 与接受/交付 evidence；结果未知阻止 retry。cancel intent 由当前 owner 处理，不等于请求方接管执行。旧 App 则结合 pipeline、turn-receipts、app run registry 和 gitDeliveries 等 journal 收尾。服务内部 provider health 与 App 在线额度/旧 health 投影分别存在，不能因旧卡片显示 Ready 推断共享运行可继续。

## 8. B02 只读清点方案（本次未执行）

### 输入与范围

由用户明确选择实际使用的 App data root、CLI `--data-dir` 和项目/旧会话 worktree 根目录；记录规范路径、平台、构建提交及读取时间。不能只枚举当前仓库或本次测试数据就宣布全部旧运行已清空，不扫描认证目录、复制凭据或改 HOME。

只读访问源 SQLite（包含 WAL 的一致性读事务，必要时 backup API 写到外部归档），不用 `Storage::open` 触发迁移，不开 owner，不执行恢复/对账命令。未知 DB schema、symlink、损坏文件、路径不明均进入 unresolved。跨库/文件无法取得同一时刻快照时记录时间段，不将移动目标当一致证据。

### 需要记录的证据

- App 库：项目和会话路径、plans/attempts 关联、旧 task/approval/gate 状态。DB 记录与原生文件分别保留身份，不按 UI 标题合并。
- 共享库：Reader.list/snapshot/events/transcript（或对应纯读命令），记录 run ID、schema/revision、状态、turn cleanup、work/delivery intent。Reader 错误不得转旧引擎。
- 旧文件：直接解析 `.tokage/execution/*.json` 及已登记的 plan/工作树路径，校验 schema/session ID，记录状态、taskTurnIds、gitDeliveries、delivery、保留报告与源/目标 refs。未知/非终态/待确认副作用一律计未关闭。
- CLI 旧 JSON：只读 `legacy-show` 或原文解析，保留哈希；`list` 不替你统计旧目录。`import-history`、ResumeRun 和旧 pipeline load 都是另行收尾操作，不在清点阶段执行。
- 进程：App `<data>/run` 的 children/pending、turn-receipts；共享 `<data>/runtime-v1.native/<run digest>/<attempt>` 的 receipts/processes，以及 git-processes/gate-processes 下的登记。PID/PGID、原始路径、缺失/损坏/权限不足分别记录。只读核对 OS 进程身份；历史登记数量不是活进程数，PID 不在或 lease 文件存在都不足以单独证明清理。
- Git：只读 refs、HEAD/tree、worktree 列表、原始 target 身份、已接受源、未提交变化、进行中的 Git 操作和 journal 意图。禁止 merge、reset、worktree remove、删锁或清 registry 来“修出零”。

### 输出与停止规则

每个 data root 输出目录身份与证据哈希；每个 run 输出 engine、schema、owner 证据（known/unknown）、业务终态、进程清理、Git intent 状态和下一步。汇总至少区分：共享正常运行、旧活动运行、旧终态但未对账、可只读归档、损坏/未知。

任一未知来源/活进程/不完整接受或副作用未对账都阻塞退役。若需恢复或取消，停止只读阶段，保留原证据，再通过对应产品的明确命令收尾；修改前后另记结果，不用读操作偷改状态。

## 9. 删除候选及退出条件

候选不是当前获准删除的文件清单：

1. 真实旧 `legacyTaskRunner` 派发、调度、修复、接受、集成/交付和旧 `useExecution` 的活动执行写路径；其 Mock 展示与普通聊天共享 helper 先拆分，保留必要读兼容。
2. 只服务旧执行的 Tauri plans/pipeline/report/attempt/gate 写命令及包装；逐一核对普通聊天、项目管理、历史展示和 Mock 调用方。通用 native gate/Git、模型 Adapter/沙箱及 storage 库不是可连带删除对象。
3. 旧状态自动恢复与写入器：先归档已完成状态，保留历史读取/导出及迁移解释；未知格式不能迁移为成功，不能直接删除历史数据。
4. 最后收紧/删除相应 IPC 及 allowlist 条目，检查没有悬空调用方，更新架构/恢复说明。共享身份和路径冻结保护在旧写入口彻底移除前不得提前删除。

L04 的必需退出证据：范围完整的 B02 清点、旧活动数为零、所有进程/副作用已确认、历史不可变归档与可读性、Mock/普通聊天拆分、回退/升级方案，以及共享运行与原有回归通过。任何一项未知即不删除。不同数据根的旧用户升级路线必须说明，不能只验证开发机空目录。

## 10. 本次验证

在 `7d202df` 上运行既有回归，没有修改产品或测试代码、没有新增测试基础设施/账户访问：

- 前端边界/transport/config/失败/投影 5 文件、25 项通过（2 worker）：共享拒绝旧派发、cancel/retry 转发、读取失败不回退、投影和命令边界。
- runtime repository/transport/turn_owner 18 项通过（2 job/2 测试线程）：durable/revision/幂等/owner、只读不取 owner、不恢复/派发、detach、审批/取消与未知清理阻断。
- App legacy_guard 3 项通过（2 job/2 测试线程）：新旧互斥 owner、共享目录拒绝、旧原始读取不改写以及未知 schema 拒绝。

以上为离线测试和代码审计，不是新真实模型/正常桌面验收，不恢复 9/20。日志与核查范围见 [T02 证据](reliability-evidence/t02/audit.json)。T02 的盘点和方案完成，实际清点 B02、代码退役 L04、指标修复 T03/T04 与上下文实现 T06–T12 分别保持未完成。

## 2026-11-01 后续进展

[T03](t03-quality-acceptance.md) 已补齐只读质量/首次通过学习及看板来源接线；本文第 7.2 节保留 T02 基线时的缺口描述。实际 usage/费用/节省汇总仍属 T04，B02/L04 退役条件不变。
