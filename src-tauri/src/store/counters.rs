//! Local usage counters (FR-SET-09). Only counts, never content.

use super::Db;

pub const REQUESTS: &str = "requests";
pub const RATE_LIMITED: &str = "http_429";

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

pub fn increment(db: &Db, metric: &str) {
    let res = db.with(|c| {
        c.execute(
            "INSERT INTO usage_counters (day, metric, value) VALUES (?1, ?2, 1) \
             ON CONFLICT(day, metric) DO UPDATE SET value = value + 1",
            rusqlite::params![today(), metric],
        )
    });
    if let Err(e) = res {
        tracing::warn!(error = %e, "counter write failed");
    }
}

pub fn today_value(db: &Db, metric: &str) -> i64 {
    db.with(|c| {
        c.query_row(
            "SELECT value FROM usage_counters WHERE day = ?1 AND metric = ?2",
            rusqlite::params![today(), metric],
            |r| r.get(0),
        )
    })
    .unwrap_or(0)
}
