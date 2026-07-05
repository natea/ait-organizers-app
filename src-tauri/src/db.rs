use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

use crate::error::AppResult;

/// Initialize the schema. Tables mirror specs/background-sync: events,
/// rsvp_summaries, awaiting_payment, performance_snapshots, sync_state.
pub fn init(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        r#"
        PRAGMA journal_mode = WAL;

        CREATE TABLE IF NOT EXISTS events (
            meetup_token   TEXT PRIMARY KEY,
            weblog_token   TEXT,
            starts_at_utc  TEXT,
            attending      INTEGER,
            waitlisted     INTEGER,
            paid           INTEGER,
            raw_json       TEXT NOT NULL,
            updated_at     TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS rsvp_summaries (
            meetup_token  TEXT PRIMARY KEY,
            total_count   INTEGER,
            groups_json   TEXT,
            updated_at    TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS awaiting_payment (
            meetup_token  TEXT PRIMARY KEY,
            count         INTEGER,
            results_json  TEXT,
            unavailable   INTEGER NOT NULL DEFAULT 0,
            updated_at    TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS performance_snapshots (
            meetup_token  TEXT PRIMARY KEY,
            perf_json     TEXT,
            unavailable   INTEGER NOT NULL DEFAULT 0,
            reason        TEXT,
            updated_at    TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS sync_state (
            key            TEXT PRIMARY KEY,
            last_fetch_at  TEXT,
            remaining      INTEGER,
            backoff_until  TEXT,
            unavailable    INTEGER NOT NULL DEFAULT 0,
            note           TEXT
        );
        "#,
    )?;
    Ok(())
}

pub fn upsert_event(
    conn: &Connection,
    meetup_token: &str,
    weblog_token: &str,
    starts_at_utc: &str,
    attending: i64,
    waitlisted: i64,
    paid: bool,
    raw: &Value,
    now: &str,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO events
           (meetup_token, weblog_token, starts_at_utc, attending, waitlisted, paid, raw_json, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(meetup_token) DO UPDATE SET
           weblog_token=excluded.weblog_token,
           starts_at_utc=excluded.starts_at_utc,
           attending=excluded.attending,
           waitlisted=excluded.waitlisted,
           paid=excluded.paid,
           raw_json=excluded.raw_json,
           updated_at=excluded.updated_at",
        params![
            meetup_token,
            weblog_token,
            starts_at_utc,
            attending,
            waitlisted,
            paid as i64,
            raw.to_string(),
            now
        ],
    )?;
    Ok(())
}

/// Previous (attending, waitlisted) for an event, for poll-diff notifications.
pub fn prev_counts(conn: &Connection, meetup_token: &str) -> AppResult<Option<(i64, i64)>> {
    let row = conn
        .query_row(
            "SELECT attending, waitlisted FROM events WHERE meetup_token = ?1",
            params![meetup_token],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
        )
        .optional()?;
    Ok(row)
}

/// Remove events no longer present upstream so the overview stays accurate.
pub fn retain_events(conn: &Connection, keep_tokens: &[String]) -> AppResult<()> {
    let existing: Vec<String> = {
        let mut stmt = conn.prepare("SELECT meetup_token FROM events")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.filter_map(Result::ok).collect()
    };
    for token in existing {
        if !keep_tokens.iter().any(|k| k == &token) {
            conn.execute("DELETE FROM events WHERE meetup_token = ?1", params![token])?;
        }
    }
    Ok(())
}

pub fn upsert_performance(
    conn: &Connection,
    meetup_token: &str,
    perf: Option<&Value>,
    unavailable: bool,
    reason: Option<&str>,
    now: &str,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO performance_snapshots (meetup_token, perf_json, unavailable, reason, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(meetup_token) DO UPDATE SET
           perf_json=excluded.perf_json,
           unavailable=excluded.unavailable,
           reason=excluded.reason,
           updated_at=excluded.updated_at",
        params![
            meetup_token,
            perf.map(|v| v.to_string()),
            unavailable as i64,
            reason,
            now
        ],
    )?;
    Ok(())
}

pub fn upsert_awaiting(
    conn: &Connection,
    meetup_token: &str,
    count: i64,
    results: Option<&Value>,
    unavailable: bool,
    now: &str,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO awaiting_payment (meetup_token, count, results_json, unavailable, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(meetup_token) DO UPDATE SET
           count=excluded.count,
           results_json=excluded.results_json,
           unavailable=excluded.unavailable,
           updated_at=excluded.updated_at",
        params![
            meetup_token,
            count,
            results.map(|v| v.to_string()),
            unavailable as i64,
            now
        ],
    )?;
    Ok(())
}

pub fn upsert_summary(
    conn: &Connection,
    meetup_token: &str,
    total_count: i64,
    groups: Option<&Value>,
    now: &str,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO rsvp_summaries (meetup_token, total_count, groups_json, updated_at)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(meetup_token) DO UPDATE SET
           total_count=excluded.total_count,
           groups_json=excluded.groups_json,
           updated_at=excluded.updated_at",
        params![meetup_token, total_count, groups.map(|v| v.to_string()), now],
    )?;
    Ok(())
}

/// All cached events, newest-soonest first, as their raw API JSON.
pub fn get_events(conn: &Connection) -> AppResult<Vec<Value>> {
    let mut stmt = conn.prepare(
        "SELECT raw_json FROM events ORDER BY (starts_at_utc IS NULL), starts_at_utc ASC",
    )?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    let mut out = Vec::new();
    for raw in rows.filter_map(Result::ok) {
        if let Ok(v) = serde_json::from_str::<Value>(&raw) {
            out.push(v);
        }
    }
    Ok(out)
}

/// One event merged with its detail (performance + awaiting-payment + summary).
pub fn get_event_detail(conn: &Connection, meetup_token: &str) -> AppResult<Option<Value>> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT raw_json FROM events WHERE meetup_token = ?1",
            params![meetup_token],
            |r| r.get(0),
        )
        .optional()?;
    let Some(raw) = raw else { return Ok(None) };
    let mut event: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);

    let perf = conn
        .query_row(
            "SELECT perf_json, unavailable, reason FROM performance_snapshots WHERE meetup_token = ?1",
            params![meetup_token],
            |r| {
                Ok(json!({
                    "perf": r.get::<_, Option<String>>(0)?
                        .and_then(|s| serde_json::from_str::<Value>(&s).ok()),
                    "unavailable": r.get::<_, i64>(1)? != 0,
                    "reason": r.get::<_, Option<String>>(2)?,
                }))
            },
        )
        .optional()?;

    let awaiting = conn
        .query_row(
            "SELECT count, results_json, unavailable FROM awaiting_payment WHERE meetup_token = ?1",
            params![meetup_token],
            |r| {
                Ok(json!({
                    "count": r.get::<_, i64>(0)?,
                    "results": r.get::<_, Option<String>>(1)?
                        .and_then(|s| serde_json::from_str::<Value>(&s).ok()),
                    "unavailable": r.get::<_, i64>(2)? != 0,
                }))
            },
        )
        .optional()?;

    let summary = conn
        .query_row(
            "SELECT total_count, groups_json FROM rsvp_summaries WHERE meetup_token = ?1",
            params![meetup_token],
            |r| {
                Ok(json!({
                    "total_count": r.get::<_, i64>(0)?,
                    "groups": r.get::<_, Option<String>>(1)?
                        .and_then(|s| serde_json::from_str::<Value>(&s).ok()),
                }))
            },
        )
        .optional()?;

    if let Value::Object(ref mut map) = event {
        map.insert("performance".into(), perf.unwrap_or(Value::Null));
        map.insert("awaiting_payment".into(), awaiting.unwrap_or(Value::Null));
        map.insert("rsvp_summary".into(), summary.unwrap_or(Value::Null));
    }
    Ok(Some(event))
}

pub fn set_sync_state(
    conn: &Connection,
    key: &str,
    last_fetch_at: Option<&str>,
    remaining: Option<i64>,
    backoff_until: Option<&str>,
    unavailable: bool,
    note: Option<&str>,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO sync_state (key, last_fetch_at, remaining, backoff_until, unavailable, note)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(key) DO UPDATE SET
           last_fetch_at=COALESCE(excluded.last_fetch_at, sync_state.last_fetch_at),
           remaining=excluded.remaining,
           backoff_until=excluded.backoff_until,
           unavailable=excluded.unavailable,
           note=excluded.note",
        params![key, last_fetch_at, remaining, backoff_until, unavailable as i64, note],
    )?;
    Ok(())
}

/// backoff_until timestamp for an endpoint key, if currently backed off.
pub fn get_backoff(conn: &Connection, key: &str) -> AppResult<Option<String>> {
    let v = conn
        .query_row(
            "SELECT backoff_until FROM sync_state WHERE key = ?1",
            params![key],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten();
    Ok(v)
}

/// Feature availability map for the UI (background-sync degradation).
pub fn feature_states(conn: &Connection) -> AppResult<Value> {
    let mut stmt =
        conn.prepare("SELECT key, unavailable, note, last_fetch_at FROM sync_state")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)? != 0,
            r.get::<_, Option<String>>(2)?,
            r.get::<_, Option<String>>(3)?,
        ))
    })?;
    let mut map = serde_json::Map::new();
    for (key, unavailable, note, last) in rows.filter_map(Result::ok) {
        map.insert(
            key,
            json!({ "unavailable": unavailable, "note": note, "last_fetch_at": last }),
        );
    }
    Ok(Value::Object(map))
}
