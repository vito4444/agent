use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Journal event kinds — semantic names must stay stable across UI/daemon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JournalKind {
    RunCreated,
    GraphAccepted,
    TaskReady,
    WorktreeCreated,
    WorktreeBootstrapped,
    WorktreeRemoved,
    AgentSpawned,
    SessionCreated,
    SessionClosed,
    PromptStarted,
    SessionUpdate,
    PermissionRequested,
    PermissionResolved,
    GateStarted,
    GatePassed,
    GateFailed,
    ArtifactRecorded,
    MergeQueued,
    MergeCompleted,
    MergeRejected,
    TaskCancelled,
    ProcessCrashed,
    OrphanDetected,
    ProposalCreated,
    ProposalApproved,
    ProposalRejected,
    RulesInjected,
    MemoryInvalidated,
    ModelSwitchDenied,
    SessionForkedSummary,
}

impl JournalKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RunCreated => "run_created",
            Self::GraphAccepted => "graph_accepted",
            Self::TaskReady => "task_ready",
            Self::WorktreeCreated => "worktree_created",
            Self::WorktreeBootstrapped => "worktree_bootstrapped",
            Self::WorktreeRemoved => "worktree_removed",
            Self::AgentSpawned => "agent_spawned",
            Self::SessionCreated => "session_created",
            Self::SessionClosed => "session_closed",
            Self::PromptStarted => "prompt_started",
            Self::SessionUpdate => "session_update",
            Self::PermissionRequested => "permission_requested",
            Self::PermissionResolved => "permission_resolved",
            Self::GateStarted => "gate_started",
            Self::GatePassed => "gate_passed",
            Self::GateFailed => "gate_failed",
            Self::ArtifactRecorded => "artifact_recorded",
            Self::MergeQueued => "merge_queued",
            Self::MergeCompleted => "merge_completed",
            Self::MergeRejected => "merge_rejected",
            Self::TaskCancelled => "task_cancelled",
            Self::ProcessCrashed => "process_crashed",
            Self::OrphanDetected => "orphan_detected",
            Self::ProposalCreated => "proposal_created",
            Self::ProposalApproved => "proposal_approved",
            Self::ProposalRejected => "proposal_rejected",
            Self::RulesInjected => "rules_injected",
            Self::MemoryInvalidated => "memory_invalidated",
            Self::ModelSwitchDenied => "model_switch_denied",
            Self::SessionForkedSummary => "session_forked_summary",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "run_created" => Self::RunCreated,
            "graph_accepted" => Self::GraphAccepted,
            "task_ready" => Self::TaskReady,
            "worktree_created" => Self::WorktreeCreated,
            "worktree_bootstrapped" => Self::WorktreeBootstrapped,
            "worktree_removed" => Self::WorktreeRemoved,
            "agent_spawned" => Self::AgentSpawned,
            "session_created" => Self::SessionCreated,
            "session_closed" => Self::SessionClosed,
            "prompt_started" => Self::PromptStarted,
            "session_update" => Self::SessionUpdate,
            "permission_requested" => Self::PermissionRequested,
            "permission_resolved" => Self::PermissionResolved,
            "gate_started" => Self::GateStarted,
            "gate_passed" => Self::GatePassed,
            "gate_failed" => Self::GateFailed,
            "artifact_recorded" => Self::ArtifactRecorded,
            "merge_queued" => Self::MergeQueued,
            "merge_completed" => Self::MergeCompleted,
            "merge_rejected" => Self::MergeRejected,
            "task_cancelled" => Self::TaskCancelled,
            "process_crashed" => Self::ProcessCrashed,
            "orphan_detected" => Self::OrphanDetected,
            "proposal_created" => Self::ProposalCreated,
            "proposal_approved" => Self::ProposalApproved,
            "proposal_rejected" => Self::ProposalRejected,
            "rules_injected" => Self::RulesInjected,
            "memory_invalidated" => Self::MemoryInvalidated,
            "model_switch_denied" => Self::ModelSwitchDenied,
            "session_forked_summary" => Self::SessionForkedSummary,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEvent {
    pub seq: i64,
    pub kind: String,
    pub run_id: Option<String>,
    pub task_id: Option<String>,
    pub payload: Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    Ready,
    Running,
    Gate,
    Merging,
    Done,
    Failed,
    Cancelled,
    Blocked,
}

impl TaskStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Ready => "ready",
            Self::Running => "running",
            Self::Gate => "gate",
            Self::Merging => "merging",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Blocked => "blocked",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "pending" => Self::Pending,
            "ready" => Self::Ready,
            "running" => Self::Running,
            "gate" => Self::Gate,
            "merging" => Self::Merging,
            "done" => Self::Done,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            "blocked" => Self::Blocked,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactRef {
    pub id: String,
    pub path: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDepEdge {
    pub from: String,
    pub to: String,
    /// Declared artifacts the downstream task needs from upstream.
    /// Missing any of these is a hard failure — fake edges must not silently succeed.
    pub artifacts: Vec<ArtifactRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSpec {
    pub id: String,
    pub title: String,
    pub prompt: String,
    #[serde(default)]
    pub gate_command: Option<String>,
    #[serde(default)]
    pub produces: Vec<ArtifactRef>,
    /// When true, allow in-place cwd (UI must show ⚠ 原地·非隔离).
    #[serde(default)]
    pub allow_inplace: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskGraph {
    pub name: String,
    pub tasks: Vec<TaskSpec>,
    pub deps: Vec<TaskDepEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessPoolKey {
    pub agent: String,
    pub executable: String,
    pub version: String,
    pub cwd: String,
    pub auth_fingerprint: String,
}

impl ProcessPoolKey {
    pub fn fingerprint(&self) -> String {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(self.agent.as_bytes());
        h.update(b"|");
        h.update(self.executable.as_bytes());
        h.update(b"|");
        h.update(self.version.as_bytes());
        h.update(b"|");
        h.update(self.cwd.as_bytes());
        h.update(b"|");
        h.update(self.auth_fingerprint.as_bytes());
        hex::encode(h.finalize())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigOptionAdvertisement {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(rename = "type")]
    pub option_type: String,
    #[serde(default, rename = "currentValue")]
    pub current_value: Option<Value>,
    #[serde(default)]
    pub options: Vec<ConfigOptionValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigOptionValue {
    pub value: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
}
