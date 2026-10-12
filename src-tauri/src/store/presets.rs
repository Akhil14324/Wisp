//! Customization slot presets (section 4.10).

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::Db;
use crate::error::{UiError, UiResult};
use crate::tools::ToolName;

pub const SLOTS: &[&str] = &["soul", "loadout", "skin", "keys"];
pub const EXPORT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preset {
    pub id: i64,
    pub slot: String,
    pub name: String,
    pub data: Value,
    pub is_builtin: bool,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Soul {
    pub name: String,
    pub tone: String,
    pub language: String,
    #[serde(default)]
    pub addendum: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Loadout {
    pub tools: Vec<ToolName>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skin {
    pub theme: String,
    pub accent: String,
    pub radius: u32,
    pub opacity: f64,
    pub font_size: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeysPreset {
    pub dictation: String,
    pub circle: String,
    pub command: String,
    pub search_engine: String,
    pub input_device: Option<String>,
    pub target_monitor: String,
}

fn builtins() -> Vec<(&'static str, &'static str, Value, bool)> {
    let all: Vec<ToolName> = ToolName::ALL.to_vec();
    let standard: Vec<ToolName> = all
        .iter()
        .copied()
        .filter(|t| *t != ToolName::RunComputerTask)
        .collect();
    vec![
        (
            "soul",
            "Default",
            json!({"name": "Wisp", "tone": "calm and helpful", "language": "English", "addendum": ""}),
            true,
        ),
        (
            "soul",
            "Concise",
            json!({"name": "Wisp", "tone": "terse, no small talk", "language": "English", "addendum": "Answer in as few words as possible."}),
            false,
        ),
        (
            "soul",
            "Friendly",
            json!({"name": "Wisp", "tone": "warm and upbeat", "language": "English", "addendum": ""}),
            false,
        ),
        (
            "loadout",
            "Minimal",
            json!({"tools": [ToolName::OpenApp, ToolName::OpenUrl, ToolName::WebSearch]}),
            false,
        ),
        ("loadout", "Standard", json!({ "tools": standard }), false),
        ("loadout", "Full", json!({ "tools": all }), true),
        (
            "skin",
            "Midnight",
            json!({"theme": "dark", "accent": "#8b9cff", "radius": 8, "opacity": 0.82, "font_size": 13}),
            true,
        ),
        (
            "skin",
            "Paper",
            json!({"theme": "light", "accent": "#2f6fed", "radius": 8, "opacity": 0.92, "font_size": 13}),
            false,
        ),
        (
            "skin",
            "Ember",
            json!({"theme": "dark", "accent": "#ff8a4c", "radius": 12, "opacity": 0.86, "font_size": 13}),
            false,
        ),
        (
            "keys",
            "Default",
            json!({"dictation": "ctrl+win", "circle": "ctrl+shift", "command": "ctrl+alt+space", "search_engine": "google", "input_device": null, "target_monitor": "primary"}),
            true,
        ),
    ]
}

/// Inserts built-in presets that are missing; activates defaults for empty slots.
pub fn seed(db: &Db) -> rusqlite::Result<()> {
    db.with(|c| {
        let tx = c.transaction()?;
        for (slot, name, data, _) in builtins() {
            tx.execute(
                "INSERT INTO presets (slot, name, json, is_builtin) VALUES (?1, ?2, ?3, 1) \
                 ON CONFLICT(slot, name) DO UPDATE SET json = excluded.json WHERE is_builtin = 1",
                params![slot, name, data.to_string()],
            )?;
        }
        for (slot, name, _, default_active) in builtins() {
            if !default_active {
                continue;
            }
            let active: i64 = tx.query_row(
                "SELECT COUNT(*) FROM presets WHERE slot = ?1 AND is_active = 1",
                [slot],
                |r| r.get(0),
            )?;
            if active == 0 {
                tx.execute(
                    "UPDATE presets SET is_active = 1 WHERE slot = ?1 AND name = ?2",
                    params![slot, name],
                )?;
            }
        }
        tx.commit()
    })
}

/// Checks that `data` is valid for `slot`.
pub fn validate(slot: &str, data: &Value) -> UiResult<()> {
    let bad = |m: &str| UiError::invalid(m.to_string());
    match slot {
        "soul" => {
            let s: Soul = serde_json::from_value(data.clone())
                .map_err(|_| bad("Soul preset has the wrong shape"))?;
            if s.addendum.chars().count() > 500 {
                return Err(bad("The Soul addendum is limited to 500 characters"));
            }
            if s.name.trim().is_empty()
                || s.name.len() > 40
                || s.tone.len() > 100
                || s.language.len() > 40
            {
                return Err(bad("Soul name, tone or language is too long or empty"));
            }
        }
        "loadout" => {
            serde_json::from_value::<Loadout>(data.clone())
                .map_err(|_| bad("Loadout lists unknown tools"))?;
        }
        "skin" => {
            let s: Skin = serde_json::from_value(data.clone())
                .map_err(|_| bad("Skin preset has the wrong shape"))?;
            let hex = s.accent.len() == 7
                && s.accent.starts_with('#')
                && s.accent[1..].chars().all(|c| c.is_ascii_hexdigit());
            if !["light", "dark", "auto"].contains(&s.theme.as_str())
                || !hex
                || s.radius > 24
                || !(0.3..=1.0).contains(&s.opacity)
                || !(11..=18).contains(&s.font_size)
            {
                return Err(bad("Skin values are out of range"));
            }
        }
        "keys" => {
            let k: KeysPreset = serde_json::from_value(data.clone())
                .map_err(|_| bad("Keys preset has the wrong shape"))?;
            use crate::store::settings::{ALLOWED_CHORDS, SEARCH_ENGINES};
            for chord in [&k.dictation, &k.circle, &k.command] {
                if !ALLOWED_CHORDS.contains(&chord.as_str()) {
                    return Err(bad("Keys preset uses a chord that is not allowed"));
                }
            }
            if k.dictation == k.circle || k.dictation == k.command || k.circle == k.command {
                return Err(bad("Each action needs a different chord"));
            }
            if !SEARCH_ENGINES.contains(&k.search_engine.as_str()) {
                return Err(bad("Unknown search engine"));
            }
        }
        _ => return Err(bad("Unknown slot")),
    }
    Ok(())
}

fn map_row(r: &rusqlite::Row) -> rusqlite::Result<Preset> {
    let json: String = r.get(3)?;
    Ok(Preset {
        id: r.get(0)?,
        slot: r.get(1)?,
        name: r.get(2)?,
        data: serde_json::from_str(&json).unwrap_or(Value::Null),
        is_builtin: r.get::<_, i64>(4)? != 0,
        is_active: r.get::<_, i64>(5)? != 0,
    })
}

pub fn list(db: &Db) -> rusqlite::Result<Vec<Preset>> {
    db.with(|c| {
        let mut st =
            c.prepare("SELECT id, slot, name, json, is_builtin, is_active FROM presets ORDER BY slot, is_builtin DESC, name")?;
        let rows = st.query_map([], map_row)?;
        rows.collect()
    })
}

pub fn get(db: &Db, id: i64) -> rusqlite::Result<Option<Preset>> {
    db.with(|c| {
        c.query_row(
            "SELECT id, slot, name, json, is_builtin, is_active FROM presets WHERE id = ?1",
            [id],
            map_row,
        )
        .optional()
    })
}

pub fn active(db: &Db, slot: &str) -> Option<Preset> {
    db.with(|c| {
        c.query_row(
            "SELECT id, slot, name, json, is_builtin, is_active FROM presets WHERE slot = ?1 AND is_active = 1",
            [slot],
            map_row,
        )
        .optional()
    })
    .ok()
    .flatten()
}

/// Creates or updates a user preset. Built-in presets cannot be overwritten.
pub fn save(db: &Db, id: Option<i64>, slot: &str, name: &str, data: &Value) -> UiResult<Preset> {
    validate(slot, data)?;
    let name = name.trim();
    if name.is_empty() || name.len() > 40 {
        return Err(UiError::invalid("Preset names are 1 to 40 characters"));
    }
    let new_id = db
        .with(|c| {
            if let Some(id) = id {
                let builtin: i64 = c.query_row("SELECT is_builtin FROM presets WHERE id = ?1", [id], |r| r.get(0))?;
                if builtin != 0 {
                    return Err(rusqlite::Error::InvalidQuery);
                }
                c.execute(
                    "UPDATE presets SET name = ?1, json = ?2 WHERE id = ?3 AND slot = ?4",
                    params![name, data.to_string(), id, slot],
                )?;
                Ok(id)
            } else {
                c.execute(
                    "INSERT INTO presets (slot, name, json, is_builtin, is_active) VALUES (?1, ?2, ?3, 0, 0)",
                    params![slot, name, data.to_string()],
                )?;
                Ok(c.last_insert_rowid())
            }
        })
        .map_err(|e| match e {
            rusqlite::Error::InvalidQuery => UiError::invalid("Built-in presets cannot be edited; duplicate it first"),
            rusqlite::Error::SqliteFailure(f, _) if f.code == rusqlite::ErrorCode::ConstraintViolation => {
                UiError::invalid("A preset with that name already exists")
            }
            other => UiError::from(other),
        })?;
    get(db, new_id)?.ok_or_else(|| UiError::internal("preset vanished"))
}

pub fn activate(db: &Db, id: i64) -> UiResult<Preset> {
    let p = get(db, id)?.ok_or_else(|| UiError::invalid("No such preset"))?;
    db.with(|c| {
        let tx = c.transaction()?;
        tx.execute(
            "UPDATE presets SET is_active = 0 WHERE slot = ?1",
            [&p.slot],
        )?;
        tx.execute("UPDATE presets SET is_active = 1 WHERE id = ?1", [id])?;
        tx.commit()
    })?;
    get(db, id)?.ok_or_else(|| UiError::internal("preset vanished"))
}

pub fn delete(db: &Db, id: i64) -> UiResult<()> {
    let p = get(db, id)?.ok_or_else(|| UiError::invalid("No such preset"))?;
    if p.is_builtin {
        return Err(UiError::invalid("Built-in presets cannot be deleted"));
    }
    db.with(|c| {
        let tx = c.transaction()?;
        tx.execute("DELETE FROM presets WHERE id = ?1", [id])?;
        if p.is_active {
            tx.execute(
                "UPDATE presets SET is_active = 1 WHERE id = \
                 (SELECT id FROM presets WHERE slot = ?1 AND is_builtin = 1 ORDER BY id LIMIT 1)",
                [&p.slot],
            )?;
        }
        tx.commit()
    })?;
    Ok(())
}

/// JSON document for a preset export (FR-CUS-03).
pub fn export_doc(p: &Preset) -> Value {
    json!({ "schema_version": EXPORT_SCHEMA_VERSION, "slot": p.slot, "name": p.name, "data": p.data })
}

/// Parses an exported preset; rejects unknown schema versions.
pub fn parse_import(text: &str) -> UiResult<(String, String, Value)> {
    let v: Value =
        serde_json::from_str(text).map_err(|_| UiError::invalid("That file is not valid JSON"))?;
    match v.get("schema_version").and_then(Value::as_u64) {
        Some(n) if n == EXPORT_SCHEMA_VERSION as u64 => {}
        Some(n) => {
            return Err(UiError::invalid(format!(
                "This preset uses schema version {n}; this Wisp only reads version {EXPORT_SCHEMA_VERSION}"
            )))
        }
        None => return Err(UiError::invalid("This file has no schema_version, so it is not a Wisp preset")),
    }
    let slot = v
        .get("slot")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let name = v
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let data = v.get("data").cloned().unwrap_or(Value::Null);
    if !SLOTS.contains(&slot.as_str()) {
        return Err(UiError::invalid("Unknown slot in preset file"));
    }
    validate(&slot, &data)?;
    Ok((slot, name, data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_activates_defaults() {
        let db = Db::open_in_memory().unwrap();
        seed(&db).unwrap();
        seed(&db).unwrap();
        assert_eq!(active(&db, "loadout").unwrap().name, "Full");
        assert_eq!(active(&db, "skin").unwrap().name, "Midnight");
        assert_eq!(list(&db).unwrap().len(), 10);
    }

    #[test]
    fn user_preset_lifecycle() {
        let db = Db::open_in_memory().unwrap();
        seed(&db).unwrap();
        let p = save(
            &db,
            None,
            "soul",
            "Mine",
            &json!({"name": "Hal", "tone": "dry", "language": "English", "addendum": ""}),
        )
        .unwrap();
        activate(&db, p.id).unwrap();
        assert_eq!(active(&db, "soul").unwrap().name, "Mine");
        delete(&db, p.id).unwrap();
        assert_eq!(active(&db, "soul").unwrap().name, "Default");
        let builtin = active(&db, "soul").unwrap();
        assert!(delete(&db, builtin.id).is_err());
    }

    #[test]
    fn import_rejects_unknown_version() {
        let err = parse_import(r#"{"schema_version": 9, "slot": "soul", "name": "x", "data": {}}"#)
            .unwrap_err();
        assert!(err.message.contains("version 9"));
        let ok = parse_import(
            r##"{"schema_version": 1, "slot": "skin", "name": "x", "data": {"theme":"dark","accent":"#112233","radius":8,"opacity":0.8,"font_size":13}}"##,
        );
        assert!(ok.is_ok());
    }

    #[test]
    fn soul_addendum_limit() {
        let long = "x".repeat(501);
        assert!(validate(
            "soul",
            &json!({"name": "a", "tone": "b", "language": "c", "addendum": long})
        )
        .is_err());
    }
}
