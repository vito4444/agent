use crate::db::Db;
use crate::error::{CoreError, Result};
use crate::journal::Journal;
use crate::pathutil::{assert_contained, canonicalize_existing, canonicalize_for_create};
use crate::types::{ArtifactRef, JournalKind};
use chrono::Utc;
use rusqlite::params;
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct WorktreeInfo {
    pub id: String,
    pub path: PathBuf,
    pub branch: String,
    pub task_id: String,
    pub inplace: bool,
}

pub struct WorktreeManager<'a> {
    db: &'a Db,
    repo_root: PathBuf,
    worktree_root: PathBuf,
}

impl<'a> WorktreeManager<'a> {
    pub fn new(db: &'a Db, repo_root: PathBuf, worktree_root: PathBuf) -> Result<Self> {
        let repo_root = canonicalize_for_create(&repo_root)?;
        std::fs::create_dir_all(&worktree_root)?;
        let worktree_root = canonicalize_for_create(&worktree_root)?;
        Ok(Self {
            db,
            repo_root,
            worktree_root,
        })
    }

    pub fn repo_root(&self) -> &Path {
        &self.repo_root
    }

    /// Parallel tasks MUST be isolated. ask/single-file may opt into inplace,
    /// but callers must surface ⚠ 原地·非隔离 in UI — silent in-place races corrupt demos.
    pub fn create_for_task(
        &self,
        run_id: &str,
        task_id: &str,
        allow_inplace: bool,
    ) -> Result<WorktreeInfo> {
        let journal = Journal::new(self.db);
        if allow_inplace {
            let id = Uuid::new_v4().to_string();
            let now = Utc::now().to_rfc3339();
            let path = self.repo_root.clone();
            self.db.conn().execute(
                "INSERT INTO worktrees(id, run_id, task_id, path, branch, base_commit, status, created_at)
                 VALUES (?1,?2,?3,?4,?5,?6,'inplace',?7)",
                params![
                    id,
                    run_id,
                    task_id,
                    path.display().to_string(),
                    format!("inplace/{task_id}"),
                    Option::<String>::None,
                    now
                ],
            )?;
            journal.append(
                JournalKind::WorktreeCreated,
                Some(run_id),
                Some(task_id),
                json!({
                    "path": path.display().to_string(),
                    "inplace": true,
                    "warning": "⚠ 原地·非隔离"
                }),
            )?;
            return Ok(WorktreeInfo {
                id,
                path,
                branch: format!("inplace/{task_id}"),
                task_id: task_id.to_string(),
                inplace: true,
            });
        }

        self.ensure_git_repo()?;
        let branch = format!("agent/{run_id}/{task_id}");
        self.assert_branch_not_double_checked_out(&branch)?;

        let path = self.worktree_root.join(format!("{run_id}_{task_id}"));
        if path.exists() {
            // Leftover from crash — remove before recreate so orphan scan stays meaningful.
            let _ = Command::new("git")
                .args(["worktree", "remove", "--force"])
                .arg(&path)
                .current_dir(&self.repo_root)
                .status();
            if path.exists() {
                std::fs::remove_dir_all(&path)?;
            }
        }
        assert_contained(&self.worktree_root, &path)?;

        let out = Command::new("git")
            .args(["worktree", "add", "-b", &branch])
            .arg(&path)
            .current_dir(&self.repo_root)
            .output()?;
        if !out.status.success() {
            // Branch may already exist after retry — try without -b.
            let out2 = Command::new("git")
                .args(["worktree", "add"])
                .arg(&path)
                .arg(&branch)
                .current_dir(&self.repo_root)
                .output()?;
            if !out2.status.success() {
                return Err(CoreError::Other(format!(
                    "git worktree add failed: {}",
                    String::from_utf8_lossy(&out2.stderr)
                )));
            }
        }

        let path = canonicalize_existing(&path)?;
        let base = git_rev_parse(&self.repo_root, "HEAD")?;
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        self.db.conn().execute(
            "INSERT INTO worktrees(id, run_id, task_id, path, branch, base_commit, status, created_at)
             VALUES (?1,?2,?3,?4,?5,?6,'active',?7)",
            params![
                id,
                run_id,
                task_id,
                path.display().to_string(),
                branch,
                base,
                now
            ],
        )?;
        journal.append(
            JournalKind::WorktreeCreated,
            Some(run_id),
            Some(task_id),
            json!({
                "path": path.display().to_string(),
                "branch": branch,
                "inplace": false
            }),
        )?;
        Ok(WorktreeInfo {
            id,
            path,
            branch,
            task_id: task_id.to_string(),
            inplace: false,
        })
    }

    /// Copy declared upstream artifacts into the downstream worktree.
    /// Missing files hard-fail — a "fake dependency edge" must not look green.
    pub fn bootstrap_artifacts(
        &self,
        run_id: &str,
        task_id: &str,
        dest_worktree: &Path,
        required: &[(String, Vec<ArtifactRef>)],
    ) -> Result<Vec<String>> {
        let journal = Journal::new(self.db);
        let mut copied = Vec::new();
        for (from_task, arts) in required {
            for art in arts {
                let row: Result<(String, String)> = self.db.conn().query_row(
                    "SELECT path, content_sha256 FROM artifacts WHERE run_id = ?1 AND task_id = ?2 AND artifact_key = ?3",
                    params![run_id, from_task, art.id],
                    |r| Ok((r.get(0)?, r.get::<_, Option<String>>(1)?.unwrap_or_default())),
                ).map_err(|_| CoreError::MissingArtifact {
                    task_id: task_id.to_string(),
                    artifact_id: art.id.clone(),
                    from_task: from_task.clone(),
                    path: art.path.clone(),
                });
                let (src_rel, _sha) = row?;
                // Prefer artifact path recorded relative to upstream worktree; look up upstream path.
                let upstream_path: String = self.db.conn().query_row(
                    "SELECT path FROM worktrees WHERE run_id = ?1 AND task_id = ?2 ORDER BY created_at DESC LIMIT 1",
                    params![run_id, from_task],
                    |r| r.get(0),
                ).map_err(|_| CoreError::MissingArtifact {
                    task_id: task_id.to_string(),
                    artifact_id: art.id.clone(),
                    from_task: from_task.clone(),
                    path: art.path.clone(),
                })?;
                let src = PathBuf::from(&upstream_path).join(&src_rel);
                if !src.exists() {
                    // Also try absolute recorded path / repo-relative
                    let alt = PathBuf::from(&src_rel);
                    if alt.exists() {
                        self.copy_into(dest_worktree, &alt, &art.path)?;
                    } else {
                        return Err(CoreError::MissingArtifact {
                            task_id: task_id.to_string(),
                            artifact_id: art.id.clone(),
                            from_task: from_task.clone(),
                            path: art.path.clone(),
                        });
                    }
                } else {
                    self.copy_into(dest_worktree, &src, &art.path)?;
                }
                copied.push(art.id.clone());
            }
        }
        journal.append(
            JournalKind::WorktreeBootstrapped,
            Some(run_id),
            Some(task_id),
            json!({ "artifacts": copied }),
        )?;
        Ok(copied)
    }

    fn copy_into(&self, dest_root: &Path, src_file: &Path, rel_dest: &str) -> Result<()> {
        assert_contained(dest_root, &dest_root.join(rel_dest))?;
        let dest = dest_root.join(rel_dest);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(src_file, &dest)?;
        Ok(())
    }

    pub fn record_artifact(
        &self,
        run_id: &str,
        task_id: &str,
        artifact: &ArtifactRef,
        worktree: &Path,
    ) -> Result<()> {
        let full = worktree.join(&artifact.path);
        if !full.exists() {
            return Err(CoreError::MissingArtifact {
                task_id: task_id.to_string(),
                artifact_id: artifact.id.clone(),
                from_task: task_id.to_string(),
                path: artifact.path.clone(),
            });
        }
        assert_contained(worktree, &full)?;
        let bytes = std::fs::read(&full)?;
        let sha = {
            use sha2::{Digest, Sha256};
            hex::encode(Sha256::digest(&bytes))
        };
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        self.db.conn().execute(
            "INSERT OR REPLACE INTO artifacts(id, run_id, task_id, artifact_key, path, content_sha256, created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![id, run_id, task_id, artifact.id, artifact.path, sha, now],
        )?;
        Journal::new(self.db).append(
            JournalKind::ArtifactRecorded,
            Some(run_id),
            Some(task_id),
            json!({
                "artifact_id": artifact.id,
                "path": artifact.path,
                "sha256": sha
            }),
        )?;
        Ok(())
    }

    /// Scan for orphan worktrees / double checkouts. Emits journal events; does not auto-delete.
    pub fn scan_orphans(&self) -> Result<Vec<String>> {
        let journal = Journal::new(self.db);
        let mut found = Vec::new();

        // 1) DB worktrees whose directories vanished
        let mut stmt = self.db.conn().prepare(
            "SELECT id, path, task_id, run_id FROM worktrees WHERE status = 'active'",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        for row in rows {
            let (id, path, task_id, run_id) = row?;
            if !Path::new(&path).exists() {
                found.push(path.clone());
                journal.append(
                    JournalKind::OrphanDetected,
                    Some(&run_id),
                    Some(&task_id),
                    json!({"reason": "missing_dir", "path": path, "worktree_id": id}),
                )?;
            }
        }

        // 2) git worktree list entries under our root not in DB
        if self.repo_root.join(".git").exists() || self.repo_root.join(".git").is_file() {
            let out = Command::new("git")
                .args(["worktree", "list", "--porcelain"])
                .current_dir(&self.repo_root)
                .output()?;
            let text = String::from_utf8_lossy(&out.stdout);
            let mut branch_paths: HashMap<String, Vec<String>> = HashMap::new();
            let mut current_path = None;
            for line in text.lines() {
                if let Some(p) = line.strip_prefix("worktree ") {
                    current_path = Some(p.to_string());
                } else if let Some(b) = line.strip_prefix("branch refs/heads/") {
                    if let Some(p) = &current_path {
                        branch_paths.entry(b.to_string()).or_default().push(p.clone());
                    }
                }
            }
            for (branch, paths) in branch_paths {
                if paths.len() > 1 {
                    let msg = format!("branch {branch} checked out at {:?}", paths);
                    found.push(msg.clone());
                    journal.append(
                        JournalKind::OrphanDetected,
                        None,
                        None,
                        json!({"reason": "double_checkout", "branch": branch, "paths": paths}),
                    )?;
                }
            }
        }

        // 3) directories under worktree_root not registered
        if self.worktree_root.exists() {
            for entry in std::fs::read_dir(&self.worktree_root)? {
                let entry = entry?;
                let p = entry.path();
                if !p.is_dir() {
                    continue;
                }
                let ps = p.display().to_string();
                let exists: i64 = self.db.conn().query_row(
                    "SELECT COUNT(*) FROM worktrees WHERE path = ?1",
                    params![ps],
                    |r| r.get(0),
                )?;
                if exists == 0 {
                    found.push(ps.clone());
                    journal.append(
                        JournalKind::OrphanDetected,
                        None,
                        None,
                        json!({"reason": "untracked_dir", "path": ps}),
                    )?;
                }
            }
        }
        Ok(found)
    }

    fn assert_branch_not_double_checked_out(&self, branch: &str) -> Result<()> {
        let count: i64 = self.db.conn().query_row(
            "SELECT COUNT(*) FROM worktrees WHERE branch = ?1 AND status = 'active'",
            params![branch],
            |r| r.get(0),
        )?;
        if count > 0 {
            let existing: String = self.db.conn().query_row(
                "SELECT path FROM worktrees WHERE branch = ?1 AND status = 'active' LIMIT 1",
                params![branch],
                |r| r.get(0),
            )?;
            return Err(CoreError::WorktreeConflict {
                branch: branch.to_string(),
                existing,
            });
        }
        Ok(())
    }

    fn ensure_git_repo(&self) -> Result<()> {
        if self.repo_root.join(".git").exists() {
            return Ok(());
        }
        let st = Command::new("git")
            .args(["init"])
            .current_dir(&self.repo_root)
            .status()?;
        if !st.success() {
            return Err(CoreError::Other("git init failed".into()));
        }
        // Need an initial commit for worktree add -b.
        let _ = Command::new("git")
            .args(["config", "user.email", "agent@local"])
            .current_dir(&self.repo_root)
            .status();
        let _ = Command::new("git")
            .args(["config", "user.name", "agent"])
            .current_dir(&self.repo_root)
            .status();
        let _ = Command::new("git")
            .args(["add", "-A"])
            .current_dir(&self.repo_root)
            .status();
        let _ = Command::new("git")
            .args(["commit", "--allow-empty", "-m", "init"])
            .current_dir(&self.repo_root)
            .status();
        Ok(())
    }
}

fn git_rev_parse(repo: &Path, rev: &str) -> Result<String> {
    let out = Command::new("git")
        .args(["rev-parse", rev])
        .current_dir(repo)
        .output()?;
    if !out.status.success() {
        return Ok(String::new());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::types::ArtifactRef;
    use tempfile::tempdir;

    fn init_repo(dir: &Path) {
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/lib.rs"), "pub fn add(a:i32,b:i32)->i32{a+b}\n").unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname=\"demo\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
        )
        .unwrap();
        let _ = Command::new("git").args(["init"]).current_dir(dir).status();
        let _ = Command::new("git")
            .args(["config", "user.email", "t@t"])
            .current_dir(dir)
            .status();
        let _ = Command::new("git")
            .args(["config", "user.name", "t"])
            .current_dir(dir)
            .status();
        let _ = Command::new("git")
            .args(["add", "-A"])
            .current_dir(dir)
            .status();
        let _ = Command::new("git")
            .args(["commit", "-m", "i"])
            .current_dir(dir)
            .status();
    }

    #[test]
    fn dependency_edge_hard_fails_without_artifact() {
        let tmp = tempdir().unwrap();
        let repo = tmp.path().join("repo");
        let wt = tmp.path().join("wts");
        init_repo(&repo);
        let db = Db::open_in_memory().unwrap();
        // Minimal run row for FK if needed — artifacts table refs runs.
        db.conn()
            .execute(
                "INSERT INTO runs(id,name,repo_root,status,created_at,updated_at) VALUES ('r1','n',?1,'running',datetime('now'),datetime('now'))",
                params![repo.display().to_string()],
            )
            .unwrap();
        let mgr = WorktreeManager::new(&db, repo.clone(), wt).unwrap();
        let a = mgr.create_for_task("r1", "A", false).unwrap();
        let b = mgr.create_for_task("r1", "B", false).unwrap();
        // Do NOT record artifact from A
        let err = mgr
            .bootstrap_artifacts(
                "r1",
                "B",
                &b.path,
                &[(
                    "A".into(),
                    vec![ArtifactRef {
                        id: "fixed_lib".into(),
                        path: "src/lib.rs".into(),
                        description: None,
                    }],
                )],
            )
            .unwrap_err();
        match &err {
            CoreError::MissingArtifact {
                artifact_id, path, ..
            } => {
                assert_eq!(artifact_id, "fixed_lib");
                assert_eq!(path, "src/lib.rs");
                assert!(
                    err.to_string().contains("src/lib.rs"),
                    "Display must name path: {err}"
                );
            }
            other => panic!("expected MissingArtifact, got {other}"),
        }
        // Now record and bootstrap succeeds
        mgr.record_artifact(
            "r1",
            "A",
            &ArtifactRef {
                id: "fixed_lib".into(),
                path: "src/lib.rs".into(),
                description: None,
            },
            &a.path,
        )
        .unwrap();
        // Mutate A's file so B can observe upstream content
        std::fs::write(a.path.join("src/lib.rs"), "pub fn add(a:i32,b:i32)->i32{a+b} // fixed\n")
            .unwrap();
        mgr.record_artifact(
            "r1",
            "A",
            &ArtifactRef {
                id: "fixed_lib".into(),
                path: "src/lib.rs".into(),
                description: None,
            },
            &a.path,
        )
        .unwrap();
        mgr.bootstrap_artifacts(
            "r1",
            "B",
            &b.path,
            &[(
                "A".into(),
                vec![ArtifactRef {
                    id: "fixed_lib".into(),
                    path: "src/lib.rs".into(),
                    description: None,
                }],
            )],
        )
        .unwrap();
        let content = std::fs::read_to_string(b.path.join("src/lib.rs")).unwrap();
        assert!(content.contains("fixed"));
    }

    #[test]
    fn orphan_scan_finds_untracked_dir() {
        let tmp = tempdir().unwrap();
        let repo = tmp.path().join("repo");
        let wt = tmp.path().join("wts");
        init_repo(&repo);
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::create_dir_all(wt.join("ghost")).unwrap();
        let db = Db::open_in_memory().unwrap();
        let mgr = WorktreeManager::new(&db, repo, wt).unwrap();
        let found = mgr.scan_orphans().unwrap();
        assert!(found.iter().any(|p| p.contains("ghost")));
    }
}
