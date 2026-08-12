use crate::db::Db;
use crate::error::{CoreError, Result};
use crate::journal::Journal;
use crate::types::JournalKind;
use chrono::Utc;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct L0Rule {
    pub id: String,
    pub content: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct L1Fact {
    pub id: String,
    pub content: String,
    pub source: Option<String>,
    pub invalid_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proposal {
    pub id: String,
    pub kind: String,
    pub content: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct L2Bullet {
    pub id: String,
    pub proposal_id: Option<String>,
    pub content: String,
}

/// L0 user rules vs L1 mock facts stay separate tables on purpose.
/// Mixing them would let "invalidate memory" silently drop standing user rules.
pub struct MemoryStore<'a> {
    db: &'a Db,
}

impl<'a> MemoryStore<'a> {
    pub fn new(db: &'a Db) -> Self {
        Self { db }
    }

    pub fn add_l0_rule(&self, content: &str) -> Result<L0Rule> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        self.db.conn().execute(
            "INSERT INTO l0_rules(id, content, enabled, created_at) VALUES (?1,?2,1,?3)",
            params![id, content, now],
        )?;
        Ok(L0Rule {
            id,
            content: content.to_string(),
            enabled: true,
        })
    }

    pub fn list_l0(&self) -> Result<Vec<L0Rule>> {
        let mut stmt = self
            .db
            .conn()
            .prepare("SELECT id, content, enabled FROM l0_rules ORDER BY created_at")?;
        let rows = stmt.query_map([], |r| {
            Ok(L0Rule {
                id: r.get(0)?,
                content: r.get(1)?,
                enabled: r.get::<_, i64>(2)? == 1,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Inject enabled L0 rules verbatim into a prompt preamble.
    /// Journal stores the exact text so audits can diff against what the agent saw.
    pub fn inject_l0_into_prompt(&self, user_prompt: &str) -> Result<(String, Vec<String>)> {
        let rules = self.list_l0()?;
        let enabled: Vec<_> = rules.into_iter().filter(|r| r.enabled).collect();
        let mut blocks = Vec::new();
        let mut ids = Vec::new();
        for r in &enabled {
            blocks.push(format!("[L0 RULE]\n{}", r.content));
            ids.push(r.id.clone());
        }
        let combined = if blocks.is_empty() {
            user_prompt.to_string()
        } else {
            format!("{}\n\n{}", blocks.join("\n\n"), user_prompt)
        };
        Journal::new(self.db).append(
            JournalKind::RulesInjected,
            None,
            None,
            json!({
                "rule_ids": ids,
                "injected_text": blocks,
            }),
        )?;
        Ok((combined, enabled.into_iter().map(|r| r.content).collect()))
    }

    pub fn add_l1_fact(&self, content: &str, source: Option<&str>) -> Result<L1Fact> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        self.db.conn().execute(
            "INSERT INTO l1_facts_mock(id, content, source, created_at, invalid_at) VALUES (?1,?2,?3,?4,NULL)",
            params![id, content, source, now],
        )?;
        Ok(L1Fact {
            id,
            content: content.to_string(),
            source: source.map(|s| s.to_string()),
            invalid_at: None,
        })
    }

    /// Soft-invalidate only. Physical delete would break the demo requirement
    /// that rows remain after invalidate.
    pub fn invalidate_l1(&self, id: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let n = self.db.conn().execute(
            "UPDATE l1_facts_mock SET invalid_at = ?1 WHERE id = ?2 AND invalid_at IS NULL",
            params![now, id],
        )?;
        if n == 0 {
            return Err(CoreError::NotFound(format!("l1 fact {id}")));
        }
        Journal::new(self.db).append(
            JournalKind::MemoryInvalidated,
            None,
            None,
            json!({ "fact_id": id, "invalid_at": now }),
        )?;
        Ok(())
    }

    pub fn list_l1(&self, include_invalid: bool) -> Result<Vec<L1Fact>> {
        let sql = if include_invalid {
            "SELECT id, content, source, invalid_at FROM l1_facts_mock ORDER BY created_at"
        } else {
            "SELECT id, content, source, invalid_at FROM l1_facts_mock WHERE invalid_at IS NULL ORDER BY created_at"
        };
        let mut stmt = self.db.conn().prepare(sql)?;
        let rows = stmt.query_map([], |r| {
            Ok(L1Fact {
                id: r.get(0)?,
                content: r.get(1)?,
                source: r.get(2)?,
                invalid_at: r.get(3)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn create_proposal(&self, kind: &str, content: &str) -> Result<Proposal> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        self.db.conn().execute(
            "INSERT INTO proposals(id, kind, content, status, created_at) VALUES (?1,?2,?3,'pending',?4)",
            params![id, kind, content, now],
        )?;
        Journal::new(self.db).append(
            JournalKind::ProposalCreated,
            None,
            None,
            json!({ "proposal_id": id, "kind": kind, "content": content }),
        )?;
        Ok(Proposal {
            id,
            kind: kind.to_string(),
            content: content.to_string(),
            status: "pending".into(),
        })
    }

    /// ACE-style: human must approve before content lands in L2.
    pub fn approve_proposal(&self, id: &str) -> Result<L2Bullet> {
        let content: String = self.db.conn().query_row(
            "SELECT content FROM proposals WHERE id=?1 AND status='pending'",
            params![id],
            |r| r.get(0),
        )?;
        let now = Utc::now().to_rfc3339();
        self.db.conn().execute(
            "UPDATE proposals SET status='approved', resolved_at=?1 WHERE id=?2",
            params![now, id],
        )?;
        let bullet_id = Uuid::new_v4().to_string();
        self.db.conn().execute(
            "INSERT INTO l2_bullets(id, proposal_id, content, created_at) VALUES (?1,?2,?3,?4)",
            params![bullet_id, id, content, now],
        )?;
        Journal::new(self.db).append(
            JournalKind::ProposalApproved,
            None,
            None,
            json!({ "proposal_id": id, "l2_id": bullet_id }),
        )?;
        Ok(L2Bullet {
            id: bullet_id,
            proposal_id: Some(id.to_string()),
            content,
        })
    }

    pub fn reject_proposal(&self, id: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.db.conn().execute(
            "UPDATE proposals SET status='rejected', resolved_at=?1 WHERE id=?2",
            params![now, id],
        )?;
        Journal::new(self.db).append(
            JournalKind::ProposalRejected,
            None,
            None,
            json!({ "proposal_id": id }),
        )?;
        Ok(())
    }

    pub fn list_proposals(&self, status: Option<&str>) -> Result<Vec<Proposal>> {
        let mut out = Vec::new();
        if let Some(st) = status {
            let mut stmt = self.db.conn().prepare(
                "SELECT id, kind, content, status FROM proposals WHERE status=?1 ORDER BY created_at",
            )?;
            for r in stmt.query_map(params![st], |r| {
                Ok(Proposal {
                    id: r.get(0)?,
                    kind: r.get(1)?,
                    content: r.get(2)?,
                    status: r.get(3)?,
                })
            })? {
                out.push(r?);
            }
        } else {
            let mut stmt = self
                .db
                .conn()
                .prepare("SELECT id, kind, content, status FROM proposals ORDER BY created_at")?;
            for r in stmt.query_map([], |r| {
                Ok(Proposal {
                    id: r.get(0)?,
                    kind: r.get(1)?,
                    content: r.get(2)?,
                    status: r.get(3)?,
                })
            })? {
                out.push(r?);
            }
        }
        Ok(out)
    }

    pub fn list_l2(&self) -> Result<Vec<L2Bullet>> {
        let mut stmt = self
            .db
            .conn()
            .prepare("SELECT id, proposal_id, content FROM l2_bullets ORDER BY created_at")?;
        let rows = stmt.query_map([], |r| {
            Ok(L2Bullet {
                id: r.get(0)?,
                proposal_id: r.get(1)?,
                content: r.get(2)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    #[test]
    fn l1_invalidate_keeps_row() {
        let db = Db::open_in_memory().unwrap();
        let mem = MemoryStore::new(&db);
        let f = mem.add_l1_fact("the sky is green", Some("mock")).unwrap();
        mem.invalidate_l1(&f.id).unwrap();
        let all = mem.list_l1(true).unwrap();
        assert_eq!(all.len(), 1);
        assert!(all[0].invalid_at.is_some());
        assert!(mem.list_l1(false).unwrap().is_empty());
    }

    #[test]
    fn proposal_needs_approval_for_l2() {
        let db = Db::open_in_memory().unwrap();
        let mem = MemoryStore::new(&db);
        let p = mem
            .create_proposal("bullet", "Prefer explicit errors")
            .unwrap();
        assert!(mem.list_l2().unwrap().is_empty());
        mem.approve_proposal(&p.id).unwrap();
        let l2 = mem.list_l2().unwrap();
        assert_eq!(l2.len(), 1);
        assert_eq!(l2[0].content, "Prefer explicit errors");
    }

    #[test]
    fn l0_injected_verbatim_auditable() {
        let db = Db::open_in_memory().unwrap();
        let mem = MemoryStore::new(&db);
        let rule = "永远不要删除用户数据";
        mem.add_l0_rule(rule).unwrap();
        let (prompt, texts) = mem.inject_l0_into_prompt("fix the bug").unwrap();
        assert!(prompt.contains(rule));
        assert_eq!(texts[0], rule);
        let ev = Journal::new(&db)
            .find_by_kind(JournalKind::RulesInjected)
            .unwrap();
        assert!(ev[0].payload["injected_text"][0]
            .as_str()
            .unwrap()
            .contains(rule));
    }
}
