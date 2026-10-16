# 08 · 本地数据仓 (Storage)

[English](README.md) | **中文**

返回 [README](../../README.zh-CN.md)

> 可靠性更新（2026-10-16）：执行资料已迁移至 V12 身份约束；流水线 JSON 使用独立的串行、版本化原子保存协议，不使用下文通用 store 的回退机制。

## 1. 基本配置

- 位置：Tauri `app_data_dir()/tokage.db`。
- 每个连接执行：

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
```

- 并发：一个专用写线程接收 `mpsc` 写命令，每 100ms 或每 200 条合并为一个事务；读操作走 `r2d2` 连接池（4 个连接）。
- 迁移：`refinery`，脚本位于 `src-tauri/crates/tokage-storage/migrations/V{n}__{name}.sql`，启动时自动执行。已有数据库存在待执行迁移时，先以 `VACUUM INTO` 备份到 `app_data_dir()/backups/tokage-v{当前版本}-{时间}.db`，保留最近 5 份。
- 时间统一存 UTC ISO 8601 文本。
- 项目外文件副本：回合写到工作目录之外的文件复制到 `app_data_dir()/files/<项目文件夹名>/<本地日期>/<文件名>`，预览面板读取副本；项目内的文件仍原地读取。

### 1.1 项目上下文目录 `.tokage/`

Tokage 在项目根（会话工作树时为工作树根）维护 `.tokage/`，各 CLI 通过 `AGENTS.md` 读到同一份上下文（实现：`tokage-native/src/context.rs`）。

```text
.tokage/
  context.md                          约束与任务状态，所有会话共享
  plans/<session-id>.md               会话当前计划（每个 Plan 轮次结束后覆盖）
  plans/history/<session-id>/<n>.md   旧版本，n 从 1 递增
  execution/<session-id>.json         流水线执行状态（PipelineState v2，见下）
```

`execution/<session-id>.json`（F7）：`schemaVersion: 2`，含完整 `taskItems` / `contracts` / `upstreamArtifacts` / `assignments`，以及每任务的 `threadId`、`cwd`、`allowedCommands`、`approvals` 等。旧 v1 文件缺 `schemaVersion` 时视为 1，加载时用同会话计划回填任务定义；回填失败则前端标记不可恢复。冷启动时 `RUNNING` / `VERIFYING` / `REPAIRING` → `INTERRUPTED`（由 Rust 写入回盘）。

- 计划正文取自 Plan 轮次的回复文本，不使用 CLI 自带的计划模式落盘；Claude Code 的 Plan 轮次以 `default` 权限模式运行，只放行只读工具，禁用编辑工具与 `ExitPlanMode`。
- 文件名用会话 ID，标题写在文件首行；同一会话的后续轮次（任意 CLI）在提示词中给出计划路径。
- 写入边界：Tokage 写 `.tokage/` 全部内容；项目根下只追加 `AGENTS.md` 中 `<!-- tokage:begin -->` / `<!-- tokage:end -->` 包裹的段落（可重复更新）、`CLAUDE.md` 中一行 `@AGENTS.md`；`.gitignore` 仅在用户同意后追加一行 `.tokage/`。CLI 只读 `.tokage/`。
- 首次创建 `.tokage/` 时询问是否加入 `.gitignore`，选择按项目存入设置 `settings.tokageGitignore`。

## 2. 实体关系

```
projects
 └─ main_sessions
     ├─ plans (version)
     │    ├─ plan_contracts
     │    └─ tasks ── task_edges（依赖）
     │         ├─ task_assignments（分配历史）
     │         ├─ attempts（执行窗口）── chat_logs / usage_events / context_actions
     │         ├─ task_reports
     │         └─ task_outcomes（学习样本）
     └─ attempts（规划窗口、直接分发窗口，task_id 可为空）

handoff 类文本（简报、进展报告）存于 briefs，关联 attempt

savings_events · usage_snapshots · provider_health                 用量与熔断
provider_profiles · model_knowledge_base · model_offerings · knowledge_blobs   定义包
model_overrides · custom_packs · learned_stats · triage_feedback · settings   本地覆盖与学习
command_rules · approvals                                           指令授权
```

## 3. 表一览

| 表 | 用途 |
|----|------|
| `projects` | 项目路径与元数据 |
| `repo_digests` | 项目摘要缓存 |
| `main_sessions` | 主会话 |
| `plans` | 计划版本：设计稿、预估成本、状态 |
| `plan_contracts` | 契约 |
| `tasks` / `task_edges` | 任务与依赖 |
| `task_assignments` | 每次分配的模型、成功概率、预估成本、原因 |
| `attempts` | 执行窗口（对应一个 CLI 会话） |
| `briefs` | 任务简报与进展报告 |
| `chat_logs` + `chat_logs_fts` | 过滤后的日志与全文检索 |
| `task_reports` | 任务报告 |
| `gate_runs` | 验收命令执行记录 |
| `task_outcomes` | 首次通过 / 修复 / 改派 / 阻塞，学习样本 |
| `context_actions` | 上下文动作与各候选预估成本 |
| `usage_events` | 逐轮用量 |
| `usage_snapshots` / `provider_health` | 订阅额度快照 / Provider 当前状态 |
| `savings_events` | 节省账本 |
| `provider_profiles` / `model_knowledge_base` / `model_offerings` / `knowledge_blobs` | 定义包 |
| `model_overrides` / `custom_packs` / `learned_stats` / `triage_feedback` / `settings` | 用户覆盖、自定义定义包、本地学习、设置 |
| `command_rules` / `approvals` | 指令名单覆盖、审批记录 |

完整 DDL 见 [schema.md](schema.zh-CN.md)。

## 4. 数据保留与体积

- 单条 `chat_logs.content` 上限 64 KB，超出保留头 16KB + 尾 16KB 并标记截断；完整内容在 CLI 自身会话文件中仍可查看。
- `chat_logs` 默认保留 90 天；`usage_events`、`savings_events`、`usage_snapshots`、`task_outcomes` 默认保留 365 天；`learned_stats` 永久保留。
- 清理调度（F17.b）：`setup` 在数据库打开后启动低优先级任务——约 60 秒后首次执行 `cleanup_retention_records`，之后每 24 小时一次；分批删除（每批 5000 行）后 `PRAGMA incremental_vacuum`，结果写 `tracing::info!`。设置 → 关于 提供「立即清理」按钮调用同一命令。
- 删除项目只删除 Tokage 自己的记录与项目内 `.tokage/` 目录（需用户确认），不触碰其他文件与 CLI 原生会话文件。

## 5. 隐私

- 数据库不存储 API Key 或 CLI 凭据；代理凭据存于系统钥匙串。日志与审批记录中的命令写入前脱敏（R17）。
- 日志可能包含代码与终端输出，提供「导出 / 清空某项目全部记录」。

## 6. 前端同步与旧数据导入

- 启动时 `hydrate()` 在首次渲染前调用 `storage_load`，把快照写入各 store；之后订阅各 store，300ms 合并一次，按引用比较找出变化的行，经 `storage_write` 交给写线程。窗口关闭与页面隐藏时立即写出；应用退出前写线程提交全部排队写入。
- 每个写入项在写线程中各占一个 savepoint，单项失败只回滚该项。upsert 使用 `ON CONFLICT DO UPDATE`，不用 `REPLACE`（后者先删行，会级联删除子表）。
- 主键沿用前端 UUID（TEXT）。`attempts` 暂按「会话 × Provider × 模型」各一条，对应同一模型续用的 CLI 会话；`chat_logs` 每条消息一行，正文存 `content`，推理、工具步骤、文件变更、用量等结构化内容存 `meta_json`。
- 键值类状态存 `settings`，键带前缀：`settings.*`（设置页）、`ui.*`（侧栏宽度等）、`chat.*`（模型选择与选项）、`projects.active*`、`knowledge.url`、`providers.lastScan`；`meta.legacyImported` 标记旧数据已导入。CLI 探测结果缓存于 `provider_health.info_json`。
- 旧数据导入：首次启动时若数据库无 `meta.legacyImported` 且存在 localStorage 旧键（`tokage.projects` / `tokage.chat` / `tokage.settings` / `tokage.providers` / `tokage.ui` / `tokage.knowledge`），读取并执行原 store 的版本迁移后，经 `storage_import` 在一个事务内全部写入并置标记。导入只做 upsert，可重复执行。
- 失败处理：数据库打不开或导入失败时，Tokage 以旧数据运行并继续按旧格式写 localStorage，界面顶部提示并提供重试；旧键从不删除。回退期间写入会置 `tokage.legacyDirty`，之后成功启动时重新导入一次以带上这段时间的改动。

## 可靠性协议（m1）

- pipeline 按项目/会话排队，入队复制快照；递增 revision 阻止旧请求覆盖，原生锁内唯一临时文件、文件同步、原子替换及 Unix 目录同步。失败可发生在替换后，仍需确认重试，不推定未写入。
- 关键派发/审批/验收/Git 阶段先等待保存；失败暂停派发、界面重试，关闭等待所有写入，失败保持窗口。恢复保留 DONE，在途任务 INTERRUPTED，不自动重派发。
- 报告/brief/gate 必须 sessionId + plans.id + taskId；attempt 归属由 V12 约束验证。历史不能证明归属的记录保留 LEGACY_UNASSOCIATED，不按 T1 猜测关联。replan 清理受影响证据；保留 DONE 报告时用明确原计划指针。
- 持久化 scope 基线/复核、失败 turnId、gitDeliveries 阶段与全量验收内容证据。回滚字节在 checkout 的私有 Git 目录 tokage-rollback 中，不在 pipeline JSON。
- SQLite、JSON、Git、CLI 不是统一事务；单进程保存锁不覆盖其他应用进程，硬件断电未实测。完整恢复保证见 [m1a](../reliability-m1a.md)、[m1b](../reliability-m1b.md)、[m1d](../reliability-m1d.md)、[m1e](../reliability-m1e.md)，验收范围见 [m1g](../reliability-m1g.md)。
