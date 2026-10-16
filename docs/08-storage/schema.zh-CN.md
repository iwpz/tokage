# SQLite Schema（DDL）

[English](schema.md) | **中文**

返回 [08 · 本地数据仓](README.zh-CN.md)

下文以 `NOW` 代表默认值 `(strftime('%Y-%m-%dT%H:%M:%fZ','now'))`。

> 执行资料身份更新（V12，2026-10-16）：下文 DDL 含历史结构片段；实际升级脚本为 `src-tauri/crates/tokage-storage/migrations/V12__artifact_identity.sql`，报告/brief/gate 新读写必须使用 session + plans.id + task 身份，历史未知归属保留未关联。完整约束与迁移验收见 [m1b](../reliability-m1b.md)。

## 1. 项目与会话

```sql
CREATE TABLE projects (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    name           TEXT NOT NULL,
    local_path     TEXT NOT NULL UNIQUE,
    created_at     TEXT NOT NULL DEFAULT NOW,
    last_opened_at TEXT
);

CREATE TABLE repo_digests (
    project_id   INTEGER PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    content      TEXT NOT NULL,
    token_count  INTEGER NOT NULL,
    git_head     TEXT,
    updated_at   TEXT NOT NULL DEFAULT NOW
);

CREATE TABLE main_sessions (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id  INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    title       TEXT NOT NULL,
    route       TEXT NOT NULL CHECK (route IN ('PLAN','DIRECT')),
    status      TEXT NOT NULL DEFAULT 'PLANNING'
                CHECK (status IN ('PLANNING','REVIEW','RUNNING','PAUSED','BLOCKED','IDLE','COMPLETED')),
    created_at  TEXT NOT NULL DEFAULT NOW,
    updated_at  TEXT NOT NULL DEFAULT NOW
);
CREATE INDEX idx_main_sessions_project ON main_sessions(project_id, updated_at DESC);
```

## 2. 计划与任务

```sql
CREATE TABLE plans (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    main_session_id  INTEGER NOT NULL REFERENCES main_sessions(id) ON DELETE CASCADE,
    version          INTEGER NOT NULL,
    parent_plan_id   INTEGER REFERENCES plans(id) ON DELETE SET NULL,   -- 增量重规划的上一版
    planner_attempt_id INTEGER,                                          -- 规划窗口
    spec_md          TEXT NOT NULL,
    notes_md         TEXT,
    status           TEXT NOT NULL CHECK (status IN ('DRAFT','APPROVED','RUNNING','PAUSED','DONE','DISCARDED','SUPERSEDED')),
    est_cost_usd     REAL,
    actual_cost_usd  REAL NOT NULL DEFAULT 0,
    disk_path        TEXT,                                               -- .tokage/plans/<id>/
    created_at       TEXT NOT NULL DEFAULT NOW,
    approved_at      TEXT,
    finished_at      TEXT,
    UNIQUE (main_session_id, version)
);

CREATE TABLE plan_contracts (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    plan_id     INTEGER NOT NULL REFERENCES plans(id) ON DELETE CASCADE,
    code        TEXT NOT NULL,                 -- C1, C2 ...
    title       TEXT NOT NULL,
    content     TEXT NOT NULL,
    token_count INTEGER NOT NULL,
    UNIQUE (plan_id, code)
);

CREATE TABLE tasks (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    plan_id       INTEGER NOT NULL REFERENCES plans(id) ON DELETE CASCADE,
    code          TEXT NOT NULL,               -- T1, T2 ...
    domain        TEXT NOT NULL,
    title         TEXT NOT NULL,
    goal          TEXT NOT NULL,
    contracts     TEXT NOT NULL DEFAULT '[]',  -- JSON 数组：契约编号
    write_scope   TEXT NOT NULL DEFAULT '[]',  -- JSON 数组：glob
    deliverables  TEXT NOT NULL DEFAULT '[]',
    acceptance    TEXT NOT NULL DEFAULT '{}',  -- JSON：commands / criteria
    size          TEXT NOT NULL CHECK (size IN ('S','M','L')),
    difficulty    INTEGER NOT NULL CHECK (difficulty BETWEEN 1 AND 3),
    role          TEXT NOT NULL DEFAULT 'EXECUTOR' CHECK (role IN ('EXECUTOR','TESTER','REVIEWER')),
    state         TEXT NOT NULL DEFAULT 'PENDING'
                  CHECK (state IN ('PENDING','READY','RUNNING','VERIFYING','REPAIRING','DONE','BLOCKED','CANCELLED','SKIPPED')),
    -- 不含 PAUSED_QUOTA / WAITING_UPSTREAM / INTERRUPTED：F12 只在插入与计划结束时写 state，不镜像运行态
    repair_step   INTEGER NOT NULL DEFAULT 0,
    est_cost_usd  REAL,
    actual_cost_usd REAL NOT NULL DEFAULT 0,
    started_at    TEXT,
    finished_at   TEXT,
    UNIQUE (plan_id, code)
);
CREATE INDEX idx_tasks_plan_state ON tasks(plan_id, state);

CREATE TABLE task_edges (
    plan_id     INTEGER NOT NULL REFERENCES plans(id) ON DELETE CASCADE,
    from_task   INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,   -- 上游
    to_task     INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,   -- 下游
    PRIMARY KEY (from_task, to_task)
);

CREATE TABLE task_assignments (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id       INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    model_id      TEXT NOT NULL,
    provider_name TEXT NOT NULL,
    source        TEXT NOT NULL CHECK (source IN ('AUTO','USER','MELT_REASSIGN','REPAIR_REASSIGN','PACK_UPDATE')),
    p_success     REAL NOT NULL,
    est_cost_usd  REAL NOT NULL,
    reuse_attempt_id INTEGER,                  -- 计划续用的窗口
    detail_json   TEXT NOT NULL,               -- 候选列表、各自 p 与成本、排除原因
    explanation   TEXT NOT NULL,
    is_current    INTEGER NOT NULL DEFAULT 1,
    created_at    TEXT NOT NULL DEFAULT NOW
);
CREATE INDEX idx_assignments_task ON task_assignments(task_id, is_current);
```

## 3. 执行窗口与日志

```sql
CREATE TABLE attempts (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    main_session_id     INTEGER NOT NULL REFERENCES main_sessions(id) ON DELETE CASCADE,
    task_id             INTEGER REFERENCES tasks(id) ON DELETE CASCADE,   -- 规划窗口 / 直接分发为空
    role                TEXT NOT NULL CHECK (role IN ('PLANNER','EXECUTOR','TESTER','REVIEWER')),
    provider_name       TEXT NOT NULL,
    model_id            TEXT NOT NULL,
    cli_window_identity TEXT,                   -- CLI 原生会话 ID
    reused_from_id      INTEGER REFERENCES attempts(id) ON DELETE SET NULL,  -- 续用的窗口
    predecessor_id      INTEGER REFERENCES attempts(id) ON DELETE SET NULL,  -- 修复改派 / FRESH 的上一窗口
    exec_mode           TEXT NOT NULL DEFAULT 'HEADLESS' CHECK (exec_mode IN ('HEADLESS','PTY')),
    workdir             TEXT NOT NULL,          -- 项目根或 worktree
    status              TEXT NOT NULL DEFAULT 'STARTING',
    baseline_tokens     INTEGER NOT NULL DEFAULT 0,
    context_tokens      INTEGER NOT NULL DEFAULT 0,
    token_source        TEXT NOT NULL DEFAULT 'ESTIMATED' CHECK (token_source IN ('EXACT','ESTIMATED')),
    turn_count          INTEGER NOT NULL DEFAULT 0,
    last_turn_ended_at  TEXT,
    touched_files_json  TEXT NOT NULL DEFAULT '[]',
    created_at          TEXT NOT NULL DEFAULT NOW,
    ended_at            TEXT
);
CREATE INDEX idx_attempts_task ON attempts(task_id);
CREATE INDEX idx_attempts_main ON attempts(main_session_id);

CREATE TABLE briefs (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    attempt_id  INTEGER NOT NULL REFERENCES attempts(id) ON DELETE CASCADE,
    kind        TEXT NOT NULL CHECK (kind IN ('TASK_BRIEF','PROGRESS_REPORT','DIRECT')),
    content     TEXT NOT NULL,
    token_count INTEGER NOT NULL,
    edited_by_user INTEGER NOT NULL DEFAULT 0,
    created_at  TEXT NOT NULL DEFAULT NOW
);

CREATE TABLE chat_logs (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    attempt_id        INTEGER NOT NULL REFERENCES attempts(id) ON DELETE CASCADE,
    turn_index        INTEGER NOT NULL,
    role              TEXT NOT NULL CHECK (role IN ('USER','ASSISTANT','SYSTEM','TOOL','SHELL_STDOUT','BRIEF')),
    content           TEXT NOT NULL,
    content_truncated INTEGER NOT NULL DEFAULT 0,
    created_at        TEXT NOT NULL DEFAULT NOW
);
CREATE INDEX idx_chat_logs_attempt ON chat_logs(attempt_id, id);

CREATE VIRTUAL TABLE chat_logs_fts USING fts5(
    content, content='chat_logs', content_rowid='id', tokenize='trigram'
);
-- 配套 AFTER INSERT / AFTER DELETE 触发器同步 FTS
```

## 4. 结果、关卡与学习样本

```sql
CREATE TABLE task_reports (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id         INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    attempt_id      INTEGER NOT NULL REFERENCES attempts(id) ON DELETE CASCADE,
    summary_md      TEXT NOT NULL,           -- 传给下游的部分
    full_md         TEXT NOT NULL,
    contract_deviation TEXT,                 -- 偏离契约之处，空为无
    changed_files   TEXT NOT NULL DEFAULT '[]',
    created_at      TEXT NOT NULL DEFAULT NOW
);

CREATE TABLE gate_runs (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id     INTEGER REFERENCES tasks(id) ON DELETE CASCADE,  -- 计划级全量关卡为空
    plan_id     INTEGER NOT NULL REFERENCES plans(id) ON DELETE CASCADE,
    attempt_id  INTEGER REFERENCES attempts(id) ON DELETE SET NULL,
    kind        TEXT NOT NULL CHECK (kind IN ('COMMAND','SCOPE')),
    command     TEXT,
    exit_code   INTEGER,
    passed      INTEGER NOT NULL,
    output_tail TEXT,
    duration_ms INTEGER,
    created_at  TEXT NOT NULL DEFAULT NOW
);

CREATE TABLE task_outcomes (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id       INTEGER REFERENCES tasks(id) ON DELETE SET NULL,
    model_id      TEXT NOT NULL,
    provider_name TEXT NOT NULL,
    domain        TEXT NOT NULL,
    difficulty    INTEGER NOT NULL,
    size          TEXT NOT NULL,
    result        TEXT NOT NULL CHECK (result IN ('FIRST_PASS','FIXED_IN_WINDOW','REASSIGNED','BLOCKED','USER_TAKEOVER')),
    p_predicted   REAL NOT NULL,
    est_cost_usd  REAL,
    actual_cost_usd REAL,
    turns         INTEGER,
    created_at    TEXT NOT NULL DEFAULT NOW
);
CREATE INDEX idx_outcomes_model_domain ON task_outcomes(model_id, domain, difficulty);
CREATE INDEX idx_outcomes_offering ON task_outcomes(provider_name, model_id, domain, difficulty);

CREATE TABLE context_actions (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    attempt_id      INTEGER NOT NULL REFERENCES attempts(id) ON DELETE CASCADE,
    kind            TEXT NOT NULL CHECK (kind IN ('COMPACT','FRESH','REUSE')),
    trigger         TEXT NOT NULL CHECK (trigger IN ('PLANNER','HARD_LIMIT','USER')),
    status          TEXT NOT NULL CHECK (status IN ('SUGGESTED','APPLIED','DISMISSED','UNDONE','FAILED')),
    est_costs_json  TEXT NOT NULL,
    est_saving_pct  REAL NOT NULL,
    new_attempt_id  INTEGER REFERENCES attempts(id) ON DELETE SET NULL,
    created_at      TEXT NOT NULL DEFAULT NOW,
    applied_at      TEXT
);
```

## 5. 用量、节省与熔断

```sql
CREATE TABLE usage_events (
    id                 INTEGER PRIMARY KEY AUTOINCREMENT,
    attempt_id         INTEGER REFERENCES attempts(id) ON DELETE SET NULL,   -- 删会话不丢账
    task_id            INTEGER,
    plan_id            INTEGER,
    provider_name      TEXT NOT NULL,
    model_id           TEXT,
    role               TEXT,
    domain             TEXT,
    turn_index         INTEGER,
    input_tokens       INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens  INTEGER NOT NULL DEFAULT 0,
    cache_write_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens      INTEGER NOT NULL DEFAULT 0,
    api_calls          INTEGER,
    context_tokens     INTEGER,
    gap_seconds        INTEGER,
    cost_usd           REAL,
    equiv_cost_usd     REAL,
    phase              TEXT NOT NULL CHECK (phase IN ('PLANNING','EXECUTION','REPAIR','CONTEXT','DIRECT')),
    -- phase ≠ role：role 为 PLANNER/EXECUTOR/TESTER/REVIEWER；phase 描述成本阶段（F12）
    source             TEXT NOT NULL CHECK (source IN ('STRUCTURED','STDOUT_REGEX','ESTIMATED')),
    created_at         TEXT NOT NULL DEFAULT NOW
);
-- cost_usd / equiv_cost_usd：订阅制写 equiv；按量制缺 CLI 成本时按定义包价表推算（06 §3）
CREATE INDEX idx_usage_events_time ON usage_events(provider_name, created_at);
CREATE INDEX idx_usage_events_plan ON usage_events(plan_id);

CREATE TABLE savings_events (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    main_session_id  INTEGER REFERENCES main_sessions(id) ON DELETE SET NULL,
    plan_id          INTEGER,
    kind             TEXT NOT NULL CHECK (kind IN ('ASSIGNMENT','CONTEXT','DIRECT')),
    ref_id           INTEGER,
    baseline_usd     REAL NOT NULL,
    actual_usd       REAL,
    settled          INTEGER NOT NULL DEFAULT 0,
    detail_json      TEXT NOT NULL,
    created_at       TEXT NOT NULL DEFAULT NOW
);
CREATE INDEX idx_savings_time ON savings_events(created_at);

CREATE TABLE usage_snapshots (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    provider_name       TEXT NOT NULL,
    window_kind         TEXT NOT NULL,          -- SESSION_5H / WEEKLY / WEEKLY_MODEL / DAILY_BUDGET
    usage_percentage    REAL,
    quota_remaining_usd REAL,
    next_reset_time     TEXT,
    raw_text            TEXT,
    data_source         TEXT NOT NULL CHECK (data_source IN ('PASSIVE','STRUCTURED','POLL','MANUAL')),
    captured_at         TEXT NOT NULL DEFAULT NOW
);
CREATE INDEX idx_usage_snapshots ON usage_snapshots(provider_name, window_kind, captured_at DESC);

CREATE TABLE provider_health (
    provider_name     TEXT PRIMARY KEY,
    status            TEXT NOT NULL,
    cli_version       TEXT,
    melt_reason       TEXT,
    melted_until      TEXT,
    consecutive_fails INTEGER NOT NULL DEFAULT 0,
    updated_at        TEXT NOT NULL DEFAULT NOW
);
```

## 6. 定义包、覆盖与学习

```sql
CREATE TABLE provider_profiles (
    provider_name TEXT PRIMARY KEY,
    profile_json  TEXT NOT NULL,
    pack_version  TEXT NOT NULL
);

CREATE TABLE model_knowledge_base (            -- 模型固有数据，与渠道无关
    model_id          TEXT PRIMARY KEY,
    display_name      TEXT NOT NULL,
    vendor            TEXT,
    roles             TEXT NOT NULL,            -- JSON 数组
    skills_json       TEXT NOT NULL,
    price_json        TEXT NOT NULL,            -- 含 long_context_pricing / price_schedule
    cache_json        TEXT,
    context_window    INTEGER NOT NULL,
    effective_context INTEGER NOT NULL,
    speed             INTEGER,
    tags              TEXT NOT NULL,            -- JSON 数组
    description       TEXT NOT NULL,
    meta_json         TEXT,                     -- max_output / released / benchmarks / *_estimated
    deprecated        INTEGER NOT NULL DEFAULT 0,
    pack_version      TEXT NOT NULL,
    updated_at        TEXT NOT NULL DEFAULT NOW
);

CREATE TABLE model_offerings (                  -- 某 CLI 提供某模型：只存随渠道变化的字段
    provider_name     TEXT NOT NULL,
    model_id          TEXT NOT NULL REFERENCES model_knowledge_base(model_id) ON DELETE CASCADE,
    cli_model_arg     TEXT NOT NULL,
    context_window    INTEGER,                  -- NULL = 沿用模型自身的值
    effective_context INTEGER,
    credit_multiplier REAL,
    skills_patch_json TEXT,                     -- 可选的渠道级领域分数修正
    verified          INTEGER NOT NULL DEFAULT 1,
    pack_version      TEXT NOT NULL,
    PRIMARY KEY (provider_name, model_id)
);

CREATE TABLE knowledge_blobs (
    key          TEXT PRIMARY KEY,              -- domains / success_model / usage_profiles / triage_rules
                                                -- usage_patterns / noise_rules / gate_allowlist
    value_json   TEXT NOT NULL,
    pack_version TEXT NOT NULL
);

CREATE TABLE model_overrides (
    provider_name TEXT NOT NULL DEFAULT '*',    -- '*' = 覆盖模型条目，所有渠道生效
    model_id      TEXT NOT NULL,
    patch_json    TEXT NOT NULL,
    updated_at    TEXT NOT NULL DEFAULT NOW,
    PRIMARY KEY (provider_name, model_id)
);

CREATE TABLE custom_packs (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT NOT NULL,
    position    INTEGER NOT NULL,               -- 合并顺序
    pack_json   TEXT NOT NULL,
    enabled     INTEGER NOT NULL DEFAULT 1,
    imported_at TEXT NOT NULL DEFAULT NOW
);

CREATE TABLE learned_stats (
    provider_name TEXT NOT NULL DEFAULT '*',     -- '*' = 模型级，汇总所有渠道（04 §4）
    model_id      TEXT NOT NULL DEFAULT '*',
    domain        TEXT NOT NULL DEFAULT '*',
    bucket        TEXT NOT NULL DEFAULT '*',     -- 难度或规模，如 'd2' / 'M'
    metric        TEXT NOT NULL,                 -- success_alpha / success_beta / fix_rate
                                                 -- turns / calls_per_turn / growth_per_turn / output_per_turn
                                                 -- compact_output_ratio / estimate_calibration / cost_calibration
                                                 -- quota_equiv_cost
    value         REAL NOT NULL,
    samples       INTEGER NOT NULL,
    updated_at    TEXT NOT NULL DEFAULT NOW,
    PRIMARY KEY (provider_name, model_id, domain, bucket, metric)
);

CREATE TABLE triage_feedback (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    prompt_hash TEXT NOT NULL,
    keywords    TEXT NOT NULL,
    predicted   TEXT NOT NULL,
    corrected   TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT NOW
);

CREATE TABLE settings (
    key        TEXT PRIMARY KEY,
    value_json TEXT NOT NULL
);
```

## 7. 指令授权

```sql
CREATE TABLE command_rules (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id  TEXT REFERENCES projects(id) ON DELETE CASCADE,     -- NULL 为全局
    provider    TEXT,                                               -- NULL 为全部 CLI
    pattern     TEXT NOT NULL,          -- 语法见 11 §3.3
    level       TEXT NOT NULL CHECK (level IN ('read_only','project','sensitive','forbidden')),
    source      TEXT NOT NULL CHECK (source IN ('digest','user','approval')),
    created_at  TEXT NOT NULL DEFAULT NOW
);
CREATE UNIQUE INDEX idx_command_rules_scope
    ON command_rules(IFNULL(project_id, ''), IFNULL(provider, ''), pattern);

CREATE TABLE approvals (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    attempt_id  INTEGER NOT NULL REFERENCES attempts(id) ON DELETE CASCADE,
    command     TEXT NOT NULL,          -- 已脱敏
    level       TEXT NOT NULL,
    reason      TEXT,
    decision    TEXT NOT NULL,          -- allow_once / allow_session / allow_project / deny / auto / forbidden
    mode        TEXT NOT NULL,          -- 当时的授权模式
    decided_at  TEXT NOT NULL DEFAULT NOW
);
CREATE INDEX idx_approvals_attempt ON approvals(attempt_id);
```

轮间间隔分布从 `usage_events.gap_seconds` 按 Provider 取最近 200 个样本计算，不单独建表。
