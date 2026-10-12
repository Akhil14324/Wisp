//! The structured `when` format (section 5.5) and next-run calculation.

use std::str::FromStr;

use chrono::{DateTime, Local, NaiveDate, NaiveTime, TimeZone};
use croner::Cron;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Once,
    Daily,
    Weekdays,
    Weekly,
    Monthly,
    Cron,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct When {
    pub kind: Kind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub days: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub day_of_month: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cron: Option<String>,
}

const DAYS: &[&str] = &["sun", "mon", "tue", "wed", "thu", "fri", "sat"];

fn parse_time(s: &str) -> Result<NaiveTime, String> {
    NaiveTime::parse_from_str(s.trim(), "%H:%M").map_err(|_| format!("time '{s}' must be HH:MM"))
}

fn parse_date(s: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d")
        .map_err(|_| format!("date '{s}' must be YYYY-MM-DD"))
}

fn day_index(d: &str) -> Result<usize, String> {
    let d = d.trim().to_ascii_lowercase();
    let short = d.get(..3).unwrap_or(&d);
    DAYS.iter()
        .position(|x| *x == short)
        .ok_or_else(|| format!("unknown day '{d}'"))
}

/// Resolves a local wall-clock time, picking the earlier instant on DST overlaps.
fn local(date: NaiveDate, time: NaiveTime) -> Option<DateTime<Local>> {
    Local.from_local_datetime(&date.and_time(time)).earliest()
}

impl When {
    /// Builds the cron pattern for repeating kinds.
    pub fn cron_pattern(&self) -> Result<String, String> {
        let hm = |w: &When| -> Result<(u32, u32), String> {
            let t = parse_time(w.time.as_deref().ok_or("time is required")?)?;
            use chrono::Timelike;
            Ok((t.hour(), t.minute()))
        };
        Ok(match self.kind {
            Kind::Once => return Err("once has no cron pattern".into()),
            Kind::Daily => {
                let (h, m) = hm(self)?;
                format!("{m} {h} * * *")
            }
            Kind::Weekdays => {
                let (h, m) = hm(self)?;
                format!("{m} {h} * * 1-5")
            }
            Kind::Weekly => {
                let (h, m) = hm(self)?;
                let days = self
                    .days
                    .as_ref()
                    .filter(|d| !d.is_empty())
                    .ok_or("days are required")?;
                let mut idx: Vec<usize> = days
                    .iter()
                    .map(|d| day_index(d))
                    .collect::<Result<_, _>>()?;
                idx.sort_unstable();
                idx.dedup();
                let list: Vec<String> = idx.iter().map(|i| i.to_string()).collect();
                format!("{m} {h} * * {}", list.join(","))
            }
            Kind::Monthly => {
                let (h, m) = hm(self)?;
                let dom = self.day_of_month.ok_or("day_of_month is required")?;
                if !(1..=31).contains(&dom) {
                    return Err("day_of_month must be 1 to 31".into());
                }
                format!("{m} {h} {dom} * *")
            }
            Kind::Cron => {
                let c = self
                    .cron
                    .as_deref()
                    .ok_or("cron is required")?
                    .trim()
                    .to_string();
                if c.split_whitespace().count() != 5 {
                    return Err("cron must have 5 fields".into());
                }
                c
            }
        })
    }

    /// Validates required fields; rejects one-time dates in the past (section 5.5).
    pub fn validate(&self, now: DateTime<Local>) -> Result<(), String> {
        match self.kind {
            Kind::Once => {
                let d = parse_date(self.date.as_deref().ok_or("date is required for once")?)?;
                let t = parse_time(self.time.as_deref().ok_or("time is required for once")?)?;
                let at = local(d, t).ok_or("that local time does not exist")?;
                if at <= now {
                    return Err("that date and time is in the past".into());
                }
                Ok(())
            }
            _ => {
                let p = self.cron_pattern()?;
                Cron::from_str(&p).map_err(|e| format!("invalid schedule: {e}"))?;
                Ok(())
            }
        }
    }

    /// The first run strictly after `after`, or None when the job is finished.
    pub fn next_after(&self, after: DateTime<Local>) -> Option<DateTime<Local>> {
        match self.kind {
            Kind::Once => {
                let d = parse_date(self.date.as_deref()?).ok()?;
                let t = parse_time(self.time.as_deref()?).ok()?;
                local(d, t).filter(|at| *at > after)
            }
            _ => {
                let cron = Cron::from_str(&self.cron_pattern().ok()?).ok()?;
                cron.find_next_occurrence(&after, false).ok()
            }
        }
    }

    /// Short human description for lists and confirmation cards.
    pub fn describe(&self) -> String {
        let t = self.time.clone().unwrap_or_default();
        match self.kind {
            Kind::Once => format!("Once on {} at {t}", self.date.clone().unwrap_or_default()),
            Kind::Daily => format!("Every day at {t}"),
            Kind::Weekdays => format!("Weekdays at {t}"),
            Kind::Weekly => format!(
                "Every {} at {t}",
                self.days.clone().unwrap_or_default().join(", ")
            ),
            Kind::Monthly => format!("Monthly on day {} at {t}", self.day_of_month.unwrap_or(1)),
            Kind::Cron => format!("Cron {}", self.cron.clone().unwrap_or_default()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn at(s: &str) -> DateTime<Local> {
        let n = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap();
        Local.from_local_datetime(&n).earliest().unwrap()
    }

    fn when(v: serde_json::Value) -> When {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn daily_next_run() {
        let w = when(json!({"kind": "daily", "time": "09:00"}));
        assert_eq!(
            w.next_after(at("2026-10-12 08:00")).unwrap(),
            at("2026-10-12 09:00")
        );
        assert_eq!(
            w.next_after(at("2026-10-12 09:00")).unwrap(),
            at("2026-10-13 09:00")
        );
    }

    #[test]
    fn weekdays_skip_weekend() {
        // 2026-10-16 is a Friday.
        let w = when(json!({"kind": "weekdays", "time": "07:30"}));
        assert_eq!(
            w.next_after(at("2026-10-16 08:00")).unwrap(),
            at("2026-10-19 07:30")
        );
    }

    #[test]
    fn weekly_days() {
        // 2026-10-12 is a Monday.
        let w = when(json!({"kind": "weekly", "time": "18:00", "days": ["wed", "mon"]}));
        assert_eq!(w.cron_pattern().unwrap(), "0 18 * * 1,3");
        assert_eq!(
            w.next_after(at("2026-10-12 19:00")).unwrap(),
            at("2026-10-14 18:00")
        );
    }

    #[test]
    fn monthly_and_cron() {
        let w = when(json!({"kind": "monthly", "time": "10:15", "day_of_month": 15}));
        assert_eq!(
            w.next_after(at("2026-10-16 00:00")).unwrap(),
            at("2026-11-15 10:15")
        );
        let c = when(json!({"kind": "cron", "cron": "0 9 * * 1-5"}));
        assert_eq!(
            c.next_after(at("2026-10-12 10:00")).unwrap(),
            at("2026-10-13 09:00")
        );
    }

    #[test]
    fn once_rejects_past_and_finishes() {
        let w = when(json!({"kind": "once", "date": "2026-11-10", "time": "11:00"}));
        assert!(w.validate(at("2026-10-12 09:00")).is_ok());
        assert!(w.validate(at("2026-11-11 09:00")).is_err());
        assert_eq!(
            w.next_after(at("2026-10-12 09:00")).unwrap(),
            at("2026-11-10 11:00")
        );
        assert!(w.next_after(at("2026-11-10 11:00")).is_none());
    }

    #[test]
    fn required_fields() {
        let now = at("2026-10-12 09:00");
        assert!(when(json!({"kind": "once", "time": "11:00"}))
            .validate(now)
            .is_err());
        assert!(when(json!({"kind": "daily"})).validate(now).is_err());
        assert!(when(json!({"kind": "weekly", "time": "10:00"}))
            .validate(now)
            .is_err());
        assert!(
            when(json!({"kind": "monthly", "time": "10:00", "day_of_month": 40}))
                .validate(now)
                .is_err()
        );
        assert!(when(json!({"kind": "cron", "cron": "bad"}))
            .validate(now)
            .is_err());
        assert!(when(json!({"kind": "daily", "time": "25:00"}))
            .validate(now)
            .is_err());
    }
}
