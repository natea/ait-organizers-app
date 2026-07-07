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

        -- Sponsor tools (specs/sponsor-tools). Sponsors found via search are
        -- upserted here keyed by sponsor_token so a detail open can render a
        -- sponsor's name/domain/city without holding onto search-query state.
        CREATE TABLE IF NOT EXISTS sponsors (
            sponsor_token  TEXT PRIMARY KEY,
            name           TEXT,
            domain         TEXT,
            city           TEXT,
            short_profile  TEXT,
            raw_json       TEXT,
            fetched_at     TEXT NOT NULL
        );

        -- One row per distinct search (query + filters), storing the ordered
        -- list of matched sponsor_tokens (the sponsor data itself lives in
        -- `sponsors`, deduped across searches). Degrade state is per-search so
        -- a blocked search doesn't stomp a previously successful one.
        CREATE TABLE IF NOT EXISTS sponsor_search_cache (
            query          TEXT NOT NULL,
            city           TEXT NOT NULL DEFAULT '',
            industry       TEXT NOT NULL DEFAULT '',
            active_only    INTEGER NOT NULL DEFAULT 0,
            tokens_json    TEXT,
            truncated      INTEGER NOT NULL DEFAULT 0,
            unavailable    INTEGER NOT NULL DEFAULT 0,
            reason         TEXT,
            fetched_at     TEXT NOT NULL,
            PRIMARY KEY (query, city, industry, active_only)
        );

        -- Contacts for one sponsor. A fresh fetch replaces the whole set for
        -- that sponsor_token (task 3.1) rather than merging, so stale contacts
        -- never linger. Email/phone are stored exactly as the API returns them
        -- (already masked server-side when visibility is off) — the app never
        -- unmasks; `*_masked` is a display hint only (design D1).
        CREATE TABLE IF NOT EXISTS sponsor_contacts (
            sponsor_token  TEXT NOT NULL,
            contact_id     TEXT NOT NULL,
            role           TEXT,
            title          TEXT,
            email          TEXT,
            email_masked   INTEGER NOT NULL DEFAULT 0,
            phone          TEXT,
            phone_masked   INTEGER NOT NULL DEFAULT 0,
            linkedin       TEXT,
            confidence     REAL,
            raw_json       TEXT,
            PRIMARY KEY (sponsor_token, contact_id)
        );

        -- Per-sponsor contact-fetch header: degrade state + cap indicator,
        -- separate from the contact rows so a blocked/empty fetch doesn't need
        -- a sentinel contact row.
        CREATE TABLE IF NOT EXISTS sponsor_contacts_meta (
            sponsor_token  TEXT PRIMARY KEY,
            truncated      INTEGER NOT NULL DEFAULT 0,
            unavailable    INTEGER NOT NULL DEFAULT 0,
            reason         TEXT,
            fetched_at     TEXT NOT NULL
        );

        -- Reusable generated drafts (research brief or pitch) per sponsor/company
        -- (design D3, task 2.3). Unlike promotion_drafts this keeps history (one
        -- row per generation, keyed by draft_id) so a regeneration never clobbers
        -- an earlier draft of a *different* kind or an earlier draft the
        -- organizer may still want — reopening is explicit, never automatic.
        CREATE TABLE IF NOT EXISTS sponsor_drafts (
            draft_id       TEXT PRIMARY KEY,
            subject        TEXT NOT NULL,
            sponsor_token  TEXT,
            company_name   TEXT,
            kind           TEXT NOT NULL,
            params_json    TEXT,
            result_json    TEXT,
            status         TEXT NOT NULL DEFAULT 'ready',
            created_at     TEXT NOT NULL,
            updated_at     TEXT NOT NULL
        );

        -- Tracked async generation jobs for research/pitch (design D2, mirrors
        -- promotion_jobs). Never polled — created only on an explicit
        -- user-initiated kickoff.
        CREATE TABLE IF NOT EXISTS sponsor_jobs (
            id             TEXT PRIMARY KEY,
            subject        TEXT NOT NULL,
            sponsor_token  TEXT,
            company_name   TEXT,
            kind           TEXT NOT NULL,
            params_hash    TEXT,
            status         TEXT NOT NULL DEFAULT 'pending',
            started_at     TEXT NOT NULL,
            error_code     TEXT,
            draft_id       TEXT
        );

        -- RSVP screening (specs/rsvp-screening). One row per RSVP visible in a
        -- cached event's attendee list. `state` is the raw internal state that
        -- mutation decisions key off; `registrant_status*` are the
        -- registrant-facing labels shown to the user (internal `denied` reads
        -- as "waitlisted" externally — the API's own semantics, task 1.2).
        CREATE TABLE IF NOT EXISTS rsvp_rows (
            rsvp_ref                 TEXT PRIMARY KEY,
            meetup_token             TEXT NOT NULL,
            name                     TEXT,
            email                    TEXT,
            state                    TEXT NOT NULL DEFAULT 'unknown',
            registrant_status        TEXT,
            registrant_status_label  TEXT,
            registrant_status_text   TEXT,
            checked_in               INTEGER NOT NULL DEFAULT 0,
            score                    REAL,
            raw_json                 TEXT,
            updated_at               TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_rsvp_rows_meetup ON rsvp_rows(meetup_token);

        -- Per-registrant detail: AI assessment, append-only status history, and
        -- subscriber engagement score breakdown. Each source degrades
        -- independently (same pattern as survey_followup) so one forbidden
        -- endpoint never blocks the other two or the rest of the row.
        CREATE TABLE IF NOT EXISTS rsvp_detail (
            rsvp_ref           TEXT PRIMARY KEY,
            assessment_json    TEXT,
            assessment_status  TEXT NOT NULL DEFAULT 'unavailable',
            history_json       TEXT,
            history_status     TEXT NOT NULL DEFAULT 'unavailable',
            score_json         TEXT,
            score_status       TEXT NOT NULL DEFAULT 'unavailable',
            updated_at         TEXT NOT NULL
        );

        -- Append-only write-audit trail (design D3) — the FIRST write feature
        -- in the app's history. A row is inserted with outcome='attempted'
        -- BEFORE the API call and updated with the real outcome after, so a
        -- crash mid-call, a denial, or a rate limit still leaves evidence.
        -- This table is never touched by sign-out's cache wipe (commands.rs) —
        -- it is a durable audit log, not a cache, and must survive it.
        CREATE TABLE IF NOT EXISTS write_audit (
            id            TEXT PRIMARY KEY,
            created_at    TEXT NOT NULL,
            actor         TEXT,
            action        TEXT NOT NULL,
            meetup_token  TEXT,
            targets_json  TEXT NOT NULL,
            from_state    TEXT,
            to_state      TEXT,
            send_email    INTEGER NOT NULL DEFAULT 0,
            confirmed     INTEGER NOT NULL DEFAULT 0,
            outcome       TEXT NOT NULL DEFAULT 'attempted',
            error_code    TEXT,
            updated_at    TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_write_audit_meetup ON write_audit(meetup_token);
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

// ── Sponsor tools (specs/sponsor-tools) ────────────────────────────────────

/// Build the (sponsor_token or free-text-company-name) key used to correlate
/// jobs/drafts to a subject regardless of which one the caller supplied.
pub fn sponsor_subject_key(sponsor_token: Option<&str>, name: Option<&str>) -> String {
    match sponsor_token {
        Some(t) if !t.is_empty() => format!("token:{t}"),
        _ => format!("name:{}", name.unwrap_or("").trim().to_lowercase()),
    }
}

/// Upsert one sponsor row from a search match (fields read defensively — live
/// shapes are unverifiable, so every name has fallbacks).
fn upsert_sponsor_row(conn: &Connection, m: &Value, now: &str) -> AppResult<Option<String>> {
    let Some(token) = pick_str(m, &["sponsor_token", "token"]) else {
        return Ok(None);
    };
    let name = pick_str(m, &["name", "company_name", "sponsor_name"]);
    let domain = pick_str(m, &["domain", "website", "website_url"]);
    let city = pick_str(m, &["city"]);
    let profile = pick_str(m, &["short_profile", "profile", "summary", "description"]);
    conn.execute(
        "INSERT INTO sponsors (sponsor_token, name, domain, city, short_profile, raw_json, fetched_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(sponsor_token) DO UPDATE SET
           name=excluded.name,
           domain=excluded.domain,
           city=excluded.city,
           short_profile=excluded.short_profile,
           raw_json=excluded.raw_json,
           fetched_at=excluded.fetched_at",
        params![token, name, domain, city, profile, m.to_string(), now],
    )?;
    Ok(Some(token.to_string()))
}

/// Cache a sponsor search result page: upsert each matched sponsor row, then
/// record this query's ordered token list under its own degrade state so a
/// later blocked/failed search doesn't erase a previously successful one for a
/// *different* query (task 3.1).
#[allow(clippy::too_many_arguments)]
pub fn upsert_sponsor_search(
    conn: &Connection,
    query: &str,
    city: &str,
    industry: &str,
    active_only: bool,
    matches: &[Value],
    truncated: bool,
    unavailable: bool,
    reason: Option<&str>,
    now: &str,
) -> AppResult<()> {
    let mut tokens = Vec::new();
    for m in matches {
        if let Some(t) = upsert_sponsor_row(conn, m, now)? {
            tokens.push(t);
        }
    }
    conn.execute(
        "INSERT INTO sponsor_search_cache
           (query, city, industry, active_only, tokens_json, truncated, unavailable, reason, fetched_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(query, city, industry, active_only) DO UPDATE SET
           tokens_json=excluded.tokens_json,
           truncated=excluded.truncated,
           unavailable=excluded.unavailable,
           reason=excluded.reason,
           fetched_at=excluded.fetched_at",
        params![
            query,
            city,
            industry,
            active_only as i64,
            json!(tokens).to_string(),
            truncated as i64,
            unavailable as i64,
            reason,
            now
        ],
    )?;
    Ok(())
}

/// The cached search result for one query+filters combo, with sponsor rows
/// resolved from `sponsors` in their original match order.
pub fn get_sponsor_search(
    conn: &Connection,
    query: &str,
    city: &str,
    industry: &str,
    active_only: bool,
) -> AppResult<Value> {
    let row = conn
        .query_row(
            "SELECT tokens_json, truncated, unavailable, reason, fetched_at
             FROM sponsor_search_cache WHERE query = ?1 AND city = ?2 AND industry = ?3 AND active_only = ?4",
            params![query, city, industry, active_only as i64],
            |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?,
                    r.get::<_, i64>(1)? != 0,
                    r.get::<_, i64>(2)? != 0,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, String>(4)?,
                ))
            },
        )
        .optional()?;
    let Some((tokens_json, truncated, unavailable, reason, fetched_at)) = row else {
        return Ok(json!({
            "results": Value::Array(vec![]), "truncated": false,
            "unavailable": false, "reason": Value::Null, "fetched_at": Value::Null,
        }));
    };
    let tokens: Vec<String> = tokens_json
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let mut results = Vec::new();
    for t in &tokens {
        if let Some(v) = get_sponsor(conn, t)? {
            results.push(v);
        }
    }
    Ok(json!({
        "results": results,
        "truncated": truncated,
        "unavailable": unavailable,
        "reason": reason,
        "fetched_at": fetched_at,
    }))
}

/// One cached sponsor row by token (raw_json, unpacked), or `None`.
pub fn get_sponsor(conn: &Connection, sponsor_token: &str) -> AppResult<Option<Value>> {
    let raw = conn
        .query_row(
            "SELECT raw_json FROM sponsors WHERE sponsor_token = ?1",
            params![sponsor_token],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten();
    Ok(raw.and_then(|s| serde_json::from_str::<Value>(&s).ok()))
}

/// Cache the contacts for one sponsor. Replaces the whole set for that
/// sponsor_token (task 3.1) — a fresh fetch never merges with stale rows.
pub fn upsert_sponsor_contacts(
    conn: &Connection,
    sponsor_token: &str,
    contacts: &[Value],
    truncated: bool,
    unavailable: bool,
    reason: Option<&str>,
    now: &str,
) -> AppResult<()> {
    conn.execute(
        "DELETE FROM sponsor_contacts WHERE sponsor_token = ?1",
        params![sponsor_token],
    )?;
    for (i, c) in contacts.iter().enumerate() {
        let contact_id = pick_str(c, &["contact_id", "id", "token"])
            .map(str::to_string)
            .unwrap_or_else(|| i.to_string());
        let role = pick_str(c, &["role"]);
        let title = pick_str(c, &["title", "job_title"]);
        let email = pick_str(c, &["email", "contact_email"]);
        let phone = pick_str(c, &["phone", "phone_number"]);
        let linkedin = pick_str(c, &["linkedin", "linkedin_url"]);
        let confidence = pick_num(c, &["confidence", "match_confidence", "score"]);
        let email_masked = field_masked(c, "email_masked", email);
        let phone_masked = field_masked(c, "phone_masked", phone);
        conn.execute(
            "INSERT INTO sponsor_contacts
               (sponsor_token, contact_id, role, title, email, email_masked, phone, phone_masked, linkedin, confidence, raw_json)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)
             ON CONFLICT(sponsor_token, contact_id) DO UPDATE SET
               role=excluded.role, title=excluded.title, email=excluded.email,
               email_masked=excluded.email_masked, phone=excluded.phone,
               phone_masked=excluded.phone_masked, linkedin=excluded.linkedin,
               confidence=excluded.confidence, raw_json=excluded.raw_json",
            params![
                sponsor_token, contact_id, role, title, email, email_masked as i64,
                phone, phone_masked as i64, linkedin, confidence, c.to_string()
            ],
        )?;
    }
    conn.execute(
        "INSERT INTO sponsor_contacts_meta (sponsor_token, truncated, unavailable, reason, fetched_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(sponsor_token) DO UPDATE SET
           truncated=excluded.truncated, unavailable=excluded.unavailable,
           reason=excluded.reason, fetched_at=excluded.fetched_at",
        params![sponsor_token, truncated as i64, unavailable as i64, reason, now],
    )?;
    Ok(())
}

/// True when an explicit `<field>_masked` boolean is set, else inferred from a
/// mask sentinel (`*`) in the value — never inferred as unmasked from absence.
fn field_masked(c: &Value, explicit_field: &str, value: Option<&str>) -> bool {
    if let Some(b) = c.get(explicit_field).and_then(Value::as_bool) {
        return b;
    }
    value.map(|v| v.contains('*')).unwrap_or(false)
}

/// Cached contacts for one sponsor (role/title/contact fields + masking hints).
pub fn get_sponsor_contacts(conn: &Connection, sponsor_token: &str) -> AppResult<Value> {
    let meta = conn
        .query_row(
            "SELECT truncated, unavailable, reason, fetched_at FROM sponsor_contacts_meta WHERE sponsor_token = ?1",
            params![sponsor_token],
            |r| {
                Ok((
                    r.get::<_, i64>(0)? != 0,
                    r.get::<_, i64>(1)? != 0,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, String>(3)?,
                ))
            },
        )
        .optional()?;
    let mut stmt = conn.prepare(
        "SELECT contact_id, role, title, email, email_masked, phone, phone_masked, linkedin, confidence
         FROM sponsor_contacts WHERE sponsor_token = ?1 ORDER BY rowid",
    )?;
    let rows = stmt.query_map(params![sponsor_token], |r| {
        Ok(json!({
            "contact_id": r.get::<_, String>(0)?,
            "role": r.get::<_, Option<String>>(1)?,
            "title": r.get::<_, Option<String>>(2)?,
            "email": r.get::<_, Option<String>>(3)?,
            "email_masked": r.get::<_, i64>(4)? != 0,
            "phone": r.get::<_, Option<String>>(5)?,
            "phone_masked": r.get::<_, i64>(6)? != 0,
            "linkedin": r.get::<_, Option<String>>(7)?,
            "confidence": r.get::<_, Option<f64>>(8)?,
        }))
    })?;
    let contacts: Vec<Value> = rows.filter_map(Result::ok).collect();
    let (truncated, unavailable, reason, fetched_at) = meta.unwrap_or((false, false, None, String::new()));
    Ok(json!({
        "sponsor_token": sponsor_token,
        "contacts": contacts,
        "truncated": truncated,
        "unavailable": unavailable,
        "reason": reason,
        "fetched_at": if fetched_at.is_empty() { Value::Null } else { json!(fetched_at) },
    }))
}

/// Insert a completed draft (research brief or pitch). History is kept — one
/// row per generation — so regenerating never clobbers an earlier draft of a
/// different kind or an earlier draft the organizer wants to keep.
#[allow(clippy::too_many_arguments)]
pub fn insert_sponsor_draft(
    conn: &Connection,
    draft_id: &str,
    subject: &str,
    sponsor_token: Option<&str>,
    company_name: Option<&str>,
    kind: &str,
    params: &Value,
    result: &Value,
    now: &str,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO sponsor_drafts
           (draft_id, subject, sponsor_token, company_name, kind, params_json, result_json, status, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'ready', ?8, ?8)",
        params![
            draft_id, subject, sponsor_token, company_name, kind,
            params.to_string(), result.to_string(), now
        ],
    )?;
    Ok(())
}

fn sponsor_draft_row(r: &rusqlite::Row) -> rusqlite::Result<Value> {
    Ok(json!({
        "draft_id": r.get::<_, String>(0)?,
        "sponsor_token": r.get::<_, Option<String>>(1)?,
        "company_name": r.get::<_, Option<String>>(2)?,
        "kind": r.get::<_, String>(3)?,
        "params": r.get::<_, Option<String>>(4)?
            .and_then(|s| serde_json::from_str::<Value>(&s).ok()),
        "result": r.get::<_, Option<String>>(5)?
            .and_then(|s| serde_json::from_str::<Value>(&s).ok()),
        "status": r.get::<_, String>(6)?,
        "created_at": r.get::<_, String>(7)?,
        "updated_at": r.get::<_, String>(8)?,
    }))
}

const DRAFT_COLS: &str = "draft_id, sponsor_token, company_name, kind, params_json, result_json, status, created_at, updated_at";

/// One draft by id, or `None`.
pub fn get_sponsor_draft(conn: &Connection, draft_id: &str) -> AppResult<Option<Value>> {
    let sql = format!("SELECT {DRAFT_COLS} FROM sponsor_drafts WHERE draft_id = ?1");
    let row = conn
        .query_row(&sql, params![draft_id], sponsor_draft_row)
        .optional()?;
    Ok(row)
}

/// All cached drafts for one subject (sponsor_token or free-text company),
/// newest first, so the organizer can reopen a prior draft without
/// regenerating (design D3). `kind` narrows to `research` or `pitch`.
pub fn list_sponsor_drafts(conn: &Connection, subject: &str, kind: Option<&str>) -> AppResult<Value> {
    let drafts: Vec<Value> = if let Some(k) = kind {
        let sql = format!("SELECT {DRAFT_COLS} FROM sponsor_drafts WHERE subject = ?1 AND kind = ?2 ORDER BY created_at DESC");
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![subject, k], sponsor_draft_row)?;
        rows.filter_map(Result::ok).collect()
    } else {
        let sql = format!("SELECT {DRAFT_COLS} FROM sponsor_drafts WHERE subject = ?1 ORDER BY created_at DESC");
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![subject], sponsor_draft_row)?;
        rows.filter_map(Result::ok).collect()
    };
    Ok(json!(drafts))
}

/// Create a job row in `pending` status. Kickoff must check
/// `find_active_sponsor_job` first — this always inserts.
#[allow(clippy::too_many_arguments)]
pub fn create_sponsor_job(
    conn: &Connection,
    id: &str,
    subject: &str,
    sponsor_token: Option<&str>,
    company_name: Option<&str>,
    kind: &str,
    params_hash: &str,
    now: &str,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO sponsor_jobs (id, subject, sponsor_token, company_name, kind, params_hash, status, started_at, error_code, draft_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending', ?7, NULL, NULL)",
        params![id, subject, sponsor_token, company_name, kind, params_hash, now],
    )?;
    Ok(())
}

/// Move a job to a new status (`running`, `ready`, `error`, `timeout`,
/// `cancelled`), with an optional error code and, on success, the resulting
/// draft id.
pub fn set_sponsor_job_status(
    conn: &Connection,
    id: &str,
    status: &str,
    error_code: Option<&str>,
    draft_id: Option<&str>,
) -> AppResult<()> {
    conn.execute(
        "UPDATE sponsor_jobs SET status = ?2, error_code = ?3, draft_id = COALESCE(?4, draft_id) WHERE id = ?1",
        params![id, status, error_code, draft_id],
    )?;
    Ok(())
}

/// The in-flight (`pending`/`running`) job id for one (subject, kind), if
/// any — suppresses a duplicate kickoff and guards the tight 10rpm generation
/// budget (spec: "MUST prevent overlapping generations").
pub fn find_active_sponsor_job(conn: &Connection, subject: &str, kind: &str) -> AppResult<Option<String>> {
    let id = conn
        .query_row(
            "SELECT id FROM sponsor_jobs
             WHERE subject = ?1 AND kind = ?2 AND status IN ('pending', 'running')
             ORDER BY started_at DESC LIMIT 1",
            params![subject, kind],
            |r| r.get::<_, String>(0),
        )
        .optional()?;
    Ok(id)
}

/// One job's current state, for the frontend to poll if it missed an event.
pub fn get_sponsor_job(conn: &Connection, id: &str) -> AppResult<Option<Value>> {
    let row = conn
        .query_row(
            "SELECT id, subject, sponsor_token, company_name, kind, status, started_at, error_code, draft_id
             FROM sponsor_jobs WHERE id = ?1",
            params![id],
            |r| {
                Ok(json!({
                    "id": r.get::<_, String>(0)?,
                    "subject": r.get::<_, String>(1)?,
                    "sponsor_token": r.get::<_, Option<String>>(2)?,
                    "company_name": r.get::<_, Option<String>>(3)?,
                    "kind": r.get::<_, String>(4)?,
                    "status": r.get::<_, String>(5)?,
                    "started_at": r.get::<_, String>(6)?,
                    "error_code": r.get::<_, Option<String>>(7)?,
                    "draft_id": r.get::<_, Option<String>>(8)?,
                }))
            },
        )
        .optional()?;
    Ok(row)
}

/// Drop a job row entirely — used on cancel, so the action falls back to
/// showing only its cached drafts (no partial draft body is ever written).
pub fn delete_sponsor_job(conn: &Connection, id: &str) -> AppResult<()> {
    conn.execute("DELETE FROM sponsor_jobs WHERE id = ?1", params![id])?;
    Ok(())
}

// ── RSVP screening (specs/rsvp-screening) ──────────────────────────────────

/// Upsert one RSVP row from either an `rsvps/search` result item or an
/// `rsvps/get` response — both nest the record under a top-level `rsvp`
/// (+ `client`) object, so the same defensive extraction handles either shape
/// (task 1.2/1.3). Returns the resolved `rsvp_ref`, or `None` if the payload
/// carries no identifiable token.
pub fn upsert_rsvp_row(
    conn: &Connection,
    meetup_token: &str,
    row: &Value,
    now: &str,
) -> AppResult<Option<String>> {
    let rsvp = row.get("rsvp").cloned().unwrap_or_else(|| row.clone());
    let client = row.get("client").cloned().unwrap_or(Value::Null);

    let Some(rsvp_ref) = pick_str(&rsvp, &["rsvp_token", "token", "rsvp_ref"])
        .or_else(|| pick_str(row, &["rsvp_token", "token", "rsvp_ref"]))
        .map(str::to_string)
    else {
        return Ok(None);
    };

    let name = pick_str(&client, &["name"]).or_else(|| pick_str(row, &["name"])).map(str::to_string);
    let email = pick_str(&client, &["email"]).or_else(|| pick_str(row, &["email"])).map(str::to_string);
    let state = pick_str(&rsvp, &["state"]).unwrap_or("unknown").to_string();
    let registrant_status = pick_str(&rsvp, &["registrant_status"]).map(str::to_string);
    let registrant_status_label = pick_str(&rsvp, &["registrant_status_label"]).map(str::to_string);
    let registrant_status_text = pick_str(&rsvp, &["registrant_status_text"]).map(str::to_string);
    let checked_in = rsvp.get("checked_in").and_then(Value::as_bool).unwrap_or(false);
    let score = pick_num(&rsvp, &["score", "engagement_score", "user_score"])
        .or_else(|| pick_num(&client, &["score", "engagement_score"]));

    conn.execute(
        "INSERT INTO rsvp_rows
           (rsvp_ref, meetup_token, name, email, state, registrant_status,
            registrant_status_label, registrant_status_text, checked_in, score, raw_json, updated_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
         ON CONFLICT(rsvp_ref) DO UPDATE SET
           meetup_token=excluded.meetup_token,
           name=excluded.name,
           email=excluded.email,
           state=excluded.state,
           registrant_status=excluded.registrant_status,
           registrant_status_label=excluded.registrant_status_label,
           registrant_status_text=excluded.registrant_status_text,
           checked_in=excluded.checked_in,
           score=excluded.score,
           raw_json=excluded.raw_json,
           updated_at=excluded.updated_at",
        params![
            rsvp_ref, meetup_token, name, email, state, registrant_status,
            registrant_status_label, registrant_status_text, checked_in as i64,
            score, row.to_string(), now
        ],
    )?;
    Ok(Some(rsvp_ref))
}

fn rsvp_row_json(r: &rusqlite::Row) -> rusqlite::Result<Value> {
    Ok(json!({
        "rsvp_ref": r.get::<_, String>(0)?,
        "meetup_token": r.get::<_, String>(1)?,
        "name": r.get::<_, Option<String>>(2)?,
        "email": r.get::<_, Option<String>>(3)?,
        "state": r.get::<_, String>(4)?,
        "registrant_status": r.get::<_, Option<String>>(5)?,
        "registrant_status_label": r.get::<_, Option<String>>(6)?,
        "registrant_status_text": r.get::<_, Option<String>>(7)?,
        "checked_in": r.get::<_, i64>(8)? != 0,
        "score": r.get::<_, Option<f64>>(9)?,
        "updated_at": r.get::<_, String>(10)?,
    }))
}

const RSVP_ROW_COLS: &str = "rsvp_ref, meetup_token, name, email, state, registrant_status,
    registrant_status_label, registrant_status_text, checked_in, score, updated_at";

/// One cached RSVP row (used to look up `from_state` before a mutation, and
/// as the post-commit re-read result).
pub fn get_rsvp_row(conn: &Connection, rsvp_ref: &str) -> AppResult<Option<Value>> {
    let sql = format!("SELECT {RSVP_ROW_COLS} FROM rsvp_rows WHERE rsvp_ref = ?1");
    let row = conn.query_row(&sql, params![rsvp_ref], rsvp_row_json).optional()?;
    Ok(row)
}

/// The cached attendee list for one event. Free-text/status filtering happens
/// client-side over this set (spec: "list MUST render only from cached data").
pub fn get_rsvp_rows(conn: &Connection, meetup_token: &str) -> AppResult<Value> {
    let sql = format!(
        "SELECT {RSVP_ROW_COLS} FROM rsvp_rows WHERE meetup_token = ?1 ORDER BY (name IS NULL), name"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![meetup_token], rsvp_row_json)?;
    let list: Vec<Value> = rows.filter_map(Result::ok).collect();
    Ok(json!({ "meetup_token": meetup_token, "rows": list }))
}

/// Evict rows for this event no longer returned by the latest sync sweep,
/// scoped by `meetup_token` so refreshing one event never touches another's
/// cached attendees (mirrors `retain_events`/`retain_send_jobs`).
pub fn retain_rsvp_rows(conn: &Connection, meetup_token: &str, keep: &[String]) -> AppResult<()> {
    let existing: Vec<String> = {
        let mut stmt = conn.prepare("SELECT rsvp_ref FROM rsvp_rows WHERE meetup_token = ?1")?;
        let rows = stmt.query_map(params![meetup_token], |r| r.get::<_, String>(0))?;
        rows.filter_map(Result::ok).collect()
    };
    for rref in existing {
        if !keep.iter().any(|k| k == &rref) {
            conn.execute("DELETE FROM rsvp_rows WHERE rsvp_ref = ?1", params![rref])?;
        }
    }
    Ok(())
}

/// Upsert one registrant's detail sources. Each of the three is only touched
/// when its `Some((json, status))` argument is provided, so fetching one
/// source never clobbers a previously-fetched one (same pattern as
/// `upsert_survey_followup`).
#[allow(clippy::too_many_arguments)]
pub fn upsert_rsvp_detail(
    conn: &Connection,
    rsvp_ref: &str,
    assessment: Option<(Option<&Value>, &str)>,
    history: Option<(Option<&Value>, &str)>,
    score: Option<(Option<&Value>, &str)>,
    now: &str,
) -> AppResult<()> {
    let touch_a = assessment.is_some();
    let (a_json, a_status) = assessment
        .map(|(j, s)| (j.map(|v| v.to_string()), s.to_string()))
        .unwrap_or((None, "unavailable".to_string()));
    let touch_h = history.is_some();
    let (h_json, h_status) = history
        .map(|(j, s)| (j.map(|v| v.to_string()), s.to_string()))
        .unwrap_or((None, "unavailable".to_string()));
    let touch_s = score.is_some();
    let (s_json, s_status) = score
        .map(|(j, s)| (j.map(|v| v.to_string()), s.to_string()))
        .unwrap_or((None, "unavailable".to_string()));

    conn.execute(
        "INSERT INTO rsvp_detail (rsvp_ref, assessment_json, assessment_status, history_json, history_status, score_json, score_status, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(rsvp_ref) DO UPDATE SET
           assessment_json=CASE WHEN ?9 THEN excluded.assessment_json ELSE rsvp_detail.assessment_json END,
           assessment_status=CASE WHEN ?9 THEN excluded.assessment_status ELSE rsvp_detail.assessment_status END,
           history_json=CASE WHEN ?10 THEN excluded.history_json ELSE rsvp_detail.history_json END,
           history_status=CASE WHEN ?10 THEN excluded.history_status ELSE rsvp_detail.history_status END,
           score_json=CASE WHEN ?11 THEN excluded.score_json ELSE rsvp_detail.score_json END,
           score_status=CASE WHEN ?11 THEN excluded.score_status ELSE rsvp_detail.score_status END,
           updated_at=excluded.updated_at",
        params![rsvp_ref, a_json, a_status, h_json, h_status, s_json, s_status, now, touch_a, touch_h, touch_s],
    )?;
    Ok(())
}

pub fn get_rsvp_detail(conn: &Connection, rsvp_ref: &str) -> AppResult<Option<Value>> {
    let row = conn
        .query_row(
            "SELECT assessment_json, assessment_status, history_json, history_status, score_json, score_status, updated_at
             FROM rsvp_detail WHERE rsvp_ref = ?1",
            params![rsvp_ref],
            |r| {
                Ok(json!({
                    "rsvp_ref": rsvp_ref,
                    "assessment": r.get::<_, Option<String>>(0)?
                        .and_then(|s| serde_json::from_str::<Value>(&s).ok()),
                    "assessment_status": r.get::<_, String>(1)?,
                    "history": r.get::<_, Option<String>>(2)?
                        .and_then(|s| serde_json::from_str::<Value>(&s).ok()),
                    "history_status": r.get::<_, String>(3)?,
                    "score": r.get::<_, Option<String>>(4)?
                        .and_then(|s| serde_json::from_str::<Value>(&s).ok()),
                    "score_status": r.get::<_, String>(5)?,
                    "updated_at": r.get::<_, String>(6)?,
                }))
            },
        )
        .optional()?;
    Ok(row)
}

// ── Write audit trail (design D3) ───────────────────────────────────────────
// Append-only. Never deleted by sign-out's cache wipe (commands.rs) — this is
// the durable record of every mutation attempt, not a cache.

/// Insert the `attempted` row BEFORE the API call. Returns nothing further to
/// update by primary key; the caller already has `id`.
#[allow(clippy::too_many_arguments)]
pub fn insert_write_audit(
    conn: &Connection,
    id: &str,
    action: &str,
    meetup_token: Option<&str>,
    targets: &[String],
    from_state: Option<&str>,
    to_state: Option<&str>,
    send_email: bool,
    confirmed: bool,
    now: &str,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO write_audit
           (id, created_at, actor, action, meetup_token, targets_json, from_state, to_state, send_email, confirmed, outcome, error_code, updated_at)
         VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'attempted', NULL, ?2)",
        params![
            id, now, action, meetup_token, json!(targets).to_string(),
            from_state, to_state, send_email as i64, confirmed as i64
        ],
    )?;
    Ok(())
}

/// Update the outcome AFTER the API call resolves (success, forbidden_*,
/// rate_limited, network, or any other error code) — so the row always ends
/// up reflecting what actually happened, even on a crash between the two.
pub fn update_write_audit_outcome(
    conn: &Connection,
    id: &str,
    outcome: &str,
    error_code: Option<&str>,
    now: &str,
) -> AppResult<()> {
    conn.execute(
        "UPDATE write_audit SET outcome = ?2, error_code = ?3, updated_at = ?4 WHERE id = ?1",
        params![id, outcome, error_code, now],
    )?;
    Ok(())
}

fn write_audit_row(r: &rusqlite::Row) -> rusqlite::Result<Value> {
    let targets: Vec<String> = r
        .get::<_, String>(3)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    Ok(json!({
        "id": r.get::<_, String>(0)?,
        "created_at": r.get::<_, String>(1)?,
        "action": r.get::<_, String>(2)?,
        "targets": targets,
        "from_state": r.get::<_, Option<String>>(4)?,
        "to_state": r.get::<_, Option<String>>(5)?,
        "send_email": r.get::<_, i64>(6)? != 0,
        "confirmed": r.get::<_, i64>(7)? != 0,
        "outcome": r.get::<_, String>(8)?,
        "error_code": r.get::<_, Option<String>>(9)?,
        "updated_at": r.get::<_, String>(10)?,
    }))
}

const WRITE_AUDIT_COLS: &str = "id, created_at, action, targets_json, from_state, to_state, send_email, confirmed, outcome, error_code, updated_at";

/// Recent write-audit entries for one event, newest first — the attendee
/// screen surfaces this alongside the server-side status history (design D3).
pub fn get_write_audit_for_event(conn: &Connection, meetup_token: &str, limit: i64) -> AppResult<Value> {
    let sql = format!(
        "SELECT {WRITE_AUDIT_COLS} FROM write_audit WHERE meetup_token = ?1 ORDER BY created_at DESC LIMIT ?2"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![meetup_token, limit], write_audit_row)?;
    Ok(json!(rows.filter_map(Result::ok).collect::<Vec<_>>()))
}

/// One audit entry by id — exercised by the guardrail tests below; also
/// available for a future direct outcome lookup by callers outside this module.
#[allow(dead_code)]
pub fn get_write_audit(conn: &Connection, id: &str) -> AppResult<Option<Value>> {
    let sql = format!("SELECT {WRITE_AUDIT_COLS} FROM write_audit WHERE id = ?1");
    let row = conn.query_row(&sql, params![id], write_audit_row).optional()?;
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

    // ── Sponsor tools (specs/sponsor-tools) ────────────────────────────────

    #[test]
    fn sponsor_search_round_trip_resolves_sponsor_rows_in_order() {
        let c = mem();
        let matches = vec![
            json!({ "sponsor_token": "sp1", "name": "Acme Robotics", "city": "Denver", "domain": "acme.dev" }),
            json!({ "sponsor_token": "sp2", "name": "Beta Corp", "city": "Denver" }),
        ];
        upsert_sponsor_search(&c, "acme", "", "", false, &matches, false, false, None, "t1").unwrap();

        let res = get_sponsor_search(&c, "acme", "", "", false).unwrap();
        let results = res.get("results").and_then(Value::as_array).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].get("sponsor_token").and_then(Value::as_str), Some("sp1"));
        assert_eq!(results[0].get("name").and_then(Value::as_str), Some("Acme Robotics"));
        assert_eq!(res.get("truncated").and_then(Value::as_bool), Some(false));
        assert_eq!(res.get("unavailable").and_then(Value::as_bool), Some(false));

        // A distinct query has its own cache slot and doesn't see these results.
        let empty = get_sponsor_search(&c, "other", "", "", false).unwrap();
        assert_eq!(empty.get("results").and_then(Value::as_array).unwrap().len(), 0);
    }

    #[test]
    fn sponsor_search_degrade_state_is_isolated_per_query() {
        let c = mem();
        upsert_sponsor_search(&c, "ok_query", "", "", false, &[], false, false, None, "t1").unwrap();
        upsert_sponsor_search(&c, "blocked_query", "", "", false, &[], false, true, Some("forbidden_api_group"), "t1").unwrap();

        let ok = get_sponsor_search(&c, "ok_query", "", "", false).unwrap();
        assert_eq!(ok.get("unavailable").and_then(Value::as_bool), Some(false));

        let blocked = get_sponsor_search(&c, "blocked_query", "", "", false).unwrap();
        assert_eq!(blocked.get("unavailable").and_then(Value::as_bool), Some(true));
        assert_eq!(blocked.get("reason").and_then(Value::as_str), Some("forbidden_api_group"));
    }

    #[test]
    fn sponsor_contacts_round_trip_and_masking_flags() {
        let c = mem();
        let contacts = vec![
            json!({ "contact_id": "c1", "role": "Marketing", "title": "VP Marketing", "email": "***@acme.dev", "linkedin": "in/x" }),
            json!({ "contact_id": "c2", "role": "Sales", "email": "sales@acme.dev", "email_masked": false }),
        ];
        upsert_sponsor_contacts(&c, "sp1", &contacts, false, false, None, "t1").unwrap();

        let row = get_sponsor_contacts(&c, "sp1").unwrap();
        let list = row.get("contacts").and_then(Value::as_array).unwrap();
        assert_eq!(list.len(), 2);
        // Masking is inferred from the sentinel value when no explicit flag is present.
        assert_eq!(list[0].get("email_masked").and_then(Value::as_bool), Some(true));
        assert_eq!(list[0].get("email").and_then(Value::as_str), Some("***@acme.dev"));
        // An explicit `email_masked: false` is honored even though absent here.
        assert_eq!(list[1].get("email_masked").and_then(Value::as_bool), Some(false));
        assert_eq!(row.get("unavailable").and_then(Value::as_bool), Some(false));
    }

    #[test]
    fn sponsor_contacts_refetch_replaces_the_whole_set() {
        let c = mem();
        upsert_sponsor_contacts(&c, "sp1", &[json!({ "contact_id": "c1", "role": "Old" })], false, false, None, "t1").unwrap();
        upsert_sponsor_contacts(&c, "sp1", &[json!({ "contact_id": "c2", "role": "New" })], false, false, None, "t2").unwrap();

        let row = get_sponsor_contacts(&c, "sp1").unwrap();
        let list = row.get("contacts").and_then(Value::as_array).unwrap();
        assert_eq!(list.len(), 1, "a fresh fetch must replace stale contacts, not merge with them");
        assert_eq!(list[0].get("role").and_then(Value::as_str), Some("New"));
    }

    #[test]
    fn sponsor_draft_upsert_per_kind_does_not_clobber_another_kind() {
        let c = mem();
        let subject = sponsor_subject_key(Some("sp1"), None);
        insert_sponsor_draft(&c, "d1", &subject, Some("sp1"), None, "research", &json!({}), &json!({ "research_summary": "brief" }), "t1").unwrap();
        insert_sponsor_draft(&c, "d2", &subject, Some("sp1"), None, "pitch", &json!({}), &json!({ "pitch_text": "hello" }), "t2").unwrap();

        let research = list_sponsor_drafts(&c, &subject, Some("research")).unwrap();
        let research_arr = research.as_array().unwrap();
        assert_eq!(research_arr.len(), 1);
        assert_eq!(
            research_arr[0].get("result").and_then(|r| r.get("research_summary")).and_then(Value::as_str),
            Some("brief"),
            "generating a pitch draft must not clobber the research draft for the same subject"
        );

        let pitch = list_sponsor_drafts(&c, &subject, Some("pitch")).unwrap();
        let pitch_arr = pitch.as_array().unwrap();
        assert_eq!(pitch_arr.len(), 1);
        assert_eq!(pitch_arr[0].get("result").and_then(|r| r.get("pitch_text")).and_then(Value::as_str), Some("hello"));

        let all = list_sponsor_drafts(&c, &subject, None).unwrap();
        assert_eq!(all.as_array().unwrap().len(), 2, "unfiltered list must include both kinds");
    }

    #[test]
    fn sponsor_draft_history_keeps_prior_generations() {
        let c = mem();
        let subject = sponsor_subject_key(None, Some("Acme Robotics"));
        insert_sponsor_draft(&c, "d1", &subject, None, Some("Acme Robotics"), "research", &json!({}), &json!({ "research_summary": "v1" }), "t1").unwrap();
        insert_sponsor_draft(&c, "d2", &subject, None, Some("Acme Robotics"), "research", &json!({}), &json!({ "research_summary": "v2" }), "t2").unwrap();

        let drafts = list_sponsor_drafts(&c, &subject, Some("research")).unwrap();
        let arr = drafts.as_array().unwrap();
        assert_eq!(arr.len(), 2, "regeneration must not clobber the earlier draft — history is kept");
        // Newest first.
        assert_eq!(arr[0].get("draft_id").and_then(Value::as_str), Some("d2"));
        assert_eq!(arr[1].get("draft_id").and_then(Value::as_str), Some("d1"));
    }

    #[test]
    fn sponsor_job_status_transitions_and_duplicate_suppression() {
        let c = mem();
        let subject = sponsor_subject_key(Some("sp1"), None);
        create_sponsor_job(&c, "job1", &subject, Some("sp1"), None, "research", "hash1", "t1").unwrap();
        let job = get_sponsor_job(&c, "job1").unwrap().expect("job must exist");
        assert_eq!(job.get("status").and_then(Value::as_str), Some("pending"));

        // Duplicate kickoff suppression: a pending job for the same (subject, kind)
        // is found by find_active_sponsor_job (rate-limit guard).
        assert_eq!(find_active_sponsor_job(&c, &subject, "research").unwrap().as_deref(), Some("job1"));
        // A different kind for the same subject is not blocked.
        assert!(find_active_sponsor_job(&c, &subject, "pitch").unwrap().is_none());

        set_sponsor_job_status(&c, "job1", "running", None, None).unwrap();
        assert_eq!(get_sponsor_job(&c, "job1").unwrap().unwrap().get("status").and_then(Value::as_str), Some("running"));
        assert!(find_active_sponsor_job(&c, &subject, "research").unwrap().is_some());

        set_sponsor_job_status(&c, "job1", "ready", None, Some("d1")).unwrap();
        let job = get_sponsor_job(&c, "job1").unwrap().unwrap();
        assert_eq!(job.get("status").and_then(Value::as_str), Some("ready"));
        assert_eq!(job.get("draft_id").and_then(Value::as_str), Some("d1"));
        assert!(find_active_sponsor_job(&c, &subject, "research").unwrap().is_none(), "a terminal job must no longer be in-flight");
    }

    #[test]
    fn sponsor_job_error_and_timeout_carry_a_code() {
        let c = mem();
        let subject = sponsor_subject_key(Some("sp1"), None);
        create_sponsor_job(&c, "job2", &subject, Some("sp1"), None, "pitch", "hash2", "t1").unwrap();
        set_sponsor_job_status(&c, "job2", "timeout", Some("timeout"), None).unwrap();
        let job = get_sponsor_job(&c, "job2").unwrap().unwrap();
        assert_eq!(job.get("status").and_then(Value::as_str), Some("timeout"));
        assert_eq!(job.get("error_code").and_then(Value::as_str), Some("timeout"));

        create_sponsor_job(&c, "job3", &subject, Some("sp1"), None, "research", "hash3", "t1").unwrap();
        set_sponsor_job_status(&c, "job3", "error", Some("rate_limited"), None).unwrap();
        let job = get_sponsor_job(&c, "job3").unwrap().unwrap();
        assert_eq!(job.get("status").and_then(Value::as_str), Some("error"));
        assert_eq!(job.get("error_code").and_then(Value::as_str), Some("rate_limited"));
    }

    #[test]
    fn sponsor_job_cancel_deletes_the_row_and_frees_the_slot() {
        let c = mem();
        let subject = sponsor_subject_key(Some("sp1"), None);
        create_sponsor_job(&c, "job4", &subject, Some("sp1"), None, "research", "hash4", "t1").unwrap();
        delete_sponsor_job(&c, "job4").unwrap();
        assert!(get_sponsor_job(&c, "job4").unwrap().is_none());
        assert!(find_active_sponsor_job(&c, &subject, "research").unwrap().is_none());
    }

    #[test]
    fn sponsor_subject_key_distinguishes_token_and_name() {
        assert_eq!(sponsor_subject_key(Some("sp1"), None), "token:sp1");
        assert_eq!(sponsor_subject_key(None, Some("Acme Robotics")), "name:acme robotics");
        // A blank sponsor_token falls back to the name form.
        assert_eq!(sponsor_subject_key(Some(""), Some("Acme")), "name:acme");
    }

    // ── RSVP screening (specs/rsvp-screening) ──────────────────────────────

    #[test]
    fn rsvp_row_round_trip_from_search_shaped_payload() {
        let c = mem();
        let row = json!({
            "rsvp": {
                "rsvp_token": "rsvp1", "state": "waitlisted",
                "registrant_status": "waitlisted", "registrant_status_label": "Waitlisted",
                "registrant_status_text": "You're on the waitlist", "checked_in": false, "score": 42.5
            },
            "client": { "name": "Jane Smith", "email": "jane@example.com" }
        });
        let rref = upsert_rsvp_row(&c, "m1", &row, "t1").unwrap();
        assert_eq!(rref.as_deref(), Some("rsvp1"));

        let cached = get_rsvp_row(&c, "rsvp1").unwrap().expect("row must exist");
        assert_eq!(cached.get("state").and_then(Value::as_str), Some("waitlisted"));
        assert_eq!(cached.get("registrant_status_label").and_then(Value::as_str), Some("Waitlisted"));
        assert_eq!(cached.get("name").and_then(Value::as_str), Some("Jane Smith"));
        assert_eq!(cached.get("score").and_then(Value::as_f64), Some(42.5));

        let list = get_rsvp_rows(&c, "m1").unwrap();
        let rows = list.get("rows").and_then(Value::as_array).unwrap();
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn rsvp_row_missing_token_is_skipped() {
        let c = mem();
        let row = json!({ "rsvp": { "state": "attending" }, "client": { "name": "No Token" } });
        assert!(upsert_rsvp_row(&c, "m1", &row, "t1").unwrap().is_none());
    }

    #[test]
    fn rsvp_row_retention_is_event_scoped() {
        let c = mem();
        upsert_rsvp_row(&c, "m1", &json!({ "rsvp": { "rsvp_token": "a1", "state": "attending" } }), "t1").unwrap();
        upsert_rsvp_row(&c, "m2", &json!({ "rsvp": { "rsvp_token": "b1", "state": "attending" } }), "t1").unwrap();
        // A refresh of m1 that keeps nothing must not evict m2's row.
        retain_rsvp_rows(&c, "m1", &[]).unwrap();
        let m1_rows = get_rsvp_rows(&c, "m1").unwrap();
        assert_eq!(m1_rows.get("rows").and_then(Value::as_array).unwrap().len(), 0);
        let m2_rows = get_rsvp_rows(&c, "m2").unwrap();
        assert_eq!(m2_rows.get("rows").and_then(Value::as_array).unwrap().len(), 1, "other event's row must survive");
    }

    #[test]
    fn rsvp_detail_sources_degrade_independently() {
        let c = mem();
        let assessment = json!({ "summary": "Strong fit" });
        upsert_rsvp_detail(&c, "rsvp1", Some((Some(&assessment), "ok")), None, Some((None, "forbidden_scope")), "t1").unwrap();
        let row = get_rsvp_detail(&c, "rsvp1").unwrap().expect("row must exist");
        assert_eq!(row.get("assessment_status").and_then(Value::as_str), Some("ok"));
        assert_eq!(row.get("history_status").and_then(Value::as_str), Some("unavailable"), "untouched source stays at its default");
        assert_eq!(row.get("score_status").and_then(Value::as_str), Some("forbidden_scope"));

        // A later fetch of only the history source must not clobber the
        // already-fetched assessment or the still-forbidden score.
        let history = json!({ "events": [{ "to_status": "waitlisted" }] });
        upsert_rsvp_detail(&c, "rsvp1", None, Some((Some(&history), "ok")), None, "t2").unwrap();
        let row2 = get_rsvp_detail(&c, "rsvp1").unwrap().unwrap();
        assert_eq!(row2.get("assessment_status").and_then(Value::as_str), Some("ok"));
        assert_eq!(row2.get("history_status").and_then(Value::as_str), Some("ok"));
        assert_eq!(row2.get("score_status").and_then(Value::as_str), Some("forbidden_scope"), "score status must persist untouched");
    }

    #[test]
    fn write_audit_before_and_after_success() {
        let c = mem();
        insert_write_audit(&c, "aud1", "rsvp_state_update", Some("m1"), &["rsvp1".to_string()], Some("waitlisted"), Some("attending"), true, true, "t1").unwrap();
        let row = get_write_audit(&c, "aud1").unwrap().expect("row must exist");
        assert_eq!(row.get("outcome").and_then(Value::as_str), Some("attempted"), "row must exist before the API call resolves");

        update_write_audit_outcome(&c, "aud1", "ok", None, "t2").unwrap();
        let row2 = get_write_audit(&c, "aud1").unwrap().unwrap();
        assert_eq!(row2.get("outcome").and_then(Value::as_str), Some("ok"));
        assert_eq!(row2.get("from_state").and_then(Value::as_str), Some("waitlisted"));
        assert_eq!(row2.get("to_state").and_then(Value::as_str), Some("attending"));
    }

    #[test]
    fn write_audit_records_denied_and_failed_outcomes() {
        let c = mem();
        insert_write_audit(&c, "aud2", "rsvp_state_update", Some("m1"), &["rsvp2".to_string()], Some("registered"), Some("denied"), true, true, "t1").unwrap();
        update_write_audit_outcome(&c, "aud2", "forbidden_scope", Some("forbidden_scope"), "t2").unwrap();
        let row = get_write_audit(&c, "aud2").unwrap().unwrap();
        assert_eq!(row.get("outcome").and_then(Value::as_str), Some("forbidden_scope"));
        assert_eq!(row.get("error_code").and_then(Value::as_str), Some("forbidden_scope"));

        insert_write_audit(&c, "aud3", "rsvp_bulk_state_update", Some("m1"), &["rsvp3".to_string(), "rsvp4".to_string()], None, Some("attending"), false, true, "t1").unwrap();
        update_write_audit_outcome(&c, "aud3", "rate_limited", Some("rate_limited"), "t2").unwrap();
        let row3 = get_write_audit(&c, "aud3").unwrap().unwrap();
        assert_eq!(row3.get("outcome").and_then(Value::as_str), Some("rate_limited"));
        let targets = row3.get("targets").and_then(Value::as_array).unwrap();
        assert_eq!(targets.len(), 2);
    }

    #[test]
    fn write_audit_for_event_orders_newest_first_and_is_scoped() {
        let c = mem();
        insert_write_audit(&c, "e1", "rsvp_state_update", Some("m1"), &["r1".to_string()], None, Some("attending"), true, true, "t1").unwrap();
        insert_write_audit(&c, "e2", "rsvp_state_update", Some("m1"), &["r2".to_string()], None, Some("denied"), true, true, "t2").unwrap();
        insert_write_audit(&c, "e3", "rsvp_state_update", Some("m2"), &["r3".to_string()], None, Some("attending"), true, true, "t3").unwrap();

        let list = get_write_audit_for_event(&c, "m1", 10).unwrap();
        let arr = list.as_array().unwrap();
        assert_eq!(arr.len(), 2, "must not include the other event's audit rows");
        assert_eq!(arr[0].get("id").and_then(Value::as_str), Some("e2"), "newest first");
        assert_eq!(arr[1].get("id").and_then(Value::as_str), Some("e1"));
    }
}
