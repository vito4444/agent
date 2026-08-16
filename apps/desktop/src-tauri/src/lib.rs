use agent_acp::{AdvertisedMenus, NormalizedEvent};
use agent_daemon::{StartupBanner, Workbench};
use parking_lot::Mutex;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;

pub struct AppState {
    pub wb: Mutex<Option<Workbench>>,
    pub transcript: Mutex<Vec<NormalizedEvent>>,
    pub banners: Mutex<Vec<StartupBanner>>,
    pub thought_overridden: Mutex<bool>,
}

#[derive(Serialize)]
pub struct UiBootstrap {
    pub banners: Vec<StartupBanner>,
    pub demo_yaml: String,
    pub opencode_available: bool,
    pub transcript: Vec<NormalizedEvent>,
    pub repo_root: String,
}

#[tauri::command]
fn bootstrap(state: State<Arc<AppState>>) -> Result<UiBootstrap, String> {
    let mut guard = state.wb.lock();
    if guard.is_none() {
        let data = dirs_data();
        let repo = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        match Workbench::open(&data, &repo) {
            Ok(wb) => {
                *state.banners.lock() = wb.state.banners.clone();
                // Fixture replay so UI always has a transcript without OpenCode.
                if let Ok(evs) = wb.replay_fixture_transcript() {
                    *state.transcript.lock() = evs;
                }
                let _ = wb.seed_demo_memory();
                *guard = Some(wb);
            }
            Err(e) => {
                state.banners.lock().push(StartupBanner {
                    level: "error".into(),
                    message: format!("workbench open failed: {e}"),
                });
            }
        }
    }
    let banners = state.banners.lock().clone();
    let transcript = state.transcript.lock().clone();
    let (demo_yaml, opencode_available, repo_root) = if let Some(wb) = guard.as_ref() {
        (
            wb.demo_yaml().to_string(),
            wb.state.opencode_available,
            wb.state.repo_root.clone(),
        )
    } else {
        (
            agent_core::graph::demo_two_task_yaml().to_string(),
            false,
            String::new(),
        )
    };
    Ok(UiBootstrap {
        banners,
        demo_yaml,
        opencode_available,
        transcript,
        repo_root,
    })
}

#[tauri::command]
fn list_events(state: State<Arc<AppState>>, since: i64) -> Result<serde_json::Value, String> {
    let guard = state.wb.lock();
    let wb = guard.as_ref().ok_or("workbench not open")?;
    wb.events_since(since).map_err(|e| e.to_string())
}

#[tauri::command]
fn run_mock_graph(state: State<Arc<AppState>>, yaml: String) -> Result<serde_json::Value, String> {
    let guard = state.wb.lock();
    let wb = guard.as_ref().ok_or("workbench not open")?;
    wb.accept_and_run_mock(&yaml).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_transcript(state: State<Arc<AppState>>) -> Vec<NormalizedEvent> {
    state.transcript.lock().clone()
}

#[tauri::command]
fn set_thought_overridden(state: State<Arc<AppState>>, overridden: bool) {
    *state.thought_overridden.lock() = overridden;
}

#[tauri::command]
fn memory_snapshot(state: State<Arc<AppState>>) -> Result<serde_json::Value, String> {
    let guard = state.wb.lock();
    let wb = guard.as_ref().ok_or("workbench not open")?;
    let mem = wb.memory();
    Ok(serde_json::json!({
        "l0": mem.list_l0().map_err(|e| e.to_string())?,
        "l1": mem.list_l1(true).map_err(|e| e.to_string())?,
        "proposals": mem.list_proposals(None).map_err(|e| e.to_string())?,
        "l2": mem.list_l2().map_err(|e| e.to_string())?,
    }))
}

#[tauri::command]
fn approve_proposal(state: State<Arc<AppState>>, id: String) -> Result<serde_json::Value, String> {
    let guard = state.wb.lock();
    let wb = guard.as_ref().ok_or("workbench not open")?;
    let b = wb.memory().approve_proposal(&id).map_err(|e| e.to_string())?;
    Ok(serde_json::to_value(b).unwrap())
}

#[tauri::command]
fn invalidate_l1(state: State<Arc<AppState>>, id: String) -> Result<(), String> {
    let guard = state.wb.lock();
    let wb = guard.as_ref().ok_or("workbench not open")?;
    wb.memory().invalidate_l1(&id).map_err(|e| e.to_string())
}

#[tauri::command]
fn advertised_menus_fixture() -> AdvertisedMenus {
    // Honest empty when no live session — UI must not paint fake model menus.
    AdvertisedMenus::default()
}

fn dirs_data() -> PathBuf {
    let base = std::env::var("AGENT_WORKBENCH_DATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs_next_data().unwrap_or_else(|| PathBuf::from(".agent-workbench"))
        });
    base
}

fn dirs_next_data() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share/agent-workbench"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let state = Arc::new(AppState {
        wb: Mutex::new(None),
        transcript: Mutex::new(Vec::new()),
        banners: Mutex::new(Vec::new()),
        thought_overridden: Mutex::new(false),
    });
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            bootstrap,
            list_events,
            run_mock_graph,
            get_transcript,
            set_thought_overridden,
            memory_snapshot,
            approve_proposal,
            invalidate_l1,
            advertised_menus_fixture,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
