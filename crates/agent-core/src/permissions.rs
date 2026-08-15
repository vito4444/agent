//! Permission decisions keyed by `op_type`, persisted in SQLite.
//!
//! Remembering by tool name alone would let an "allow edit" leak into exec.
//! An in-memory HashMap also dies on restart — the schema's `permissions`
//! table exists so decisions survive process boundaries.

use crate::db::Db;
use crate::error::Result;
use crate::journal::Journal;
use crate::types::JournalKind;
use chrono::Utc;
use rusqlite::params;
use serde_json::{json, Value};
use std::path::Path;

pub struct PermissionStore<'a> {
    db: &'a Db,
}

impl<'a> PermissionStore<'a> {
    pub fn new(db: &'a Db) -> Self {
        Self { db }
    }

    /// Latest remembered decision for this exact `op_type`, if any.
    pub fn lookup_remembered(&self, op_type: &str) -> Result<Option<bool>> {
        let mut stmt = self.db.conn().prepare(
            "SELECT status FROM permissions
             WHERE op_type = ?1 AND remembered = 1
             ORDER BY id DESC LIMIT 1",
        )?;
        let mut rows = stmt.query(params![op_type])?;
        match rows.next()? {
            Some(row) => {
                let status: String = row.get(0)?;
                Ok(Some(status == "allowed"))
            }
            None => Ok(None),
        }
    }

    /// Persist a standing remember decision for `op_type` (survives restart).
    pub fn remember(&self, op_type: &str, allow: bool) -> Result<i64> {
        let now = Utc::now().to_rfc3339();
        let status = if allow { "allowed" } else { "denied" };
        self.db.conn().execute(
            "INSERT INTO permissions(session_id, op_type, tool_call_id, status, remembered, payload_json, created_at, resolved_at)
             VALUES (NULL, ?1, NULL, ?2, 1, ?3, ?4, ?4)",
            params![
                op_type,
                status,
                json!({ "source": "remember", "op_type": op_type }).to_string(),
                now
            ],
        )?;
        Ok(self.db.conn().last_insert_rowid())
    }

    /// Record an incoming permission request. Does not grant anything.
    pub fn record_request(
        &self,
        session_id: Option<&str>,
        op_type: &str,
        tool_call_id: Option<&str>,
        payload: &Value,
    ) -> Result<i64> {
        let now = Utc::now().to_rfc3339();
        self.db.conn().execute(
            "INSERT INTO permissions(session_id, op_type, tool_call_id, status, remembered, payload_json, created_at)
             VALUES (?1, ?2, ?3, 'requested', 0, ?4, ?5)",
            params![
                session_id,
                op_type,
                tool_call_id,
                payload.to_string(),
                now
            ],
        )?;
        let id = self.db.conn().last_insert_rowid();
        Journal::new(self.db).append(
            JournalKind::PermissionRequested,
            None,
            None,
            json!({
                "permission_id": id,
                "op_type": op_type,
                "tool_call_id": tool_call_id,
                "session_id": session_id,
            }),
        )?;
        Ok(id)
    }

    /// Resolve a request. `remember=true` also writes a standing op_type decision.
    pub fn resolve(
        &self,
        permission_id: i64,
        allow: bool,
        remember: bool,
        op_type: &str,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let status = if allow { "allowed" } else { "denied" };
        self.db.conn().execute(
            "UPDATE permissions SET status = ?1, remembered = ?2, resolved_at = ?3 WHERE id = ?4",
            params![status, remember as i64, now, permission_id],
        )?;
        if remember {
            // Standing row so a fresh process can lookup_remembered(op_type).
            self.remember(op_type, allow)?;
        }
        Journal::new(self.db).append(
            JournalKind::PermissionResolved,
            None,
            None,
            json!({
                "permission_id": permission_id,
                "op_type": op_type,
                "allow": allow,
                "remembered": remember,
            }),
        )?;
        Ok(())
    }

    /// Decide how to auto-answer an ACP permission request.
    /// Unremembered ops return `None` — callers MUST NOT treat that as allow.
    pub fn decision_for(&self, op_type: &str) -> Result<Option<bool>> {
        if op_type.is_empty() {
            // Empty op_type cannot share grants with a real type; force deny path.
            return Ok(None);
        }
        self.lookup_remembered(op_type)
    }
}

/// Helper for tests / clients that own a DB path across process restarts.
pub fn open_store_at(path: &Path) -> Result<Db> {
    Db::open(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use tempfile::tempdir;

    #[test]
    fn remember_survives_new_db_handle() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("perm.sqlite");
        {
            let db = Db::open(&path).unwrap();
            let store = PermissionStore::new(&db);
            store.remember("fs_write", true).unwrap();
            assert_eq!(store.lookup_remembered("fs_write").unwrap(), Some(true));
        }
        // New process / new connection — same file.
        let db2 = Db::open(&path).unwrap();
        let store2 = PermissionStore::new(&db2);
        assert_eq!(store2.lookup_remembered("fs_write").unwrap(), Some(true));
    }

    #[test]
    fn fs_write_remember_does_not_authorize_exec() {
        let db = Db::open_in_memory().unwrap();
        let store = PermissionStore::new(&db);
        store.remember("fs_write", true).unwrap();
        assert_eq!(store.lookup_remembered("fs_write").unwrap(), Some(true));
        assert_eq!(
            store.lookup_remembered("exec").unwrap(),
            None,
            "different op_type must not inherit grant"
        );
        assert_eq!(store.decision_for("exec").unwrap(), None);
    }

    #[test]
    fn unremembered_is_not_auto_allowed() {
        let db = Db::open_in_memory().unwrap();
        let store = PermissionStore::new(&db);
        assert_eq!(store.decision_for("edit").unwrap(), None);
        let id = store
            .record_request(Some("sess"), "edit", Some("call_1"), &json!({}))
            .unwrap();
        // Explicit deny without remember — standing lookup still empty.
        store.resolve(id, false, false, "edit").unwrap();
        assert_eq!(store.lookup_remembered("edit").unwrap(), None);
        let kinds: Vec<_> = Journal::new(&db)
            .list_since(0, 20)
            .unwrap()
            .into_iter()
            .map(|e| e.kind)
            .collect();
        assert!(kinds.iter().any(|k| k == "permission_requested"));
        assert!(kinds.iter().any(|k| k == "permission_resolved"));
    }
}
