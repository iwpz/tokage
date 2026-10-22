# sh3 入口与兼容边界审计

日期：2026-10-22。本记录覆盖 sh3 代码与离线回归，不替代 sh4 的跨入口一致性、故障矩阵和真实模型 / 正常桌面 UI 验收。

## 权威与依赖方向

App 的 Tauri transport 和 CLI 命令层调用 tokage-runtime；runtime 调用既有领域、storage 和 native crate，不依赖 Tauri、React 或终端 IO。服务持久化命令、授权、任务接受、交付意图和回执。只有该服务能为新运行写入任务完成及交付状态。

共享数据位于所选 data directory 的 `runtime-v1/tokage.db`；原生工作树与进程账本位于相邻 `runtime-v1.native`。两端必须选择相同 data directory 才能观察同一运行。CLI 默认目录和参数见 cli.md。只读 Reader 不迁移数据库、不获取执行 owner，也不恢复任务。App 的运行列表、快照、事件、逐 turn transcript 及侧栏状态都是投影；关联 main_sessions 只用于展示，不是另一份任务权威。

CLI 已删除独立 execute/delivery/turn/policy 编排。App 新运行经 runtimeDispatch、runtime Store 和 RuntimeView 提交命令；taskRunner 仅是兼容选路边界，真实旧实现位于 legacyTaskRunner。读取失败阻断选路，不回退旧执行。卸载视图只结束观察；App 服务 owner 和原生清理不依赖视图。

## 后端写入口冻结

legacy_guard 使用共享运行 ID、规范化原生目录和保留分支前缀 `tokage/rt-` 拒绝旧编排写入。受保护入口包括计划持久化、pipeline 保存与恢复、计划批准记录、执行任务 / 报告 / gate 记录、旧 pipeline turn，以及 gate 执行、任务提交、集成准备、合并、导出和工作树删除中的共享路径或引用。

旧元数据写入先取得进程内同 ID 异步串行锁，再取得与 runtime 完全相同的 OS owner lease。共享绑定持久化后，即使 owner 退出也不能通过旧入口写入。旧原始读取先验证已知 schema、session identity 和文件类型，不调用会将 RUNNING 改为 interrupted 的恢复器；只有确认为旧运行且原生空闲的显式旧恢复路径可调用原恢复逻辑。

现有已保存的旧 pipeline 可留在原引擎收尾。正常托管的显式 mock 工作区保留演示能力；不能以此创建新的真实旧编排。普通非 pipeline 聊天、用户编辑器和终端不参与运行接受，仍可供用户操作；这些人工文件修改不能制造服务的接受或交付回执。

## 引擎身份与普通计划导入

CreateRun 的准入在 runtime 仓储中执行，而非仅靠 UI。已有旧 CLI JSON、归档历史、App plans 记录或旧 pipeline 文件会保留原 ID；未知旧 schema 不会被解释为可执行共享状态。正在运行的旧任务不能原地切换引擎。新建共享运行必须使用新 ID，导入普通 Markdown 计划并重新批准，不继承旧 DONE、授权或交付证据。

App 可以直接导入普通计划，也可以从项目共享运行列表关联 CLI 创建的运行。列表、关联和重连不会执行、恢复或批准；导入仅 CreateRun / ImportPlan，不隐式发起模型调用。初始命令失败仍保留服务 Draft 投影。跨进程取消由 owner 在同一仓储锁下对当前版本持久化，再走正常幂等信号 / 清理，避免连续流事件推进版本时提前退出 owner。

## 兼容代码删除条件与回退

兼容期仅用于消化迁移前运行。sh4d 发布时须清点旧活跃运行；活跃数量为零、所有原生进程和副作用 journal 已对账、历史完成不可变归档后，删除真实 legacyTaskRunner 和旧编排 mutating IPC。浏览器 / mock 演示应独立为展示模拟，不能成为永久保留第二套真实引擎的理由。本次未检查用户个人数据，不能宣称旧活跃数量已经为零。

回退须先停止写 owner、确认原生清理并备份数据及版本；旧版本只能读取其支持的历史格式，不能盲写 runtime-v1 或将共享运行改绑旧引擎。保留失败工作树和 journal；不自动删除历史、不自动 push。更完整的运维步骤和迁移期退出验证仍由 sh4d 完成。

当前新自动执行 profile 为受支持平台 / adapter 的 worktree 加用户显式 Full Auto；inplace 和付费 Debug surrogate 在创建前明确拒绝。真实 Provider 可通过正常配置选择。正常授权、人工接手源确认和三种交付确认均保留；未运行真实模型或正常桌面 UI 验收，可靠性 10/20 仍暂停。
