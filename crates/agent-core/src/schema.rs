//! SQLite schema. Table semantics are locked for V0 — do not rename columns lightly.
//! Soft-invalidation (`invalid_at`) keeps audit history; physical deletes would break demos/replay.

pub const SCHEMA_SQL: &str = r#"
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS seq_counters (
    name TEXT PRIMARY KEY,
    value INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS runs (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    repo_root TEXT NOT NULL,
    status TEXT NOT NULL,
    graph_yaml TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS tasks (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES runs(id),
    title TEXT NOT NULL,
    prompt TEXT NOT NULL,
    status TEXT NOT NULL,
    gate_command TEXT,
    allow_inplace INTEGER NOT NULL DEFAULT 0,
    produces_json TEXT NOT NULL DEFAULT '[]',
    worktree_path TEXT,
    branch TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS task_deps (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id TEXT NOT NULL REFERENCES runs(id),
    from_task TEXT NOT NULL,
    to_task TEXT NOT NULL,
    artifacts_json TEXT NOT NULL DEFAULT '[]',
    UNIQUE(run_id, from_task, to_task)
);

CREATE TABLE IF NOT EXISTS artifacts (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES runs(id),
    task_id TEXT NOT NULL,
    artifact_key TEXT NOT NULL,
    path TEXT NOT NULL,
    content_sha256 TEXT,
    created_at TEXT NOT NULL,
    UNIQUE(run_id, task_id, artifact_key)
);

CREATE TABLE IF NOT EXISTS worktrees (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES runs(id),
    task_id TEXT NOT NULL,
    path TEXT NOT NULL UNIQUE,
    branch TEXT NOT NULL,
    base_commit TEXT,
    status TEXT NOT NULL,
    created_at TEXT NOT NULL
);

-- seq is the durable global order for UI replay / audit.
CREATE TABLE IF NOT EXISTS events (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL,
    run_id TEXT,
    task_id TEXT,
    payload_json TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    run_id TEXT,
    task_id TEXT,
    agent TEXT NOT NULL DEFAULT 'opencode',
    acp_session_id TEXT,
    cwd TEXT NOT NULL,
    pool_key TEXT NOT NULL,
    config_options_json TEXT,
    status TEXT NOT NULL,
    created_at TEXT NOT NULL,
    closed_at TEXT
);

-- Permission ids come from DB so "remember by op_type" survives restarts.
CREATE TABLE IF NOT EXISTS permissions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT,
    op_type TEXT NOT NULL,
    tool_call_id TEXT,
    status TEXT NOT NULL,
    remembered INTEGER NOT NULL DEFAULT 0,
    payload_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    resolved_at TEXT
);

CREATE TABLE IF NOT EXISTS gates (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES runs(id),
    task_id TEXT NOT NULL,
    command TEXT NOT NULL,
    exit_code INTEGER,
    stdout TEXT,
    stderr TEXT,
    status TEXT NOT NULL,
    created_at TEXT NOT NULL,
    finished_at TEXT
);

CREATE TABLE IF NOT EXISTS l0_rules (
    id TEXT PRIMARY KEY,
    content TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL
);

-- L1 mock facts: invalidate sets invalid_at, never physical delete.
CREATE TABLE IF NOT EXISTS l1_facts_mock (
    id TEXT PRIMARY KEY,
    content TEXT NOT NULL,
    source TEXT,
    created_at TEXT NOT NULL,
    invalid_at TEXT
);

CREATE TABLE IF NOT EXISTS proposals (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    content TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at TEXT NOT NULL,
    resolved_at TEXT
);

CREATE TABLE IF NOT EXISTS l2_bullets (
    id TEXT PRIMARY KEY,
    proposal_id TEXT REFERENCES proposals(id),
    content TEXT NOT NULL,
    created_at TEXT NOT NULL
);

INSERT OR IGNORE INTO seq_counters(name, value) VALUES ('event', 0);
INSERT OR IGNORE INTO meta(key, value) VALUES ('schema_version', '1');
"#;

pub const SCHEMA_VERSION: &str = "1";
