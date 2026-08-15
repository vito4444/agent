//! Multi-agent local workbench core.
//!
//! Orchestration durability lives in a self-written SQLite journal.
//! Using an external workflow engine would hide dependency/artifact
//! failures behind opaque retries — V0 needs hard-fail semantics we can audit.

pub mod db;
pub mod error;
pub mod gate;
pub mod graph;
pub mod journal;
pub mod memory;
pub mod merge;
pub mod pathutil;
pub mod permissions;
pub mod schema;
pub mod scheduler;
pub mod types;
pub mod worktree;

pub use error::{CoreError, Result};
pub use types::*;
