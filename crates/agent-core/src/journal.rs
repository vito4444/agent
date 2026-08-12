use crate::db::Db;
use crate::error::Result;
use crate::types::{JournalEvent, JournalKind};
use chrono::Utc;
use rusqlite::params;
use serde_json::Value;

/// Append-only journal. UI and audit both consume `events.seq` order.
/// We never rewrite history — corrections are new events.
pub struct Journal<'a> {
    db: &'a Db,
}

impl<'a> Journal<'a> {
    pub fn new(db: &'a Db) -> Self {
        Self { db }
    }

    pub fn append(
        &self,
        kind: JournalKind,
        run_id: Option<&str>,
        task_id: Option<&str>,
        payload: Value,
    ) -> Result<JournalEvent> {
        let now = Utc::now().to_rfc3339();
        let payload_json = serde_json::to_string(&payload)?;
        self.db.conn().execute(
            "INSERT INTO events(kind, run_id, task_id, payload_json, created_at) VALUES (?1,?2,?3,?4,?5)",
            params![kind.as_str(), run_id, task_id, payload_json, now],
        )?;
        let seq = self.db.conn().last_insert_rowid();
        // Mirror into seq_counters for callers that peek without reading events.
        self.db.conn().execute(
            "UPDATE seq_counters SET value = ?1 WHERE name = 'event'",
            params![seq],
        )?;
        Ok(JournalEvent {
            seq,
            kind: kind.as_str().to_string(),
            run_id: run_id.map(|s| s.to_string()),
            task_id: task_id.map(|s| s.to_string()),
            payload,
            created_at: Utc::now(),
        })
    }

    pub fn list_since(&self, after_seq: i64, limit: usize) -> Result<Vec<JournalEvent>> {
        let mut stmt = self.db.conn().prepare(
            "SELECT seq, kind, run_id, task_id, payload_json, created_at FROM events WHERE seq > ?1 ORDER BY seq ASC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![after_seq, limit as i64], |row| {
            let payload_json: String = row.get(4)?;
            let created_at: String = row.get(5)?;
            Ok(JournalEvent {
                seq: row.get(0)?,
                kind: row.get(1)?,
                run_id: row.get(2)?,
                task_id: row.get(3)?,
                payload: serde_json::from_str(&payload_json).unwrap_or(Value::Null),
                created_at: created_at
                    .parse()
                    .unwrap_or_else(|_| Utc::now()),
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn latest_seq(&self) -> Result<i64> {
        let v: i64 = self.db.conn().query_row(
            "SELECT value FROM seq_counters WHERE name = 'event'",
            [],
            |r| r.get(0),
        )?;
        Ok(v)
    }

    pub fn find_by_kind(&self, kind: JournalKind) -> Result<Vec<JournalEvent>> {
        let mut stmt = self.db.conn().prepare(
            "SELECT seq, kind, run_id, task_id, payload_json, created_at FROM events WHERE kind = ?1 ORDER BY seq ASC",
        )?;
        let rows = stmt.query_map(params![kind.as_str()], |row| {
            let payload_json: String = row.get(4)?;
            let created_at: String = row.get(5)?;
            Ok(JournalEvent {
                seq: row.get(0)?,
                kind: row.get(1)?,
                run_id: row.get(2)?,
                task_id: row.get(3)?,
                payload: serde_json::from_str(&payload_json).unwrap_or(Value::Null),
                created_at: created_at
                    .parse()
                    .unwrap_or_else(|_| Utc::now()),
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use serde_json::json;

    #[test]
    fn journal_writes_monotonic_seq() {
        let db = Db::open_in_memory().unwrap();
        let j = Journal::new(&db);
        let e1 = j
            .append(JournalKind::RunCreated, Some("r1"), None, json!({"n": 1}))
            .unwrap();
        let e2 = j
            .append(
                JournalKind::GraphAccepted,
                Some("r1"),
                None,
                json!({"n": 2}),
            )
            .unwrap();
        assert_eq!(e1.seq, 1);
        assert_eq!(e2.seq, 2);
        assert_eq!(j.latest_seq().unwrap(), 2);
        let all = j.list_since(0, 10).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].kind, "run_created");
    }
}
