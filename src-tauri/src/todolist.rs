//! The to-do list Wisp shows and edits: TaskHub when signed in (user
//! decision), otherwise the local SQLite list (section 4.8).

use crate::error::UiResult;
use crate::state::ctx;
use crate::store::todos::{self, NewTodo, Todo, TodoPatch};

fn remote() -> bool {
    crate::taskhub::signed_in()
}

pub async fn list() -> UiResult<Vec<Todo>> {
    if remote() {
        ctx().taskhub.list(false).await
    } else {
        Ok(todos::list(&ctx().db)?)
    }
}

/// Fresh list from TaskHub (background refresh, after edits elsewhere).
pub async fn refresh() -> UiResult<Vec<Todo>> {
    if remote() {
        ctx().taskhub.list(true).await
    } else {
        Ok(todos::list(&ctx().db)?)
    }
}

pub async fn create(t: &NewTodo) -> UiResult<Todo> {
    if remote() {
        ctx().taskhub.create(t).await
    } else {
        Ok(todos::create(&ctx().db, t)?)
    }
}

pub async fn update(p: &TodoPatch) -> UiResult<Option<Todo>> {
    if remote() {
        ctx().taskhub.update(p).await
    } else {
        Ok(todos::update(&ctx().db, p)?)
    }
}

pub async fn delete(id: i64) -> UiResult<()> {
    if remote() {
        ctx().taskhub.delete(id).await
    } else {
        Ok(todos::delete(&ctx().db, id)?)
    }
}

pub async fn reorder(ids: &[i64]) -> UiResult<()> {
    if remote() {
        ctx().taskhub.reorder(ids).await
    } else {
        Ok(todos::reorder(&ctx().db, ids)?)
    }
}

pub async fn snooze(id: i64, until: &str) -> UiResult<()> {
    if remote() {
        ctx().taskhub.snooze(id, until);
        Ok(())
    } else {
        Ok(todos::snooze(&ctx().db, id, until)?)
    }
}

/// Open to-dos with a clock time (or a snooze) for the reminder timer.
pub async fn pending_reminders() -> Vec<Todo> {
    if remote() {
        ctx()
            .taskhub
            .list(false)
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|t| {
                !t.done && ((t.due_at.is_some() && !t.all_day) || t.snoozed_until.is_some())
            })
            .collect()
    } else {
        todos::pending_reminders(&ctx().db).unwrap_or_default()
    }
}
