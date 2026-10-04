# 08 · 本地数据仓 (Storage)

返回 [README](../../README.md)

> 现状（2026-10-04）：尚未接入 SQLite。项目、会话、对话记录与设置由前端 Zustand `persist` 存于 WebView localStorage（`tokage.projects` / `tokage.chat` / `tokage.settings` / `tokage.providers` / `tokage.ui`）；仅「项目外文件副本」（§1 末条）已按本文实现。迁移计划见 [TODO](../TODO.md) P1.m3。

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
- 迁移：`refinery`，脚本位于 `crates/tokage-storage/migrations/V{n}__{name}.sql`，启动时自动执行，执行前备份数据库文件。
- 时间统一存 UTC ISO 8601 文本。
- 项目外文件副本：回合写到工作目录之外的文件（如 Claude Code 在 `~/.claude/plans/` 生成的计划）复制到 `app_data_dir()/files/<项目文件夹名>/<本地日期>/<文件名>`，预览面板读取副本；项目内的文件仍原地读取。

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
provider_profiles · model_knowledge_base · knowledge_blobs          定义包
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
| `provider_profiles` / `model_knowledge_base` / `knowledge_blobs` | 定义包 |
| `model_overrides` / `custom_packs` / `learned_stats` / `triage_feedback` / `settings` | 用户覆盖、自定义定义包、本地学习、设置 |
| `command_rules` / `approvals` | 指令名单覆盖、审批记录 |

完整 DDL 见 [schema.md](schema.md)。

## 4. 数据保留与体积

- 单条 `chat_logs.content` 上限 64 KB，超出保留头 16KB + 尾 16KB 并标记截断；完整内容在 CLI 自身会话文件中仍可查看。
- `chat_logs` 默认保留 90 天；`usage_events`、`savings_events`、`usage_snapshots`、`task_outcomes` 默认保留 365 天；`learned_stats` 永久保留。
- 清理任务在应用空闲时每天执行一次，分批删除（每批 5000 行）后 `PRAGMA incremental_vacuum`。
- 删除项目只删除 Tokage 自己的记录与项目内 `.tokage/` 目录（需用户确认），不触碰其他文件与 CLI 原生会话文件。

## 5. 隐私

- 数据库不存储 API Key 或 CLI 凭据；代理凭据存于系统钥匙串。日志与审批记录中的命令写入前脱敏（R17）。
- 日志可能包含代码与终端输出，提供「导出 / 清空某项目全部记录」。
