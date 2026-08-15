//! OpenCode ACP client (stdio JSON-RPC).
//!
//! We talk to `opencode acp` only — scraping the TUI would break on every
//! theme/layout change and cannot carry permission/tool structured events.

pub mod client;
pub mod normalize;
pub mod pool;
pub mod types;

pub use client::{AcpClient, AcpSpawnOpts, EventSink, SharedDb};
pub use normalize::{normalize_session_update, NormalizedEvent};
pub use pool::{ProcessPool, ProcessPoolKey};
pub use types::*;
