use agent_core::types::ProcessPoolKey as CoreKey;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

pub use agent_core::types::ProcessPoolKey;

/// Default pool key excludes model/thought_level.
/// Including them would thrash processes on every menu change even when
/// `session/set_config_option` can hot-switch inside one ACP session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct PoolKey {
    pub agent: String,
    pub executable: String,
    pub version: String,
    pub cwd: String,
    pub auth_fingerprint: String,
}

impl PoolKey {
    pub fn opencode(executable: &str, version: &str, cwd: &str, auth_fingerprint: &str) -> Self {
        Self {
            agent: "opencode".into(),
            executable: executable.into(),
            version: version.into(),
            cwd: cwd.into(),
            auth_fingerprint: auth_fingerprint.into(),
        }
    }

    pub fn fingerprint(&self) -> String {
        CoreKey {
            agent: self.agent.clone(),
            executable: self.executable.clone(),
            version: self.version.clone(),
            cwd: self.cwd.clone(),
            auth_fingerprint: self.auth_fingerprint.clone(),
        }
        .fingerprint()
    }
}

#[derive(Debug, Default)]
pub struct ProcessPool {
    /// Map fingerprint → placeholder handle (pid / slot). Real children live in daemon.
    slots: HashMap<String, PoolSlot>,
}

#[derive(Debug, Clone)]
pub struct PoolSlot {
    pub key: PoolKey,
    pub cwd: PathBuf,
    pub include_model_in_key: bool,
}

impl ProcessPool {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn acquire(&mut self, key: PoolKey) -> &PoolSlot {
        let fp = key.fingerprint();
        self.slots.entry(fp).or_insert_with(|| PoolSlot {
            cwd: PathBuf::from(&key.cwd),
            key,
            // Only flip true if probing proves model can solely be injected at spawn
            // via OPENCODE_CONFIG_CONTENT — never via nonexistent `acp --model`.
            include_model_in_key: false,
        })
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_not_in_default_key() {
        let a = PoolKey::opencode("opencode", "1.0", "/tmp/p", "auth");
        let b = PoolKey::opencode("opencode", "1.0", "/tmp/p", "auth");
        assert_eq!(a.fingerprint(), b.fingerprint());
        // Different cwd → different slot (worktree isolation)
        let c = PoolKey::opencode("opencode", "1.0", "/tmp/other", "auth");
        assert_ne!(a.fingerprint(), c.fingerprint());
    }
}
