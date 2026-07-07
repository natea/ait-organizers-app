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

        -- Email lifecycle cache (specs/email-lifecycle). Aggregates only; no
        -- recipient rows or email addresses are ever stored.
        CREATE TABLE IF NOT EXISTS email_send_jobs (
            token             TEXT PRIMARY KEY,
            meetup_token      TEXT,
            weblog_token      TEXT,
            content_page_token TEXT,
            subject           TEXT,
            status            TEXT,
            distribution_option TEXT,
            sent_count        INTEGER,
            pending_count     INTEGER,
            suppressed_count  INTEGER,
            intended_count    INTEGER,
            delivered_percent REAL,
            observed_rate     REAL,
            predicted_finish  TEXT,
            done              INTEGER NOT NULL DEFAULT 0,
            partition         TEXT NOT NULL DEFAULT 'chapter',
            raw_json          TEXT,
            fetched_at        TEXT NOT NULL
        );

        -- Per-event aggregate summary + campaign open/click rates.
        CREATE TABLE IF NOT EXISTS email_event_summary (
            meetup_token  TEXT PRIMARY KEY,
            summary_json  TEXT,
            campaign_json TEXT,
            unavailable   INTEGER NOT NULL DEFAULT 0,
            reason        TEXT,
            updated_at    TEXT NOT NULL
        );

        -- Active-send throughput series + progress, keyed by send-job token.
        CREATE TABLE IF NOT EXISTS email_throughput (
            token         TEXT PRIMARY KEY,
            throughput_json TEXT,
            progress_json TEXT,
            peak_rate     REAL,
            average_rate  REAL,
            total_sent    INTEGER,
            done          INTEGER NOT NULL DEFAULT 0,
            updated_at    TEXT NOT NULL
        );

        -- Chapter deliverability (singleton row): health + sender-domain rows +
        -- fatigue tier summary. No per-subscriber rows.
        CREATE TABLE IF NOT EXISTS email_deliverability (
            id            INTEGER PRIMARY KEY CHECK (id = 1),
            health_json   TEXT,
            fatigue_json  TEXT,
            truncated     INTEGER NOT NULL DEFAULT 0,
            unavailable   INTEGER NOT NULL DEFAULT 0,
            reason        TEXT,
            updated_at    TEXT NOT NULL
        );

        -- Post-event survey + follow-up email engagement (specs/survey-followup).
        -- One row per meetup_token; per-source status lets the panel degrade the
        -- survey and email sub-sections independently. Only populated for past
        -- events, fetched on detail-open/manual-refresh — never the upcoming poll.
        CREATE TABLE IF NOT EXISTS survey_followup (
            meetup_token  TEXT PRIMARY KEY,
            survey_json   TEXT,
            survey_status TEXT NOT NULL DEFAULT 'unavailable',
            email_json    TEXT,
            email_status  TEXT NOT NULL DEFAULT 'unavailable',
            updated_at    TEXT NOT NULL
        );

        -- Promotion tools (specs/promotion-tools). Latest generated draft per
        -- event/kind/platform (`platform` is '' for kinds that aren't
        -- per-platform, e.g. event_promo/discussion_topics) — a regeneration
        -- upserts only its own (meetup_token, kind, platform) row so platforms
        -- never clobber each other (design D3).
        CREATE TABLE IF NOT EXISTS promotion_drafts (
            meetup_token  TEXT NOT NULL,
            kind          TEXT NOT NULL,
            platform      TEXT NOT NULL DEFAULT '',
            params_json   TEXT,
            result_json   TEXT,
            generated_at  TEXT NOT NULL,
            PRIMARY KEY (meetup_token, kind, platform)
        );

        -- Tracked async generation jobs (design D2). Never polled — created
        -- only on an explicit user-initiated kickoff (promotion_generate).
        CREATE TABLE IF NOT EXISTS promotion_jobs (
            id            TEXT PRIMARY KEY,
            meetup_token  TEXT NOT NULL,
            kind          TEXT NOT NULL,
            platform      TEXT NOT NULL DEFAULT '',
            params_hash   TEXT,
            status        TEXT NOT NULL DEFAULT 'pending',
            started_at    TEXT NOT NULL,
            error_code    TEXT
        );

        -- Logo search results are a cheap GET, not a billed generation, so they
        -- get a short freshness-window cache keyed by query params (design D3).
        CREATE TABLE IF NOT EXISTS logo_search_cache (
            query               TEXT NOT NULL,
            scope               TEXT NOT NULL,
            include_co_branded  INTEGER NOT NULL,
            result_json         TEXT,
            fetched_at          TEXT NOT NULL,
            PRIMARY KEY (query, scope, include_co_branded)
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

// ── Email lifecycle cache (specs/email-lifecycle) ──────────────────────────

/// Read a numeric field tolerating several fallback names and string encodings.
pub(crate) fn pick_num(v: &Value, names: &[&str]) -> Option<f64> {
    for n in names {
        if let Some(x) = v.get(n) {
            if let Some(f) = x.as_f64() {
                return Some(f);
            }
            if let Some(s) = x.as_str() {
                if let Ok(f) = s.parse::<f64>() {
                    return Some(f);
                }
            }
        }
    }
    None
}

pub(crate) fn pick_str<'a>(v: &'a Value, names: &[&str]) -> Option<&'a str> {
    names.iter().find_map(|n| v.get(n).and_then(Value::as_str))
}

/// Upsert one send-job row, extracting aggregate fields defensively (field
/// shapes are unverifiable live, so every name has fallbacks). Completed rows
/// are frozen: once `done=1` the row is not overwritten (design D4/task 2.5).
pub fn upsert_send_job(
    conn: &Connection,
    job: &Value,
    meetup_token: Option<&str>,
    partition: &str,
    now: &str,
) -> AppResult<()> {
    let Some(token) = pick_str(job, &["token", "send_job_token", "id"]) else {
        return Ok(());
    };
    let status = pick_str(job, &["status", "state"]).unwrap_or("unknown");
    let done_flag = job
        .get("done")
        .and_then(Value::as_bool)
        .unwrap_or_else(|| matches!(status, "completed" | "failed" | "cancelled"));
    // Prefer refs.meetup_token from the row, else the caller-supplied scope.
    let row_meetup = pick_str(job, &["meetup_token"]).or_else(|| {
        job.get("refs")
            .and_then(|r| r.get("meetup_token"))
            .and_then(Value::as_str)
    });
    let meetup = row_meetup.or(meetup_token);
    let weblog = pick_str(job, &["weblog_token"]).or_else(|| {
        job.get("refs")
            .and_then(|r| r.get("weblog_token"))
            .and_then(Value::as_str)
    });
    let cpt = pick_str(job, &["content_page_token"]).or_else(|| {
        job.get("refs")
            .and_then(|r| r.get("content_page_token"))
            .and_then(Value::as_str)
    });
    let subject = pick_str(job, &["subject", "campaign_label", "title"]);
    let distribution = pick_str(job, &["distribution_option", "distribution"]);
    let sent = pick_num(job, &["sent_count", "sent"]);
    let pending = pick_num(job, &["pending_count", "pending"]);
    let suppressed = pick_num(job, &["suppressed_count", "suppressed"]);
    let intended = pick_num(job, &["intended_recipient_count", "intended_count", "intended"]);
    let delivered_pct = pick_num(job, &["delivered_percent", "delivered_pct", "delivery_rate"]);
    let observed = pick_num(
        job,
        &["observed_send_rate_per_minute", "observed_rate_per_minute", "observed_send_rate"],
    );
    let predicted = pick_str(job, &["predicted_finish_at", "predicted_finish"]);

    conn.execute(
        "INSERT INTO email_send_jobs
           (token, meetup_token, weblog_token, content_page_token, subject, status,
            distribution_option, sent_count, pending_count, suppressed_count,
            intended_count, delivered_percent, observed_rate, predicted_finish,
            done, partition, raw_json, fetched_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)
         ON CONFLICT(token) DO UPDATE SET
           meetup_token=COALESCE(excluded.meetup_token, email_send_jobs.meetup_token),
           weblog_token=COALESCE(excluded.weblog_token, email_send_jobs.weblog_token),
           content_page_token=COALESCE(excluded.content_page_token, email_send_jobs.content_page_token),
           subject=excluded.subject,
           status=excluded.status,
           distribution_option=excluded.distribution_option,
           sent_count=excluded.sent_count,
           pending_count=excluded.pending_count,
           suppressed_count=excluded.suppressed_count,
           intended_count=excluded.intended_count,
           delivered_percent=excluded.delivered_percent,
           observed_rate=excluded.observed_rate,
           predicted_finish=excluded.predicted_finish,
           done=excluded.done,
           partition=excluded.partition,
           raw_json=excluded.raw_json,
           fetched_at=excluded.fetched_at
         WHERE email_send_jobs.done = 0",
        params![
            token,
            meetup,
            weblog,
            cpt,
            subject,
            status,
            distribution,
            sent,
            pending,
            suppressed,
            intended,
            delivered_pct,
            observed,
            predicted,
            done_flag as i64,
            partition,
            job.to_string(),
            now,
        ],
    )?;
    Ok(())
}

/// Remove send jobs of one partition no longer returned upstream, so a chapter
/// refresh never evicts event-scoped jobs and vice versa (task 2.5).
pub fn retain_send_jobs(conn: &Connection, partition: &str, keep: &[String]) -> AppResult<()> {
    let existing: Vec<String> = {
        let mut stmt =
            conn.prepare("SELECT token FROM email_send_jobs WHERE partition = ?1")?;
        let rows = stmt.query_map(params![partition], |r| r.get::<_, String>(0))?;
        rows.filter_map(Result::ok).collect()
    };
    for token in existing {
        if !keep.iter().any(|k| k == &token) {
            conn.execute(
                "DELETE FROM email_send_jobs WHERE token = ?1 AND partition = ?2",
                params![token, partition],
            )?;
        }
    }
    Ok(())
}

/// True when a send job is cached and already terminal (freeze / stop polling).
pub fn send_job_done(conn: &Connection, token: &str) -> AppResult<bool> {
    let v = conn
        .query_row(
            "SELECT done FROM email_send_jobs WHERE token = ?1",
            params![token],
            |r| r.get::<_, i64>(0),
        )
        .optional()?;
    Ok(v == Some(1))
}

/// Serialize a cached send-job row back to JSON for the frontend.
fn send_job_row(r: &rusqlite::Row) -> rusqlite::Result<Value> {
    Ok(json!({
        "token": r.get::<_, String>(0)?,
        "meetup_token": r.get::<_, Option<String>>(1)?,
        "subject": r.get::<_, Option<String>>(2)?,
        "status": r.get::<_, Option<String>>(3)?,
        "distribution_option": r.get::<_, Option<String>>(4)?,
        "sent_count": r.get::<_, Option<f64>>(5)?,
        "pending_count": r.get::<_, Option<f64>>(6)?,
        "suppressed_count": r.get::<_, Option<f64>>(7)?,
        "intended_count": r.get::<_, Option<f64>>(8)?,
        "delivered_percent": r.get::<_, Option<f64>>(9)?,
        "observed_rate": r.get::<_, Option<f64>>(10)?,
        "predicted_finish": r.get::<_, Option<String>>(11)?,
        "done": r.get::<_, i64>(12)? != 0,
        "fetched_at": r.get::<_, String>(13)?,
    }))
}

const SEND_JOB_COLS: &str = "token, meetup_token, subject, status, distribution_option,
    sent_count, pending_count, suppressed_count, intended_count, delivered_percent,
    observed_rate, predicted_finish, done, fetched_at";

/// Cache the per-event summary + campaign performance (either may be absent).
pub fn upsert_event_summary(
    conn: &Connection,
    meetup_token: &str,
    summary: Option<&Value>,
    campaign: Option<&Value>,
    unavailable: bool,
    reason: Option<&str>,
    now: &str,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO email_event_summary
           (meetup_token, summary_json, campaign_json, unavailable, reason, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(meetup_token) DO UPDATE SET
           summary_json=COALESCE(excluded.summary_json, email_event_summary.summary_json),
           campaign_json=COALESCE(excluded.campaign_json, email_event_summary.campaign_json),
           unavailable=excluded.unavailable,
           reason=excluded.reason,
           updated_at=excluded.updated_at",
        params![
            meetup_token,
            summary.map(|v| v.to_string()),
            campaign.map(|v| v.to_string()),
            unavailable as i64,
            reason,
            now
        ],
    )?;
    Ok(())
}

/// True when the event's campaign performance is already cached (so gentle
/// polling can skip re-fetching slow-moving open/click rates).
pub fn has_campaign(conn: &Connection, meetup_token: &str) -> AppResult<bool> {
    let v = conn
        .query_row(
            "SELECT campaign_json IS NOT NULL FROM email_event_summary WHERE meetup_token = ?1",
            params![meetup_token],
            |r| r.get::<_, i64>(0),
        )
        .optional()?;
    Ok(v == Some(1))
}

/// Cached email surface for one event: summary, campaign rates, its send jobs.
pub fn get_event_email(conn: &Connection, meetup_token: &str) -> AppResult<Value> {
    let head = conn
        .query_row(
            "SELECT summary_json, campaign_json, unavailable, reason, updated_at
             FROM email_event_summary WHERE meetup_token = ?1",
            params![meetup_token],
            |r| {
                Ok(json!({
                    "summary": r.get::<_, Option<String>>(0)?
                        .and_then(|s| serde_json::from_str::<Value>(&s).ok()),
                    "campaign": r.get::<_, Option<String>>(1)?
                        .and_then(|s| serde_json::from_str::<Value>(&s).ok()),
                    "unavailable": r.get::<_, i64>(2)? != 0,
                    "reason": r.get::<_, Option<String>>(3)?,
                    "updated_at": r.get::<_, Option<String>>(4)?,
                }))
            },
        )
        .optional()?;

    let mut jobs = Vec::new();
    {
        let sql = format!(
            "SELECT {SEND_JOB_COLS} FROM email_send_jobs WHERE meetup_token = ?1
             ORDER BY (done = 0) DESC, fetched_at DESC"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![meetup_token], send_job_row)?;
        for j in rows.filter_map(Result::ok) {
            jobs.push(j);
        }
    }

    let mut out = head.unwrap_or_else(|| {
        json!({ "summary": Value::Null, "campaign": Value::Null, "unavailable": false, "reason": Value::Null, "updated_at": Value::Null })
    });
    if let Value::Object(ref mut map) = out {
        map.insert("send_jobs".into(), Value::Array(jobs));
        map.insert("meetup_token".into(), Value::String(meetup_token.to_string()));
    }
    Ok(out)
}

/// Cache throughput series + progress for a send job; freeze when done.
pub fn upsert_throughput(
    conn: &Connection,
    token: &str,
    throughput: Option<&Value>,
    progress: Option<&Value>,
    peak: Option<f64>,
    average: Option<f64>,
    total_sent: Option<f64>,
    done: bool,
    now: &str,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO email_throughput
           (token, throughput_json, progress_json, peak_rate, average_rate, total_sent, done, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(token) DO UPDATE SET
           throughput_json=excluded.throughput_json,
           progress_json=excluded.progress_json,
           peak_rate=excluded.peak_rate,
           average_rate=excluded.average_rate,
           total_sent=excluded.total_sent,
           done=excluded.done,
           updated_at=excluded.updated_at
         WHERE email_throughput.done = 0",
        params![
            token,
            throughput.map(|v| v.to_string()),
            progress.map(|v| v.to_string()),
            peak,
            average,
            total_sent,
            done as i64,
            now
        ],
    )?;
    Ok(())
}

/// Cached throughput + progress for one send job.
pub fn get_throughput(conn: &Connection, token: &str) -> AppResult<Value> {
    let v = conn
        .query_row(
            "SELECT throughput_json, progress_json, peak_rate, average_rate, total_sent, done, updated_at
             FROM email_throughput WHERE token = ?1",
            params![token],
            |r| {
                Ok(json!({
                    "token": token,
                    "throughput": r.get::<_, Option<String>>(0)?
                        .and_then(|s| serde_json::from_str::<Value>(&s).ok()),
                    "progress": r.get::<_, Option<String>>(1)?
                        .and_then(|s| serde_json::from_str::<Value>(&s).ok()),
                    "peak_rate": r.get::<_, Option<f64>>(2)?,
                    "average_rate": r.get::<_, Option<f64>>(3)?,
                    "total_sent": r.get::<_, Option<f64>>(4)?,
                    "done": r.get::<_, i64>(5)? != 0,
                    "updated_at": r.get::<_, Option<String>>(6)?,
                }))
            },
        )
        .optional()?;
    Ok(v.unwrap_or(Value::Null))
}

/// Cache chapter deliverability health + fatigue tier summary (singleton).
pub fn upsert_deliverability(
    conn: &Connection,
    health: Option<&Value>,
    fatigue: Option<&Value>,
    truncated: bool,
    unavailable: bool,
    reason: Option<&str>,
    now: &str,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO email_deliverability
           (id, health_json, fatigue_json, truncated, unavailable, reason, updated_at)
         VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(id) DO UPDATE SET
           health_json=COALESCE(excluded.health_json, email_deliverability.health_json),
           fatigue_json=COALESCE(excluded.fatigue_json, email_deliverability.fatigue_json),
           truncated=excluded.truncated,
           unavailable=excluded.unavailable,
           reason=excluded.reason,
           updated_at=excluded.updated_at",
        params![
            health.map(|v| v.to_string()),
            fatigue.map(|v| v.to_string()),
            truncated as i64,
            unavailable as i64,
            reason,
            now
        ],
    )?;
    Ok(())
}

/// Cached chapter deliverability view: health, fatigue tier summary, recent jobs.
pub fn get_chapter_deliverability(conn: &Connection) -> AppResult<Value> {
    let head = conn
        .query_row(
            "SELECT health_json, fatigue_json, truncated, unavailable, reason, updated_at
             FROM email_deliverability WHERE id = 1",
            [],
            |r| {
                Ok(json!({
                    "health": r.get::<_, Option<String>>(0)?
                        .and_then(|s| serde_json::from_str::<Value>(&s).ok()),
                    "fatigue": r.get::<_, Option<String>>(1)?
                        .and_then(|s| serde_json::from_str::<Value>(&s).ok()),
                    "truncated": r.get::<_, i64>(2)? != 0,
                    "unavailable": r.get::<_, i64>(3)? != 0,
                    "reason": r.get::<_, Option<String>>(4)?,
                    "updated_at": r.get::<_, Option<String>>(5)?,
                }))
            },
        )
        .optional()?;

    let mut jobs = Vec::new();
    {
        let sql = format!(
            "SELECT {SEND_JOB_COLS} FROM email_send_jobs
             ORDER BY (done = 0) DESC, fetched_at DESC LIMIT 25"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], send_job_row)?;
        for j in rows.filter_map(Result::ok) {
            jobs.push(j);
        }
    }

    let mut out = head.unwrap_or_else(|| {
        json!({ "health": Value::Null, "fatigue": Value::Null, "truncated": false, "unavailable": false, "reason": Value::Null, "updated_at": Value::Null })
    });
    if let Value::Object(ref mut map) = out {
        map.insert("recent_jobs".into(), Value::Array(jobs));
    }
    Ok(out)
}

// ── Survey + follow-up cache (specs/survey-followup) ───────────────────────

/// Upsert one or both sources for a `survey_followup` row. Each source is only
/// touched when its `Some((json, status))` argument is provided, so refreshing
/// one source (e.g. the survey diagnostic) never clobbers the other (e.g. the
/// campaign-performance engagement) — callers pass `None` for the source they
/// didn't fetch this cycle.
pub fn upsert_survey_followup(
    conn: &Connection,
    meetup_token: &str,
    survey_update: Option<(Option<&Value>, &str)>,
    email_update: Option<(Option<&Value>, &str)>,
    now: &str,
) -> AppResult<()> {
    let touch_survey = survey_update.is_some();
    let (survey_json, survey_status) = survey_update
        .map(|(j, s)| (j.map(|v| v.to_string()), s.to_string()))
        .unwrap_or((None, "unavailable".to_string()));
    let touch_email = email_update.is_some();
    let (email_json, email_status) = email_update
        .map(|(j, s)| (j.map(|v| v.to_string()), s.to_string()))
        .unwrap_or((None, "unavailable".to_string()));

    conn.execute(
        "INSERT INTO survey_followup (meetup_token, survey_json, survey_status, email_json, email_status, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(meetup_token) DO UPDATE SET
           survey_json=CASE WHEN ?7 THEN excluded.survey_json ELSE survey_followup.survey_json END,
           survey_status=CASE WHEN ?7 THEN excluded.survey_status ELSE survey_followup.survey_status END,
           email_json=CASE WHEN ?8 THEN excluded.email_json ELSE survey_followup.email_json END,
           email_status=CASE WHEN ?8 THEN excluded.email_status ELSE survey_followup.email_status END,
           updated_at=excluded.updated_at",
        params![
            meetup_token,
            survey_json,
            survey_status,
            email_json,
            email_status,
            now,
            touch_survey,
            touch_email
        ],
    )?;
    Ok(())
}

/// Cached survey + follow-up row for one event, or `None` if never fetched.
pub fn get_survey_followup(conn: &Connection, meetup_token: &str) -> AppResult<Option<Value>> {
    let row = conn
        .query_row(
            "SELECT survey_json, survey_status, email_json, email_status, updated_at
             FROM survey_followup WHERE meetup_token = ?1",
            params![meetup_token],
            |r| {
                Ok(json!({
                    "meetup_token": meetup_token,
                    "survey": r.get::<_, Option<String>>(0)?
                        .and_then(|s| serde_json::from_str::<Value>(&s).ok()),
                    "survey_status": r.get::<_, String>(1)?,
                    "email": r.get::<_, Option<String>>(2)?
                        .and_then(|s| serde_json::from_str::<Value>(&s).ok()),
                    "email_status": r.get::<_, String>(3)?,
                    "updated_at": r.get::<_, Option<String>>(4)?,
                }))
            },
        )
        .optional()?;
    Ok(row)
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

/// The caller's primary weblog token (most-referenced across cached events),
/// used to scope chapter deliverability/fatigue when no explicit scope is set.
pub fn primary_weblog(conn: &Connection) -> AppResult<Option<String>> {
    let v = conn
        .query_row(
            "SELECT weblog_token FROM events
             WHERE weblog_token IS NOT NULL AND weblog_token != ''
             GROUP BY weblog_token ORDER BY COUNT(*) DESC LIMIT 1",
            [],
            |r| r.get::<_, String>(0),
        )
        .optional()?;
    Ok(v)
}

/// Feature availability map for the UI (background-sync degradation).
pub fn feature_states(conn: &Connection) -> AppResult<Value> {
    let mut stmt = conn
        .prepare("SELECT key, unavailable, note, last_fetch_at, backoff_until FROM sync_state")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)? != 0,
            r.get::<_, Option<String>>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, Option<String>>(4)?,
        ))
    })?;
    let mut map = serde_json::Map::new();
    for (key, unavailable, note, last, backoff_until) in rows.filter_map(Result::ok) {
        map.insert(
            key,
            json!({
                "unavailable": unavailable,
                "note": note,
                "last_fetch_at": last,
                "backoff_until": backoff_until,
            }),
        );
    }
    Ok(Value::Object(map))
}

// ── Promotion tools (specs/promotion-tools) ────────────────────────────────

/// Upsert the latest draft for one `(meetup_token, kind, platform)`. `platform`
/// is `""` for kinds that aren't per-platform (event_promo, discussion_topics).
pub fn upsert_promotion_draft(
    conn: &Connection,
    meetup_token: &str,
    kind: &str,
    platform: &str,
    params: &Value,
    result: &Value,
    now: &str,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO promotion_drafts (meetup_token, kind, platform, params_json, result_json, generated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(meetup_token, kind, platform) DO UPDATE SET
           params_json=excluded.params_json,
           result_json=excluded.result_json,
           generated_at=excluded.generated_at",
        params![
            meetup_token,
            kind,
            platform,
            params.to_string(),
            result.to_string(),
            now
        ],
    )?;
    Ok(())
}

/// The cached draft for one `(meetup_token, kind, platform)`, if any.
pub fn get_promotion_draft(
    conn: &Connection,
    meetup_token: &str,
    kind: &str,
    platform: &str,
) -> AppResult<Option<Value>> {
    let row = conn
        .query_row(
            "SELECT params_json, result_json, generated_at FROM promotion_drafts
             WHERE meetup_token = ?1 AND kind = ?2 AND platform = ?3",
            params![meetup_token, kind, platform],
            |r| {
                Ok(json!({
                    "params": r.get::<_, Option<String>>(0)?
                        .and_then(|s| serde_json::from_str::<Value>(&s).ok()),
                    "result": r.get::<_, Option<String>>(1)?
                        .and_then(|s| serde_json::from_str::<Value>(&s).ok()),
                    "generated_at": r.get::<_, String>(2)?,
                }))
            },
        )
        .optional()?;
    Ok(row)
}

/// All cached promotion drafts for one event, keyed `"kind"` or `"kind:platform"`
/// — lets the Promote panel paint every cached slot in a single round trip.
pub fn get_promotion_drafts(conn: &Connection, meetup_token: &str) -> AppResult<Value> {
    let mut stmt = conn.prepare(
        "SELECT kind, platform, result_json, generated_at FROM promotion_drafts WHERE meetup_token = ?1",
    )?;
    let rows = stmt.query_map(params![meetup_token], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, Option<String>>(2)?,
            r.get::<_, String>(3)?,
        ))
    })?;
    let mut map = serde_json::Map::new();
    for (kind, platform, result, generated_at) in rows.filter_map(Result::ok) {
        let key = if platform.is_empty() {
            kind
        } else {
            format!("{kind}:{platform}")
        };
        map.insert(
            key,
            json!({
                "result": result.and_then(|s| serde_json::from_str::<Value>(&s).ok()),
                "generated_at": generated_at,
            }),
        );
    }
    Ok(Value::Object(map))
}

/// Create a new job row in `pending` status (design D2). Kickoff must check
/// `find_active_promotion_job` first — this always inserts.
pub fn create_promotion_job(
    conn: &Connection,
    id: &str,
    meetup_token: &str,
    kind: &str,
    platform: &str,
    params_hash: &str,
    now: &str,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO promotion_jobs (id, meetup_token, kind, platform, params_hash, status, started_at, error_code)
         VALUES (?1, ?2, ?3, ?4, ?5, 'pending', ?6, NULL)",
        params![id, meetup_token, kind, platform, params_hash, now],
    )?;
    Ok(())
}

/// Move a job to a new status (`running`, `ready`, `error`, `timeout`), with an
/// optional error code (forbidden_* / rate_limited / timeout / other).
pub fn set_promotion_job_status(
    conn: &Connection,
    id: &str,
    status: &str,
    error_code: Option<&str>,
) -> AppResult<()> {
    conn.execute(
        "UPDATE promotion_jobs SET status = ?2, error_code = ?3 WHERE id = ?1",
        params![id, status, error_code],
    )?;
    Ok(())
}

/// The in-flight (`pending`/`running`) job id for one action, if any — used to
/// suppress a duplicate kickoff (design D7).
pub fn find_active_promotion_job(
    conn: &Connection,
    meetup_token: &str,
    kind: &str,
    platform: &str,
) -> AppResult<Option<String>> {
    let id = conn
        .query_row(
            "SELECT id FROM promotion_jobs
             WHERE meetup_token = ?1 AND kind = ?2 AND platform = ?3
               AND status IN ('pending', 'running')
             ORDER BY started_at DESC LIMIT 1",
            params![meetup_token, kind, platform],
            |r| r.get::<_, String>(0),
        )
        .optional()?;
    Ok(id)
}

/// One job's current state, for the frontend to poll if it missed an event.
pub fn get_promotion_job(conn: &Connection, id: &str) -> AppResult<Option<Value>> {
    let row = conn
        .query_row(
            "SELECT id, meetup_token, kind, platform, status, started_at, error_code
             FROM promotion_jobs WHERE id = ?1",
            params![id],
            |r| {
                Ok(json!({
                    "id": r.get::<_, String>(0)?,
                    "meetup_token": r.get::<_, String>(1)?,
                    "kind": r.get::<_, String>(2)?,
                    "platform": r.get::<_, String>(3)?,
                    "status": r.get::<_, String>(4)?,
                    "started_at": r.get::<_, String>(5)?,
                    "error_code": r.get::<_, Option<String>>(6)?,
                }))
            },
        )
        .optional()?;
    Ok(row)
}

/// Drop a job row entirely — used on cancel, so the action falls back to
/// showing only its last cached draft (design D5).
pub fn delete_promotion_job(conn: &Connection, id: &str) -> AppResult<()> {
    conn.execute("DELETE FROM promotion_jobs WHERE id = ?1", params![id])?;
    Ok(())
}

/// Cache a logo-search result page for its query params.
pub fn upsert_logo_cache(
    conn: &Connection,
    query: &str,
    scope: &str,
    include_co_branded: bool,
    result: &Value,
    now: &str,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO logo_search_cache (query, scope, include_co_branded, result_json, fetched_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(query, scope, include_co_branded) DO UPDATE SET
           result_json=excluded.result_json,
           fetched_at=excluded.fetched_at",
        params![query, scope, include_co_branded as i64, result.to_string(), now],
    )?;
    Ok(())
}

/// The cached logo-search result for these query params, with its fetch time
/// so the caller can apply its own freshness window.
pub fn get_logo_cache(
    conn: &Connection,
    query: &str,
    scope: &str,
    include_co_branded: bool,
) -> AppResult<Option<Value>> {
    let row = conn
        .query_row(
            "SELECT result_json, fetched_at FROM logo_search_cache
             WHERE query = ?1 AND scope = ?2 AND include_co_branded = ?3",
            params![query, scope, include_co_branded as i64],
            |r| {
                Ok(json!({
                    "result": r.get::<_, Option<String>>(0)?
                        .and_then(|s| serde_json::from_str::<Value>(&s).ok()),
                    "fetched_at": r.get::<_, String>(1)?,
                }))
            },
        )
        .optional()?;
    Ok(row)
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
    fn completed_send_job_is_frozen() {
        let c = mem();
        let active = json!({ "token": "j1", "status": "sending", "sent_count": 10, "done": false });
        upsert_send_job(&c, &active, Some("m1"), "event", "t1").unwrap();
        let done = json!({ "token": "j1", "status": "completed", "sent_count": 100, "done": true });
        upsert_send_job(&c, &done, Some("m1"), "event", "t2").unwrap();
        // A late refresh with a different snapshot must not overwrite a done job.
        let late = json!({ "token": "j1", "status": "sending", "sent_count": 5, "done": false });
        upsert_send_job(&c, &late, Some("m1"), "event", "t3").unwrap();

        let email = get_event_email(&c, "m1").unwrap();
        let jobs = email.get("send_jobs").and_then(Value::as_array).unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].get("status").and_then(Value::as_str), Some("completed"));
        assert_eq!(jobs[0].get("sent_count").and_then(Value::as_f64), Some(100.0));
        assert_eq!(jobs[0].get("done").and_then(Value::as_bool), Some(true));
    }

    #[test]
    fn send_job_retention_is_partition_scoped() {
        let c = mem();
        upsert_send_job(&c, &json!({ "token": "ev1", "status": "completed", "done": true }), Some("m1"), "event", "t1").unwrap();
        upsert_send_job(&c, &json!({ "token": "ch1", "status": "completed", "done": true }), None, "chapter", "t1").unwrap();
        // A chapter refresh that keeps nothing must not evict the event job.
        retain_send_jobs(&c, "chapter", &[]).unwrap();
        let email = get_event_email(&c, "m1").unwrap();
        let jobs = email.get("send_jobs").and_then(Value::as_array).unwrap();
        assert_eq!(jobs.len(), 1, "event-scoped job must survive a chapter retain");
        let chapter = get_chapter_deliverability(&c).unwrap();
        let recent = chapter.get("recent_jobs").and_then(Value::as_array).unwrap();
        assert!(recent.iter().all(|j| j.get("token").and_then(Value::as_str) != Some("ch1")),
            "chapter job was retained-out");
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

    #[test]
    fn survey_followup_missing_row_returns_none() {
        let c = mem();
        assert!(get_survey_followup(&c, "m1").unwrap().is_none());
    }

    #[test]
    fn survey_followup_upsert_and_read_round_trip() {
        let c = mem();
        let survey = json!({ "response_count": 12, "eligible_attendees": 40, "response_rate": 0.3 });
        let email = json!({ "sends": 40, "opens": 18, "open_rate": 0.45 });
        upsert_survey_followup(
            &c,
            "m1",
            Some((Some(&survey), "ok")),
            Some((Some(&email), "ok")),
            "t1",
        )
        .unwrap();

        let row = get_survey_followup(&c, "m1").unwrap().expect("row must exist");
        assert_eq!(row.get("survey_status").and_then(Value::as_str), Some("ok"));
        assert_eq!(row.get("email_status").and_then(Value::as_str), Some("ok"));
        assert_eq!(
            row.get("survey").and_then(|s| s.get("response_count")).and_then(Value::as_i64),
            Some(12)
        );
        assert_eq!(
            row.get("email").and_then(|e| e.get("opens")).and_then(Value::as_i64),
            Some(18)
        );
        assert_eq!(row.get("updated_at").and_then(Value::as_str), Some("t1"));
    }

    #[test]
    fn survey_followup_refreshing_one_source_does_not_clobber_the_other() {
        let c = mem();
        let survey = json!({ "response_count": 5 });
        // First cycle: only the survey source was fetched.
        upsert_survey_followup(&c, "m1", Some((Some(&survey), "ok")), None, "t1").unwrap();
        // Second cycle: only the email source was fetched (e.g. survey already cached).
        let email = json!({ "sends": 20, "opens": 9 });
        upsert_survey_followup(&c, "m1", None, Some((Some(&email), "ok")), "t2").unwrap();

        let row = get_survey_followup(&c, "m1").unwrap().expect("row must exist");
        // The survey data from cycle 1 must still be present after cycle 2 only
        // touched the email source, and vice versa.
        assert_eq!(
            row.get("survey").and_then(|s| s.get("response_count")).and_then(Value::as_i64),
            Some(5),
            "email-only refresh must not clobber the survey source"
        );
        assert_eq!(row.get("survey_status").and_then(Value::as_str), Some("ok"));
        assert_eq!(
            row.get("email").and_then(|e| e.get("sends")).and_then(Value::as_i64),
            Some(20)
        );
        assert_eq!(row.get("email_status").and_then(Value::as_str), Some("ok"));
    }

    #[test]
    fn survey_followup_degradation_flags_persist() {
        let c = mem();
        upsert_survey_followup(
            &c,
            "m1",
            Some((None, "forbidden_api_group")),
            Some((None, "forbidden_scope")),
            "t1",
        )
        .unwrap();
        let row = get_survey_followup(&c, "m1").unwrap().expect("row must exist");
        assert_eq!(row.get("survey_status").and_then(Value::as_str), Some("forbidden_api_group"));
        assert_eq!(row.get("survey").cloned(), Some(Value::Null));
        assert_eq!(row.get("email_status").and_then(Value::as_str), Some("forbidden_scope"));
        assert_eq!(row.get("email").cloned(), Some(Value::Null));

        // A later cycle that only refreshes the survey source (now succeeding)
        // must leave the still-forbidden email status untouched.
        let survey = json!({ "response_count": 3 });
        upsert_survey_followup(&c, "m1", Some((Some(&survey), "ok")), None, "t2").unwrap();
        let row2 = get_survey_followup(&c, "m1").unwrap().expect("row must exist");
        assert_eq!(row2.get("survey_status").and_then(Value::as_str), Some("ok"));
        assert_eq!(row2.get("email_status").and_then(Value::as_str), Some("forbidden_scope"),
            "degradation flag for the untouched source must persist");
    }

    // ── Promotion tools (specs/promotion-tools) ────────────────────────────

    #[test]
    fn promotion_draft_upsert_and_read_round_trip() {
        let c = mem();
        let params = json!({ "package_type": "full_campaign", "audience": "general" });
        let result = json!({ "artifact": { "headline": "Join us!" }, "draft_only": true });
        upsert_promotion_draft(&c, "m1", "event_promo", "", &params, &result, "t1").unwrap();

        let row = get_promotion_draft(&c, "m1", "event_promo", "").unwrap().expect("row must exist");
        assert_eq!(row.get("generated_at").and_then(Value::as_str), Some("t1"));
        assert_eq!(
            row.get("result").and_then(|r| r.get("artifact")).and_then(|a| a.get("headline")).and_then(Value::as_str),
            Some("Join us!")
        );
        assert_eq!(
            row.get("params").and_then(|p| p.get("audience")).and_then(Value::as_str),
            Some("general")
        );

        // Missing (meetup_token, kind, platform) combos must read back None.
        assert!(get_promotion_draft(&c, "m1", "discussion_topics", "").unwrap().is_none());
    }

    #[test]
    fn promotion_draft_regenerating_one_platform_does_not_clobber_another() {
        let c = mem();
        let li_result = json!({ "artifact": { "text": "LinkedIn draft" } });
        let x_result = json!({ "artifact": { "text": "X draft" } });
        upsert_promotion_draft(&c, "m1", "social_post", "linkedin", &json!({}), &li_result, "t1").unwrap();
        upsert_promotion_draft(&c, "m1", "social_post", "x", &json!({}), &x_result, "t1").unwrap();

        // Regenerate only the LinkedIn draft.
        let li_result_v2 = json!({ "artifact": { "text": "LinkedIn draft v2" } });
        upsert_promotion_draft(&c, "m1", "social_post", "linkedin", &json!({}), &li_result_v2, "t2").unwrap();

        let li = get_promotion_draft(&c, "m1", "social_post", "linkedin").unwrap().unwrap();
        let x = get_promotion_draft(&c, "m1", "social_post", "x").unwrap().unwrap();
        assert_eq!(
            li.get("result").and_then(|r| r.get("artifact")).and_then(|a| a.get("text")).and_then(Value::as_str),
            Some("LinkedIn draft v2")
        );
        assert_eq!(li.get("generated_at").and_then(Value::as_str), Some("t2"));
        assert_eq!(
            x.get("result").and_then(|r| r.get("artifact")).and_then(|a| a.get("text")).and_then(Value::as_str),
            Some("X draft"),
            "regenerating the LinkedIn draft must not clobber the X draft"
        );
        assert_eq!(x.get("generated_at").and_then(Value::as_str), Some("t1"));

        // get_promotion_drafts must expose both under distinct platform-scoped keys.
        let all = get_promotion_drafts(&c, "m1").unwrap();
        assert!(all.get("social_post:linkedin").is_some());
        assert!(all.get("social_post:x").is_some());
    }

    #[test]
    fn promotion_job_status_transitions_persist() {
        let c = mem();
        create_promotion_job(&c, "job1", "m1", "event_promo", "", "hash1", "t1").unwrap();
        let job = get_promotion_job(&c, "job1").unwrap().expect("job must exist");
        assert_eq!(job.get("status").and_then(Value::as_str), Some("pending"));

        // Duplicate kickoff suppression: a pending job for the same action is
        // found by find_active_promotion_job (design D7).
        let active = find_active_promotion_job(&c, "m1", "event_promo", "").unwrap();
        assert_eq!(active.as_deref(), Some("job1"));

        set_promotion_job_status(&c, "job1", "running", None).unwrap();
        let job = get_promotion_job(&c, "job1").unwrap().unwrap();
        assert_eq!(job.get("status").and_then(Value::as_str), Some("running"));
        assert!(find_active_promotion_job(&c, "m1", "event_promo", "").unwrap().is_some(),
            "a running job must still suppress a duplicate kickoff");

        set_promotion_job_status(&c, "job1", "ready", None).unwrap();
        let job = get_promotion_job(&c, "job1").unwrap().unwrap();
        assert_eq!(job.get("status").and_then(Value::as_str), Some("ready"));
        assert!(job.get("error_code").and_then(Value::as_str).is_none());
        // Once terminal, the action is no longer considered in-flight.
        assert!(find_active_promotion_job(&c, "m1", "event_promo", "").unwrap().is_none());
    }

    #[test]
    fn promotion_job_error_and_timeout_carry_a_code() {
        let c = mem();
        create_promotion_job(&c, "job2", "m1", "social_post", "linkedin", "hash2", "t1").unwrap();
        set_promotion_job_status(&c, "job2", "timeout", Some("timeout")).unwrap();
        let job = get_promotion_job(&c, "job2").unwrap().unwrap();
        assert_eq!(job.get("status").and_then(Value::as_str), Some("timeout"));
        assert_eq!(job.get("error_code").and_then(Value::as_str), Some("timeout"));

        create_promotion_job(&c, "job3", "m1", "discussion_topics", "", "hash3", "t1").unwrap();
        set_promotion_job_status(&c, "job3", "error", Some("forbidden_api_group")).unwrap();
        let job = get_promotion_job(&c, "job3").unwrap().unwrap();
        assert_eq!(job.get("status").and_then(Value::as_str), Some("error"));
        assert_eq!(job.get("error_code").and_then(Value::as_str), Some("forbidden_api_group"));
    }

    #[test]
    fn promotion_job_cancel_deletes_the_row() {
        let c = mem();
        create_promotion_job(&c, "job4", "m1", "event_promo", "", "hash4", "t1").unwrap();
        delete_promotion_job(&c, "job4").unwrap();
        assert!(get_promotion_job(&c, "job4").unwrap().is_none());
        assert!(find_active_promotion_job(&c, "m1", "event_promo", "").unwrap().is_none());
    }

    #[test]
    fn logo_cache_upsert_and_read_round_trip() {
        let c = mem();
        assert!(get_logo_cache(&c, "denver", "smart_match", false).unwrap().is_none());

        let result = json!({ "matches": [{ "token": "logo1" }] });
        upsert_logo_cache(&c, "denver", "smart_match", false, &result, "t1").unwrap();
        let row = get_logo_cache(&c, "denver", "smart_match", false).unwrap().unwrap();
        assert_eq!(row.get("fetched_at").and_then(Value::as_str), Some("t1"));
        assert_eq!(
            row.get("result").and_then(|r| r.get("matches")).and_then(Value::as_array).map(Vec::len),
            Some(1)
        );

        // A different include_co_branded is a distinct cache key.
        assert!(get_logo_cache(&c, "denver", "smart_match", true).unwrap().is_none());
    }
}
