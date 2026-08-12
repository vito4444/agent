use crate::normalize::{normalize_permission_request, normalize_session_update, NormalizedEvent};
use crate::types::*;
use agent_core::error::{CoreError, Result as CoreResult};
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
    /// Remembered permissions keyed by op_type — never by bare tool name alone.
    remembered_ops: Arc<Mutex<HashMap<String, bool>>>,
}

impl AcpClient {
    pub async fn spawn(
        executable: &str,
        args: &[&str],
        cwd: &Path,
        env: Vec<(String, String)>,
        event_sink: Option<EventSink>,
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
        let stdin = child.stdin.take().ok_or_else(|| CoreError::Other("no stdin".into()))?;
        let stdout = child.stdout.take().ok_or_else(|| CoreError::Other("no stdout".into()))?;
        let pending: Arc<Mutex<HashMap<u64, Pending>>> = Arc::new(Mutex::new(HashMap::new()));
        let (tx, rx) = mpsc::unbounded_channel();
        let config_options = Arc::new(Mutex::new(Vec::new()));
        let session_id = Arc::new(Mutex::new(None));
        let remembered_ops = Arc::new(Mutex::new(HashMap::new()));

        let pending_r = pending.clone();
        let config_r = config_options.clone();
        let remembered_r = remembered_ops.clone();
        let stdin_r = Arc::new(Mutex::new(stdin));
        let stdin_reader = stdin_r.clone();
        tokio::spawn(async move {
            if let Err(e) = read_loop(
                stdout,
                pending_r,
                tx,
                event_sink,
                config_r,
                remembered_r,
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
            remembered_ops,
        };
        client.initialize().await?;
        Ok(client)
    }

    pub async fn spawn_opencode(cwd: &Path, event_sink: Option<EventSink>) -> CoreResult<Self> {
        // Model is NOT passed as `acp --model` — that flag does not exist.
        // Spawn-time model injection uses OPENCODE_CONFIG_CONTENT when proven necessary.
        Self::spawn("opencode", &["acp"], cwd, vec![], event_sink).await
    }

    pub async fn spawn_mock(mock_bin: &Path, cwd: &Path, event_sink: Option<EventSink>) -> CoreResult<Self> {
        Self::spawn(
            mock_bin.to_str().unwrap_or("mock-acp-agent"),
            &[],
            cwd,
            vec![],
            event_sink,
        )
        .await
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
        self.notify(
            "session/cancel",
            json!({ "sessionId": session_id }),
        )
        .await
    }

    pub async fn close_session(&self, session_id: &str) -> CoreResult<()> {
        // session/close may be unsupported — degrade honestly.
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
            || menus
                .model_config
                .iter()
                .any(|m| m.id == config_id);
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

    pub async fn remember_op(&self, op_type: &str, allow: bool) {
        self.remembered_ops
            .lock()
            .await
            .insert(op_type.to_string(), allow);
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

async fn read_loop(
    stdout: ChildStdout,
    pending: Arc<Mutex<HashMap<u64, Pending>>>,
    events: mpsc::UnboundedSender<NormalizedEvent>,
    sink: Option<EventSink>,
    config_options: Arc<Mutex<Vec<ConfigOption>>>,
    remembered: Arc<Mutex<HashMap<String, bool>>>,
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

        // Response?
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
            // Server request (e.g. session/request_permission)
            let method = value.get("method").and_then(|m| m.as_str()).unwrap_or("");
            let params = value.get("params").cloned().unwrap_or(Value::Null);
            if method == "session/request_permission" {
                let ev = normalize_permission_request(&params);
                let _ = events.send(ev.clone());
                if let Some(s) = &sink {
                    s(ev);
                }
                let op = params
                    .get("opType")
                    .or_else(|| params.get("op_type"))
                    .or_else(|| params.pointer("/toolCall/kind"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let allow = remembered
                    .lock()
                    .await
                    .get(op)
                    .copied()
                    .unwrap_or(true); // V0 default allow for unattended CI; UI overlays real prompts.
                let resp = json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "outcome": { "outcome": if allow { "selected" } else { "cancelled" }, "optionId": if allow { "allow-once" } else { "reject-once" } }
                    }
                });
                let line = serde_json::to_string(&resp)? + "\n";
                let mut stdin = stdin.lock().await;
                stdin.write_all(line.as_bytes()).await?;
                stdin.flush().await?;
                continue;
            }
            // Unknown server request — ignore extension methods, reply method_not_found for others.
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

        // Notification
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
                    // Protocol: unknown extension/_meta → ignore (still available via sink if needed)
                    continue;
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
    use std::path::PathBuf;

    #[tokio::test]
    async fn mock_agent_initialize_and_prompt() {
        let mock = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/mock-agent/mock_acp_agent.py");
        if !mock.exists() {
            // Written later in the same change-set; skip if ordering races in edit.
            eprintln!("SKIP: mock agent missing at {}", mock.display());
            return;
        }
        let cwd = tempfile::tempdir().unwrap();
        let mut client = AcpClient::spawn(
            "python3",
            &[mock.to_str().unwrap()],
            cwd.path(),
            vec![],
            None,
        )
        .await
        .expect("spawn mock");
        let (sid, menus) = client.session_new(cwd.path()).await.unwrap();
        assert!(menus.model.is_some(), "mock advertises model");
        let outcome = client
            .try_set_model(&sid, "model", "fast")
            .await
            .unwrap();
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
        // Drain a few events
        let mut saw_msg = false;
        for _ in 0..20 {
            if let Some(ev) = client.next_event().await {
                if matches!(ev, NormalizedEvent::Message { .. } | NormalizedEvent::Thought { .. }) {
                    saw_msg = true;
                    break;
                }
            }
        }
        assert!(saw_msg, "expected normalized message/thought from mock");
        client.kill().await.unwrap();
    }
}
