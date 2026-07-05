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
            kind           TEXT NOT NULL DEFAULT 'upcoming',
            raw_json       TEXT NOT NULL,
            updated_at     TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS rsvp_summaries (
            meetup_token  TEXT PRIMARY KEY,
            total_count   INTEGER,
            checked_in    INTEGER,
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

        CREATE TABLE IF NOT EXISTS content_pages (
            meetup_token  TEXT PRIMARY KEY,
            page_json     TEXT,
            metrics_json  TEXT,
            unavailable   INTEGER NOT NULL DEFAULT 0,
            reason        TEXT,
            updated_at    TEXT NOT NULL
        );
        "#,
    )?;
    // Migration for caches created before the `kind` column existed. ALTER
    // errors with "duplicate column name" on already-migrated DBs — ignore it.
    let _ = conn.execute(
        "ALTER TABLE events ADD COLUMN kind TEXT NOT NULL DEFAULT 'upcoming'",
        [],
    );
    // Real check-in count (rsvps/summary status=checked_in). ALTER errors on
    // already-migrated DBs — ignore.
    let _ = conn.execute("ALTER TABLE rsvp_summaries ADD COLUMN checked_in INTEGER", []);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn upsert_event(
    conn: &Connection,
    meetup_token: &str,
    weblog_token: &str,
    starts_at_utc: &str,
    attending: i64,
    waitlisted: i64,
    paid: bool,
    kind: &str,
    raw: &Value,
    now: &str,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO events
           (meetup_token, weblog_token, starts_at_utc, attending, waitlisted, paid, kind, raw_json, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(meetup_token) DO UPDATE SET
           weblog_token=excluded.weblog_token,
           starts_at_utc=excluded.starts_at_utc,
           attending=excluded.attending,
           waitlisted=excluded.waitlisted,
           paid=excluded.paid,
           kind=excluded.kind,
           raw_json=excluded.raw_json,
           updated_at=excluded.updated_at",
        params![
            meetup_token,
            weblog_token,
            starts_at_utc,
            attending,
            waitlisted,
            paid as i64,
            kind,
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

/// Remove events of one `kind` no longer present upstream. Scoping by kind is
/// required so an upcoming refresh never evicts cached past events, and vice
/// versa (specs/past-events).
pub fn retain_events(conn: &Connection, kind: &str, keep_tokens: &[String]) -> AppResult<()> {
    let existing: Vec<String> = {
        let mut stmt = conn.prepare("SELECT meetup_token FROM events WHERE kind = ?1")?;
        let rows = stmt.query_map(params![kind], |r| r.get::<_, String>(0))?;
        rows.filter_map(Result::ok).collect()
    };
    for token in existing {
        if !keep_tokens.iter().any(|k| k == &token) {
            conn.execute(
                "DELETE FROM events WHERE meetup_token = ?1 AND kind = ?2",
                params![token, kind],
            )?;
        }
    }
    Ok(())
}

/// True when a token is already cached under a different kind — used to keep an
/// upcoming row from being shadowed by a past fetch around start time.
pub fn event_kind(conn: &Connection, meetup_token: &str) -> AppResult<Option<String>> {
    let v = conn
        .query_row(
            "SELECT kind FROM events WHERE meetup_token = ?1",
            params![meetup_token],
            |r| r.get::<_, String>(0),
        )
        .optional()?;
    Ok(v)
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
    checked_in: Option<i64>,
    groups: Option<&Value>,
    now: &str,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO rsvp_summaries (meetup_token, total_count, checked_in, groups_json, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(meetup_token) DO UPDATE SET
           total_count=excluded.total_count,
           checked_in=COALESCE(excluded.checked_in, rsvp_summaries.checked_in),
           groups_json=excluded.groups_json,
           updated_at=excluded.updated_at",
        params![
            meetup_token,
            total_count,
            checked_in,
            groups.map(|v| v.to_string()),
            now
        ],
    )?;
    Ok(())
}

/// All cached events with their `kind` injected. Upcoming sort soonest-first;
/// past sort most-recent-first (the frontend filters by the active tab).
pub fn get_events(conn: &Connection) -> AppResult<Vec<Value>> {
    let mut stmt = conn.prepare(
        "SELECT raw_json, kind FROM events
         ORDER BY
           CASE kind WHEN 'upcoming' THEN 0 ELSE 1 END,
           (starts_at_utc IS NULL),
           CASE WHEN kind = 'past' THEN starts_at_utc END DESC,
           starts_at_utc ASC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;
    let mut out = Vec::new();
    for (raw, kind) in rows.filter_map(Result::ok) {
        if let Ok(mut v) = serde_json::from_str::<Value>(&raw) {
            if let Value::Object(ref mut map) = v {
                map.insert("kind".into(), Value::String(kind));
            }
            out.push(v);
        }
    }
    Ok(out)
}

/// One event merged with its detail (performance + awaiting-payment + summary).
pub fn get_event_detail(conn: &Connection, meetup_token: &str) -> AppResult<Option<Value>> {
    let row: Option<(String, String)> = conn
        .query_row(
            "SELECT raw_json, kind FROM events WHERE meetup_token = ?1",
            params![meetup_token],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((raw, kind)) = row else { return Ok(None) };
    let mut event: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);
    if let Value::Object(ref mut map) = event {
        map.insert("kind".into(), Value::String(kind));
    }

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
            "SELECT total_count, checked_in, groups_json FROM rsvp_summaries WHERE meetup_token = ?1",
            params![meetup_token],
            |r| {
                Ok(json!({
                    "total_count": r.get::<_, i64>(0)?,
                    "checked_in": r.get::<_, Option<i64>>(1)?,
                    "groups": r.get::<_, Option<String>>(2)?
                        .and_then(|s| serde_json::from_str::<Value>(&s).ok()),
                }))
            },
        )
        .optional()?;

    let content_page = conn
        .query_row(
            "SELECT page_json, metrics_json, unavailable, reason FROM content_pages WHERE meetup_token = ?1",
            params![meetup_token],
            |r| {
                Ok(json!({
                    "page": r.get::<_, Option<String>>(0)?
                        .and_then(|s| serde_json::from_str::<Value>(&s).ok()),
                    "metrics": r.get::<_, Option<String>>(1)?
                        .and_then(|s| serde_json::from_str::<Value>(&s).ok()),
                    "unavailable": r.get::<_, i64>(2)? != 0,
                    "reason": r.get::<_, Option<String>>(3)?,
                }))
            },
        )
        .optional()?;

    if let Value::Object(ref mut map) = event {
        map.insert("performance".into(), perf.unwrap_or(Value::Null));
        map.insert("awaiting_payment".into(), awaiting.unwrap_or(Value::Null));
        map.insert("rsvp_summary".into(), summary.unwrap_or(Value::Null));
        map.insert("content_page".into(), content_page.unwrap_or(Value::Null));
    }
    Ok(Some(event))
}

/// Cache the content page + email metrics for an event (specs/event-page-view).
pub fn upsert_content_page(
    conn: &Connection,
    meetup_token: &str,
    page: Option<&Value>,
    metrics: Option<&Value>,
    unavailable: bool,
    reason: Option<&str>,
    now: &str,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO content_pages (meetup_token, page_json, metrics_json, unavailable, reason, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(meetup_token) DO UPDATE SET
           page_json=COALESCE(excluded.page_json, content_pages.page_json),
           metrics_json=COALESCE(excluded.metrics_json, content_pages.metrics_json),
           unavailable=excluded.unavailable,
           reason=excluded.reason,
           updated_at=excluded.updated_at",
        params![
            meetup_token,
            page.map(|v| v.to_string()),
            metrics.map(|v| v.to_string()),
            unavailable as i64,
            reason,
            now
        ],
    )?;
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        c
    }

    fn insert(c: &Connection, token: &str, kind: &str) {
        upsert_event(c, token, "blog_x", "2026-01-01T00:00:00Z", 1, 0, false, kind, &json!({"meetup_token": token, "event_name": token}), "now").unwrap();
    }

    #[test]
    fn upcoming_retention_preserves_past_events() {
        let c = mem();
        insert(&c, "up1", "upcoming");
        insert(&c, "past1", "past");
        insert(&c, "past2", "past");

        // An upcoming refresh that keeps nothing must not touch past rows.
        retain_events(&c, "upcoming", &[]).unwrap();

        let kinds: Vec<String> = get_events(&c)
            .unwrap()
            .iter()
            .map(|e| e.get("kind").and_then(Value::as_str).unwrap_or("").to_string())
            .collect();
        assert_eq!(kinds.iter().filter(|k| *k == "past").count(), 2, "past events must survive an upcoming retain");
        assert_eq!(kinds.iter().filter(|k| *k == "upcoming").count(), 0, "upcoming events were retained-out");
    }

    #[test]
    fn past_retention_preserves_upcoming_events() {
        let c = mem();
        insert(&c, "up1", "upcoming");
        insert(&c, "past1", "past");
        retain_events(&c, "past", &[]).unwrap();
        let kinds: Vec<String> = get_events(&c)
            .unwrap()
            .iter()
            .map(|e| e.get("kind").and_then(Value::as_str).unwrap_or("").to_string())
            .collect();
        assert_eq!(kinds, vec!["upcoming".to_string()], "upcoming must survive a past retain");
    }
}
