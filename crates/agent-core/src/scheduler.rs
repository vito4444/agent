use crate::db::Db;
use crate::error::{CoreError, Result};
use crate::gate::GateRunner;
use crate::graph::parse_graph_yaml;
use crate::journal::Journal;
use crate::merge::MergeQueue;
use crate::types::{ArtifactRef, JournalKind, TaskGraph, TaskStatus};
use crate::worktree::WorktreeManager;
use chrono::Utc;
use rusqlite::params;
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct RunSummary {
    pub run_id: String,
    pub task_states: HashMap<String, String>,
}

/// Deterministic scheduler for task graphs. No LLM client here —
/// model calls belong only in plan/replan UX, not gate/merge/schedule.
pub struct Scheduler<'a> {
    db: &'a Db,
    repo_root: PathBuf,
    worktree_root: PathBuf,
}

impl<'a> Scheduler<'a> {
    pub fn new(db: &'a Db, repo_root: PathBuf, worktree_root: PathBuf) -> Self {
        Self {
            db,
            repo_root,
            worktree_root,
        }
    }

    pub fn accept_graph(&self, name: &str, yaml: &str) -> Result<(String, TaskGraph)> {
        let graph = parse_graph_yaml(yaml)?;
        let run_id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let journal = Journal::new(self.db);
        self.db.conn().execute(
            "INSERT INTO runs(id, name, repo_root, status, graph_yaml, created_at, updated_at)
             VALUES (?1,?2,?3,'accepted',?4,?5,?5)",
            params![
                run_id,
                name,
                self.repo_root.display().to_string(),
                yaml,
                now
            ],
        )?;
        journal.append(
            JournalKind::RunCreated,
            Some(&run_id),
            None,
            json!({ "name": name }),
        )?;

        for t in &graph.tasks {
            let produces = serde_json::to_string(&t.produces)?;
            self.db.conn().execute(
                "INSERT INTO tasks(id, run_id, title, prompt, status, gate_command, allow_inplace, produces_json, created_at, updated_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?9)",
                params![
                    format!("{run_id}:{}", t.id),
                    run_id,
                    t.title,
                    t.prompt,
                    TaskStatus::Pending.as_str(),
                    t.gate_command,
                    t.allow_inplace as i64,
                    produces,
                    now
                ],
            )?;
        }
        for d in &graph.deps {
            let arts = serde_json::to_string(&d.artifacts)?;
            self.db.conn().execute(
                "INSERT INTO task_deps(run_id, from_task, to_task, artifacts_json) VALUES (?1,?2,?3,?4)",
                params![run_id, d.from, d.to, arts],
            )?;
        }
        journal.append(
            JournalKind::GraphAccepted,
            Some(&run_id),
            None,
            json!({ "tasks": graph.tasks.len(), "deps": graph.deps.len() }),
        )?;
        Ok((run_id, graph))
    }

    /// Execute ready tasks in topological waves. Downstream never starts if upstream gate fails.
    pub fn run_until_idle<F>(&self, run_id: &str, mut agent_exec: F) -> Result<RunSummary>
    where
        F: FnMut(&str, &str, &Path, &str) -> Result<()>,
    {
        let graph = self.load_graph(run_id)?;
        let mgr = WorktreeManager::new(self.db, self.repo_root.clone(), self.worktree_root.clone())?;
        let gate = GateRunner::new(self.db);
        let merge = MergeQueue::new(self.db);
        let journal = Journal::new(self.db);

        loop {
            let ready = self.ready_tasks(run_id, &graph)?;
            if ready.is_empty() {
                break;
            }
            for task_id in ready {
                let spec = graph
                    .tasks
                    .iter()
                    .find(|t| t.id == task_id)
                    .ok_or_else(|| CoreError::NotFound(task_id.clone()))?;
                self.set_status(run_id, &task_id, TaskStatus::Ready)?;
                journal.append(
                    JournalKind::TaskReady,
                    Some(run_id),
                    Some(&task_id),
                    json!({}),
                )?;

                let wt = mgr.create_for_task(run_id, &task_id, spec.allow_inplace)?;
                self.db.conn().execute(
                    "UPDATE tasks SET worktree_path=?1, branch=?2, updated_at=?3 WHERE id=?4",
                    params![
                        wt.path.display().to_string(),
                        wt.branch,
                        Utc::now().to_rfc3339(),
                        format!("{run_id}:{task_id}")
                    ],
                )?;

                // Bootstrap deps
                let deps = self.deps_for(run_id, &task_id)?;
                if !deps.is_empty() {
                    if let Err(e) = mgr.bootstrap_artifacts(run_id, &task_id, &wt.path, &deps) {
                        self.set_status(run_id, &task_id, TaskStatus::Failed)?;
                        return Err(e);
                    }
                }

                self.set_status(run_id, &task_id, TaskStatus::Running)?;
                if let Err(e) = agent_exec(run_id, &task_id, &wt.path, &spec.prompt) {
                    self.set_status(run_id, &task_id, TaskStatus::Failed)?;
                    journal.append(
                        JournalKind::ProcessCrashed,
                        Some(run_id),
                        Some(&task_id),
                        json!({ "error": e.to_string() }),
                    )?;
                    return Err(e);
                }

                // Declared produces are a contract: missing file must fail the producer
                // before gate/merge. Skipping here made upstream look green while the
                // edge only blew up downstream — a fake dependency.
                if !spec.produces.is_empty() {
                    for art in &spec.produces {
                        let full = wt.path.join(&art.path);
                        if !full.exists() {
                            self.set_status(run_id, &task_id, TaskStatus::Failed)?;
                            return Err(CoreError::MissingArtifact {
                                task_id: task_id.clone(),
                                artifact_id: art.id.clone(),
                                from_task: task_id.clone(),
                                path: art.path.clone(),
                            });
                        }
                        mgr.record_artifact(run_id, &task_id, art, &wt.path)?;
                    }
                }

                if let Some(cmd) = &spec.gate_command {
                    self.set_status(run_id, &task_id, TaskStatus::Gate)?;
                    match gate.run(run_id, &task_id, cmd, &wt.path) {
                        Ok(_) => {}
                        Err(e) => {
                            self.set_status(run_id, &task_id, TaskStatus::Failed)?;
                            // Gate red → no merge, downstream stays pending (never ready).
                            return Err(e);
                        }
                    }
                }

                self.set_status(run_id, &task_id, TaskStatus::Merging)?;
                merge.enqueue_and_merge(run_id, &task_id, &self.repo_root, &wt.path, &wt.branch)?;
                self.set_status(run_id, &task_id, TaskStatus::Done)?;
            }
        }

        Ok(RunSummary {
            run_id: run_id.to_string(),
            task_states: self.all_statuses(run_id)?,
        })
    }

    fn load_graph(&self, run_id: &str) -> Result<TaskGraph> {
        let yaml: String = self.db.conn().query_row(
            "SELECT graph_yaml FROM runs WHERE id=?1",
            params![run_id],
            |r| r.get(0),
        )?;
        parse_graph_yaml(&yaml)
    }

    fn ready_tasks(&self, run_id: &str, graph: &TaskGraph) -> Result<Vec<String>> {
        let statuses = self.all_statuses(run_id)?;
        let mut ready = Vec::new();
        for t in &graph.tasks {
            let st = statuses
                .get(&t.id)
                .cloned()
                .unwrap_or_else(|| TaskStatus::Pending.as_str().to_string());
            if st != TaskStatus::Pending.as_str() {
                continue;
            }
            let preds: Vec<_> = graph
                .deps
                .iter()
                .filter(|d| d.to == t.id)
                .collect();
            let ok = preds.iter().all(|d| {
                statuses.get(&d.from).map(|s| s.as_str()) == Some(TaskStatus::Done.as_str())
            });
            if ok {
                ready.push(t.id.clone());
            }
        }
        Ok(ready)
    }

    fn deps_for(&self, run_id: &str, task_id: &str) -> Result<Vec<(String, Vec<ArtifactRef>)>> {
        let mut stmt = self.db.conn().prepare(
            "SELECT from_task, artifacts_json FROM task_deps WHERE run_id=?1 AND to_task=?2",
        )?;
        let rows = stmt.query_map(params![run_id, task_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        let mut out = Vec::new();
        for r in rows {
            let (from, arts) = r?;
            let arts: Vec<ArtifactRef> = serde_json::from_str(&arts)?;
            out.push((from, arts));
        }
        Ok(out)
    }

    fn set_status(&self, run_id: &str, task_id: &str, status: TaskStatus) -> Result<()> {
        self.db.conn().execute(
            "UPDATE tasks SET status=?1, updated_at=?2 WHERE id=?3",
            params![
                status.as_str(),
                Utc::now().to_rfc3339(),
                format!("{run_id}:{task_id}")
            ],
        )?;
        Ok(())
    }

    fn all_statuses(&self, run_id: &str) -> Result<HashMap<String, String>> {
        let mut stmt = self
            .db
            .conn()
            .prepare("SELECT id, status FROM tasks WHERE run_id=?1")?;
        let rows = stmt.query_map(params![run_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        let mut map = HashMap::new();
        for r in rows {
            let (id, st) = r?;
            let short = id
                .split_once(':')
                .map(|(_, t)| t.to_string())
                .unwrap_or(id);
            map.insert(short, st);
        }
        Ok(map)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::demo_two_task_yaml;
    use crate::db::Db;
    use std::process::Command;
    use tempfile::tempdir;

    fn init_fixture(repo: &Path) {
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::create_dir_all(repo.join("tests")).unwrap();
        std::fs::write(
            repo.join("Cargo.toml"),
            "[package]\nname=\"demo_fix\"\nversion=\"0.1.0\"\nedition=\"2021\"\n\n[[test]]\nname=\"add_works\"\npath=\"tests/add_works.rs\"\n",
        )
        .unwrap();
        // Broken add
        std::fs::write(repo.join("src/lib.rs"), "pub fn add(a:i32,b:i32)->i32{a-b}\n").unwrap();
        std::fs::write(
            repo.join("tests/add_works.rs"),
            "#[test]\nfn add_works(){ assert_eq!(demo_fix::add(2,2), 4); }\n",
        )
        .unwrap();
        let _ = Command::new("git").args(["init"]).current_dir(repo).status();
        let _ = Command::new("git")
            .args(["config", "user.email", "t@t"])
            .current_dir(repo)
            .status();
        let _ = Command::new("git")
            .args(["config", "user.name", "t"])
            .current_dir(repo)
            .status();
        let _ = Command::new("git")
            .args(["add", "-A"])
            .current_dir(repo)
            .status();
        let _ = Command::new("git")
            .args(["commit", "-m", "i"])
            .current_dir(repo)
            .status();
    }

    #[test]
    fn gate_red_blocks_downstream() {
        let tmp = tempdir().unwrap();
        let repo = tmp.path().join("repo");
        let wts = tmp.path().join("wts");
        init_fixture(&repo);
        let db = Db::open_in_memory().unwrap();
        let sched = Scheduler::new(&db, repo.clone(), wts);
        let yaml = r#"
name: t
tasks:
  - id: A
    title: fix
    prompt: fix
    gate_command: "exit 1"
    produces:
      - id: fixed_lib
        path: src/lib.rs
  - id: B
    title: test
    prompt: test
    produces: []
deps:
  - from: A
    to: B
    artifacts:
      - id: fixed_lib
        path: src/lib.rs
"#;
        let (run_id, _) = sched.accept_graph("t", yaml).unwrap();
        let err = sched
            .run_until_idle(&run_id, |_r, _t, path, _p| {
                // Agent "works" but gate will fail
                std::fs::write(path.join("src/lib.rs"), "pub fn add(a:i32,b:i32)->i32{a+b}\n")
                    .unwrap();
                Ok(())
            })
            .unwrap_err();
        assert!(matches!(err, CoreError::GateFailed { .. }));
        let statuses = sched.all_statuses(&run_id).unwrap();
        assert_eq!(statuses.get("A").map(|s| s.as_str()), Some("failed"));
        assert_eq!(statuses.get("B").map(|s| s.as_str()), Some("pending"));
    }

    #[test]
    fn two_node_green_path_bootstraps_artifact() {
        let tmp = tempdir().unwrap();
        let repo = tmp.path().join("repo");
        let wts = tmp.path().join("wts");
        init_fixture(&repo);
        let db = Db::open_in_memory().unwrap();
        let sched = Scheduler::new(&db, repo.clone(), wts);
        // Use trivial gates so CI without cargo target quirks still passes logic.
        let yaml = r#"
name: t
tasks:
  - id: A
    title: fix
    prompt: fix
    gate_command: "test -f src/lib.rs"
    produces:
      - id: fixed_lib
        path: src/lib.rs
  - id: B
    title: test
    prompt: test
    gate_command: "grep -q fixed src/lib.rs"
    produces:
      - id: extra_tests
        path: tests/extra.rs
deps:
  - from: A
    to: B
    artifacts:
      - id: fixed_lib
        path: src/lib.rs
"#;
        let (run_id, _) = sched.accept_graph("t", yaml).unwrap();
        let summary = sched
            .run_until_idle(&run_id, |_r, task, path, _p| {
                if task == "A" {
                    std::fs::write(
                        path.join("src/lib.rs"),
                        "pub fn add(a:i32,b:i32)->i32{a+b} // fixed\n",
                    )
                    .unwrap();
                } else if task == "B" {
                    // Must see upstream fixed content already bootstrapped
                    let c = std::fs::read_to_string(path.join("src/lib.rs")).unwrap();
                    assert!(c.contains("fixed"), "B must see upstream artifact");
                    std::fs::create_dir_all(path.join("tests")).unwrap();
                    std::fs::write(path.join("tests/extra.rs"), "// extra\n").unwrap();
                }
                Ok(())
            })
            .unwrap();
        assert_eq!(summary.task_states.get("A").map(|s| s.as_str()), Some("done"));
        assert_eq!(summary.task_states.get("B").map(|s| s.as_str()), Some("done"));
        let _ = demo_two_task_yaml();
    }

    #[test]
    fn producer_missing_declared_produce_fails_before_merge() {
        let tmp = tempdir().unwrap();
        let repo = tmp.path().join("repo");
        let wts = tmp.path().join("wts");
        init_fixture(&repo);
        let db = Db::open_in_memory().unwrap();
        let sched = Scheduler::new(&db, repo.clone(), wts);
        let yaml = r#"
name: t
tasks:
  - id: A
    title: fix
    prompt: fix
    gate_command: "true"
    produces:
      - id: fixed_lib
        path: src/missing_produce.rs
  - id: B
    title: test
    prompt: test
    produces: []
deps:
  - from: A
    to: B
    artifacts:
      - id: fixed_lib
        path: src/missing_produce.rs
"#;
        let (run_id, _) = sched.accept_graph("t", yaml).unwrap();
        let err = sched
            .run_until_idle(&run_id, |_r, _t, _path, _p| {
                // Agent finishes but never writes the declared produce.
                Ok(())
            })
            .unwrap_err();
        match &err {
            CoreError::MissingArtifact {
                task_id,
                artifact_id,
                path,
                ..
            } => {
                assert_eq!(task_id, "A");
                assert_eq!(artifact_id, "fixed_lib");
                assert_eq!(path, "src/missing_produce.rs");
                let msg = err.to_string();
                assert!(msg.contains("src/missing_produce.rs"), "{msg}");
                assert!(msg.contains("fixed_lib"), "{msg}");
                assert!(msg.contains('A'), "{msg}");
            }
            other => panic!("expected MissingArtifact, got {other}"),
        }
        let statuses = sched.all_statuses(&run_id).unwrap();
        assert_eq!(statuses.get("A").map(|s| s.as_str()), Some("failed"));
        assert_eq!(statuses.get("B").map(|s| s.as_str()), Some("pending"));
        // Must not have merged — that would be a fake-green producer.
        let merges = Journal::new(&db)
            .find_by_kind(JournalKind::MergeCompleted)
            .unwrap();
        assert!(merges.is_empty(), "producer must not merge without produces");
        let queued = Journal::new(&db)
            .find_by_kind(JournalKind::MergeQueued)
            .unwrap();
        assert!(queued.is_empty());
    }
}
