//! TaskHub integration: when signed in, Wisp's to-do list is your TaskHub
//! to-do list (personal to-dos plus business tasks assigned to you).
//!
//! API (vgrand-taskhub-backend): `POST /auth/login {username,password}` ->
//! `{token, user}`; `GET /todos` -> `{todos, timezone, today, ...}`;
//! `POST /todos {title, notes, due_date, due_time}`; `PUT /todos/:id`;
//! `POST /todos/:id/toggle`; `DELETE /todos/:id`; `POST /todos/reorder {ids}`.
//! Dates are `YYYY-MM-DD` and times `HH:MM` in TaskHub's app time zone.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use chrono::{DateTime, NaiveDate, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use serde::Serialize;
use serde_json::{json, Value};

use crate::error::{ErrCode, UiError, UiResult};
use crate::secrets;
use crate::state::ctx;
use crate::store::todos::{NewTodo, Todo, TodoPatch};

const TOKEN: &str = "TaskHubToken";
const PASSWORD: &str = "TaskHubPassword";
const CACHE_TTL: Duration = Duration::from_secs(120);
const DEFAULT_TZ: Tz = chrono_tz::Asia::Kolkata;

struct Cache {
    at: Instant,
    todos: Vec<Todo>,
    tz: Tz,
}

#[derive(Default)]
pub struct TaskHub {
    cache: Mutex<Option<Cache>>,
    /// Snoozes are local to this PC (TaskHub has its own reminders).
    snoozed: Mutex<HashMap<i64, String>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub signed_in: bool,
    pub url: String,
    pub username: Option<String>,
}

fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(20))
        .user_agent(concat!("Wisp/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("http client")
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

fn base_url() -> String {
    ctx()
        .settings()
        .taskhub
        .url
        .trim_end_matches('/')
        .to_string()
}

pub fn status() -> Status {
    let s = ctx().settings().taskhub;
    Status {
        signed_in: signed_in(),
        url: s.url,
        username: s.username,
    }
}

/// Signed in = a username in settings and a token in Credential Manager.
pub fn signed_in() -> bool {
    let s = ctx().settings().taskhub;
    s.username.is_some() && s.user_id.is_some() && secrets::named::get(TOKEN).is_some()
}

fn net_err(e: reqwest::Error) -> UiError {
    if e.is_timeout() {
        UiError::new(ErrCode::Timeout, "TaskHub took too long to answer")
    } else {
        UiError::new(ErrCode::Network, "No connection to TaskHub")
    }
}

fn api_error(body: &str, fallback: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| {
            v.get("error")
                .and_then(Value::as_str)
                .map(|s| s.chars().take(200).collect())
        })
        .unwrap_or_else(|| fallback.to_string())
}

/// Logs in and returns (token, user id).
async fn login_raw(url: &str, username: &str, password: &str) -> UiResult<(String, i64)> {
    let res = http()
        .post(format!("{url}/auth/login"))
        .json(&json!({ "username": username, "password": password }))
        .send()
        .await
        .map_err(net_err)?;
    let status = res.status();
    let body = res.text().await.map_err(net_err)?;
    if !status.is_success() {
        return Err(UiError::invalid(api_error(
            &body,
            &format!("TaskHub sign-in failed (HTTP {})", status.as_u16()),
        )));
    }
    let v: Value = serde_json::from_str(&body)
        .map_err(|_| UiError::internal("TaskHub sent an unexpected reply"))?;
    let token = v
        .get("token")
        .and_then(Value::as_str)
        .ok_or_else(|| UiError::internal("TaskHub sent no token"))?;
    let id = v
        .pointer("/user/id")
        .and_then(Value::as_i64)
        .ok_or_else(|| UiError::internal("TaskHub sent no user"))?;
    Ok((token.to_string(), id))
}

/// Settings > TaskHub: sign in and keep the token and password in Credential
/// Manager so an expired token renews itself.
pub async fn sign_in(url: &str, username: &str, password: &str) -> UiResult<Status> {
    let url = url.trim().trim_end_matches('/').to_string();
    let username = username.trim();
    if username.is_empty() || password.is_empty() {
        return Err(UiError::invalid("Enter your TaskHub username and password"));
    }
    crate::store::settings::validate("taskhub.url", &json!(url))?;
    let (token, id) = login_raw(&url, username, password).await?;
    secrets::named::set(TOKEN, &token).map_err(|e| UiError::internal(e.to_string()))?;
    secrets::named::set(PASSWORD, password).map_err(|e| UiError::internal(e.to_string()))?;
    let db = &ctx().db;
    crate::store::settings::set(db, "taskhub.url", json!(url))?;
    crate::store::settings::set(db, "taskhub.username", json!(username))?;
    crate::store::settings::set(db, "taskhub.user_id", json!(id))?;
    ctx().reload_settings();
    lock(&ctx().taskhub.cache).take();
    tracing::info!("taskhub signed in");
    Ok(status())
}

pub fn sign_out() -> UiResult<Status> {
    secrets::named::delete(TOKEN);
    secrets::named::delete(PASSWORD);
    let db = &ctx().db;
    crate::store::settings::set(db, "taskhub.username", Value::Null)?;
    crate::store::settings::set(db, "taskhub.user_id", Value::Null)?;
    ctx().reload_settings();
    lock(&ctx().taskhub.cache).take();
    tracing::info!("taskhub signed out");
    Ok(status())
}

/// One authenticated call; on 401 it signs in again with the stored password once.
async fn call(method: reqwest::Method, path: &str, body: Option<Value>) -> UiResult<Value> {
    for attempt in 0..2 {
        let token = secrets::named::get(TOKEN)
            .ok_or_else(|| UiError::invalid("Sign in to TaskHub in Settings"))?;
        let mut req = http()
            .request(method.clone(), format!("{}{path}", base_url()))
            .bearer_auth(token.expose());
        if let Some(b) = &body {
            req = req.json(b);
        }
        let res = req.send().await.map_err(net_err)?;
        let status = res.status();
        let text = res.text().await.map_err(net_err)?;
        if status.as_u16() == 401 && attempt == 0 {
            let s = ctx().settings().taskhub;
            let (Some(user), Some(pw)) = (s.username, secrets::named::get(PASSWORD)) else {
                return Err(UiError::invalid("Sign in to TaskHub again in Settings"));
            };
            let (token, _) = login_raw(&base_url(), &user, pw.expose())
                .await
                .map_err(|_| UiError::invalid("Sign in to TaskHub again in Settings"))?;
            secrets::named::set(TOKEN, &token).map_err(|e| UiError::internal(e.to_string()))?;
            continue;
        }
        if !status.is_success() {
            return Err(UiError::invalid(api_error(
                &text,
                &format!("TaskHub said no (HTTP {})", status.as_u16()),
            )));
        }
        return Ok(if text.trim().is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&text).unwrap_or(Value::Null)
        });
    }
    Err(UiError::invalid("Sign in to TaskHub again in Settings"))
}

fn parse_tz(v: &Value) -> Tz {
    v.get("timezone")
        .and_then(Value::as_str)
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_TZ)
}

/// Converts one TaskHub to-do. Returns None for items that are not yours to
/// do (business tasks assigned to someone else), sub-tasks, or old done items.
pub fn from_remote(t: &Value, me: i64, tz: Tz, order: usize, now: DateTime<Utc>) -> Option<Todo> {
    if t.get("parent_id").map(|p| !p.is_null()).unwrap_or(false) {
        return None;
    }
    let business = t.get("business_id").map(|b| !b.is_null()).unwrap_or(false);
    let assignee = t.get("assignee_id").and_then(Value::as_i64);
    if business && assignee != Some(me) {
        return None;
    }
    let done = t.get("is_done").and_then(Value::as_bool).unwrap_or(false);
    let done_at = t.get("done_at").and_then(Value::as_str).map(String::from);
    if done {
        let recent = done_at
            .as_deref()
            .and_then(|d| DateTime::parse_from_rfc3339(d).ok())
            .map(|d| now - d.with_timezone(&Utc) < chrono::Duration::days(30))
            .unwrap_or(false);
        if !recent {
            return None;
        }
    }
    let date = t
        .get("due_date")
        .and_then(Value::as_str)
        .and_then(|d| NaiveDate::parse_from_str(d.get(..10)?, "%Y-%m-%d").ok());
    let time = t
        .get("due_time")
        .and_then(Value::as_str)
        .and_then(|s| NaiveTime::parse_from_str(s.get(..5)?, "%H:%M").ok());
    let (due_at, all_day) = match (date, time) {
        (Some(d), Some(tm)) => (tz.from_local_datetime(&d.and_time(tm)).earliest(), false),
        (Some(d), None) => (
            tz.from_local_datetime(&d.and_hms_opt(23, 59, 0)?)
                .earliest(),
            true,
        ),
        _ => (None, false),
    };
    Some(Todo {
        id: t.get("id").and_then(Value::as_i64)?,
        title: t
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        notes: t
            .get("notes")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(String::from),
        due_at: due_at.map(|d| {
            d.with_timezone(&Utc)
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
        }),
        done,
        sort_order: order as i64,
        created_at: t
            .get("created_at")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        completed_at: if done { done_at } else { None },
        snoozed_until: None,
        all_day,
        source: "taskhub".into(),
    })
}

/// UTC ISO due time -> TaskHub's (date, time) in its time zone.
pub fn to_remote_due(due_at: &str, tz: Tz) -> Option<(String, String)> {
    let d = DateTime::parse_from_rfc3339(due_at)
        .ok()?
        .with_timezone(&tz);
    Some((
        d.format("%Y-%m-%d").to_string(),
        d.format("%H:%M").to_string(),
    ))
}

impl TaskHub {
    fn tz(&self) -> Tz {
        lock(&self.cache)
            .as_ref()
            .map(|c| c.tz)
            .unwrap_or(DEFAULT_TZ)
    }

    fn invalidate(&self) {
        lock(&self.cache).take();
    }

    /// Your TaskHub to-dos, cached for 2 minutes unless `force`.
    pub async fn list(&self, force: bool) -> UiResult<Vec<Todo>> {
        if !force {
            if let Some(c) = lock(&self.cache).as_ref() {
                if c.at.elapsed() < CACHE_TTL {
                    return Ok(self.with_snoozes(c.todos.clone()));
                }
            }
        }
        let me = ctx()
            .settings()
            .taskhub
            .user_id
            .ok_or_else(|| UiError::invalid("Sign in to TaskHub in Settings"))?;
        let v = call(reqwest::Method::GET, "/todos", None).await?;
        let tz = parse_tz(&v);
        let now = Utc::now();
        let list: Vec<Todo> = v
            .get("todos")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .enumerate()
                    .filter_map(|(i, t)| from_remote(t, me, tz, i, now))
                    .collect()
            })
            .unwrap_or_default();
        *lock(&self.cache) = Some(Cache {
            at: Instant::now(),
            todos: list.clone(),
            tz,
        });
        Ok(self.with_snoozes(list))
    }

    fn with_snoozes(&self, mut list: Vec<Todo>) -> Vec<Todo> {
        let s = lock(&self.snoozed);
        for t in list.iter_mut() {
            t.snoozed_until = s.get(&t.id).cloned();
        }
        list
    }

    fn due_fields(&self, due_at: &Option<String>) -> (Value, Value) {
        match due_at.as_deref().and_then(|d| to_remote_due(d, self.tz())) {
            Some((d, t)) => (json!(d), json!(t)),
            None => (Value::Null, Value::Null),
        }
    }

    pub async fn create(&self, t: &NewTodo) -> UiResult<Todo> {
        if lock(&self.cache).is_none() {
            let _ = self.list(true).await;
        }
        let (date, time) = self.due_fields(&t.due_at);
        let v = call(
            reqwest::Method::POST,
            "/todos",
            Some(json!({ "title": t.title, "notes": t.notes, "due_date": date, "due_time": time })),
        )
        .await?;
        self.invalidate();
        let me = ctx().settings().taskhub.user_id.unwrap_or(0);
        v.get("todo")
            .and_then(|t| from_remote(t, me, self.tz(), 0, Utc::now()))
            .ok_or_else(|| UiError::internal("TaskHub did not return the new to-do"))
    }

    pub async fn update(&self, p: &TodoPatch) -> UiResult<Option<Todo>> {
        let mut body = serde_json::Map::new();
        if let Some(t) = &p.title {
            body.insert("title".into(), json!(t));
        }
        if let Some(n) = &p.notes {
            body.insert("notes".into(), json!(n));
        }
        if let Some(d) = &p.due_at {
            let (date, time) = self.due_fields(d);
            body.insert("due_date".into(), date);
            body.insert("due_time".into(), time);
            lock(&self.snoozed).remove(&p.id);
        }
        if !body.is_empty() {
            call(
                reqwest::Method::PUT,
                &format!("/todos/{}", p.id),
                Some(Value::Object(body)),
            )
            .await?;
        }
        if let Some(want) = p.done {
            let current = self
                .list(false)
                .await?
                .into_iter()
                .find(|t| t.id == p.id)
                .map(|t| t.done);
            if current != Some(want) {
                call(
                    reqwest::Method::POST,
                    &format!("/todos/{}/toggle", p.id),
                    None,
                )
                .await?;
            }
        }
        self.invalidate();
        Ok(self.list(true).await?.into_iter().find(|t| t.id == p.id))
    }

    pub async fn delete(&self, id: i64) -> UiResult<()> {
        call(reqwest::Method::DELETE, &format!("/todos/{id}"), None).await?;
        self.invalidate();
        Ok(())
    }

    pub async fn reorder(&self, ids: &[i64]) -> UiResult<()> {
        call(
            reqwest::Method::POST,
            "/todos/reorder",
            Some(json!({ "ids": ids })),
        )
        .await?;
        self.invalidate();
        Ok(())
    }

    pub fn snooze(&self, id: i64, until: &str) {
        lock(&self.snoozed).insert(id, until.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 13, 6, 0, 0).unwrap()
    }

    #[test]
    fn converts_personal_todo_with_time() {
        let t = json!({ "id": 7, "title": "Gym", "notes": "", "due_date": "2026-10-13", "due_time": "18:00:00",
                        "is_done": false, "business_id": null, "parent_id": null, "created_at": "2026-10-01T00:00:00Z" });
        let todo = from_remote(&t, 1, chrono_tz::Asia::Kolkata, 0, now()).unwrap();
        // 18:00 in Kolkata (UTC+5:30) is 12:30 UTC.
        assert_eq!(todo.due_at.as_deref(), Some("2026-10-13T12:30:00Z"));
        assert!(!todo.all_day);
        assert_eq!(todo.notes, None);
        assert_eq!(todo.source, "taskhub");
    }

    #[test]
    fn date_only_is_all_day() {
        let t = json!({ "id": 8, "title": "Report", "due_date": "2026-10-14", "due_time": null, "is_done": false });
        let todo = from_remote(&t, 1, chrono_tz::Asia::Kolkata, 0, now()).unwrap();
        assert!(todo.all_day);
    }

    #[test]
    fn filters_other_peoples_tasks_subtasks_and_old_done() {
        let tz = chrono_tz::Asia::Kolkata;
        let theirs =
            json!({ "id": 1, "title": "x", "business_id": 3, "assignee_id": 2, "is_done": false });
        let mine =
            json!({ "id": 2, "title": "x", "business_id": 3, "assignee_id": 1, "is_done": false });
        let sub = json!({ "id": 3, "title": "x", "parent_id": 2, "is_done": false });
        let old =
            json!({ "id": 4, "title": "x", "is_done": true, "done_at": "2026-08-01T00:00:00Z" });
        let recent =
            json!({ "id": 5, "title": "x", "is_done": true, "done_at": "2026-10-12T00:00:00Z" });
        assert!(from_remote(&theirs, 1, tz, 0, now()).is_none());
        assert!(from_remote(&mine, 1, tz, 0, now()).is_some());
        assert!(from_remote(&sub, 1, tz, 0, now()).is_none());
        assert!(from_remote(&old, 1, tz, 0, now()).is_none());
        assert!(from_remote(&recent, 1, tz, 0, now())
            .unwrap()
            .completed_at
            .is_some());
    }

    #[test]
    fn due_round_trip() {
        let (d, t) = to_remote_due("2026-10-13T12:30:00Z", chrono_tz::Asia::Kolkata).unwrap();
        assert_eq!((d.as_str(), t.as_str()), ("2026-10-13", "18:00"));
    }
}
