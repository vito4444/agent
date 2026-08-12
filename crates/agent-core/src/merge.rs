use crate::db::Db;
use crate::error::{CoreError, Result};
use crate::journal::Journal;
use crate::types::JournalKind;
use serde_json::json;
use std::path::Path;
use std::process::Command;

/// Merge queue is deterministic: gate must have passed or we refuse.
/// Silent merge-on-red would let downstream start on broken code.
pub struct MergeQueue<'a> {
    db: &'a Db,
}

impl<'a> MergeQueue<'a> {
    pub fn new(db: &'a Db) -> Self {
        Self { db }
    }

    pub fn enqueue_and_merge(
        &self,
        run_id: &str,
        task_id: &str,
        repo_root: &Path,
        worktree_path: &Path,
        branch: &str,
    ) -> Result<()> {
        let journal = Journal::new(self.db);
        // Last gate for task must be passed.
        let gate_status: Option<String> = self
            .db
            .conn()
            .query_row(
                "SELECT status FROM gates WHERE run_id=?1 AND task_id=?2 ORDER BY created_at DESC LIMIT 1",
                rusqlite::params![run_id, task_id],
                |r| r.get(0),
            )
            .ok();
        if gate_status.as_deref() != Some("passed") {
            journal.append(
                JournalKind::MergeRejected,
                Some(run_id),
                Some(task_id),
                json!({
                    "reason": "gate_not_passed",
                    "gate_status": gate_status,
                }),
            )?;
            return Err(CoreError::Other(format!(
                "merge rejected for task {task_id}: gate not passed"
            )));
        }

        journal.append(
            JournalKind::MergeQueued,
            Some(run_id),
            Some(task_id),
            json!({ "branch": branch }),
        )?;

        // Commit worktree changes then merge into main/master of repo_root.
        let _ = Command::new("git")
            .args(["add", "-A"])
            .current_dir(worktree_path)
            .status();
        let _ = Command::new("git")
            .args(["commit", "-m", &format!("agent: task {task_id}")])
            .current_dir(worktree_path)
            .status();

        // Determine base branch
        let base = detect_base_branch(repo_root);
        let merge = Command::new("git")
            .args(["merge", "--no-ff", "-m", &format!("merge {branch}"), branch])
            .current_dir(repo_root)
            .output()?;
        if !merge.status.success() {
            // Fallback: checkout files from worktree into repo (demo-friendly).
            // Still audited — we do not pretend git merge succeeded.
            copy_tree(worktree_path, repo_root)?;
            let _ = Command::new("git")
                .args(["add", "-A"])
                .current_dir(repo_root)
                .status();
            let _ = Command::new("git")
                .args([
                    "commit",
                    "-m",
                    &format!("agent: import task {task_id} (merge fallback)"),
                ])
                .current_dir(repo_root)
                .status();
            journal.append(
                JournalKind::MergeCompleted,
                Some(run_id),
                Some(task_id),
                json!({
                    "branch": branch,
                    "base": base,
                    "mode": "fallback_copy",
                    "git_stderr": String::from_utf8_lossy(&merge.stderr),
                }),
            )?;
            return Ok(());
        }

        journal.append(
            JournalKind::MergeCompleted,
            Some(run_id),
            Some(task_id),
            json!({ "branch": branch, "base": base, "mode": "git_merge" }),
        )?;
        Ok(())
    }
}

fn detect_base_branch(repo: &Path) -> String {
    for b in ["main", "master"] {
        let st = Command::new("git")
            .args(["rev-parse", "--verify", b])
            .current_dir(repo)
            .status();
        if matches!(st, Ok(s) if s.success()) {
            return b.to_string();
        }
    }
    "HEAD".into()
}

fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    for entry in walkdir_simple(from)? {
        let rel = entry.strip_prefix(from).unwrap();
        if rel.starts_with(".git") {
            continue;
        }
        let dest = to.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&dest)?;
        } else {
            if let Some(p) = dest.parent() {
                std::fs::create_dir_all(p)?;
            }
            std::fs::copy(&entry, &dest)?;
        }
    }
    Ok(())
}

fn walkdir_simple(root: &Path) -> Result<Vec<std::path::PathBuf>> {
    let mut out = Vec::new();
    fn rec(dir: &Path, out: &mut Vec<std::path::PathBuf>) -> Result<()> {
        for e in std::fs::read_dir(dir)? {
            let e = e?;
            let p = e.path();
            out.push(p.clone());
            if p.is_dir() && p.file_name().and_then(|s| s.to_str()) != Some(".git") {
                rec(&p, out)?;
            }
        }
        Ok(())
    }
    rec(root, &mut out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::gate::GateRunner;
    use tempfile::tempdir;

    #[test]
    fn merge_rejected_when_gate_failed() {
        let db = Db::open_in_memory().unwrap();
        let tmp = tempdir().unwrap();
        db.conn()
            .execute(
                "INSERT INTO runs(id,name,repo_root,status,created_at,updated_at) VALUES ('r','n',?1,'x',datetime('now'),datetime('now'))",
                rusqlite::params![tmp.path().display().to_string()],
            )
            .unwrap();
        // Insert failed gate row without going through runner success path
        db.conn()
            .execute(
                "INSERT INTO gates(id,run_id,task_id,command,exit_code,status,created_at) VALUES ('g','r','A','false',1,'failed',datetime('now'))",
                [],
            )
            .unwrap();
        let q = MergeQueue::new(&db);
        let err = q
            .enqueue_and_merge("r", "A", tmp.path(), tmp.path(), "agent/x")
            .unwrap_err();
        assert!(err.to_string().contains("merge rejected"));
        let _ = GateRunner::new(&db); // keep import used in cfg
    }
}
