use crate::normalize::{normalize_permission_request, normalize_session_update, NormalizedEvent};
use crate::types::*;
use agent_core::db::Db;
use agent_core::error::{CoreError, Result as CoreResult};
use agent_core::journal::Journal;
use agent_core::permissions::PermissionStore;
use agent_core::types::JournalKind;
use parking_lot::Mutex as ParkingMutex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{mpsc, Mutex, oneshot};
use tracing::{debug, warn};

pub type EventSink = Arc<dyn Fn(NormalizedEvent) + Send + Sync>;

/// Shared durable state for permissions + journal. Process restart reloads from the same DB file.
pub type SharedDb = Arc<ParkingMutex<Db>>;

struct Pending {
    tx: oneshot::Sender<CoreResult<Value>>,
}

/// Stdio JSON-RPC ACP client aimed at `opencode acp` (or the mock agent).
pub struct AcpClient {
    child: Child,
    stdin: Arc<Mutex<ChildStdin>>,
    next_id: AtomicU64,
    pending: Arc<Mutex<HashMap<u64, Pending>>>,
    events: mpsc::UnboundedReceiver<NormalizedEvent>,
    config_options: Arc<Mutex<Vec<ConfigOption>>>,
    session_id: Arc<Mutex<Option<String>>>,
    db: Option<SharedDb>,
}

pub struct AcpSpawnOpts {
    pub event_sink: Option<EventSink>,
    /// When set, remember/lookup uses `permissions` table; session updates are journaled.
    pub db: Option<SharedDb>,
}

impl Default for AcpSpawnOpts {
    fn default() -> Self {
        Self {
            event_sink: None,
            db: None,
        }
    }
}

impl AcpClient {
    pub async fn spawn(
        executable: &str,
        args: &[&str],
        cwd: &Path,
        env: Vec<(String, String)>,
        opts: AcpSpawnOpts,
    ) -> CoreResult<Self> {
        let mut cmd = Command::new(executable);
        cmd.args(args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        for (k, v) in env {
            cmd.env(k, v);
        }
        let mut child = cmd.spawn().map_err(|e| {
            CoreError::Other(format!(
                "failed to spawn {executable}: {e}. Is OpenCode installed? Use mock agent for CI."
            ))
        })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| CoreError::Other("no stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| CoreError::Other("no stdout".into()))?;
        let pending: Arc<Mutex<HashMap<u64, Pending>>> = Arc::new(Mutex::new(HashMap::new()));
        let (tx, rx) = mpsc::unbounded_channel();
        let config_options = Arc::new(Mutex::new(Vec::new()));
        let session_id = Arc::new(Mutex::new(None));

        let pending_r = pending.clone();
        let config_r = config_options.clone();
        let stdin_r = Arc::new(Mutex::new(stdin));
        let stdin_reader = stdin_r.clone();
        let db_r = opts.db.clone();
        let session_r = session_id.clone();
        tokio::spawn(async move {
            if let Err(e) = read_loop(
                stdout,
                pending_r,
                tx,
                opts.event_sink,
                config_r,
                db_r,
                session_r,
                stdin_reader,
            )
            .await
            {
                warn!("acp read loop ended: {e}");
            }
        });

        let client = Self {
            child,
            stdin: stdin_r,
            next_id: AtomicU64::new(1),
            pending,
            events: rx,
            config_options,
            session_id,
            db: opts.db,
        };
        client.initialize().await?;
        Ok(client)
    }

    pub async fn spawn_opencode(cwd: &Path, opts: AcpSpawnOpts) -> CoreResult<Self> {
        // Model is NOT passed as `acp --model` — that flag does not exist.
        Self::spawn("opencode", &["acp"], cwd, vec![], opts).await
    }

    async fn initialize(&self) -> CoreResult<()> {
        let result = self
            .request(
                "initialize",
                json!({
                    "protocolVersion": PROTOCOL_VERSION,
                    "clientCapabilities": {
                        "fs": { "readTextFile": true, "writeTextFile": true },
                        "terminal": true,
                        "session": { "configOptions": { "boolean": {} } }
                    },
                    "clientInfo": {
                        "name": "agent-workbench",
                        "title": "Multi-Agent Workbench",
                        "version": "0.1.0"
                    }
                }),
            )
            .await?;
        debug!("acp initialize ok: {}", result);
        Ok(())
    }

    pub async fn session_new(&self, cwd: &Path) -> CoreResult<(String, AdvertisedMenus)> {
        let result = self
            .request(
                "session/new",
                json!({
                    "cwd": cwd.display().to_string(),
                    "mcpServers": []
                }),
            )
            .await?;
        let sid = result
            .get("sessionId")
            .or_else(|| result.get("session_id"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| CoreError::Other("session/new missing sessionId".into()))?
            .to_string();
        *self.session_id.lock().await = Some(sid.clone());
        let opts = parse_config_options(&result);
        *self.config_options.lock().await = opts.clone();
        Ok((sid, AdvertisedMenus::from_options(&opts)))
    }

    pub async fn prompt(&self, session_id: &str, text: &str) -> CoreResult<Value> {
        self.request(
            "session/prompt",
            json!({
                "sessionId": session_id,
                "prompt": [{ "type": "text", "text": text }]
            }),
        )
        .await
    }

    pub async fn cancel(&self, session_id: &str) -> CoreResult<()> {
        self.notify("session/cancel", json!({ "sessionId": session_id }))
            .await
    }

    pub async fn close_session(&self, session_id: &str) -> CoreResult<()> {
        match self
            .request("session/close", json!({ "sessionId": session_id }))
            .await
        {
            Ok(_) => Ok(()),
            Err(e) => {
                warn!("session/close failed (degraded): {e}");
                Ok(())
            }
        }
    }

    pub fn advertised_menus(&self) -> impl std::future::Future<Output = AdvertisedMenus> + '_ {
        async move {
            let opts = self.config_options.lock().await.clone();
            AdvertisedMenus::from_options(&opts)
        }
    }

    /// Hot-switch via set_config_option only when advertised.
    /// Otherwise return RequireNewSessionWithSummary — never pretend the model changed.
    pub async fn try_set_model(
        &self,
        session_id: &str,
        config_id: &str,
        value: &str,
    ) -> CoreResult<ModelSwitchOutcome> {
        let menus = self.advertised_menus().await;
        let advertised = menus
            .model
            .as_ref()
            .map(|m| m.id == config_id)
            .unwrap_or(false)
            || menus
                .thought_level
                .as_ref()
                .map(|m| m.id == config_id)
                .unwrap_or(false)
            || menus.model_config.iter().any(|m| m.id == config_id);
        if !advertised {
            return Ok(ModelSwitchOutcome::RequireNewSessionWithSummary {
                reason: format!(
                    "config option `{config_id}` not advertised; UI must start a new session with summary (no fake switch)"
                ),
            });
        }
        match self
            .request(
                "session/set_config_option",
                json!({
                    "sessionId": session_id,
                    "configId": config_id,
                    "value": value
                }),
            )
            .await
        {
            Ok(result) => {
                let opts = parse_config_options(&result);
                if !opts.is_empty() {
                    *self.config_options.lock().await = opts;
                }
                Ok(ModelSwitchOutcome::Applied)
            }
            Err(e) => Ok(ModelSwitchOutcome::RequireNewSessionWithSummary {
                reason: format!("set_config_option failed: {e}"),
            }),
        }
    }

    pub async fn next_event(&mut self) -> Option<NormalizedEvent> {
        self.events.recv().await
    }

    /// Persist a standing remember decision by op_type (survives restart via DB).
    pub fn remember_op(&self, op_type: &str, allow: bool) -> CoreResult<()> {
        let db = self
            .db
            .as_ref()
            .ok_or_else(|| CoreError::Other("remember_op requires SharedDb".into()))?;
        let guard = db.lock();
        PermissionStore::new(&guard).remember(op_type, allow)?;
        Ok(())
    }

    async fn request(&self, method: &str, params: Value) -> CoreResult<Value> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let req = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id,
            method: method.into(),
            params: Some(params),
        };
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, Pending { tx });
        let line = serde_json::to_string(&req)? + "\n";
        {
            let mut stdin = self.stdin.lock().await;
            stdin.write_all(line.as_bytes()).await?;
            stdin.flush().await?;
        }
        rx.await
            .map_err(|_| CoreError::Other("acp response channel closed".into()))?
    }

    async fn notify(&self, method: &str, params: Value) -> CoreResult<()> {
        let n = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        });
        let line = serde_json::to_string(&n)? + "\n";
        let mut stdin = self.stdin.lock().await;
        stdin.write_all(line.as_bytes()).await?;
        stdin.flush().await?;
        Ok(())
    }

    pub async fn kill(mut self) -> CoreResult<()> {
        let _ = self.child.kill().await;
        Ok(())
    }
}

fn parse_config_options(result: &Value) -> Vec<ConfigOption> {
    result
        .get("configOptions")
        .or_else(|| result.get("config_options"))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

fn journal_normalized(db: &Db, ev: &NormalizedEvent) -> CoreResult<()> {
    // PermissionRequested is written inside PermissionStore::record_request —
    // journaling the same event twice would break seq-based assertions.
    if matches!(ev, NormalizedEvent::PermissionRequest { .. }) {
        return Ok(());
    }
    if matches!(ev, NormalizedEvent::Ignored { .. }) {
        return Ok(());
    }
    Journal::new(db).append(
        JournalKind::SessionUpdate,
        None,
        None,
        serde_json::to_value(ev)?,
    )?;
    Ok(())
}

async fn read_loop(
    stdout: ChildStdout,
    pending: Arc<Mutex<HashMap<u64, Pending>>>,
    events: mpsc::UnboundedSender<NormalizedEvent>,
    sink: Option<EventSink>,
    config_options: Arc<Mutex<Vec<ConfigOption>>>,
    db: Option<SharedDb>,
    session_id: Arc<Mutex<Option<String>>>,
    stdin: Arc<Mutex<ChildStdin>>,
) -> CoreResult<()> {
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let value: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => {
                warn!("bad json from agent: {e} — {trimmed}");
                continue;
            }
        };

        if let Some(id) = value.get("id").and_then(|v| v.as_u64()) {
            if value.get("method").is_none() {
                let result = if let Some(err) = value.get("error") {
                    Err(CoreError::Other(err.to_string()))
                } else {
                    Ok(value.get("result").cloned().unwrap_or(Value::Null))
                };
                if let Some(p) = pending.lock().await.remove(&id) {
                    let _ = p.tx.send(result);
                }
                continue;
            }
            let method = value.get("method").and_then(|m| m.as_str()).unwrap_or("");
            let params = value.get("params").cloned().unwrap_or(Value::Null);
            if method == "session/request_permission" {
                let ev = normalize_permission_request(&params);
                let op = params
                    .get("opType")
                    .or_else(|| params.get("op_type"))
                    .or_else(|| params.pointer("/toolCall/kind"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let tool_call_id = params
                    .pointer("/toolCall/toolCallId")
                    .or_else(|| params.get("toolCallId"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let sid = session_id.lock().await.clone();

                // Never silent-allow: unremembered ⇒ deny + surface the request event.
                let allow = if let Some(db) = &db {
                    let guard = db.lock();
                    let store = PermissionStore::new(&guard);
                    let pid = store.record_request(
                        sid.as_deref(),
                        &op,
                        tool_call_id.as_deref(),
                        &params,
                    )?;
                    match store.decision_for(&op)? {
                        Some(a) => {
                            store.resolve(pid, a, false, &op)?;
                            a
                        }
                        None => {
                            store.resolve(pid, false, false, &op)?;
                            false
                        }
                    }
                } else {
                    // No DB attached: still must not default-allow.
                    false
                };

                let _ = events.send(ev.clone());
                if let Some(s) = &sink {
                    s(ev);
                }

                let resp = json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "outcome": {
                            "outcome": if allow { "selected" } else { "cancelled" },
                            "optionId": if allow { "allow-once" } else { "reject-once" }
                        }
                    }
                });
                let line = serde_json::to_string(&resp)? + "\n";
                let mut stdin = stdin.lock().await;
                stdin.write_all(line.as_bytes()).await?;
                stdin.flush().await?;
                continue;
            }
            if method.starts_with('_') {
                continue;
            }
            let resp = json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32601, "message": format!("method not found: {method}") }
            });
            let line = serde_json::to_string(&resp)? + "\n";
            let mut stdin = stdin.lock().await;
            stdin.write_all(line.as_bytes()).await?;
            stdin.flush().await?;
            continue;
        }

        let method = value.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let params = value.get("params").cloned().unwrap_or(Value::Null);
        if method == "session/update" {
            for ev in normalize_session_update(&params) {
                if let NormalizedEvent::ConfigOptionUpdate { options } = &ev {
                    if let Ok(opts) = serde_json::from_value::<Vec<ConfigOption>>(options.clone()) {
                        *config_options.lock().await = opts;
                    }
                }
                if matches!(ev, NormalizedEvent::Ignored { .. }) {
                    continue;
                }
                if let Some(db) = &db {
                    let guard = db.lock();
                    if let Err(e) = journal_normalized(&guard, &ev) {
                        warn!("journal session_update failed: {e}");
                    }
                }
                let _ = events.send(ev.clone());
                if let Some(s) = &sink {
                    s(ev);
                }
            }
        } else if method.starts_with('_') {
            // ignore
        } else {
            debug!("unhandled notification {method}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::db::Db;
    use agent_core::journal::Journal;
    use agent_core::memory::MemoryStore;
    use agent_core::permissions::PermissionStore;
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn mock_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/mock-agent/mock_acp_agent.py")
    }

    #[tokio::test]
    async fn mock_agent_initialize_and_prompt() {
        let mock = mock_path();
        assert!(mock.exists(), "mock agent missing at {}", mock.display());
        let cwd = tempdir().unwrap();
        let db = Arc::new(ParkingMutex::new(Db::open_in_memory().unwrap()));
        // Pre-remember edit — otherwise deny-by-default blocks the mock tool path.
        {
            let guard = db.lock();
            PermissionStore::new(&guard)
                .remember("edit", true)
                .unwrap();
        }
        let mut client = AcpClient::spawn(
            "python3",
            &[mock.to_str().unwrap()],
            cwd.path(),
            vec![],
            AcpSpawnOpts {
                event_sink: None,
                db: Some(db),
            },
        )
        .await
        .expect("spawn mock");
        let (sid, menus) = client.session_new(cwd.path()).await.unwrap();
        assert!(menus.model.is_some(), "mock advertises model");
        let outcome = client.try_set_model(&sid, "model", "fast").await.unwrap();
        assert_eq!(outcome, ModelSwitchOutcome::Applied);
        let denied = client
            .try_set_model(&sid, "nonexistent", "x")
            .await
            .unwrap();
        assert!(matches!(
            denied,
            ModelSwitchOutcome::RequireNewSessionWithSummary { .. }
        ));
        client.prompt(&sid, "hello").await.unwrap();
        let mut saw_msg = false;
        for _ in 0..20 {
            if let Some(ev) = client.next_event().await {
                if matches!(
                    ev,
                    NormalizedEvent::Message { .. } | NormalizedEvent::Thought { .. }
                ) {
                    saw_msg = true;
                    break;
                }
            }
        }
        assert!(saw_msg, "expected normalized message/thought from mock");
        client.kill().await.unwrap();
    }

    #[tokio::test]
    async fn unremembered_permission_is_denied_not_auto_allowed() {
        let mock = mock_path();
        let cwd = tempdir().unwrap();
        let db_path = cwd.path().join("wb.sqlite");
        let db = Arc::new(ParkingMutex::new(Db::open(&db_path).unwrap()));
        let client = AcpClient::spawn(
            "python3",
            &[mock.to_str().unwrap()],
            cwd.path(),
            vec![],
            AcpSpawnOpts {
                event_sink: None,
                db: Some(db.clone()),
            },
        )
        .await
        .unwrap();
        let (sid, _) = client.session_new(cwd.path()).await.unwrap();
        // Do NOT remember edit — must deny.
        client.prompt(&sid, "hello").await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let guard = db.lock();
        let resolved = Journal::new(&guard)
            .find_by_kind(JournalKind::PermissionResolved)
            .unwrap();
        assert!(
            !resolved.is_empty(),
            "permission resolve must be journaled"
        );
        let allow = resolved[0].payload.get("allow").and_then(|v| v.as_bool());
        assert_eq!(allow, Some(false), "unremembered must not auto-allow");
        assert_eq!(
            PermissionStore::new(&guard)
                .lookup_remembered("edit")
                .unwrap(),
            None
        );
        client.kill().await.unwrap();
    }

    #[tokio::test]
    async fn remember_op_type_survives_restart_and_does_not_cross_grant() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("perm.sqlite");
        {
            let db = Db::open(&db_path).unwrap();
            PermissionStore::new(&db).remember("fs_write", true).unwrap();
        }
        let db = Arc::new(ParkingMutex::new(Db::open(&db_path).unwrap()));
        {
            let guard = db.lock();
            assert_eq!(
                PermissionStore::new(&guard)
                    .lookup_remembered("fs_write")
                    .unwrap(),
                Some(true)
            );
            assert_eq!(
                PermissionStore::new(&guard)
                    .lookup_remembered("exec")
                    .unwrap(),
                None
            );
        }
        // Client remember_op also writes through SharedDb.
        let mock = mock_path();
        let client = AcpClient::spawn(
            "python3",
            &[mock.to_str().unwrap()],
            dir.path(),
            vec![],
            AcpSpawnOpts {
                event_sink: None,
                db: Some(db.clone()),
            },
        )
        .await
        .unwrap();
        client.remember_op("fs_write", true).unwrap();
        // exec still not granted
        let guard = db.lock();
        assert_eq!(
            PermissionStore::new(&guard)
                .decision_for("exec")
                .unwrap(),
            None
        );
        drop(guard);
        client.kill().await.unwrap();
    }

    #[tokio::test]
    async fn acp_session_updates_are_journaled() {
        let mock = mock_path();
        let cwd = tempdir().unwrap();
        let db = Arc::new(ParkingMutex::new(Db::open_in_memory().unwrap()));
        {
            let guard = db.lock();
            PermissionStore::new(&guard)
                .remember("edit", true)
                .unwrap();
        }
        let client = AcpClient::spawn(
            "python3",
            &[mock.to_str().unwrap()],
            cwd.path(),
            vec![],
            AcpSpawnOpts {
                event_sink: None,
                db: Some(db.clone()),
            },
        )
        .await
        .unwrap();
        let (sid, _) = client.session_new(cwd.path()).await.unwrap();
        client.prompt(&sid, "fix add").await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

        let guard = db.lock();
        let events = Journal::new(&guard).list_since(0, 100).unwrap();
        assert!(events.len() >= 2, "expected journaled ACP events, got {events:?}");
        // Monotonic seq
        for w in events.windows(2) {
            assert!(w[0].seq < w[1].seq);
        }
        let kinds: Vec<_> = events.iter().map(|e| e.kind.as_str()).collect();
        assert!(
            kinds.iter().any(|k| *k == "session_update"),
            "missing session_update: {kinds:?}"
        );
        assert!(
            kinds.iter().any(|k| *k == "permission_requested"),
            "missing permission_requested: {kinds:?}"
        );
        assert!(
            kinds.iter().any(|k| *k == "permission_resolved"),
            "missing permission_resolved: {kinds:?}"
        );
        // Payload of a session_update should be a normalized event kind
        let su = events
            .iter()
            .find(|e| e.kind == "session_update")
            .unwrap();
        let nk = su.payload.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        assert!(
            matches!(nk, "thought" | "message" | "tool_call" | "tool_call_update"),
            "unexpected normalized kind {nk}"
        );
        client.kill().await.unwrap();
    }

    #[tokio::test]
    async fn l0_rule_is_prefixed_into_outgoing_prompt() {
        let mock = mock_path();
        let cwd = tempdir().unwrap();
        let db = Arc::new(ParkingMutex::new(Db::open_in_memory().unwrap()));
        let rule = "只读规则：修改代码前必须先写失败测试。";
        let prompt = {
            let guard = db.lock();
            PermissionStore::new(&guard)
                .remember("edit", true)
                .unwrap();
            MemoryStore::new(&guard).add_l0_rule(rule).unwrap();
            let (combined, texts) = MemoryStore::new(&guard)
                .inject_l0_into_prompt("fix add")
                .unwrap();
            assert_eq!(texts[0], rule);
            assert!(combined.contains(rule));
            combined
        };
        let collected = Arc::new(ParkingMutex::new(Vec::new()));
        let sink_c = collected.clone();
        let sink: EventSink = Arc::new(move |ev| {
            sink_c.lock().push(ev);
        });
        let client = AcpClient::spawn(
            "python3",
            &[mock.to_str().unwrap()],
            cwd.path(),
            vec![],
            AcpSpawnOpts {
                event_sink: Some(sink),
                db: Some(db),
            },
        )
        .await
        .unwrap();
        let (sid, _) = client.session_new(cwd.path()).await.unwrap();
        client.prompt(&sid, &prompt).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let events = collected.lock().clone();
        let saw = events.iter().any(|e| match e {
            NormalizedEvent::Message { text, .. } => text.contains(rule),
            _ => false,
        });
        assert!(
            saw,
            "mock echoes prompt text; outgoing L0 must appear in agent message: {events:?}"
        );
        client.kill().await.unwrap();
    }
}
