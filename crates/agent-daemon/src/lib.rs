//! Daemon façade used by CLI and Tauri.
//! Startup steps degrade into banners instead of hard-crashing the shell —
//! a missing OpenCode binary must not prevent opening the workbench.

use agent_acp::normalize::fixture_transcript;
use agent_acp::{
    normalize_session_update, probe_opencode, AcpClient, AcpSpawnOpts, AdvertisedMenus,
    NormalizedEvent, OpenCodeProbe, SharedDb,
};
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
    /// Real probe result — never a hard-coded true.
    pub opencode_available: bool,
    pub opencode_bin: Option<String>,
    pub opencode_probe_source: String,
    pub opencode_version: Option<String>,
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

        let probe = probe_opencode();
        let opencode_available = probe.available;
        if opencode_available {
            banners.push(StartupBanner {
                level: "info".into(),
                message: format!(
                    "OpenCode available at {} (via {}, version={})",
                    probe.path.as_deref().unwrap_or("?"),
                    probe.source,
                    probe.version.as_deref().unwrap_or("unknown")
                ),
            });
        } else {
            banners.push(StartupBanner {
                level: "warn".into(),
                message: format!(
                    "OpenCode not available (probe source={}). Set OPENCODE_BIN or install `opencode` on PATH. Live ACP unverified; using fixture/mock.",
                    probe.source
                ),
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
            opencode_bin: probe.path.clone(),
            opencode_probe_source: probe.source.clone(),
            opencode_version: probe.version.clone(),
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

/// Shared handle for Tauri managed state.
pub type SharedWorkbench = Arc<Mutex<Option<Workbench>>>;

/// Outcome of a live OpenCode ACP smoke. `skipped=true` means binary absent — not a pass.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveAcpSmokeReport {
    pub skipped: bool,
    pub reason: Option<String>,
    pub probe: OpenCodeProbe,
    pub session_id: Option<String>,
    pub menus: AdvertisedMenus,
    pub menus_has_model: bool,
    pub menus_has_thought_level: bool,
    pub prompt_attempted: bool,
    pub prompt_ok: bool,
    pub prompt_error: Option<String>,
    pub journal_session_updates: usize,
    pub journal_permission_events: usize,
    pub model_switch_check: Option<String>,
}

/// Live OpenCode ACP path: SharedDb journal only (no parallel in-memory-only sink).
/// Skips cleanly when `opencode` is missing so CI stays green.
pub async fn run_live_acp_smoke(cwd: &Path) -> Result<LiveAcpSmokeReport> {
    let probe = probe_opencode();
    if !probe.available {
        return Ok(LiveAcpSmokeReport {
            skipped: true,
            reason: Some(format!(
                "OpenCode not available (source={}). Live ACP smoke skipped — not a pass.",
                probe.source
            )),
            probe,
            session_id: None,
            menus: AdvertisedMenus::default(),
            menus_has_model: false,
            menus_has_thought_level: false,
            prompt_attempted: false,
            prompt_ok: false,
            prompt_error: None,
            journal_session_updates: 0,
            journal_permission_events: 0,
            model_switch_check: None,
        });
    }

    let db_path = cwd.join(".acp-live-smoke.sqlite");
    let _ = std::fs::remove_file(&db_path);
    let shared: SharedDb = Arc::new(Mutex::new(Db::open(&db_path)?));

    // Deny-by-default: do not pre-remember. Live smoke must not reintroduce silent allow.
    let client = AcpClient::spawn_opencode(
        cwd,
        AcpSpawnOpts {
            // Live path: journal via SharedDb only — no second in-memory-only sink.
            event_sink: None,
            db: Some(shared.clone()),
        },
    )
    .await
    .context("spawn opencode acp")?;

    let (sid, menus) = client
        .session_new(cwd)
        .await
        .context("session/new")?;
    {
        let guard = shared.lock();
        Journal::new(&guard).append(
            JournalKind::SessionCreated,
            None,
            None,
            json!({
                "session_id": sid,
                "config_options_advertised": {
                    "model": menus.model.is_some(),
                    "thought_level": menus.thought_level.is_some(),
                }
            }),
        )?;
    }

    // Honesty: menus only reflect what session/new advertised.
    let menus_has_model = menus.model.is_some();
    let menus_has_thought = menus.thought_level.is_some();

    let model_switch_check = if let Some(ref model) = menus.model {
        let current = model
            .current_value
            .as_ref()
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        // Asking for a nonexistent config id must force new-session path.
        let denied = client
            .try_set_model(&sid, "__not_advertised__", "x")
            .await?;
        Some(format!(
            "current_model={current:?}; unadvertised_switch={denied:?}"
        ))
    } else {
        // No model menu advertised — try_set_model must deny hot-swap.
        let denied = client
            .try_set_model(&sid, "model", "anything")
            .await?;
        Some(format!(
            "no_model_advertised; switch_outcome={denied:?}"
        ))
    };

    let prompt = {
        let guard = shared.lock();
        let (combined, _) = MemoryStore::new(&guard).inject_l0_into_prompt("Reply with exactly: pong")?;
        combined
    };

    let mut prompt_ok = false;
    let mut prompt_error = None;
    let prompt_attempted = true;
    match client.prompt(&sid, &prompt).await {
        Ok(_) => {
            prompt_ok = true;
            // Allow agent to stream updates into the journal.
            tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        }
        Err(e) => {
            prompt_error = Some(e.to_string());
        }
    }

    let _ = client.close_session(&sid).await;
    client.kill().await?;

    let (journal_session_updates, journal_permission_events) = {
        let guard = shared.lock();
        let events = Journal::new(&guard).list_since(0, 500)?;
        let su = events.iter().filter(|e| e.kind == "session_update").count();
        let pe = events
            .iter()
            .filter(|e| e.kind == "permission_requested" || e.kind == "permission_resolved")
            .count();
        (su, pe)
    };

    // When prompt succeeded we expect at least one session_update in the journal.
    // If prompt failed (e.g. missing auth), report honestly — do not invent updates.
    if prompt_ok && journal_session_updates == 0 {
        anyhow::bail!(
            "live ACP prompt returned ok but journal has zero session_update events"
        );
    }

    Ok(LiveAcpSmokeReport {
        skipped: false,
        reason: None,
        probe,
        session_id: Some(sid),
        menus,
        menus_has_model,
        menus_has_thought_level: menus_has_thought,
        prompt_attempted,
        prompt_ok,
        prompt_error,
        journal_session_updates,
        journal_permission_events,
        model_switch_check,
    })
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn live_acp_smoke_skips_when_opencode_missing() {
        // Force missing via bogus override so CI without OpenCode stays green.
        let prev = std::env::var_os("OPENCODE_BIN");
        std::env::set_var("OPENCODE_BIN", "/no/such/opencode-for-live-smoke");
        let dir = tempdir().unwrap();
        let report = run_live_acp_smoke(dir.path()).await.unwrap();
        assert!(report.skipped, "must skip, not fake-pass");
        assert!(report.reason.as_ref().unwrap().contains("not available"));
        assert!(!report.menus_has_model);
        assert!(!report.menus_has_thought_level);
        match prev {
            Some(v) => std::env::set_var("OPENCODE_BIN", v),
            None => std::env::remove_var("OPENCODE_BIN"),
        }
    }

    /// Ignored by default: runs only when a real OpenCode binary is on PATH / OPENCODE_BIN.
    /// `cargo test -p agent-daemon live_acp_smoke_when_present -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "requires live OpenCode binary; skipped in default CI"]
    async fn live_acp_smoke_when_present() {
        let probe = probe_opencode();
        if !probe.available {
            eprintln!("SKIP ignored test: OpenCode not available ({})", probe.source);
            return;
        }
        let dir = tempdir().unwrap();
        let report = run_live_acp_smoke(dir.path()).await.expect("live smoke");
        assert!(!report.skipped);
        assert!(report.session_id.is_some());
        // Menus must match advertisement flags (never invent).
        assert_eq!(report.menus_has_model, report.menus.model.is_some());
        assert_eq!(
            report.menus_has_thought_level,
            report.menus.thought_level.is_some()
        );
        if report.prompt_ok {
            assert!(
                report.journal_session_updates > 0,
                "prompt ok ⇒ journal must have session_update"
            );
        } else {
            eprintln!(
                "live session ok but prompt failed (often auth): {:?}",
                report.prompt_error
            );
        }
    }
}
