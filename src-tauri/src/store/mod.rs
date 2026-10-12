//! SQLite access, migrations and settings (section 6).

pub mod counters;
pub mod history;
pub mod jobs;
pub mod presets;
pub mod settings;
pub mod todos;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::Context;
use rusqlite::Connection;

mod embedded {
    refinery::embed_migrations!("migrations");
}

/// Shared handle to the single database connection.
#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

/// What happened while opening the database.
pub struct OpenReport {
    pub db: Db,
    /// Set when a corrupted file was renamed and a fresh one created (NFR-34).
    pub recovered_from: Option<PathBuf>,
}

impl Db {
    /// Opens (or creates) the database at `path`, runs migrations, and
    /// recovers from corruption by renaming the bad file.
    pub fn open(path: &Path) -> anyhow::Result<OpenReport> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).context("create data dir")?;
        }
        match Self::try_open(path) {
            Ok(db) => Ok(OpenReport {
                db,
                recovered_from: None,
            }),
            Err(e) if path.exists() => {
                tracing::warn!(error = %e, "database unusable, recreating");
                let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
                let bad = path.with_extension(format!("db.corrupt-{stamp}"));
                std::fs::rename(path, &bad).context("rename corrupt db")?;
                for ext in ["db-wal", "db-shm"] {
                    let _ = std::fs::remove_file(path.with_extension(ext));
                }
                let db = Self::try_open(path)?;
                Ok(OpenReport {
                    db,
                    recovered_from: Some(bad),
                })
            }
            Err(e) => Err(e),
        }
    }

    fn try_open(path: &Path) -> anyhow::Result<Db> {
        let mut conn = Connection::open(path)?;
        Self::prepare(&mut conn)?;
        Ok(Db {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// In-memory database for tests.
    pub fn open_in_memory() -> anyhow::Result<Db> {
        let mut conn = Connection::open_in_memory()?;
        Self::prepare(&mut conn)?;
        Ok(Db {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    fn prepare(conn: &mut Connection) -> anyhow::Result<()> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        let ok: String = conn.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
        if ok != "ok" {
            anyhow::bail!("quick_check failed");
        }
        embedded::migrations::runner()
            .run(conn)
            .context("migrations")?;
        Ok(())
    }

    /// Runs `f` with exclusive access to the connection.
    pub fn with<T>(
        &self,
        f: impl FnOnce(&mut Connection) -> rusqlite::Result<T>,
    ) -> rusqlite::Result<T> {
        let mut guard = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        f(&mut guard)
    }

    /// Wipes every user table (Delete all data, section 8.2).
    pub fn wipe_all(&self) -> rusqlite::Result<()> {
        self.with(|c| {
            let tx = c.transaction()?;
            for t in [
                "agent_steps",
                "command_history",
                "dictation_history",
                "job_runs",
                "jobs",
                "todos",
                "pinned_apps",
                "usage_counters",
                "presets",
                "settings",
            ] {
                tx.execute(&format!("DELETE FROM {t}"), [])?;
            }
            tx.commit()
        })
    }

    /// Applies the retention rules of section 6.3.
    pub fn apply_retention(&self) -> rusqlite::Result<()> {
        self.with(|c| {
            let tx = c.transaction()?;
            let now = chrono::Utc::now();
            let ago = |days: i64| (now - chrono::Duration::days(days)).to_rfc3339();
            tx.execute("DELETE FROM dictation_history WHERE at < ?1", [ago(7)])?;
            tx.execute(
                "DELETE FROM dictation_history WHERE id NOT IN \
                 (SELECT id FROM dictation_history ORDER BY at DESC LIMIT 50)",
                [],
            )?;
            tx.execute("DELETE FROM command_history WHERE at < ?1", [ago(90)])?;
            tx.execute(
                "DELETE FROM command_history WHERE id NOT IN \
                 (SELECT id FROM command_history ORDER BY at DESC LIMIT 100)",
                [],
            )?;
            tx.execute("DELETE FROM job_runs WHERE due_at < ?1", [ago(30)])?;
            tx.execute(
                "DELETE FROM todos WHERE done = 1 AND completed_at < ?1",
                [ago(30)],
            )?;
            let day = (chrono::Local::now() - chrono::Duration::days(30))
                .format("%Y-%m-%d")
                .to_string();
            tx.execute("DELETE FROM usage_counters WHERE day < ?1", [day])?;
            tx.commit()
        })
    }
}

pub fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_apply_and_retention_runs() {
        let db = Db::open_in_memory().unwrap();
        db.with(|c| {
            c.execute(
                "INSERT INTO dictation_history (at, text) VALUES ('2000-01-01T00:00:00Z', 'old')",
                [],
            )?;
            c.execute(
                "INSERT INTO dictation_history (at, text) VALUES (?1, 'new')",
                [now_iso()],
            )?;
            Ok(())
        })
        .unwrap();
        db.apply_retention().unwrap();
        let n: i64 = db
            .with(|c| c.query_row("SELECT COUNT(*) FROM dictation_history", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn corrupt_file_is_renamed() {
        let dir = std::env::temp_dir().join(format!("wisp-test-{}", rand::random::<u32>()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("wisp.db");
        std::fs::write(
            &path,
            b"this is not a sqlite database at all, just junk bytes....",
        )
        .unwrap();
        let report = Db::open(&path).unwrap();
        assert!(report.recovered_from.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
