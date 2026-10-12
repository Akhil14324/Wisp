//! Scheduler (section 4.6): fires jobs, handles missed runs and locked
//! sessions, runs to-do reminders (FR-TODO-03) and daily retention (6.3).

pub mod schedule;

use std::collections::HashSet;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use chrono::{DateTime, Local, Utc};
use serde_json::json;
use tokio::sync::{mpsc, Notify};

use crate::agent::{self, CommandInput, RunOpts, Source};
use crate::state::ctx;
use crate::store::{jobs, todos};
use crate::tools::ToolName;

pub const QUEUE_MAX: usize = 5;

pub struct RunRequest {
    pub job: jobs::Job,
    pub due_at: String,
}

pub struct Scheduler {
    wake: Notify,
    queue: mpsc::Sender<RunRequest>,
    queue_rx: Mutex<Option<mpsc::Receiver<RunRequest>>>,
    reminded: Mutex<HashSet<(i64, String)>>,
}

impl Default for Scheduler {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel(QUEUE_MAX);
        Self {
            wake: Notify::new(),
            queue: tx,
            queue_rx: Mutex::new(Some(rx)),
            reminded: Mutex::new(HashSet::new()),
        }
    }
}

fn parse_utc(s: &str) -> Option<DateTime<Local>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.with_timezone(&Local))
}

pub fn to_utc_text(d: DateTime<Local>) -> String {
    d.with_timezone(&Utc)
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

impl Scheduler {
    /// Recompute now (a job changed, the session unlocked, resume from sleep).
    pub fn wake(&self) {
        self.wake.notify_one();
    }

    /// Queues a run; one job runs at a time, at most 5 wait (FR-SCH-09).
    pub fn enqueue(&self, job: jobs::Job, due_at: String) -> Result<(), String> {
        self.queue
            .try_send(RunRequest { job, due_at })
            .map_err(|_| "The job queue is full (5 waiting)".to_string())
    }

    pub fn start(&'static self) {
        let rx = self
            .queue_rx
            .lock()
            .unwrap()
            .take()
            .expect("scheduler started twice");
        tauri::async_runtime::spawn(worker(rx));
        tauri::async_runtime::spawn(self.main_loop());
    }

    async fn main_loop(&'static self) {
        let c = ctx();
        // Startup: anything due long ago counts as already reminded.
        let mut next_cleanup = next_3am(Local::now());
        if let Err(e) = c.db.apply_retention() {
            tracing::warn!(error = %e, "retention failed");
        }
        agent::computer::cleanup_debug();
        loop {
            let now = Local::now();
            let grace = chrono::Duration::minutes(c.settings().scheduler.grace_minutes as i64);
            let mut earliest = now + chrono::Duration::seconds(60);

            // Jobs.
            for job in jobs::list(&c.db).unwrap_or_default() {
                if !job.enabled {
                    continue;
                }
                let next = match job.next_run_at.as_deref().and_then(parse_utc) {
                    Some(n) => n,
                    None => {
                        if let Some(n) = job.when.next_after(now) {
                            let _ = jobs::set_next_run(&c.db, job.id, Some(&to_utc_text(n)));
                            earliest = earliest.min(n);
                        }
                        continue;
                    }
                };
                if next > now {
                    earliest = earliest.min(next);
                    continue;
                }
                self.handle_due(&job, next, now, grace);
                if let Some(n) = jobs::get(&c.db, job.id)
                    .ok()
                    .flatten()
                    .and_then(|j| j.next_run_at)
                    .and_then(|s| parse_utc(&s))
                {
                    // Still in the past means deferred while locked: re-check soon.
                    earliest = earliest.min(n.max(now + chrono::Duration::seconds(15)));
                }
            }

            // To-do reminders use the same timer, not the model (FR-TODO-03).
            for t in todos::pending_reminders(&c.db).unwrap_or_default() {
                let when_s = t.snoozed_until.clone().or_else(|| t.due_at.clone());
                let Some(when) = when_s.as_deref().and_then(parse_utc) else {
                    continue;
                };
                let key = (t.id, when_s.clone().unwrap_or_default());
                if self.reminded.lock().unwrap().contains(&key) {
                    continue;
                }
                if when > now {
                    earliest = earliest.min(when);
                } else {
                    self.reminded.lock().unwrap().insert(key);
                    if now - when <= grace {
                        remind(&t);
                    }
                }
            }

            if now >= next_cleanup {
                if let Err(e) = c.db.apply_retention() {
                    tracing::warn!(error = %e, "retention failed");
                }
                agent::computer::cleanup_debug();
                next_cleanup = next_3am(now);
                c.emit("history-changed", json!({}));
            }
            earliest = earliest.min(next_cleanup);

            let wait = (earliest - Local::now())
                .to_std()
                .unwrap_or(Duration::from_millis(200))
                .min(Duration::from_secs(60));
            tokio::select! {
                _ = tokio::time::sleep(wait) => {}
                _ = self.wake.notified() => {}
            }
        }
    }

    /// FR-SCH-05/08: run now, defer while locked, or record a miss.
    fn handle_due(
        &self,
        job: &jobs::Job,
        due: DateTime<Local>,
        now: DateTime<Local>,
        grace: chrono::Duration,
    ) {
        let c = ctx();
        let due_s = to_utc_text(due);
        let late = now - due;
        if late > grace {
            let _ = jobs::record_run(
                &c.db,
                job.id,
                &due_s,
                None,
                "missed",
                Some("Wisp was not running or the PC was locked"),
                None,
            );
            crate::notify::show(
                "Missed routine",
                &format!("{} was due at {}", job.name, due.format("%a %H:%M")),
            );
            tracing::info!(job = job.id, "missed run");
        } else if c.is_locked() {
            // Defer until unlock within the grace window; the loop retries.
            return;
        } else if let Err(e) = self.enqueue(job.clone(), due_s.clone()) {
            let _ = jobs::record_run(&c.db, job.id, &due_s, None, "failed", Some(&e), None);
        }
        let next = job.when.next_after(now.max(due));
        let _ = jobs::set_next_run(&c.db, job.id, next.map(to_utc_text).as_deref());
        c.emit("jobs-changed", json!({}));
    }
}

fn next_3am(now: DateTime<Local>) -> DateTime<Local> {
    use chrono::TimeZone;
    let today = now.date_naive().and_hms_opt(3, 0, 0).expect("valid");
    let t = Local.from_local_datetime(&today).earliest().unwrap_or(now);
    if t > now {
        t
    } else {
        Local
            .from_local_datetime(&(today + chrono::Duration::days(1)))
            .earliest()
            .unwrap_or(now + chrono::Duration::days(1))
    }
}

fn remind(t: &todos::Todo) {
    let id = t.id;
    crate::notify::show_with_actions(
        "To-do",
        &t.title,
        &[("Done", "done"), ("Snooze 10 min", "snooze")],
        move |action| {
            let c = ctx();
            match action.as_deref() {
                Some("done") => {
                    let p = todos::TodoPatch {
                        id,
                        title: None,
                        notes: None,
                        due_at: None,
                        done: Some(true),
                    };
                    let _ = todos::update(&c.db, &p);
                }
                Some("snooze") => {
                    let until = to_utc_text(Local::now() + chrono::Duration::minutes(10));
                    let _ = todos::snooze(&c.db, id, &until);
                    c.scheduler.wake();
                }
                _ => {}
            }
            c.emit("todos-changed", json!({}));
        },
    );
}

/// Tools allowed for a scheduled job (FR-SCH-06).
pub fn job_tools(job: &jobs::Job) -> Vec<ToolName> {
    let mut tools = ctx().loadout();
    if !job.allow_computer_use {
        tools.retain(|t| *t != ToolName::RunComputerTask);
    }
    tools
}

async fn worker(mut rx: mpsc::Receiver<RunRequest>) {
    while let Some(req) = rx.recv().await {
        let c = ctx();
        let started_wall = to_utc_text(Local::now());
        let started = Instant::now();
        // Wait for any interactive command to finish rather than fail.
        for _ in 0..120 {
            if !c.agent.is_running() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        let res = agent::run_command(
            CommandInput::Text(req.job.command.clone()),
            RunOpts {
                source: Source::Job,
                allowed: job_tools(&req.job),
                unattended: true,
            },
        )
        .await;
        let ms = started.elapsed().as_millis() as i64;
        let (status, summary) = match res {
            Ok(r) if r.ok => ("ok", r.text),
            Ok(r) if r.text.contains("no answer") => ("aborted", r.text),
            Ok(r) => ("failed", r.text),
            Err(e) if e.code == crate::error::ErrCode::Cancelled => {
                ("aborted", "Stopped".to_string())
            }
            Err(e) => ("failed", e.message),
        };
        let summary: String = summary
            .lines()
            .next()
            .unwrap_or("")
            .chars()
            .take(200)
            .collect();
        let _ = jobs::record_run(
            &c.db,
            req.job.id,
            &req.due_at,
            Some(&started_wall),
            status,
            Some(&summary),
            Some(ms),
        );
        crate::notify::show(&req.job.name, &summary);
        c.emit("jobs-changed", json!({}));
        tracing::info!(job = req.job.id, status, ms, "job run finished");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn cleanup_time() {
        let now = Local.with_ymd_and_hms(2026, 10, 12, 2, 0, 0).unwrap();
        assert_eq!(
            next_3am(now),
            Local.with_ymd_and_hms(2026, 10, 12, 3, 0, 0).unwrap()
        );
        let later = Local.with_ymd_and_hms(2026, 10, 12, 4, 0, 0).unwrap();
        assert_eq!(
            next_3am(later),
            Local.with_ymd_and_hms(2026, 10, 13, 3, 0, 0).unwrap()
        );
    }
}
