//! To-do items (section 4.8).

use rusqlite::{params, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

use super::{now_iso, Db};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Todo {
    pub id: i64,
    pub title: String,
    pub notes: Option<String>,
    pub due_at: Option<String>,
    pub done: bool,
    pub sort_order: i64,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub snoozed_until: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NewTodo {
    pub title: String,
    pub notes: Option<String>,
    pub due_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TodoPatch {
    pub id: i64,
    pub title: Option<String>,
    /// `Some(None)` clears the notes.
    #[serde(default, with = "double_option")]
    pub notes: Option<Option<String>>,
    #[serde(default, with = "double_option")]
    pub due_at: Option<Option<String>>,
    pub done: Option<bool>,
}

mod double_option {
    use serde::{Deserialize, Deserializer};
    pub fn deserialize<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
    where
        D: Deserializer<'de>,
        T: Deserialize<'de>,
    {
        Option::<T>::deserialize(d).map(Some)
    }
}

const COLS: &str =
    "id, title, notes, due_at, done, sort_order, created_at, completed_at, snoozed_until";

fn row(r: &Row) -> rusqlite::Result<Todo> {
    Ok(Todo {
        id: r.get(0)?,
        title: r.get(1)?,
        notes: r.get(2)?,
        due_at: r.get(3)?,
        done: r.get::<_, i64>(4)? != 0,
        sort_order: r.get(5)?,
        created_at: r.get(6)?,
        completed_at: r.get(7)?,
        snoozed_until: r.get(8)?,
    })
}

pub fn list(db: &Db) -> rusqlite::Result<Vec<Todo>> {
    db.with(|c| {
        let mut st = c.prepare(&format!(
            "SELECT {COLS} FROM todos ORDER BY done, sort_order, id"
        ))?;
        let rows = st.query_map([], row)?;
        rows.collect()
    })
}

pub fn get(db: &Db, id: i64) -> rusqlite::Result<Option<Todo>> {
    db.with(|c| {
        c.query_row(
            &format!("SELECT {COLS} FROM todos WHERE id = ?1"),
            [id],
            row,
        )
        .optional()
    })
}

pub fn create(db: &Db, t: &NewTodo) -> rusqlite::Result<Todo> {
    let id = db.with(|c| {
        let tx = c.transaction()?;
        let next: i64 = tx.query_row("SELECT COALESCE(MAX(sort_order), 0) + 1 FROM todos", [], |r| r.get(0))?;
        tx.execute(
            "INSERT INTO todos (title, notes, due_at, sort_order, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![t.title, t.notes, t.due_at, next, now_iso()],
        )?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(id)
    })?;
    Ok(get(db, id)?.expect("just inserted"))
}

pub fn update(db: &Db, p: &TodoPatch) -> rusqlite::Result<Option<Todo>> {
    db.with(|c| {
        let tx = c.transaction()?;
        if let Some(title) = &p.title {
            tx.execute(
                "UPDATE todos SET title = ?1 WHERE id = ?2",
                params![title, p.id],
            )?;
        }
        if let Some(notes) = &p.notes {
            tx.execute(
                "UPDATE todos SET notes = ?1 WHERE id = ?2",
                params![notes, p.id],
            )?;
        }
        if let Some(due) = &p.due_at {
            tx.execute(
                "UPDATE todos SET due_at = ?1, snoozed_until = NULL WHERE id = ?2",
                params![due, p.id],
            )?;
        }
        if let Some(done) = p.done {
            let completed = if done { Some(now_iso()) } else { None };
            tx.execute(
                "UPDATE todos SET done = ?1, completed_at = ?2 WHERE id = ?3",
                params![done as i64, completed, p.id],
            )?;
        }
        tx.commit()
    })?;
    get(db, p.id)
}

pub fn delete(db: &Db, id: i64) -> rusqlite::Result<()> {
    db.with(|c| {
        c.execute("DELETE FROM todos WHERE id = ?1", [id])
            .map(|_| ())
    })
}

pub fn reorder(db: &Db, ids: &[i64]) -> rusqlite::Result<()> {
    db.with(|c| {
        let tx = c.transaction()?;
        for (i, id) in ids.iter().enumerate() {
            tx.execute(
                "UPDATE todos SET sort_order = ?1 WHERE id = ?2",
                params![i as i64, id],
            )?;
        }
        tx.commit()
    })
}

pub fn snooze(db: &Db, id: i64, until: &str) -> rusqlite::Result<()> {
    db.with(|c| {
        c.execute(
            "UPDATE todos SET snoozed_until = ?1 WHERE id = ?2",
            params![until, id],
        )
        .map(|_| ())
    })
}

/// Open to-dos with a due or snooze time, for the reminder timer.
pub fn pending_reminders(db: &Db) -> rusqlite::Result<Vec<Todo>> {
    db.with(|c| {
        let mut st = c.prepare(&format!(
            "SELECT {COLS} FROM todos WHERE done = 0 AND (due_at IS NOT NULL OR snoozed_until IS NOT NULL)"
        ))?;
        let rows = st.query_map([], row)?;
        rows.collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crud_and_reorder() {
        let db = Db::open_in_memory().unwrap();
        let a = create(
            &db,
            &NewTodo {
                title: "a".into(),
                notes: None,
                due_at: None,
            },
        )
        .unwrap();
        let b = create(
            &db,
            &NewTodo {
                title: "b".into(),
                notes: None,
                due_at: None,
            },
        )
        .unwrap();
        reorder(&db, &[b.id, a.id]).unwrap();
        let l = list(&db).unwrap();
        assert_eq!(l[0].title, "b");
        let p = TodoPatch {
            id: a.id,
            title: None,
            notes: None,
            due_at: None,
            done: Some(true),
        };
        let a2 = update(&db, &p).unwrap().unwrap();
        assert!(a2.done && a2.completed_at.is_some());
        delete(&db, b.id).unwrap();
        assert_eq!(list(&db).unwrap().len(), 1);
    }
}
