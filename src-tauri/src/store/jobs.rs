//! Scheduled jobs and their run history (section 4.6).

use rusqlite::{params, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

use super::{now_iso, Db};
use crate::scheduler::schedule::When;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: i64,
    pub name: String,
    pub command: String,
    pub when: When,
    pub enabled: bool,
    pub allow_computer_use: bool,
    pub next_run_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRun {
    pub id: i64,
    pub job_id: i64,
    pub job_name: Option<String>,
    pub due_at: String,
    pub started_at: Option<String>,
    pub status: String,
    pub summary: Option<String>,
    pub duration_ms: Option<i64>,
}

const COLS: &str =
    "id, name, command, when_json, enabled, allow_computer_use, next_run_at, created_at";

fn row(r: &Row) -> rusqlite::Result<Job> {
    let when_json: String = r.get(3)?;
    let when = serde_json::from_str(&when_json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(e))
    })?;
    Ok(Job {
        id: r.get(0)?,
        name: r.get(1)?,
        command: r.get(2)?,
        when,
        enabled: r.get::<_, i64>(4)? != 0,
        allow_computer_use: r.get::<_, i64>(5)? != 0,
        next_run_at: r.get(6)?,
        created_at: r.get(7)?,
    })
}

pub fn list(db: &Db) -> rusqlite::Result<Vec<Job>> {
    db.with(|c| {
        let mut st = c.prepare(&format!("SELECT {COLS} FROM jobs ORDER BY id"))?;
        let rows = st.query_map([], row)?;
        Ok(rows.filter_map(Result::ok).collect())
    })
}

pub fn get(db: &Db, id: i64) -> rusqlite::Result<Option<Job>> {
    db.with(|c| {
        c.query_row(&format!("SELECT {COLS} FROM jobs WHERE id = ?1"), [id], row)
            .optional()
    })
}

pub fn create(
    db: &Db,
    name: &str,
    command: &str,
    when: &When,
    allow_computer_use: bool,
    next_run_at: Option<String>,
) -> rusqlite::Result<Job> {
    let when_json = serde_json::to_string(when).expect("serializable");
    let id = db.with(|c| {
        c.execute(
            "INSERT INTO jobs (name, command, when_json, allow_computer_use, next_run_at, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![name, command, when_json, allow_computer_use as i64, next_run_at, now_iso()],
        )?;
        Ok(c.last_insert_rowid())
    })?;
    Ok(get(db, id)?.expect("just inserted"))
}

pub fn save(db: &Db, j: &Job) -> rusqlite::Result<()> {
    let when_json = serde_json::to_string(&j.when).expect("serializable");
    db.with(|c| {
        c.execute(
            "UPDATE jobs SET name = ?1, command = ?2, when_json = ?3, enabled = ?4, \
             allow_computer_use = ?5, next_run_at = ?6 WHERE id = ?7",
            params![
                j.name,
                j.command,
                when_json,
                j.enabled as i64,
                j.allow_computer_use as i64,
                j.next_run_at,
                j.id
            ],
        )
        .map(|_| ())
    })
}

pub fn set_next_run(db: &Db, id: i64, next: Option<&str>) -> rusqlite::Result<()> {
    db.with(|c| {
        c.execute(
            "UPDATE jobs SET next_run_at = ?1 WHERE id = ?2",
            params![next, id],
        )
        .map(|_| ())
    })
}

pub fn delete(db: &Db, id: i64) -> rusqlite::Result<()> {
    db.with(|c| {
        c.execute("DELETE FROM jobs WHERE id = ?1", [id])
            .map(|_| ())
    })
}

pub fn record_run(
    db: &Db,
    job_id: i64,
    due_at: &str,
    started_at: Option<&str>,
    status: &str,
    summary: Option<&str>,
    duration_ms: Option<i64>,
) -> rusqlite::Result<()> {
    db.with(|c| {
        c.execute(
            "INSERT INTO job_runs (job_id, due_at, started_at, status, summary, duration_ms) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![job_id, due_at, started_at, status, summary, duration_ms],
        )
        .map(|_| ())
    })
}

pub fn runs(db: &Db, job_id: Option<i64>) -> rusqlite::Result<Vec<JobRun>> {
    db.with(|c| {
        let mut st = c.prepare(
            "SELECT r.id, r.job_id, j.name, r.due_at, r.started_at, r.status, r.summary, r.duration_ms \
             FROM job_runs r LEFT JOIN jobs j ON j.id = r.job_id \
             WHERE (?1 IS NULL OR r.job_id = ?1) ORDER BY r.due_at DESC LIMIT 500",
        )?;
        let rows = st.query_map([job_id], |r| {
            Ok(JobRun {
                id: r.get(0)?,
                job_id: r.get(1)?,
                job_name: r.get(2)?,
                due_at: r.get(3)?,
                started_at: r.get(4)?,
                status: r.get(5)?,
                summary: r.get(6)?,
                duration_ms: r.get(7)?,
            })
        })?;
        rows.collect()
    })
}
