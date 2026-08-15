//! Daemon façade used by CLI and Tauri.
//! Startup steps degrade into banners instead of hard-crashing the shell —
//! a missing OpenCode binary must not prevent opening the workbench.

use agent_acp::normalize::fixture_transcript;
use agent_acp::{normalize_session_update, AcpClient, AcpSpawnOpts, NormalizedEvent, SharedDb};
use agent_core::db::Db;
use agent_core::graph::demo_two_task_yaml;
use agent_core::journal::Journal;
use agent_core::memory::MemoryStore;
use agent_core::permissions::PermissionStore;
use agent_core::scheduler::Scheduler;
use agent_core::types::JournalKind;
use agent_core::worktree::WorktreeManager;
use anyhow::{Context, Result};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StartupBanner {
    pub level: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkbenchState {
    pub db_path: String,
    pub repo_root: String,
    pub worktree_root: String,
    pub banners: Vec<StartupBanner>,
    pub opencode_available: bool,
}

pub struct Workbench {
    pub db: Db,
    pub state: WorkbenchState,
}

impl Workbench {
    pub fn open(data_dir: &Path, repo_root: &Path) -> Result<Self> {
        let mut banners = Vec::new();
        std::fs::create_dir_all(data_dir).ok();
        let db_path = data_dir.join("workbench.sqlite");
        let db = match Db::open(&db_path) {
            Ok(db) => db,
            Err(e) => {
                banners.push(StartupBanner {
                    level: "error".into(),
                    message: format!("DB open failed, using memory: {e}"),
                });
                Db::open_in_memory().context("memory db")?
            }
        };

        let worktree_root = data_dir.join("worktrees");
        if let Err(e) = std::fs::create_dir_all(&worktree_root) {
            banners.push(StartupBanner {
                level: "warn".into(),
                message: format!("worktree root create failed: {e}"),
            });
        }

        let opencode_available = which_opencode().is_some();
        if !opencode_available {
            banners.push(StartupBanner {
                level: "warn".into(),
                message: "OpenCode (`opencode acp`) not found on PATH. Using fixture replay / mock agent. Real agent runs are unverified on this machine.".into(),
            });
        }

        match WorktreeManager::new(&db, repo_root.to_path_buf(), worktree_root.clone()) {
            Ok(mgr) => {
                if let Err(e) = mgr.scan_orphans() {
                    banners.push(StartupBanner {
                        level: "warn".into(),
                        message: format!("orphan scan failed: {e}"),
                    });
                }
            }
            Err(e) => banners.push(StartupBanner {
                level: "warn".into(),
                message: format!("worktree manager init failed: {e}"),
            }),
        }

        let state = WorkbenchState {
            db_path: db_path.display().to_string(),
            repo_root: repo_root.display().to_string(),
            worktree_root: worktree_root.display().to_string(),
            banners,
            opencode_available,
        };
        Ok(Self { db, state })
    }

    pub fn demo_yaml(&self) -> &'static str {
        demo_two_task_yaml()
    }

    pub fn accept_and_run_mock(&self, yaml: &str) -> Result<Value> {
        let sched = Scheduler::new(
            &self.db,
            PathBuf::from(&self.state.repo_root),
            PathBuf::from(&self.state.worktree_root),
        );
        let (run_id, _) = sched.accept_graph("demo", yaml)?;
        let summary = sched.run_until_idle(&run_id, |_r, task, path, _prompt| {
            if task == "A" {
                let lib = path.join("src/lib.rs");
                if let Some(parent) = lib.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&lib, "pub fn add(a:i32,b:i32)->i32{a+b} // fixed\n")?;
            } else if task == "B" {
                std::fs::create_dir_all(path.join("tests"))?;
                std::fs::write(path.join("tests/extra.rs"), "#[test]\nfn extra(){assert!(true)}\n")?;
            }
            Ok(())
        })?;
        Ok(json!({
            "run_id": summary.run_id,
            "task_states": summary.task_states,
        }))
    }

    pub fn replay_fixture_transcript(&self) -> Result<Vec<NormalizedEvent>> {
        let fixtures = fixture_transcript();
        let mut out = Vec::new();
        let journal = Journal::new(&self.db);
        for params in fixtures.as_array().unwrap() {
            for ev in normalize_session_update(params) {
                if matches!(ev, NormalizedEvent::Ignored { .. }) {
                    continue;
                }
                journal.append(
                    JournalKind::SessionUpdate,
                    None,
                    None,
                    serde_json::to_value(&ev)?,
                )?;
                out.push(ev);
            }
        }
        Ok(out)
    }

    pub fn events_since(&self, seq: i64) -> Result<Vec<Value>> {
        let evs = Journal::new(&self.db).list_since(seq, 500)?;
        Ok(evs
            .into_iter()
            .map(|e| {
                json!({
                    "seq": e.seq,
                    "kind": e.kind,
                    "run_id": e.run_id,
                    "task_id": e.task_id,
                    "payload": e.payload,
                    "created_at": e.created_at.to_rfc3339(),
                })
            })
            .collect())
    }

    pub fn memory(&self) -> MemoryStore<'_> {
        MemoryStore::new(&self.db)
    }

    /// Prefix enabled L0 rules into the prompt that will be sent over ACP.
    /// Without this seam, inject_l0 unit tests can pass while the live prompt
    /// still omits standing rules.
    pub fn assemble_prompt(&self, user_prompt: &str) -> Result<String> {
        let (combined, _) = self.memory().inject_l0_into_prompt(user_prompt)?;
        Ok(combined)
    }

    pub fn seed_demo_memory(&self) -> Result<Value> {
        let mem = self.memory();
        let rule = mem.add_l0_rule("只读规则：修改代码前必须先写失败测试。")?;
        let fact = mem.add_l1_fact("mock: add() 曾经返回 a-b", Some("fixture"))?;
        let proposal = mem.create_proposal(
            "bullet",
            "ACE假提案：优先用属性测试覆盖加法交换律",
        )?;
        Ok(json!({
            "l0": rule,
            "l1": fact,
            "proposal": proposal,
        }))
    }
}

fn which_opencode() -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        for dir in std::env::split_paths(&paths) {
            let p = dir.join("opencode");
            if p.is_file() {
                return Some(p);
            }
        }
        None
    })
}

/// Shared handle for Tauri managed state.
pub type SharedWorkbench = Arc<Mutex<Option<Workbench>>>;

/// Production-shaped ACP smoke: mock session + SharedDb journal/permissions.
/// Remembers `edit` so the mock tool path can complete; events must hit the journal.
pub async fn run_mock_acp_smoke(cwd: &Path) -> Result<Vec<NormalizedEvent>> {
    let mock = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/mock-agent/mock_acp_agent.py");
    let db_path = cwd.join(".acp-smoke.sqlite");
    let shared: SharedDb = Arc::new(Mutex::new(Db::open(&db_path)?));
    {
        let guard = shared.lock();
        PermissionStore::new(&guard).remember("edit", true)?;
    }
    let collected = Arc::new(Mutex::new(Vec::new()));
    let sink_collected = collected.clone();
    let sink: agent_acp::client::EventSink = Arc::new(move |ev| {
        sink_collected.lock().push(ev);
    });
    let client = AcpClient::spawn(
        "python3",
        &[mock.to_str().unwrap()],
        cwd,
        vec![],
        AcpSpawnOpts {
            event_sink: Some(sink),
            db: Some(shared.clone()),
        },
    )
    .await?;
    let (sid, _) = client.session_new(cwd).await?;
    // Assemble via MemoryStore so L0 wiring is exercised when rules exist.
    let prompt = {
        let guard = shared.lock();
        let mem = MemoryStore::new(&guard);
        let (combined, _) = mem.inject_l0_into_prompt("fix add")?;
        combined
    };
    client.prompt(&sid, &prompt).await?;
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let _ = client.close_session(&sid).await;
    client.kill().await?;

    // Prove journal, not only the memory sink, received updates.
    {
        let guard = shared.lock();
        let journaled = Journal::new(&guard).list_since(0, 100)?;
        if !journaled.iter().any(|e| e.kind == "session_update") {
            anyhow::bail!("acp-smoke: expected session_update in journal");
        }
        if !journaled
            .iter()
            .any(|e| e.kind == "permission_requested")
        {
            anyhow::bail!("acp-smoke: expected permission_requested in journal");
        }
    }

    let events = collected.lock().clone();
    Ok(events)
}
