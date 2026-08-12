//! Path containment helpers.
//!
//! Always canonicalize/realpath before containment checks. Symlink tricks
//! otherwise let a worktree write outside the repo root.

use crate::error::{CoreError, Result};
use std::path::{Path, PathBuf};

/// Resolve to an absolute canonical path. Creates nothing.
pub fn canonicalize_existing(path: &Path) -> Result<PathBuf> {
    path.canonicalize().map_err(|e| {
        CoreError::Other(format!(
            "canonicalize failed for {}: {e}",
            path.display()
        ))
    })
}

/// Canonicalize parent then join filename when the final component does not exist yet.
pub fn canonicalize_for_create(path: &Path) -> Result<PathBuf> {
    if path.exists() {
        return canonicalize_existing(path);
    }
    let parent = path.parent().ok_or_else(|| {
        CoreError::Other(format!("no parent for {}", path.display()))
    })?;
    let file = path.file_name().ok_or_else(|| {
        CoreError::Other(format!("no file name for {}", path.display()))
    })?;
    let parent = if parent.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        parent.to_path_buf()
    };
    if !parent.exists() {
        std::fs::create_dir_all(&parent)?;
    }
    let parent = canonicalize_existing(&parent)?;
    Ok(parent.join(file))
}

/// Return true iff `child` is equal to or nested under `root` after realpath.
pub fn is_contained(root: &Path, child: &Path) -> Result<bool> {
    let root = canonicalize_existing(root)?;
    let child = if child.exists() {
        canonicalize_existing(child)?
    } else {
        canonicalize_for_create(child)?
    };
    Ok(child.starts_with(&root))
}

pub fn assert_contained(root: &Path, child: &Path) -> Result<()> {
    if is_contained(root, child)? {
        Ok(())
    } else {
        Err(CoreError::PathEscape {
            path: child.display().to_string(),
            root: root.display().to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use tempfile::tempdir;

    #[test]
    fn rejects_symlink_escape() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("repo");
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let link = root.join("escape");
        symlink(&outside, &link).unwrap();
        // realpath of escape resolves outside — containment must fail for writes via link target
        let target_file = outside.join("secret.txt");
        std::fs::write(&target_file, "x").unwrap();
        let via_link = link.join("secret.txt");
        let ok = is_contained(&root, &via_link).unwrap();
        assert!(!ok, "symlink escape must not count as contained");
    }

    #[test]
    fn accepts_nested_path() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("repo");
        let nested = root.join("a/b");
        std::fs::create_dir_all(&nested).unwrap();
        assert!(is_contained(&root, &nested.join("c.txt")).unwrap());
    }
}
