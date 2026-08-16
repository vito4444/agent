//! Probe for the OpenCode binary. Never invent availability.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Result of looking for `opencode` for ACP (`opencode acp`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OpenCodeProbe {
    pub available: bool,
    pub path: Option<String>,
    pub version: Option<String>,
    /// `OPENCODE_BIN` | `OPENCODE_BIN_missing` | `PATH` | `missing`
    pub source: String,
}

impl OpenCodeProbe {
    pub fn path_buf(&self) -> Option<PathBuf> {
        self.path.as_ref().map(PathBuf::from)
    }
}

/// Resolve OpenCode executable.
/// Prefer explicit `OPENCODE_BIN` (even if missing — then available=false).
/// Otherwise search PATH for a file named `opencode`.
pub fn probe_opencode() -> OpenCodeProbe {
    if let Ok(raw) = std::env::var("OPENCODE_BIN") {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return OpenCodeProbe {
                available: false,
                path: None,
                version: None,
                source: "OPENCODE_BIN_missing".into(),
            };
        }
        let path = PathBuf::from(trimmed);
        if is_executable(&path) {
            let version = try_version(&path);
            return OpenCodeProbe {
                available: true,
                path: Some(path.display().to_string()),
                version,
                source: "OPENCODE_BIN".into(),
            };
        }
        return OpenCodeProbe {
            available: false,
            path: Some(path.display().to_string()),
            version: None,
            source: "OPENCODE_BIN_missing".into(),
        };
    }

    if let Some(path) = find_on_path("opencode") {
        let version = try_version(&path);
        return OpenCodeProbe {
            available: true,
            path: Some(path.display().to_string()),
            version,
            source: "PATH".into(),
        };
    }

    OpenCodeProbe {
        available: false,
        path: None,
        version: None,
        source: "missing".into(),
    }
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&paths) {
        let candidate = dir.join(name);
        if is_executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn is_executable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn try_version(path: &Path) -> Option<String> {
    let out = Command::new(path).arg("--version").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout);
    let line = s.lines().next().unwrap_or("").trim();
    if line.is_empty() {
        None
    } else {
        Some(line.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Serialize env mutations across tests in this module.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn open_code_bin_override_missing_is_not_available() {
        let _g = ENV_LOCK.lock().unwrap();
        let prev = std::env::var_os("OPENCODE_BIN");
        std::env::set_var("OPENCODE_BIN", "/definitely/not/a/real/opencode-binary-xyz");
        let probe = probe_opencode();
        assert!(!probe.available);
        assert_eq!(probe.source, "OPENCODE_BIN_missing");
        assert!(probe.path.as_ref().unwrap().contains("opencode-binary-xyz"));
        match prev {
            Some(v) => std::env::set_var("OPENCODE_BIN", v),
            None => std::env::remove_var("OPENCODE_BIN"),
        }
    }

    #[test]
    fn open_code_bin_override_empty_is_not_available() {
        let _g = ENV_LOCK.lock().unwrap();
        let prev = std::env::var_os("OPENCODE_BIN");
        std::env::set_var("OPENCODE_BIN", "   ");
        let probe = probe_opencode();
        assert!(!probe.available);
        assert_eq!(probe.source, "OPENCODE_BIN_missing");
        match prev {
            Some(v) => std::env::set_var("OPENCODE_BIN", v),
            None => std::env::remove_var("OPENCODE_BIN"),
        }
    }
}
