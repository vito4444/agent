use crate::db::Db;
use crate::error::{CoreError, Result};
use crate::journal::Journal;
use crate::types::JournalKind;
use chrono::Utc;
use rusqlite::params;
use serde_json::json;
use std::path::Path;
use std::process::Command;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct GateResult {
    pub id: String,
    pub passed: bool,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Deterministic gate runner. No LLM — exit code is the only truth.
pub struct GateRunner<'a> {
    db: &'a Db,
}

impl<'a> GateRunner<'a> {
    pub fn new(db: &'a Db) -> Self {
        Self { db }
    }

    pub fn run(
        &self,
        run_id: &str,
        task_id: &str,
        command: &str,
        cwd: &Path,
    ) -> Result<GateResult> {
        let journal = Journal::new(self.db);
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        self.db.conn().execute(
            "INSERT INTO gates(id, run_id, task_id, command, status, created_at) VALUES (?1,?2,?3,?4,'running',?5)",
            params![id, run_id, task_id, command, now],
        )?;
        journal.append(
            JournalKind::GateStarted,
            Some(run_id),
            Some(task_id),
            json!({ "command": command, "gate_id": id }),
        )?;

        let output = Command::new("bash")
            .arg("-lc")
            .arg(command)
            .current_dir(cwd)
            .output()?;
        let exit_code = output.status.code().unwrap_or(-1);
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let passed = exit_code == 0;
        let status = if passed { "passed" } else { "failed" };
        let finished = Utc::now().to_rfc3339();
        self.db.conn().execute(
            "UPDATE gates SET exit_code=?1, stdout=?2, stderr=?3, status=?4, finished_at=?5 WHERE id=?6",
            params![exit_code, stdout, stderr, status, finished, id],
        )?;

        if passed {
            journal.append(
                JournalKind::GatePassed,
                Some(run_id),
                Some(task_id),
                json!({ "gate_id": id, "exit_code": exit_code }),
            )?;
            Ok(GateResult {
                id,
                passed: true,
                exit_code,
                stdout,
                stderr,
            })
        } else {
            journal.append(
                JournalKind::GateFailed,
                Some(run_id),
                Some(task_id),
                json!({
                    "gate_id": id,
                    "exit_code": exit_code,
                    "stderr": stderr.chars().take(2000).collect::<String>()
                }),
            )?;
            Err(CoreError::GateFailed {
                task_id: task_id.to_string(),
                command: command.to_string(),
                exit_code,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use tempfile::tempdir;

    #[test]
    fn gate_failure_does_not_pass() {
        let db = Db::open_in_memory().unwrap();
        db.conn()
            .execute(
                "INSERT INTO runs(id,name,repo_root,status,created_at,updated_at) VALUES ('r','n','/tmp','x',datetime('now'),datetime('now'))",
                [],
            )
            .unwrap();
        let runner = GateRunner::new(&db);
        let dir = tempdir().unwrap();
        let err = runner
            .run("r", "A", "exit 7", dir.path())
            .unwrap_err();
        match err {
            CoreError::GateFailed { exit_code, .. } => assert_eq!(exit_code, 7),
            e => panic!("{e}"),
        }
        let kinds: Vec<String> = Journal::new(&db)
            .find_by_kind(JournalKind::GateFailed)
            .unwrap()
            .into_iter()
            .map(|e| e.kind)
            .collect();
        assert_eq!(kinds, vec!["gate_failed"]);
    }
}
