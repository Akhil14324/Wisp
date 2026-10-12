//! Orchestrator: turns an utterance or text into model calls and tool runs,
//! owns cancellation, confirmations and the Computer Use loop (section 3.4).

pub mod computer;
pub mod tool_loop;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::json;
use tokio::sync::{oneshot, Notify};
use tokio_util::sync::CancellationToken;

use crate::error::{ErrCode, UiError};
use crate::gemini::{self, client, prompts};
use crate::island::State;
use crate::state::ctx;
use crate::store::history;
use crate::tools::exec::SystemTools;
use crate::tools::ToolName;
use tool_loop::{LoopEnd, LoopRequest};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmKind {
    FirstTask,
    Safety,
    Guard,
    Takeover,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfirmRequest {
    pub id: u64,
    pub title: String,
    pub detail: String,
    pub kind: ConfirmKind,
}

#[derive(Default)]
pub struct AgentControl {
    token: Mutex<Option<(u64, CancellationToken)>>,
    run_seq: AtomicU64,
    confirms: Mutex<HashMap<u64, oneshot::Sender<bool>>>,
    next_id: AtomicU64,
    /// First Computer Use task this app session was confirmed (FR-CU-02).
    pub cu_session_ok: AtomicBool,
    pub cu_running: AtomicBool,
    paused: AtomicBool,
    resume: Notify,
}

impl AgentControl {
    /// Starts a cancellable run. Returns None if one is already running.
    pub fn begin(&self) -> Option<Run> {
        let mut g = self.token.lock().unwrap_or_else(|p| p.into_inner());
        if let Some((_, t)) = g.as_ref() {
            if !t.is_cancelled() {
                return None;
            }
        }
        let id = self.run_seq.fetch_add(1, Ordering::SeqCst) + 1;
        let token = CancellationToken::new();
        *g = Some((id, token.clone()));
        Some(Run { id, token })
    }

    pub fn end(&self, run: &Run) {
        let mut g = self.token.lock().unwrap_or_else(|p| p.into_inner());
        if g.as_ref().map(|(id, _)| *id == run.id).unwrap_or(false) {
            *g = None;
        }
    }

    pub fn is_running(&self) -> bool {
        self.token
            .lock()
            .map(|g| g.as_ref().map(|(_, t)| !t.is_cancelled()).unwrap_or(false))
            .unwrap_or(false)
    }

    pub fn has_pending_confirm(&self) -> bool {
        self.confirms.lock().map(|c| !c.is_empty()).unwrap_or(false)
    }

    /// Emergency stop (FR-CU-10): cancel, release input, answer No to cards.
    pub fn stop(&self) {
        if let Some((_, t)) = self.token.lock().unwrap_or_else(|p| p.into_inner()).take() {
            t.cancel();
        }
        crate::input::release_all();
        let pending: Vec<_> = self
            .confirms
            .lock()
            .map(|mut c| c.drain().collect())
            .unwrap_or_default();
        for (_, tx) in pending {
            let _ = tx.send(false);
        }
        self.paused.store(false, Ordering::SeqCst);
        self.resume.notify_waiters();
        crate::cta::hide_rings();
        tracing::info!("emergency stop");
    }

    /// Shows a confirmation card and waits. Timeout or cancel counts as No (FR-CU-07).
    pub async fn confirm(
        &self,
        kind: ConfirmKind,
        title: &str,
        detail: &str,
        timeout: Duration,
        cancel: &CancellationToken,
    ) -> bool {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst) + 1;
        let (tx, rx) = oneshot::channel();
        self.confirms
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(id, tx);
        let c = ctx();
        let req = ConfirmRequest {
            id,
            title: title.chars().take(200).collect(),
            detail: detail.chars().take(600).collect(),
            kind,
        };
        c.island.set(State::Confirm, Some(&req.title));
        c.emit("confirm-request", req);
        let answer = tokio::select! {
            r = rx => r.unwrap_or(false),
            _ = tokio::time::sleep(timeout) => false,
            _ = cancel.cancelled() => false,
        };
        self.confirms
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&id);
        c.emit("confirm-closed", json!({ "id": id, "yes": answer }));
        answer
    }

    pub fn answer(&self, id: u64, yes: bool) -> bool {
        match self
            .confirms
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&id)
        {
            Some(tx) => tx.send(yes).is_ok(),
            None => false,
        }
    }

    /// User takeover (FR-CU-14).
    pub fn pause(&self) {
        if self.cu_running.load(Ordering::SeqCst) && !self.paused.swap(true, Ordering::SeqCst) {
            tracing::info!("agent paused by user input");
        }
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    pub fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
        self.resume.notify_waiters();
    }

    /// Waits while paused. Returns false if cancelled.
    pub async fn wait_if_paused(&self, cancel: &CancellationToken) -> bool {
        while self.paused.load(Ordering::SeqCst) {
            let n = self.resume.notified();
            if !self.paused.load(Ordering::SeqCst) {
                break;
            }
            tokio::select! {
                _ = n => {}
                _ = cancel.cancelled() => return false,
                _ = tokio::time::sleep(Duration::from_millis(500)) => {}
            }
        }
        !cancel.is_cancelled()
    }
}

/// A running command or task; cancelling the token stops it.
pub struct Run {
    id: u64,
    pub token: CancellationToken,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Voice,
    Typed,
    Job,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Voice => "voice",
            Source::Typed => "typed",
            Source::Job => "job",
        }
    }
}

pub enum CommandInput {
    /// WAV bytes.
    Audio(Vec<u8>),
    Text(String),
}

pub struct RunOpts {
    pub source: Source,
    pub allowed: Vec<ToolName>,
    /// Scheduled runs: no island result, confirmations notify and wait 10 min.
    pub unattended: bool,
}

pub struct RunResult {
    pub text: String,
    pub ok: bool,
}

fn now_context() -> (String, String) {
    let now = chrono::Local::now();
    let tz = iana_time_zone::get_timezone().unwrap_or_else(|_| now.format("%:z").to_string());
    (now.to_rfc3339_opts(chrono::SecondsFormat::Secs, false), tz)
}

pub fn command_system_prompt() -> String {
    let soul = ctx().soul();
    let (iso, tz) = now_context();
    prompts::command(&prompts::CommandContext {
        assistant_name: &soul.name,
        tone: &soul.tone,
        language: &soul.language,
        addendum: &soul.addendum,
        iso_datetime: &iso,
        timezone: &tz,
    })
}

/// Runs a command through the tool loop (FR-CMD-01/02, FR-SCH-03).
pub async fn run_command(input: CommandInput, opts: RunOpts) -> Result<RunResult, UiError> {
    let c = ctx();
    let key = c.key().ok_or_else(|| UiError::of(ErrCode::NoKey))?;
    let run = c
        .agent
        .begin()
        .ok_or_else(|| UiError::new(ErrCode::Invalid, "Wisp is busy with another task"))?;
    let token = run.token.clone();
    let started = Instant::now();
    if !opts.unattended {
        c.island.set(State::Thinking, Some("Thinking"));
    }
    let transcript = match &input {
        CommandInput::Text(t) => Some(t.clone()),
        CommandInput::Audio(_) => None,
    };
    let user_parts = match input {
        CommandInput::Audio(wav) => vec![gemini::inline_part("audio/wav", &wav)],
        CommandInput::Text(t) => vec![gemini::text_part(&t)],
    };
    let settings = c.settings();
    let system = command_system_prompt();
    let host = SystemTools::new(opts.allowed.clone(), opts.unattended, token.clone());
    let outcome = tool_loop::run(
        &c.gemini,
        &key,
        LoopRequest {
            model: &settings.models.command,
            system: &system,
            user_parts,
            tools: gemini::schema::tools_field(&opts.allowed),
            timeout: client::TIMEOUT,
        },
        &host,
        &token,
    )
    .await;
    c.agent.end(&run);
    let duration = started.elapsed().as_millis() as i64;
    let tools_json = serde_json::to_value(&outcome.calls).unwrap_or_default();
    let (result, outcome_text) = match outcome.end {
        LoopEnd::Text(t) => (
            Ok(RunResult {
                text: t.clone(),
                ok: true,
            }),
            t,
        ),
        LoopEnd::ToolFailures(e) => {
            let msg = format!("Stopped: {}", e.chars().take(140).collect::<String>());
            (
                Ok(RunResult {
                    text: msg.clone(),
                    ok: false,
                }),
                msg,
            )
        }
        LoopEnd::RoundLimit => {
            let msg = "Stopped after 8 rounds without an answer".to_string();
            (
                Ok(RunResult {
                    text: msg.clone(),
                    ok: false,
                }),
                msg,
            )
        }
        LoopEnd::Gemini(e) => {
            let code = e.code();
            (Err(UiError::of(code)), format!("error: {}", code.as_str()))
        }
    };
    let transcript_label = transcript
        .clone()
        .unwrap_or_else(|| "(voice command)".into());
    if let Err(e) = history::add_command(
        &c.db,
        opts.source.as_str(),
        Some(&transcript_label),
        &tools_json,
        &outcome_text,
        duration,
    ) {
        tracing::warn!(error = %e, "history write failed");
    }
    c.emit("history-changed", json!({}));
    tracing::info!(
        source = opts.source.as_str(),
        tools = outcome.calls.len(),
        duration_ms = duration,
        "command done"
    );
    if !opts.unattended {
        match &result {
            Ok(r) if r.ok => c.island.result(&r.text),
            Ok(r) => c.island.error_text(&r.text),
            Err(e) if e.code == ErrCode::Cancelled => c.island.idle(),
            Err(e) => {
                if e.code == ErrCode::RateLimit {
                    tracing::warn!("rate limited after retries");
                }
                c.island.error(e.code)
            }
        }
    }
    result
}
