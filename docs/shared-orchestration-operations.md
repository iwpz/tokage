# 共享 runtime 迁移与恢复操作

适用于 schema-v1，代码入口已迁移；真实验收结果见 shared-orchestration-validation.md。不要把离线矩阵、CLI 结果或演示画面当作正常桌面 UI 通过。

## 迁移前

先记录 App / CLI 代码提交、两个 lockfile、运行 ID、data directory 与平台。App 数据目录由普通 Tauri 应用配置决定；CLI 使用公开 `--data-dir`，不自动继承桌面数据目录或设置。同目录才共享运行；不同目录是不同状态空间。运行 ID、事件 sequence 和 turn ID 是证据关联键，不使用界面标题猜测运行。

新权威库为 `<data>/runtime-v1/tokage.db`，原生工作树、launch receipts、进程记录与副作用证据在 `<data>/runtime-v1.native/`。原 App `<data>/tokage.db` 仍存项目 / 展示及旧运行记录；原 CLI `<data>/runs/<id>/run.json` 和项目 `.tokage/execution` 属于历史，不将其中 DONE 当新服务接受。

停止新的派发，使用正常取消命令并观察进程清理；不要把 WebView 关闭、命令返回或 OS lease 释放当作已停止。稳定后备份：使用 SQLite backup API 为每个库生成一致备份，同时保留原生目录、相关仓库 refs / worktree 和旧数据。运行中不能只复制主 `.db` 而遗漏 WAL；跨数据库备份需在无写 owner 时进行。记录备份哈希、源版本与 Git HEAD，不修改 Git 身份。

CLI `run legacy-show` 只读查看旧 JSON；`run import-history` 的公开参数与显式备份步骤见 cli.md。历史导入不执行、不补授权、用量、门禁或回执。旧活跃运行在原引擎收尾；普通 Markdown 计划须用新 ID 导入共享运行、重新审批。同 ID 不可改绑引擎。

## 普通执行与观察

App 正常规划与 CLI create / plan / import-plan 均进入共享服务。CLI 默认显式串行、manual 修复，不同步 App 设置；完整 EffectiveRunConfig 是实际策略依据。App 可从项目共享运行列表关联 CLI 运行，关联与重连不执行。Store / main_sessions 是投影，禁止靠覆盖它们修状态。

当前自动执行支持 worktree + 正常显式 Full Auto；inplace、付费 Debug surrogate 和不能表达权限的 adapter 明确不支持。普通非 pipeline 聊天可继续使用；人工终端 / 编辑器操作不构成任务接受。人工接手须 inspect 当前真实工作树、提交人工报告、确认准确源摘要，并经共享 gates / 提交 / 集成 / 最终审查。

events 与 transcript 支持游标；断线从最后已确认 sequence 重放。DONE 只表示服务已接受任务证据，最终交付还需独立候选审查及用户确认。Merge / ExportPatch / KeepBranch 各使用精确接受摘要；没有自动 push。未知实际模型、费用、清理状态保持未知，不能填零或推断成功。

## 故障恢复

- 活跃 owner：使用 `run cancel --id ID`，等待 CANCELLED 与 cleanupConfirmed；收到 cancellationRequested 不等于完成。另一个 owner 的写入 / retry 应冲突，读取仍可用。
- owner 被强杀：先只读 show / events / transcript，保留 launch receipts 和 children / pending journal。`run resume --id ID` 只对账；活跃或无法核实的原生进程保持 Blocked，不自动重发。不要猜测 PID、删锁文件、删 receipt 或直接改数据库来放行。
- 原生进程已退出但结构化完成丢失：可确认清理并记录 Failed，仍不接受任务。CLI 返回非零且 show 为 Blocked 是正确结果。确认历史证据后通过正常 retry 重新批准；保留原尝试，不重复计量或改写历史。
- gate 被中断：必须确认进程 / 进程组清理；没有完整成功结果不能 PASS / DONE。未知清理阻断重试。最终门禁或审查失败也不能交付。
- 提交 / 集成 / 交付副作用成功而 DB 确认丢失：修复外部存储故障后用 resume 进行精确父提交、树、范围和目标核对。核对成功只补回执，不重发模型或重复 Git 副作用；目标 dirty、推进或证据陈旧则停下人工对账。
- 数据库损坏、未知 schema 或未知旧记录：停止写入，保留证据与备份，使用支持该版本的工具恢复。不能靠旧二进制或手工 SetState 完成任务。

## 回退与兼容期退出

回退只停止创建新共享运行；已创建的共享运行及版本化证据继续保留。恢复备份必须整体核对 DB、原生 journal 与真实 Git refs，不能只回滚数据库把已经发生的合并“忘掉”。旧二进制不获准写 runtime-v1；不可逆 migration 或已交付 Git 副作用须人工版本核对，禁止盲降级。

发布时清点旧活跃运行数量及未知进程 / journal。数量为零、所有副作用已对账且历史归档可复核后，删除真实 legacyTaskRunner 和旧编排 mutating IPC；mock 演示须独立为展示模拟。当前没有清点用户个人旧活跃运行，不能宣称已满足删除条件，也不提供永久真实旧引擎开关。

## 恢复可靠性 10/20 的闸门

sh4 的迁移样本不会增加可靠性计数，也不重写历史 1–9 或原第十项。恢复前另行冻结真实需求、指定模型、源码提交、范围、原测试、授权 / 环境、停止条件与证据目录。桌面采用 external-ui-v1，从提交版本外部克隆普通构建，在正常 UI 配置模型、信任、权限和交付确认；CLI 使用公开产品命令并单列协议。UI 证据不能用 CLI 代替。

Debug Provider 必须关闭；不改 HOME、复制认证、写 Store / DB、注入页面 JS、替换确认或补隐藏运行时测试开关。依赖 / 缓存预热在派发前登记；两次同因失败停止付费重复调用。实际模型、费用未知项、所有失败与人工介入连续归档并生成哈希清单。具体步骤遵循 testing-isolation.md。

## sh4c 实测发现的限制

Provider READY 是本地探测，不能保证实际 OAuth 仍有效；haiku 实际规划返回 token revoked。OpenCode 原生适配器有意禁用 bash / delegation，由 Tokage 独立执行 gate；模型报告“未运行 shell”为契约偏差时会阻断，不能替它改报告。Codex 可能不报告实际模型与费用，保持 null。最终审查要求严格 JSON，夹带说明前缀会失败；原始 findings 保留在 transcript，不能绕过解析器或手工发交付回执。上述历史失败均未交付；2026-10-23 用户正常重新登录后，已修复服务给审查模型缺少真实 gate 回执及范围规范化说明的问题，并从新提交的外部构建完成 haiku CLI 正常交付。历史失败、流程辅助、产品修复与最终通过分开记录，见验证文档。
