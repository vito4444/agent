use thiserror::Error;

pub type Result<T> = std::result::Result<T, CoreError>;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("json: {0}")]
    Json(#[from] serde_json::Error),

    #[error("yaml: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("path escape: {path} is outside root {root}")]
    PathEscape { path: String, root: String },

    /// Producer or consumer missing a declared artifact.
    /// `path` must appear in Display so operators know which file was expected —
    /// without it, fake-green edges are undiagnosable.
    #[error(
        "missing artifact: task `{task_id}` artifact `{artifact_id}` path `{path}` (from `{from_task}`)"
    )]
    MissingArtifact {
        task_id: String,
        artifact_id: String,
        from_task: String,
        path: String,
    },

    #[error("gate failed: task `{task_id}` command `{command}` exited {exit_code}")]
    GateFailed {
        task_id: String,
        command: String,
        exit_code: i32,
    },

    #[error("worktree conflict: branch `{branch}` already checked out at `{existing}`")]
    WorktreeConflict { branch: String, existing: String },

    #[error("orphan worktree detected: {path}")]
    OrphanWorktree { path: String },

    #[error("invalid graph: {0}")]
    InvalidGraph(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("model switch denied: {0}")]
    ModelSwitchDenied(String),

    #[error("{0}")]
    Other(String),
}
