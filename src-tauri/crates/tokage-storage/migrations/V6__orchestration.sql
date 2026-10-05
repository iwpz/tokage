-- Orchestration tables for plans, contracts, tasks, task edges, briefs, and task reports (docs/08 schema.md §2-§4, P7.m1 & P7.m2).

CREATE TABLE plans (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    main_session_id TEXT NOT NULL REFERENCES main_sessions(id) ON DELETE CASCADE,
    version         INTEGER NOT NULL DEFAULT 1,
    parent_plan_id  TEXT,
    goal            TEXT NOT NULL,
    spec_json       TEXT NOT NULL,
    est_cost_usd    REAL NOT NULL DEFAULT 0,
    status          TEXT NOT NULL DEFAULT 'DRAFT'
                    CHECK (status IN ('DRAFT','APPROVED','RUNNING','PAUSED','COMPLETED','SUPERSEDED','ABANDONED')),
    created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    approved_at     TEXT,
    completed_at    TEXT
);
CREATE INDEX idx_plans_main_session ON plans(main_session_id, version);

CREATE TABLE plan_contracts (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    plan_id     INTEGER NOT NULL REFERENCES plans(id) ON DELETE CASCADE,
    code        TEXT NOT NULL,                 -- C1, C2 ...
    title       TEXT NOT NULL,
    content     TEXT NOT NULL,
    token_count INTEGER NOT NULL DEFAULT 0,
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
    from_task   TEXT NOT NULL,                 -- 上游 task code (e.g. T1)
    to_task     TEXT NOT NULL,                 -- 下游 task code (e.g. T2)
    PRIMARY KEY (plan_id, from_task, to_task)
);

CREATE TABLE briefs (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id        TEXT NOT NULL,              -- Task code or attempt_id
    kind           TEXT NOT NULL CHECK (kind IN ('TASK_BRIEF','PROGRESS_REPORT','DIRECT')),
    content        TEXT NOT NULL,
    token_count    INTEGER NOT NULL DEFAULT 0,
    edited_by_user INTEGER NOT NULL DEFAULT 0,
    created_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE INDEX idx_briefs_task ON briefs(task_id);

CREATE TABLE task_reports (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id             TEXT NOT NULL,         -- Task code
    model_id            TEXT NOT NULL,
    attempt_id          TEXT NOT NULL,
    first_pass          INTEGER NOT NULL DEFAULT 1,
    is_locally_generated INTEGER NOT NULL DEFAULT 0,
    summary_md          TEXT NOT NULL,         -- 传给下游紧凑报告
    full_md             TEXT NOT NULL,         -- 完整报告
    contract_deviation  TEXT,                  -- 偏离契约内容，无则 NULL
    changed_files_stat  TEXT NOT NULL DEFAULT '',
    changed_files_list  TEXT NOT NULL DEFAULT '[]',
    gate_results        TEXT NOT NULL DEFAULT '',
    cost_usd            REAL,
    created_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE INDEX idx_reports_task ON task_reports(task_id);
