//! Command, dictation and agent-step history (FR-CMD-09, FR-DIC-09, FR-CU-12).

use rusqlite::params;
use serde::Serialize;

use super::{now_iso, Db};

#[derive(Debug, Clone, Serialize)]
pub struct CommandEntry {
    pub id: i64,
    pub at: String,
    pub source: String,
    pub transcript: Option<String>,
    pub tools: serde_json::Value,
    pub outcome: String,
    pub duration_ms: Option<i64>,
    pub steps: Vec<AgentStep>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentStep {
    pub step: i64,
    pub at: String,
    pub intent: Option<String>,
    pub action: serde_json::Value,
    pub safety: Option<String>,
    pub user_answer: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DictationEntry {
    pub id: i64,
    pub at: String,
    pub text: String,
}

pub fn add_command(
    db: &Db,
    source: &str,
    transcript: Option<&str>,
    tools: &serde_json::Value,
    outcome: &str,
    duration_ms: i64,
) -> rusqlite::Result<i64> {
    db.with(|c| {
        c.execute(
            "INSERT INTO command_history (at, source, transcript, tools_json, outcome, duration_ms) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![now_iso(), source, transcript, tools.to_string(), outcome, duration_ms],
        )?;
        Ok(c.last_insert_rowid())
    })
}

pub fn finish_command(db: &Db, id: i64, outcome: &str, duration_ms: i64) -> rusqlite::Result<()> {
    db.with(|c| {
        c.execute(
            "UPDATE command_history SET outcome = ?1, duration_ms = ?2 WHERE id = ?3",
            params![outcome, duration_ms, id],
        )
        .map(|_| ())
    })
}

#[allow(clippy::too_many_arguments)]
pub fn add_step(
    db: &Db,
    history_id: i64,
    step: i64,
    intent: Option<&str>,
    action: &serde_json::Value,
    safety: Option<&str>,
    user_answer: Option<&str>,
) -> rusqlite::Result<()> {
    db.with(|c| {
        c.execute(
            "INSERT INTO agent_steps (history_id, at, step, intent, action_json, safety, user_answer) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![history_id, now_iso(), step, intent, action.to_string(), safety, user_answer],
        )
        .map(|_| ())
    })
}

pub fn commands(db: &Db, limit: i64) -> rusqlite::Result<Vec<CommandEntry>> {
    db.with(|c| {
        let mut st = c.prepare(
            "SELECT id, at, source, transcript, tools_json, outcome, duration_ms \
             FROM command_history ORDER BY at DESC, id DESC LIMIT ?1",
        )?;
        let mut entries: Vec<CommandEntry> = st
            .query_map([limit], |r| {
                let tools: Option<String> = r.get(4)?;
                Ok(CommandEntry {
                    id: r.get(0)?,
                    at: r.get(1)?,
                    source: r.get(2)?,
                    transcript: r.get(3)?,
                    tools: tools
                        .and_then(|t| serde_json::from_str(&t).ok())
                        .unwrap_or(serde_json::Value::Null),
                    outcome: r.get(5)?,
                    duration_ms: r.get(6)?,
                    steps: vec![],
                })
            })?
            .collect::<Result<_, _>>()?;
        let mut st = c.prepare(
            "SELECT step, at, intent, action_json, safety, user_answer FROM agent_steps \
             WHERE history_id = ?1 ORDER BY step",
        )?;
        for e in entries.iter_mut() {
            e.steps = st
                .query_map([e.id], |r| {
                    let a: String = r.get(3)?;
                    Ok(AgentStep {
                        step: r.get(0)?,
                        at: r.get(1)?,
                        intent: r.get(2)?,
                        action: serde_json::from_str(&a).unwrap_or(serde_json::Value::Null),
                        safety: r.get(4)?,
                        user_answer: r.get(5)?,
                    })
                })?
                .collect::<Result<_, _>>()?;
        }
        Ok(entries)
    })
}

pub fn add_dictation(db: &Db, text: &str) -> rusqlite::Result<()> {
    db.with(|c| {
        let tx = c.transaction()?;
        tx.execute(
            "INSERT INTO dictation_history (at, text) VALUES (?1, ?2)",
            params![now_iso(), text],
        )?;
        tx.execute(
            "DELETE FROM dictation_history WHERE id NOT IN \
             (SELECT id FROM dictation_history ORDER BY at DESC, id DESC LIMIT 50)",
            [],
        )?;
        tx.commit()
    })
}

pub fn dictations(db: &Db) -> rusqlite::Result<Vec<DictationEntry>> {
    let cutoff = (chrono::Utc::now() - chrono::Duration::days(7)).to_rfc3339();
    db.with(|c| {
        let mut st = c.prepare(
            "SELECT id, at, text FROM dictation_history WHERE at >= ?1 ORDER BY at DESC, id DESC LIMIT 50",
        )?;
        let rows = st.query_map([cutoff], |r| Ok(DictationEntry { id: r.get(0)?, at: r.get(1)?, text: r.get(2)? }))?;
        rows.collect()
    })
}

pub fn dictation_text(db: &Db, id: i64) -> rusqlite::Result<String> {
    db.with(|c| {
        c.query_row(
            "SELECT text FROM dictation_history WHERE id = ?1",
            [id],
            |r| r.get(0),
        )
    })
}

pub fn clear_dictations(db: &Db) -> rusqlite::Result<()> {
    db.with(|c| c.execute("DELETE FROM dictation_history", []).map(|_| ()))
}
