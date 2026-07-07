use std::sync::atomic::Ordering;

use chrono::{Duration, Utc};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

use crate::api::{ApiClient, RateInfo};
use crate::error::{AppError, AppResult};
use crate::state::{AppState, TRAY_ID};
use crate::db;

const UPCOMING_KEY: &str = "upcoming";
const PAST_KEY: &str = "past";
const PAST_LIMIT: u32 = 50;
// Honor the API's Retry-After up to 6 hours. The daily-budget rate limit returns
// a Retry-After of many hours; capping at 60s made the poll loop keep hammering
// the maxed-out API every ~2 min, which pinned the budget and never let it
// recover. Per-minute limits return small Retry-Afters, so those still back off
// only briefly.
const MAX_BACKOFF_SECS: i64 = 6 * 3600;

fn iso_now() -> String {
    Utc::now().to_rfc3339()
}

/// Build a client from the stored key, or `None` if onboarding isn't done.
fn client(app: &AppHandle) -> AppResult<Option<ApiClient>> {
    Ok(app
        .state::<AppState>()
        .api_key_cached()?
        .map(ApiClient::new))
}

/// Parse `retry_after=NN` out of a rate-limit error message.
fn parse_retry_after(msg: &str) -> Option<i64> {
    msg.split("retry_after=")
        .nth(1)
        .and_then(|s| s.trim().parse::<i64>().ok())
}

/// Compute and persist an exponential backoff window for an endpoint key.
fn apply_backoff(app: &AppHandle, key: &str, retry_after: Option<i64>) {
    let state = app.state::<AppState>();
    let conn = state.db.lock().unwrap();
    // Grow the wait from any prior backoff; honor Retry-After as a floor.
    let base = retry_after.unwrap_or(1).max(1);
    let wait = base.min(MAX_BACKOFF_SECS);
    let until = (Utc::now() + Duration::seconds(wait)).to_rfc3339();
    let _ = db::set_sync_state(
        &conn,
        key,
        None,
        None,
        Some(&until),
        false,
        Some("rate_limited"), // stable marker the UI can surface
    );
}

/// True when the endpoint is still inside its backoff window.
fn in_backoff(app: &AppHandle, key: &str) -> bool {
    let state = app.state::<AppState>();
    let conn = state.db.lock().unwrap();
    match db::get_backoff(&conn, key) {
        Ok(Some(until)) => chrono::DateTime::parse_from_rfc3339(&until)
            .map(|t| t > Utc::now())
            .unwrap_or(false),
        _ => false,
    }
}

fn as_i64(v: &Value, key: &str) -> i64 {
    v.get(key).and_then(Value::as_i64).unwrap_or(0)
}

/// Run one sync cycle. `force` bypasses backoff-free scheduling for a manual
/// refresh but still respects an active backoff window.
pub async fn run_cycle(app: AppHandle, force: bool) -> AppResult<()> {
    let Some(api) = client(&app)? else {
        return Ok(()); // not onboarded yet
    };

    // Respect an active rate-limit backoff even on manual refresh.
    if in_backoff(&app, UPCOMING_KEY) {
        let _ = app.emit("sync:backoff", json!({ "endpoint": UPCOMING_KEY }));
        if !force {
            return Ok(());
        }
    }

    let state = app.state::<AppState>();
    if state
        .syncing
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Ok(()); // a cycle is already running
    }
    let result = do_upcoming(&app, &api).await;
    state.syncing.store(false, Ordering::SeqCst);

    match result {
        Ok(rate) => {
            record_rate(&app, UPCOMING_KEY, &rate);
            let _ = app.emit("sync:updated", json!({ "at": iso_now() }));
            update_tray(&app);
            Ok(())
        }
        Err(AppError::RateLimited(msg)) => {
            apply_backoff(&app, UPCOMING_KEY, parse_retry_after(&msg));
            let _ = app.emit("sync:backoff", json!({ "endpoint": UPCOMING_KEY }));
            Err(AppError::RateLimited(msg))
        }
        Err(e) => {
            let _ = app.emit("sync:error", json!({ "message": e.to_string() }));
            Err(e)
        }
    }
}

/// Fetch upcoming events, diff, upsert, and fire notifications.
async fn do_upcoming(app: &AppHandle, api: &ApiClient) -> AppResult<RateInfo> {
    let ok = api.upcoming_events(100).await?;
    let events = ok
        .data
        .get("events")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let truncated = ok
        .data
        .get("truncated")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let now = iso_now();
    let first_done = app.state::<AppState>().first_sync_done.load(Ordering::SeqCst);
    let notifs_on = app
        .state::<AppState>()
        .notifications_enabled
        .load(Ordering::SeqCst);

    let mut keep: Vec<String> = Vec::new();
    let mut pending_notifs: Vec<(String, i64, i64)> = Vec::new(); // name, old, new

    for ev in &events {
        let meetup_token = ev
            .get("meetup_token")
            .and_then(Value::as_str)
            .or_else(|| ev.get("refs").and_then(|r| r.get("meetup_token")).and_then(Value::as_str))
            .unwrap_or_default()
            .to_string();
        if meetup_token.is_empty() {
            continue;
        }
        let weblog_token = ev.get("weblog_token").and_then(Value::as_str).unwrap_or_default();
        let starts_at = ev
            .get("starts_at_utc")
            .and_then(Value::as_str)
            .or_else(|| ev.get("starts_at").and_then(Value::as_str))
            .unwrap_or_default();
        let rsvps = ev.get("rsvps").cloned().unwrap_or(Value::Null);
        let attending = as_i64(&rsvps, "attending");
        let waitlisted = as_i64(&rsvps, "waitlisted");
        let paid = ev
            .get("stripe_payment_link_active")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let state = app.state::<AppState>();
        let conn = state.db.lock().unwrap();
        let prev = db::prev_counts(&conn, &meetup_token).unwrap_or(None);
        db::upsert_event(
            &conn, &meetup_token, weblog_token, starts_at, attending, waitlisted, paid,
            "upcoming", ev, &now,
        )?;
        drop(conn);

        if let Some((prev_att, prev_wait)) = prev {
            if first_done
                && notifs_on
                && (prev_att != attending || prev_wait != waitlisted)
            {
                let name = ev
                    .get("event_name")
                    .and_then(Value::as_str)
                    .unwrap_or("Event")
                    .to_string();
                pending_notifs.push((name, prev_att, attending));
            }
        }
        keep.push(meetup_token);
    }

    {
        let state = app.state::<AppState>();
        let conn = state.db.lock().unwrap();
        db::retain_events(&conn, "upcoming", &keep)?;
        db::set_sync_state(
            &conn,
            UPCOMING_KEY,
            Some(&now),
            ok.rate.remaining,
            None,
            false,
            if truncated { Some("truncated") } else { None },
        )?;
    }

    // One notification per changed event per cycle (specs/tray-notifications).
    for (name, old, new) in pending_notifs {
        let body = format!("attending {old} → {new}");
        let _ = app
            .notification()
            .builder()
            .title(&name)
            .body(&body)
            .show();
    }

    app.state::<AppState>()
        .first_sync_done
        .store(true, Ordering::SeqCst);
    Ok(ok.rate)
}

/// Fetch the caller's past events (recap data). Runs on launch and manual
/// refresh only — never on the upcoming poll interval. Past events are frozen:
/// they never fire notifications and never claim the tray "next event".
pub async fn run_past(app: AppHandle) -> AppResult<()> {
    let Some(api) = client(&app)? else {
        return Ok(());
    };
    if in_backoff(&app, PAST_KEY) {
        return Ok(());
    }

    let ok = match api.past_events(PAST_LIMIT).await {
        Ok(ok) => ok,
        Err(AppError::RateLimited(msg)) => {
            apply_backoff(&app, PAST_KEY, parse_retry_after(&msg));
            return Err(AppError::RateLimited(msg));
        }
        Err(e) => {
            // Degrade quietly; the Upcoming tab is unaffected.
            let state = app.state::<AppState>();
            let conn = state.db.lock().unwrap();
            let _ = db::set_sync_state(
                &conn, PAST_KEY, None, None, None,
                e.is_capability_block(), Some(&e.to_string()),
            );
            return Err(e);
        }
    };

    // Search returns matches[]; tolerate an events[] fallback.
    let events = ok
        .data
        .get("matches")
        .or_else(|| ok.data.get("events"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let truncated = ok.data.get("truncated").and_then(Value::as_bool).unwrap_or(false);

    let now = iso_now();
    let mut keep: Vec<String> = Vec::new();

    for ev in &events {
        let meetup_token = ev
            .get("meetup_token")
            .and_then(Value::as_str)
            .or_else(|| ev.get("refs").and_then(|r| r.get("meetup_token")).and_then(Value::as_str))
            .unwrap_or_default()
            .to_string();
        if meetup_token.is_empty() {
            continue;
        }

        let state = app.state::<AppState>();
        let conn = state.db.lock().unwrap();
        // Dedupe: if a token is already cached as upcoming (around start time),
        // let the upcoming row win — don't shadow it with a past copy.
        if db::event_kind(&conn, &meetup_token).unwrap_or(None).as_deref() == Some("upcoming") {
            continue;
        }
        let weblog_token = ev.get("weblog_token").and_then(Value::as_str).unwrap_or_default();
        let starts_at = ev
            .get("starts_at_utc")
            .and_then(Value::as_str)
            .or_else(|| ev.get("starts_at").and_then(Value::as_str))
            .unwrap_or_default();
        let rsvps = ev.get("rsvps").cloned().unwrap_or(Value::Null);
        let attending = as_i64(&rsvps, "attending");
        let waitlisted = as_i64(&rsvps, "waitlisted");
        let paid = ev.get("stripe_payment_link_active").and_then(Value::as_bool).unwrap_or(false);
        // No prev-diff and no notifications for past events (frozen recap).
        db::upsert_event(
            &conn, &meetup_token, weblog_token, starts_at, attending, waitlisted, paid,
            "past", ev, &now,
        )?;
        keep.push(meetup_token);
    }

    {
        let state = app.state::<AppState>();
        let conn = state.db.lock().unwrap();
        db::retain_events(&conn, "past", &keep)?;
        db::set_sync_state(
            &conn, PAST_KEY, Some(&now), ok.rate.remaining, None, false,
            if truncated { Some("truncated") } else { None },
        )?;
    }

    let _ = app.emit("sync:updated", json!({ "at": iso_now(), "kind": "past" }));
    Ok(())
}

/// Fetch performance + awaiting-payment for one event (on demand). Both are
/// chapter-scoped; out-of-scope events degrade to a "not enabled" state.
pub async fn fetch_event_detail(app: &AppHandle, meetup_token: &str) -> AppResult<()> {
    let Some(api) = client(app)? else {
        return Ok(());
    };

    // Locate the event to derive weblog_token + date bounds + content page token.
    let (weblog_token, event_date, paid, content_page_token) = {
        let state = app.state::<AppState>();
        let conn = state.db.lock().unwrap();
        match db::get_event_detail(&conn, meetup_token)? {
            Some(ev) => {
                let wl = ev.get("weblog_token").and_then(Value::as_str).unwrap_or_default().to_string();
                let date = ev
                    .get("starts_at_local_date")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| {
                        ev.get("starts_at_utc")
                            .and_then(Value::as_str)
                            .map(|s| s.chars().take(10).collect())
                    })
                    .unwrap_or_default();
                let paid = ev.get("stripe_payment_link_active").and_then(Value::as_bool).unwrap_or(false);
                let cpt = ev
                    .get("content_page_token")
                    .and_then(Value::as_str)
                    .or_else(|| ev.get("refs").and_then(|r| r.get("content_page_token")).and_then(Value::as_str))
                    .unwrap_or_default()
                    .to_string();
                (wl, date, paid, cpt)
            }
            None => return Ok(()),
        }
    };

    let now = iso_now();

    // Performance (aggregate row) — degrade on scope/group blocks. The traffic
    // window spans the ~6 months up to the event so page views reflect real
    // cumulative traffic, not just the single event-day (fixes >100% conversion).
    if !weblog_token.is_empty() && !event_date.is_empty() {
        let traffic_from = chrono::NaiveDate::parse_from_str(&event_date, "%Y-%m-%d")
            .map(|d| (d - Duration::days(180)).format("%Y-%m-%d").to_string())
            .unwrap_or_else(|_| event_date.clone());
        match api
            .performance_windowed(&weblog_token, &event_date, &event_date, &traffic_from, &event_date)
            .await
        {
            Ok(ok) => {
                let row = ok
                    .data
                    .get("events")
                    .and_then(Value::as_array)
                    .and_then(|a| a.iter().find(|e| e.get("meetup_token").and_then(Value::as_str) == Some(meetup_token)))
                    .cloned();
                let state = app.state::<AppState>();
                let conn = state.db.lock().unwrap();
                db::upsert_performance(&conn, meetup_token, row.as_ref(), false, None, &now)?;
                record_rate_locked(&conn, "performance", &ok.rate);
            }
            Err(e) if e.is_capability_block() => {
                let state = app.state::<AppState>();
                let conn = state.db.lock().unwrap();
                db::upsert_performance(&conn, meetup_token, None, true, Some(&e.to_string()), &now)?;
            }
            Err(AppError::RateLimited(msg)) => apply_backoff(app, "performance", parse_retry_after(&msg)),
            Err(_) => {}
        }
    }

    // Awaiting payment — only meaningful for paid events.
    if paid {
        match api.awaiting_payment(meetup_token).await {
            Ok(ok) => {
                let results = ok.data.get("results").cloned();
                let count = ok.data.get("count").and_then(Value::as_i64).unwrap_or(0);
                let state = app.state::<AppState>();
                let conn = state.db.lock().unwrap();
                db::upsert_awaiting(&conn, meetup_token, count, results.as_ref(), false, &now)?;
                record_rate_locked(&conn, "awaiting_payment", &ok.rate);
            }
            Err(e) if e.is_capability_block() => {
                let state = app.state::<AppState>();
                let conn = state.db.lock().unwrap();
                db::upsert_awaiting(&conn, meetup_token, 0, None, true, &now)?;
            }
            Err(AppError::RateLimited(msg)) => {
                apply_backoff(app, "awaiting_payment", parse_retry_after(&msg))
            }
            Err(_) => {}
        }
    }

    // RSVP total + real door check-in count (best-effort). The check-in count
    // is the true attendance figure; `performance.completed` is not.
    if let Ok(ok) = api.rsvp_summary(meetup_token).await {
        let total = ok.data.get("total_count").and_then(Value::as_i64).unwrap_or(0);
        let groups = ok.data.get("groups").cloned();
        let checked_in = match api.rsvp_checked_in_count(meetup_token).await {
            Ok(c) => c.data.get("total_count").and_then(Value::as_i64),
            Err(_) => None,
        };
        let state = app.state::<AppState>();
        let conn = state.db.lock().unwrap();
        let _ = db::upsert_summary(&conn, meetup_token, total, checked_in, groups.as_ref(), &now);
    }

    // Public content page + email metrics (specs/event-page-view). Only when the
    // event has a content page token; degrade independently on scope/group block.
    if !content_page_token.is_empty() {
        match api.content_page_get(&content_page_token).await {
            Ok(ok) => {
                // The API nests the article under data.content_page — unwrap it so
                // the cached page fields (title, body_text, url, ...) sit at the top
                // level the frontend reads. Same for metrics (data.metrics).
                let page = ok
                    .data
                    .get("content_page")
                    .cloned()
                    .unwrap_or_else(|| ok.data.clone());
                let metrics = match api.content_page_metrics_get(&content_page_token).await {
                    Ok(m) => Some(m.data.get("metrics").cloned().unwrap_or(m.data)),
                    Err(_) => None,
                };
                let state = app.state::<AppState>();
                let conn = state.db.lock().unwrap();
                db::upsert_content_page(
                    &conn, meetup_token, Some(&page), metrics.as_ref(), false, None, &now,
                )?;
                record_rate_locked(&conn, "content_page", &ok.rate);
            }
            Err(e) if e.is_capability_block() => {
                let state = app.state::<AppState>();
                let conn = state.db.lock().unwrap();
                db::upsert_content_page(&conn, meetup_token, None, None, true, Some(&e.to_string()), &now)?;
            }
            Err(AppError::RateLimited(msg)) => apply_backoff(app, "content_page", parse_retry_after(&msg)),
            Err(_) => {}
        }
    }

    let _ = app.emit("detail:updated", json!({ "meetup_token": meetup_token }));
    Ok(())
}

// ── Survey + follow-up sync (specs/survey-followup) ────────────────────────

const SURVEY_KEY: &str = "survey_followup";
/// Cross-meetup report lookback window (default per the OpenAPI spec).
const SURVEY_REPORT_DAYS: u32 = 90;

/// Map an API error to the per-source status enum design D6 defines. Anything
/// that isn't a capability block degrades to `unavailable` rather than failing
/// the whole panel.
fn source_status(e: &AppError) -> &'static str {
    match e {
        AppError::ForbiddenApiGroup(_) => "forbidden_api_group",
        AppError::ForbiddenScope(_) => "forbidden_scope",
        AppError::ForbiddenRole(_) => "forbidden_role",
        _ => "unavailable",
    }
}

/// True when a JSON value carries no meaningful fields (null, `{}`, or `[]`).
fn is_blank(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::Object(m) => m.is_empty(),
        Value::Array(a) => a.is_empty(),
        _ => false,
    }
}

/// Locate this event's row in the cross-meetup `survey_report` rollup by
/// `meetup_token`. Best-effort context only (design D3/task 3.3) — any
/// failure (including rate limit, which is backed off) just means the report
/// context is omitted; the diagnostic remains authoritative.
async fn locate_report_row(api: &ApiClient, app: &AppHandle, weblog_token: &str, meetup_token: &str) -> Option<Value> {
    if weblog_token.is_empty() {
        return None;
    }
    match api.survey_report(weblog_token, SURVEY_REPORT_DAYS).await {
        Ok(ok) => {
            let rows = ok
                .data
                .get("rows")
                .or_else(|| ok.data.get("meetups"))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            rows.into_iter().find(|r| {
                db::pick_str(r, &["meetup_token"]) == Some(meetup_token)
                    || r.get("refs")
                        .and_then(|x| db::pick_str(x, &["meetup_token"]))
                        == Some(meetup_token)
            })
        }
        Err(AppError::RateLimited(msg)) => {
            apply_backoff(app, SURVEY_KEY, parse_retry_after(&msg));
            None
        }
        Err(_) => None,
    }
}

/// Derive the survey summary (response rate guarded against a zero/unknown
/// denominator) from the diagnostic, falling back to the report row for any
/// field the diagnostic omits (design D3/D4, task 3.1/3.3).
fn derive_survey_summary(diag: &Value, report_row: Option<&Value>) -> Value {
    let eligible = db::pick_num(
        diag,
        &["eligible_attendee_count", "attendee_count", "eligible_count", "checked_in_count"],
    )
    .or_else(|| {
        report_row.and_then(|r| {
            db::pick_num(r, &["eligible_attendee_count", "attendee_count", "eligible_count"])
        })
    });
    let responses = db::pick_num(diag, &["response_count", "responses_count", "survey_response_count"])
        .or_else(|| report_row.and_then(|r| db::pick_num(r, &["response_count", "responses_count"])));
    let sent = db::pick_num(diag, &["survey_email_sent_count", "email_sent_count", "sent_count"]);
    let opened = db::pick_num(diag, &["survey_email_opened_count", "email_opened_count", "opened_count"]);
    // Never fabricate sentiment/themes — only surface them if the payload has them (D3).
    let sentiment = diag
        .get("sentiment")
        .cloned()
        .or_else(|| report_row.and_then(|r| r.get("sentiment").cloned()))
        .filter(|v| !is_blank(v));
    let themes = diag
        .get("themes")
        .cloned()
        .or_else(|| report_row.and_then(|r| r.get("themes").cloned()))
        .filter(|v| !is_blank(v));
    // Guard the zero/unknown-denominator case rather than a fake rate (D4).
    let response_rate = match (responses, eligible) {
        (Some(r), Some(e)) if e > 0.0 => Some(r / e),
        _ => None,
    };
    json!({
        "eligible_attendees": eligible,
        "response_count": responses,
        "response_rate": response_rate,
        "survey_email_sent": sent,
        "survey_email_opened": opened,
        "sentiment": sentiment,
        "themes": themes,
        "report_row_found": report_row.is_some(),
    })
}

/// Aggregate meetup-scoped campaign rows into a headline follow-up engagement
/// figure (design D5/risk: don't attribute to a single unidentified campaign).
/// Prefers rows whose label looks like the survey follow-up; falls back to all
/// rows already scoped to this meetup by the API call when none match, so a
/// differently-labeled follow-up campaign still counts.
fn derive_email_summary(campaign_data: &Value) -> Value {
    let campaigns = campaign_data
        .get("campaigns")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let followup: Vec<&Value> = campaigns
        .iter()
        .filter(|c| {
            let label = db::pick_str(c, &["campaign_label"]).unwrap_or("").to_lowercase();
            label.contains("follow") || label.contains("survey")
        })
        .collect();
    let rows: Vec<&Value> = if followup.is_empty() {
        campaigns.iter().collect()
    } else {
        followup
    };

    if rows.is_empty() {
        return json!({
            "sends": Value::Null, "delivered": Value::Null, "opens": Value::Null,
            "clicks": Value::Null, "open_rate": Value::Null, "click_rate": Value::Null,
            "campaign_count": 0,
        });
    }

    let mut sends = 0.0;
    let mut delivered = 0.0;
    let mut opens = 0.0;
    let mut clicks = 0.0;
    for c in &rows {
        sends += db::pick_num(c, &["sends", "sent"]).unwrap_or(0.0);
        delivered += db::pick_num(c, &["delivered"]).unwrap_or(0.0);
        opens += db::pick_num(c, &["opens"]).unwrap_or(0.0);
        clicks += db::pick_num(c, &["clicks"]).unwrap_or(0.0);
    }
    let open_rate = if sends > 0.0 { Some(opens / sends) } else { None };
    let click_rate = if sends > 0.0 { Some(clicks / sends) } else { None };
    json!({
        "sends": sends,
        "delivered": delivered,
        "opens": opens,
        "clicks": clicks,
        "open_rate": open_rate,
        "click_rate": click_rate,
        "campaign_count": rows.len(),
    })
}

/// Fetch survey diagnostic + report (context) + follow-up campaign performance
/// for one PAST event, on detail-open and manual refresh only (spec: never the
/// upcoming poll, never for upcoming events). Each source degrades
/// independently and is cached with its own status (design D6).
pub async fn fetch_survey_followup(app: &AppHandle, meetup_token: &str) -> AppResult<()> {
    let Some(api) = client(app)? else {
        return Ok(());
    };
    if in_backoff(app, SURVEY_KEY) {
        return Ok(());
    }

    let (weblog_token, is_past) = {
        let state = app.state::<AppState>();
        let conn = state.db.lock().unwrap();
        match db::get_event_detail(&conn, meetup_token)? {
            Some(ev) => {
                let kind = ev.get("kind").and_then(Value::as_str).unwrap_or("upcoming").to_string();
                let wl = ev.get("weblog_token").and_then(Value::as_str).unwrap_or_default().to_string();
                (wl, kind == "past")
            }
            None => return Ok(()),
        }
    };
    // Survey/follow-up data is frozen recap data — never fetched for upcoming
    // events (spec: "Survey and follow-up data source and caching").
    if !is_past {
        return Ok(());
    }

    let now = iso_now();

    // Survey diagnostic — primary source for attendee/response counts.
    let survey_update: (Option<Value>, &'static str) = match api.survey_diagnostic(meetup_token).await {
        Ok(ok) if is_blank(&ok.data) => (None, "empty"),
        Ok(ok) => {
            let report_row = locate_report_row(&api, app, &weblog_token, meetup_token).await;
            (Some(derive_survey_summary(&ok.data, report_row.as_ref())), "ok")
        }
        Err(e) if e.is_capability_block() => (None, source_status(&e)),
        Err(AppError::RateLimited(msg)) => {
            apply_backoff(app, SURVEY_KEY, parse_retry_after(&msg));
            (None, "unavailable")
        }
        Err(_) => (None, "unavailable"),
    };

    // Follow-up campaign engagement, meetup-scoped.
    let email_update: (Option<Value>, &'static str) = match api.email_campaign_performance_get(meetup_token).await {
        Ok(ok) => {
            let campaigns_empty = ok
                .data
                .get("campaigns")
                .and_then(Value::as_array)
                .map(|a| a.is_empty())
                .unwrap_or(true);
            if campaigns_empty {
                (None, "empty")
            } else {
                (Some(derive_email_summary(&ok.data)), "ok")
            }
        }
        Err(e) if e.is_capability_block() => (None, source_status(&e)),
        Err(AppError::RateLimited(msg)) => {
            apply_backoff(app, SURVEY_KEY, parse_retry_after(&msg));
            (None, "unavailable")
        }
        Err(_) => (None, "unavailable"),
    };

    {
        let state = app.state::<AppState>();
        let conn = state.db.lock().unwrap();
        db::upsert_survey_followup(
            &conn,
            meetup_token,
            Some((survey_update.0.as_ref(), survey_update.1)),
            Some((email_update.0.as_ref(), email_update.1)),
            &now,
        )?;
    }

    let _ = app.emit("survey_followup:updated", json!({ "meetup_token": meetup_token }));
    Ok(())
}

// ── Email lifecycle sync (specs/email-lifecycle) ───────────────────────────

const EMAIL_CHAPTER_KEY: &str = "email_chapter";
const EMAIL_EVENT_KEY: &str = "email_event";
const EMAIL_THROUGHPUT_KEY: &str = "email_throughput";
/// Throughput bucket size for active-send polling (design D4).
const THROUGHPUT_BUCKET: &str = "minute";

/// A send job is "active" (still moving) when queued/sending/active and not done.
fn job_is_active(job: &Value) -> bool {
    if job.get("done").and_then(Value::as_bool) == Some(true) {
        return false;
    }
    matches!(
        job.get("status").and_then(Value::as_str),
        Some("queued") | Some("sending") | Some("active")
    )
}

/// Chapter deliverability + fatigue tier summary + recent send jobs. Fetched on
/// app launch and manual refresh only — never on the 2-minute loop (task 3.1).
/// Gated by `subscribers_sponsors` + city-owner scope; degrades cleanly.
pub async fn fetch_chapter_email(app: &AppHandle) -> AppResult<()> {
    let Some(api) = client(app)? else {
        return Ok(());
    };
    if in_backoff(app, EMAIL_CHAPTER_KEY) {
        return Ok(());
    }

    let weblog = {
        let state = app.state::<AppState>();
        let conn = state.db.lock().unwrap();
        db::primary_weblog(&conn).unwrap_or(None)
    };
    let now = iso_now();
    let mut blocked: Option<String> = None;
    let mut health: Option<Value> = None;
    let mut fatigue: Option<Value> = None;

    // Deliverability health (sender-domain rows, health score).
    match api
        .email_deliverability_health_get(weblog.as_deref(), None, None)
        .await
    {
        Ok(ok) => {
            health = Some(ok.data);
            let state = app.state::<AppState>();
            let conn = state.db.lock().unwrap();
            record_rate_locked(&conn, EMAIL_CHAPTER_KEY, &ok.rate);
        }
        Err(e) if e.is_capability_block() => blocked = Some(e.code().to_string()),
        Err(AppError::RateLimited(msg)) => {
            apply_backoff(app, EMAIL_CHAPTER_KEY, parse_retry_after(&msg));
            return Err(AppError::RateLimited(msg));
        }
        Err(_) => {}
    }

    // Fatigue-risk — store the aggregate tier `summary` only, never per-subscriber
    // rows or emails (design D5). Skip if the group is already known blocked.
    if blocked.is_none() {
        match api.email_fatigue_risk_get(weblog.as_deref(), 1).await {
            Ok(ok) => {
                let truncated = ok.data.get("truncated").and_then(Value::as_bool).unwrap_or(false);
                // Keep only the tier summary; drop `subscribers[]`.
                fatigue = Some(json!({
                    "summary": ok.data.get("summary").cloned().unwrap_or(Value::Null),
                    "truncated": truncated,
                }));
                let state = app.state::<AppState>();
                let conn = state.db.lock().unwrap();
                record_rate_locked(&conn, EMAIL_CHAPTER_KEY, &ok.rate);
            }
            Err(e) if e.is_capability_block() => blocked = Some(e.code().to_string()),
            Err(AppError::RateLimited(msg)) => {
                apply_backoff(app, EMAIL_CHAPTER_KEY, parse_retry_after(&msg))
            }
            Err(_) => {}
        }
    }

    // Recent send jobs across the chapter (partition 'chapter').
    let mut list_truncated = false;
    if blocked.is_none() {
        match api.email_send_jobs_list(Some("all"), None, 25, None, None).await {
            Ok(ok) => {
                list_truncated = ok.data.get("truncated").and_then(Value::as_bool).unwrap_or(false);
                let jobs = ok
                    .data
                    .get("send_jobs")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let state = app.state::<AppState>();
                let conn = state.db.lock().unwrap();
                let mut keep = Vec::new();
                for job in &jobs {
                    db::upsert_send_job(&conn, job, None, "chapter", &now)?;
                    if let Some(t) = job
                        .get("token")
                        .or_else(|| job.get("send_job_token"))
                        .and_then(Value::as_str)
                    {
                        keep.push(t.to_string());
                    }
                }
                db::retain_send_jobs(&conn, "chapter", &keep)?;
                record_rate_locked(&conn, EMAIL_CHAPTER_KEY, &ok.rate);
            }
            Err(e) if e.is_capability_block() => blocked = Some(e.code().to_string()),
            Err(AppError::RateLimited(msg)) => {
                apply_backoff(app, EMAIL_CHAPTER_KEY, parse_retry_after(&msg))
            }
            Err(_) => {}
        }
    }

    {
        let state = app.state::<AppState>();
        let conn = state.db.lock().unwrap();
        db::upsert_deliverability(
            &conn,
            health.as_ref(),
            fatigue.as_ref(),
            list_truncated,
            blocked.is_some(),
            blocked.as_deref(),
            &now,
        )?;
        // Stop re-polling a blocked surface (task 3.4).
        db::set_sync_state(
            &conn,
            EMAIL_CHAPTER_KEY,
            Some(&now),
            None,
            None,
            blocked.is_some(),
            blocked.as_deref(),
        )?;
    }

    let _ = app.emit("email:chapter", json!({ "at": iso_now() }));
    Ok(())
}

/// Per-event email surface: send-job summary + campaign performance, plus a
/// throughput poll for any active job. Called when the Email panel opens and on
/// the gentle active-send cadence (task 3.2/3.3). Campaign rates are fetched
/// once (slow-moving) and skipped while only throughput is being polled.
pub async fn fetch_event_email(app: &AppHandle, meetup_token: &str) -> AppResult<()> {
    let Some(api) = client(app)? else {
        return Ok(());
    };
    if in_backoff(app, EMAIL_EVENT_KEY) {
        return Ok(());
    }
    let now = iso_now();
    let mut blocked: Option<String> = None;

    // Aggregate send-job summary + its recent send jobs (partition 'event').
    let mut active_tokens: Vec<String> = Vec::new();
    match api.email_send_jobs_summary(meetup_token, 25, None, None).await {
        Ok(ok) => {
            let summary = ok.data.get("summary").cloned();
            let jobs = ok
                .data
                .get("send_jobs")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let state = app.state::<AppState>();
            let conn = state.db.lock().unwrap();
            let mut keep = Vec::new();
            for job in &jobs {
                db::upsert_send_job(&conn, job, Some(meetup_token), "event", &now)?;
                if let Some(t) = job
                    .get("token")
                    .or_else(|| job.get("send_job_token"))
                    .and_then(Value::as_str)
                {
                    keep.push(t.to_string());
                    if job_is_active(job) {
                        active_tokens.push(t.to_string());
                    }
                }
            }
            db::retain_send_jobs(&conn, "event", &keep)?;
            db::upsert_event_summary(&conn, meetup_token, summary.as_ref(), None, false, None, &now)?;
            record_rate_locked(&conn, EMAIL_EVENT_KEY, &ok.rate);
        }
        Err(e) if e.is_capability_block() => blocked = Some(e.code().to_string()),
        Err(AppError::RateLimited(msg)) => {
            apply_backoff(app, EMAIL_EVENT_KEY, parse_retry_after(&msg));
            return Err(AppError::RateLimited(msg));
        }
        Err(_) => {}
    }

    if let Some(reason) = &blocked {
        let state = app.state::<AppState>();
        let conn = state.db.lock().unwrap();
        db::upsert_event_summary(&conn, meetup_token, None, None, true, Some(reason), &now)?;
        db::set_sync_state(&conn, EMAIL_EVENT_KEY, Some(&now), None, None, true, Some(reason))?;
        let _ = app.emit("email:event", json!({ "meetup_token": meetup_token }));
        return Ok(());
    }

    // Campaign open/click performance — fetch once (slow-moving). Skip if cached
    // so gentle throughput polling doesn't burn the rate budget re-fetching it.
    let need_campaign = {
        let state = app.state::<AppState>();
        let conn = state.db.lock().unwrap();
        !db::has_campaign(&conn, meetup_token).unwrap_or(false)
    };
    if need_campaign {
        match api.email_campaign_performance_get(meetup_token).await {
            Ok(ok) => {
                let state = app.state::<AppState>();
                let conn = state.db.lock().unwrap();
                db::upsert_event_summary(&conn, meetup_token, None, Some(&ok.data), false, None, &now)?;
                record_rate_locked(&conn, EMAIL_EVENT_KEY, &ok.rate);
            }
            // Campaign perf is optional enrichment; a block here must not blank
            // the send accounting (spec: omit open/click, don't error).
            Err(AppError::RateLimited(msg)) => {
                apply_backoff(app, EMAIL_EVENT_KEY, parse_retry_after(&msg))
            }
            Err(_) => {}
        }
    }

    // Poll throughput for each active job (design D4). Completed jobs are frozen
    // and skipped so we never re-poll a finished send.
    for token in &active_tokens {
        let already_done = {
            let state = app.state::<AppState>();
            let conn = state.db.lock().unwrap();
            db::send_job_done(&conn, token).unwrap_or(false)
        };
        if already_done {
            continue;
        }
        poll_send_job(app, token).await;
    }

    let _ = app.emit("email:event", json!({ "meetup_token": meetup_token }));
    Ok(())
}

/// Fetch one send job's progress + throughput series and cache them. Freezes the
/// snapshot once the job is done so it is no longer polled (spec, design D4).
async fn poll_send_job(app: &AppHandle, token: &str) {
    let Some(api) = (match client(app) {
        Ok(c) => c,
        Err(_) => return,
    }) else {
        return;
    };
    if in_backoff(app, EMAIL_THROUGHPUT_KEY) {
        return;
    }
    let now = iso_now();

    // Job detail carries send_progress (observed rate, predicted finish) + done.
    let mut progress: Option<Value> = None;
    let mut done = false;
    match api.email_send_job_get(token).await {
        Ok(ok) => {
            let job = ok.data.get("send_job").cloned().unwrap_or(ok.data.clone());
            done = job.get("done").and_then(Value::as_bool).unwrap_or(false)
                || matches!(
                    job.get("status").and_then(Value::as_str),
                    Some("completed") | Some("failed") | Some("cancelled")
                );
            progress = job.get("send_progress").cloned().or(Some(job));
            let state = app.state::<AppState>();
            let conn = state.db.lock().unwrap();
            record_rate_locked(&conn, EMAIL_THROUGHPUT_KEY, &ok.rate);
        }
        Err(AppError::RateLimited(msg)) => {
            apply_backoff(app, EMAIL_THROUGHPUT_KEY, parse_retry_after(&msg));
            return;
        }
        Err(_) => {}
    }

    match api.email_send_job_throughput_get(token, THROUGHPUT_BUCKET).await {
        Ok(ok) => {
            let throughput = ok.data.get("throughput").cloned();
            let peak = ok.data.get("peak_rate_per_minute").and_then(Value::as_f64);
            let avg = ok.data.get("average_rate_per_minute").and_then(Value::as_f64);
            let total = ok.data.get("total_sent").and_then(Value::as_f64);
            let state = app.state::<AppState>();
            let conn = state.db.lock().unwrap();
            db::upsert_throughput(
                &conn, token, throughput.as_ref(), progress.as_ref(), peak, avg, total, done, &now,
            )
            .ok();
            record_rate_locked(&conn, EMAIL_THROUGHPUT_KEY, &ok.rate);
        }
        Err(AppError::RateLimited(msg)) => {
            apply_backoff(app, EMAIL_THROUGHPUT_KEY, parse_retry_after(&msg))
        }
        Err(_) => {
            // Persist progress/done even if the throughput series is unavailable.
            let state = app.state::<AppState>();
            let conn = state.db.lock().unwrap();
            db::upsert_throughput(&conn, token, None, progress.as_ref(), None, None, None, done, &now)
                .ok();
        }
    }
}

fn record_rate(app: &AppHandle, key: &str, rate: &RateInfo) {
    let state = app.state::<AppState>();
    let conn = state.db.lock().unwrap();
    record_rate_locked(&conn, key, rate);
}

fn record_rate_locked(conn: &rusqlite::Connection, key: &str, rate: &RateInfo) {
    let _ = db::set_sync_state(conn, key, Some(&iso_now()), rate.remaining, None, false, None);
}

/// Compute the soonest upcoming event and update the tray title + emit for popover.
pub fn update_tray(app: &AppHandle) {
    let next = next_event_json(app);
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let title = match &next {
            Some(ev) => {
                let att = ev.get("attending").and_then(Value::as_i64).unwrap_or(0);
                let days = ev.get("days").and_then(Value::as_i64).unwrap_or(0);
                format!("{att} · {days}d")
            }
            None => "—".to_string(),
        };
        let _ = tray.set_title(Some(title));
    }
    let _ = app.emit("popover:data", next.unwrap_or(Value::Null));
}

// ── Promotion tools (specs/promotion-tools) ────────────────────────────────
// Generation calls are explicit user-initiated jobs, never on the poll loop
// (design D1). Each kickoff runs on its own background task and reports
// progress via `promotion:job` events, mirroring `sync:updated`.

/// Freshness window for the cheap `logo_search` GET (not a billed generation).
const LOGO_FRESHNESS_SECS: i64 = 600;

fn hash_params(v: &Value) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    v.to_string().hash(&mut h);
    format!("{:x}", h.finish())
}

fn new_job_id() -> String {
    format!("{:x}{:x}", rand::random::<u64>(), rand::random::<u32>())
}

fn emit_promotion_job(
    app: &AppHandle,
    job_id: &str,
    meetup_token: &str,
    kind: &str,
    platform: &str,
    status: &str,
    error_code: Option<&str>,
) {
    let _ = app.emit(
        "promotion:job",
        json!({
            "job_id": job_id,
            "meetup_token": meetup_token,
            "kind": kind,
            "platform": platform,
            "status": status,
            "error_code": error_code,
        }),
    );
}

/// Kick off (or, per design D7, return the id of an already in-flight) job for
/// one promotion action. Returns immediately — the request itself runs on a
/// spawned background task so the UI never blocks on the ~25s generation call.
pub fn promotion_generate(
    app: &AppHandle,
    kind: String,
    meetup_token: String,
    platform: String,
    params: Value,
) -> AppResult<String> {
    let state = app.state::<AppState>();
    let now = iso_now();

    if let Some(existing) = {
        let conn = state.db.lock().unwrap();
        db::find_active_promotion_job(&conn, &meetup_token, &kind, &platform)?
    } {
        return Ok(existing);
    }

    let id = new_job_id();
    {
        let conn = state.db.lock().unwrap();
        db::create_promotion_job(
            &conn,
            &id,
            &meetup_token,
            &kind,
            &platform,
            &hash_params(&params),
            &now,
        )?;
    }
    emit_promotion_job(app, &id, &meetup_token, &kind, &platform, "pending", None);

    let app2 = app.clone();
    let (id2, meetup2, kind2, platform2) =
        (id.clone(), meetup_token.clone(), kind.clone(), platform.clone());
    let handle = tauri::async_runtime::spawn(async move {
        run_promotion_job(app2, id2, meetup2, kind2, platform2, params).await;
    });
    state.promo_jobs.lock().unwrap().insert(id.clone(), handle);
    Ok(id)
}

/// Abort an in-flight request (if still running) and drop the action back to
/// its last cached draft by deleting the job row entirely (design D5).
pub fn promotion_cancel(app: &AppHandle, job_id: &str) -> AppResult<()> {
    let state = app.state::<AppState>();
    if let Some(handle) = state.promo_jobs.lock().unwrap().remove(job_id) {
        handle.abort();
    }
    let job = {
        let conn = state.db.lock().unwrap();
        db::get_promotion_job(&conn, job_id)?
    };
    let Some(job) = job else { return Ok(()) };
    let meetup_token = job.get("meetup_token").and_then(Value::as_str).unwrap_or_default().to_string();
    let kind = job.get("kind").and_then(Value::as_str).unwrap_or_default().to_string();
    let platform = job.get("platform").and_then(Value::as_str).unwrap_or_default().to_string();
    {
        let conn = state.db.lock().unwrap();
        db::delete_promotion_job(&conn, job_id)?;
    }
    emit_promotion_job(app, job_id, &meetup_token, &kind, &platform, "cancelled", None);
    Ok(())
}

/// Run one generation request to completion (or timeout/error) on a spawned
/// task, upsert the resulting draft on success, and emit progress events.
async fn run_promotion_job(
    app: AppHandle,
    id: String,
    meetup_token: String,
    kind: String,
    platform: String,
    params: Value,
) {
    {
        let state = app.state::<AppState>();
        let conn = state.db.lock().unwrap();
        let _ = db::set_promotion_job_status(&conn, &id, "running", None);
    }
    emit_promotion_job(&app, &id, &meetup_token, &kind, &platform, "running", None);

    let api = match client(&app) {
        Ok(Some(api)) => api,
        _ => {
            finish_promotion_job(&app, &id, &meetup_token, &kind, &platform, "error", Some("no_key"));
            app.state::<AppState>().promo_jobs.lock().unwrap().remove(&id);
            return;
        }
    };

    let backoff_key = format!("promo_{kind}");
    let result: AppResult<crate::api::ApiOk> = match kind.as_str() {
        "social_post" => {
            let source_type = params.get("source_type").and_then(Value::as_str).unwrap_or("meetup");
            let source_ref = params.get("source_ref").and_then(Value::as_str).unwrap_or(&meetup_token);
            let goal = params.get("goal").and_then(Value::as_str).unwrap_or("promote");
            let tone = params.get("tone").and_then(Value::as_str);
            let city = params.get("city").and_then(Value::as_str);
            api.social_post_generate(source_type, source_ref, &platform, goal, tone, city).await
        }
        "event_promo" => {
            let package_type = params.get("package_type").and_then(Value::as_str).unwrap_or("full_campaign");
            let audience = params.get("audience").and_then(Value::as_str).unwrap_or("general");
            api.event_promo_generate(&meetup_token, package_type, audience).await
        }
        "discussion_topics" => api.discussion_topics_generate(&meetup_token).await,
        other => Err(AppError::Other(format!("unknown promotion kind: {other}"))),
    };

    match result {
        Ok(ok) => {
            let now = iso_now();
            {
                let state = app.state::<AppState>();
                let conn = state.db.lock().unwrap();
                let _ = db::upsert_promotion_draft(&conn, &meetup_token, &kind, &platform, &params, &ok.data, &now);
                let _ = db::set_promotion_job_status(&conn, &id, "ready", None);
                record_rate_locked(&conn, &backoff_key, &ok.rate);
            }
            emit_promotion_job(&app, &id, &meetup_token, &kind, &platform, "ready", None);
        }
        Err(AppError::Timeout(_)) => {
            finish_promotion_job(&app, &id, &meetup_token, &kind, &platform, "timeout", Some("timeout"));
        }
        Err(AppError::RateLimited(msg)) => {
            apply_backoff(&app, &backoff_key, parse_retry_after(&msg));
            finish_promotion_job(&app, &id, &meetup_token, &kind, &platform, "error", Some("rate_limited"));
        }
        Err(e) => {
            let code = e.code().to_string();
            finish_promotion_job(&app, &id, &meetup_token, &kind, &platform, "error", Some(&code));
        }
    }
    app.state::<AppState>().promo_jobs.lock().unwrap().remove(&id);
}

fn finish_promotion_job(
    app: &AppHandle,
    id: &str,
    meetup_token: &str,
    kind: &str,
    platform: &str,
    status: &str,
    error_code: Option<&str>,
) {
    {
        let state = app.state::<AppState>();
        let conn = state.db.lock().unwrap();
        let _ = db::set_promotion_job_status(&conn, id, status, error_code);
    }
    emit_promotion_job(app, id, meetup_token, kind, platform, status, error_code);
}

/// Logo/brand asset search. A cheap GET, not a billed generation (design D3),
/// so it's a plain cached command rather than a tracked job: read the cache
/// within the freshness window, else fetch and upsert.
pub async fn logo_search(
    app: &AppHandle,
    query: String,
    scope: String,
    include_co_branded: bool,
    limit: u32,
) -> AppResult<Value> {
    let cached = {
        let state = app.state::<AppState>();
        let conn = state.db.lock().unwrap();
        db::get_logo_cache(&conn, &query, &scope, include_co_branded)?
    };
    if let Some(row) = &cached {
        let fresh = row
            .get("fetched_at")
            .and_then(Value::as_str)
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|t| Utc::now().signed_duration_since(t) < Duration::seconds(LOGO_FRESHNESS_SECS))
            .unwrap_or(false);
        if fresh {
            return Ok(row.clone());
        }
    }

    let Some(api) = client(app)? else { return Err(AppError::NoKey) };
    let ok = api.logo_search(&query, &scope, include_co_branded, limit).await?;
    let now = iso_now();
    let state = app.state::<AppState>();
    let conn = state.db.lock().unwrap();
    db::upsert_logo_cache(&conn, &query, &scope, include_co_branded, &ok.data, &now)?;
    record_rate_locked(&conn, "logo_search", &ok.rate);
    Ok(json!({ "result": ok.data, "fetched_at": now }))
}

// ── Sponsor tools (specs/sponsor-tools) ────────────────────────────────────
// Search + contacts are explicit-action cached reads (never the poll loop);
// research/pitch are generation kickoffs mirroring promotion tools (design D2)
// but tracked in their own job/draft tables (state.rs sponsor_jobs).

fn sponsor_error_code(e: &AppError) -> &'static str {
    match e {
        AppError::ForbiddenApiGroup(_) => "forbidden_api_group",
        AppError::ForbiddenScope(_) => "forbidden_scope",
        AppError::ForbiddenRole(_) => "forbidden_role",
        AppError::RateLimited(_) => "rate_limited",
        _ => "unavailable",
    }
}

/// Search sponsors, cache matches + this query's result list, and return the
/// cached view. Degrade states (`forbidden_api_group`/`forbidden_scope`/rate
/// limit) are stored on the search-cache row rather than bubbled as an error,
/// so the Sponsors screen can render an informative disabled state (task 3.6).
pub async fn sponsor_search(
    app: &AppHandle,
    query: String,
    city: Option<String>,
    industry: Option<String>,
    active_only: bool,
) -> AppResult<Value> {
    let city = city.unwrap_or_default();
    let industry = industry.unwrap_or_default();
    let now = iso_now();

    let Some(api) = client(app)? else {
        let state = app.state::<AppState>();
        let conn = state.db.lock().unwrap();
        return db::get_sponsor_search(&conn, &query, &city, &industry, active_only);
    };

    match api
        .sponsor_search(&query, Some(&city).filter(|s| !s.is_empty()).map(String::as_str), Some(&industry).filter(|s| !s.is_empty()).map(String::as_str), active_only, 25)
        .await
    {
        Ok(ok) => {
            let matches = ok.data.get("matches").and_then(Value::as_array).cloned().unwrap_or_default();
            let truncated = ok.data.get("truncated").and_then(Value::as_bool).unwrap_or(matches.len() >= 25);
            let state = app.state::<AppState>();
            let conn = state.db.lock().unwrap();
            db::upsert_sponsor_search(&conn, &query, &city, &industry, active_only, &matches, truncated, false, None, &now)?;
            record_rate_locked(&conn, "sponsor_search", &ok.rate);
            db::get_sponsor_search(&conn, &query, &city, &industry, active_only)
        }
        Err(e) if e.is_capability_block() => {
            let code = sponsor_error_code(&e);
            let state = app.state::<AppState>();
            let conn = state.db.lock().unwrap();
            db::upsert_sponsor_search(&conn, &query, &city, &industry, active_only, &[], false, true, Some(code), &now)?;
            db::get_sponsor_search(&conn, &query, &city, &industry, active_only)
        }
        Err(AppError::RateLimited(msg)) => {
            apply_backoff(app, "sponsor_search", parse_retry_after(&msg));
            let state = app.state::<AppState>();
            let conn = state.db.lock().unwrap();
            db::upsert_sponsor_search(&conn, &query, &city, &industry, active_only, &[], false, true, Some("rate_limited"), &now)?;
            db::get_sponsor_search(&conn, &query, &city, &industry, active_only)
        }
        Err(_) => {
            let state = app.state::<AppState>();
            let conn = state.db.lock().unwrap();
            db::get_sponsor_search(&conn, &query, &city, &industry, active_only)
        }
    }
}

/// Fetch + cache contacts for one sponsor, replacing the whole set, and
/// return the cached view. Same degrade-on-row philosophy as `sponsor_search`.
pub async fn sponsor_contacts_get(app: &AppHandle, sponsor_ref: String) -> AppResult<Value> {
    let now = iso_now();
    let Some(api) = client(app)? else {
        let state = app.state::<AppState>();
        let conn = state.db.lock().unwrap();
        return db::get_sponsor_contacts(&conn, &sponsor_ref);
    };

    match api.sponsor_contact_list(&sponsor_ref).await {
        Ok(ok) => {
            let contacts = ok.data.get("contacts").and_then(Value::as_array).cloned().unwrap_or_default();
            let truncated = ok.data.get("truncated").and_then(Value::as_bool).unwrap_or(contacts.len() >= 25);
            let state = app.state::<AppState>();
            let conn = state.db.lock().unwrap();
            db::upsert_sponsor_contacts(&conn, &sponsor_ref, &contacts, truncated, false, None, &now)?;
            record_rate_locked(&conn, "sponsor_contacts", &ok.rate);
            db::get_sponsor_contacts(&conn, &sponsor_ref)
        }
        Err(e) if e.is_capability_block() => {
            let code = sponsor_error_code(&e);
            let state = app.state::<AppState>();
            let conn = state.db.lock().unwrap();
            db::upsert_sponsor_contacts(&conn, &sponsor_ref, &[], false, true, Some(code), &now)?;
            db::get_sponsor_contacts(&conn, &sponsor_ref)
        }
        Err(AppError::RateLimited(msg)) => {
            apply_backoff(app, "sponsor_contacts", parse_retry_after(&msg));
            let state = app.state::<AppState>();
            let conn = state.db.lock().unwrap();
            db::upsert_sponsor_contacts(&conn, &sponsor_ref, &[], false, true, Some("rate_limited"), &now)?;
            db::get_sponsor_contacts(&conn, &sponsor_ref)
        }
        Err(_) => {
            let state = app.state::<AppState>();
            let conn = state.db.lock().unwrap();
            db::get_sponsor_contacts(&conn, &sponsor_ref)
        }
    }
}

/// Assemble a small, capped event-context object for a sponsor pitch from the
/// already-cached events data (design D2/task 3.5) — only summary fields, so
/// the request body comfortably stays under the 64 KB cap even before the
/// last-resort trim in `api::sponsor_pitch_generate`.
fn build_gen_context(app: &AppHandle, meetup_token: Option<&str>, notes: Option<&str>) -> Option<Value> {
    let mut ctx = serde_json::Map::new();
    if let Some(mt) = meetup_token {
        let state = app.state::<AppState>();
        let conn = state.db.lock().unwrap();
        if let Ok(Some(ev)) = db::get_event_detail(&conn, mt) {
            let rsvps = ev.get("rsvps").cloned().unwrap_or(Value::Null);
            ctx.insert(
                "event".into(),
                json!({
                    "meetup_token": mt,
                    "name": ev.get("event_name"),
                    "city": ev.get("city"),
                    "starts_at_local": ev.get("starts_at_local"),
                    "event_url": ev.get("event_url"),
                    "attending": rsvps.get("attending"),
                    "capacity": rsvps.get("capacity"),
                }),
            );
        }
    }
    if let Some(n) = notes {
        let trimmed: String = n.chars().take(4000).collect();
        if !trimmed.is_empty() {
            ctx.insert("notes".into(), json!(trimmed));
        }
    }
    if ctx.is_empty() {
        None
    } else {
        Some(Value::Object(ctx))
    }
}

fn emit_sponsor_job(
    app: &AppHandle,
    job_id: &str,
    subject: &str,
    kind: &str,
    status: &str,
    error_code: Option<&str>,
    draft_id: Option<&str>,
) {
    let _ = app.emit(
        "sponsor_draft_progress",
        json!({
            "job_id": job_id,
            "subject": subject,
            "kind": kind,
            "status": status,
            "error_code": error_code,
            "draft_id": draft_id,
        }),
    );
}

/// Parameters for one research/pitch generation kickoff.
#[derive(Clone)]
pub struct SponsorGenParams {
    pub sponsor_ref: Option<String>,
    pub name: Option<String>,
    pub domain: Option<String>,
    pub city: Option<String>,
    pub channel: Option<String>,
    pub target_audience: Option<String>,
    pub meetup_token: Option<String>,
    pub notes: Option<String>,
}

/// Kick off (or, if one is already in-flight for this subject+kind, return the
/// id of) a research or pitch generation job. Returns immediately — the
/// request itself runs on a spawned background task so the UI never blocks on
/// the ~20s generation call (spec: "Asynchronous generation with progress and
/// cancel").
pub fn sponsor_generate(app: &AppHandle, kind: String, params: SponsorGenParams) -> AppResult<String> {
    let subject = db::sponsor_subject_key(params.sponsor_ref.as_deref(), params.name.as_deref());
    let state = app.state::<AppState>();
    let now = iso_now();

    if let Some(existing) = {
        let conn = state.db.lock().unwrap();
        db::find_active_sponsor_job(&conn, &subject, &kind)?
    } {
        return Ok(existing);
    }

    let id = new_job_id();
    let hash = hash_params(&json!({
        "sponsor_ref": params.sponsor_ref, "name": params.name, "domain": params.domain,
        "city": params.city, "channel": params.channel, "target_audience": params.target_audience,
        "meetup_token": params.meetup_token, "notes": params.notes,
    }));
    {
        let conn = state.db.lock().unwrap();
        db::create_sponsor_job(&conn, &id, &subject, params.sponsor_ref.as_deref(), params.name.as_deref(), &kind, &hash, &now)?;
    }
    emit_sponsor_job(app, &id, &subject, &kind, "pending", None, None);

    let app2 = app.clone();
    let (id2, subject2, kind2) = (id.clone(), subject.clone(), kind.clone());
    let handle = tauri::async_runtime::spawn(async move {
        run_sponsor_job(app2, id2, subject2, kind2, params).await;
    });
    state.sponsor_jobs.lock().unwrap().insert(id.clone(), handle);
    Ok(id)
}

/// Abort an in-flight request (if still running) and drop the job row — the
/// action falls back to its cached drafts, and no partial draft is written.
pub fn sponsor_generation_cancel(app: &AppHandle, job_id: &str) -> AppResult<()> {
    let state = app.state::<AppState>();
    if let Some(handle) = state.sponsor_jobs.lock().unwrap().remove(job_id) {
        handle.abort();
    }
    let job = {
        let conn = state.db.lock().unwrap();
        db::get_sponsor_job(&conn, job_id)?
    };
    let Some(job) = job else { return Ok(()) };
    let subject = job.get("subject").and_then(Value::as_str).unwrap_or_default().to_string();
    let kind = job.get("kind").and_then(Value::as_str).unwrap_or_default().to_string();
    {
        let conn = state.db.lock().unwrap();
        db::delete_sponsor_job(&conn, job_id)?;
    }
    emit_sponsor_job(app, job_id, &subject, &kind, "cancelled", None, None);
    Ok(())
}

async fn run_sponsor_job(app: AppHandle, id: String, subject: String, kind: String, params: SponsorGenParams) {
    {
        let state = app.state::<AppState>();
        let conn = state.db.lock().unwrap();
        let _ = db::set_sponsor_job_status(&conn, &id, "running", None, None);
    }
    emit_sponsor_job(&app, &id, &subject, &kind, "running", None, None);

    let api = match client(&app) {
        Ok(Some(api)) => api,
        _ => {
            finish_sponsor_job(&app, &id, &subject, &kind, "error", Some("no_key"));
            app.state::<AppState>().sponsor_jobs.lock().unwrap().remove(&id);
            return;
        }
    };

    let backoff_key = format!("sponsor_{kind}");
    let result: AppResult<crate::api::ApiOk> = match kind.as_str() {
        "research" => {
            api.sponsor_research_generate(
                params.sponsor_ref.as_deref(),
                params.name.as_deref(),
                params.domain.as_deref(),
                params.city.as_deref(),
                params.target_audience.as_deref(),
                build_gen_context(&app, None, params.notes.as_deref()).as_ref(),
            )
            .await
        }
        "pitch" => {
            let context = build_gen_context(&app, params.meetup_token.as_deref(), params.notes.as_deref());
            api.sponsor_pitch_generate(
                params.sponsor_ref.as_deref(),
                params.name.as_deref(),
                params.city.as_deref(),
                params.channel.as_deref(),
                params.target_audience.as_deref(),
                context.as_ref(),
            )
            .await
        }
        other => Err(AppError::Other(format!("unknown sponsor generation kind: {other}"))),
    };

    match result {
        Ok(ok) => {
            let now = iso_now();
            let draft_id = new_job_id();
            let params_json = json!({
                "sponsor_ref": params.sponsor_ref, "name": params.name, "domain": params.domain,
                "city": params.city, "channel": params.channel, "target_audience": params.target_audience,
                "meetup_token": params.meetup_token,
            });
            {
                let state = app.state::<AppState>();
                let conn = state.db.lock().unwrap();
                let _ = db::insert_sponsor_draft(
                    &conn, &draft_id, &subject, params.sponsor_ref.as_deref(), params.name.as_deref(),
                    &kind, &params_json, &ok.data, &now,
                );
                let _ = db::set_sponsor_job_status(&conn, &id, "ready", None, Some(&draft_id));
                record_rate_locked(&conn, &backoff_key, &ok.rate);
            }
            emit_sponsor_job(&app, &id, &subject, &kind, "ready", None, Some(&draft_id));
        }
        Err(AppError::Timeout(_)) => {
            finish_sponsor_job(&app, &id, &subject, &kind, "timeout", Some("timeout"));
        }
        Err(AppError::RateLimited(msg)) => {
            apply_backoff(&app, &backoff_key, parse_retry_after(&msg));
            finish_sponsor_job(&app, &id, &subject, &kind, "error", Some("rate_limited"));
        }
        Err(e) => {
            let code = e.code().to_string();
            finish_sponsor_job(&app, &id, &subject, &kind, "error", Some(&code));
        }
    }
    app.state::<AppState>().sponsor_jobs.lock().unwrap().remove(&id);
}

fn finish_sponsor_job(app: &AppHandle, id: &str, subject: &str, kind: &str, status: &str, error_code: Option<&str>) {
    {
        let state = app.state::<AppState>();
        let conn = state.db.lock().unwrap();
        let _ = db::set_sponsor_job_status(&conn, id, status, error_code, None);
    }
    emit_sponsor_job(app, id, subject, kind, status, error_code, None);
}

/// The soonest future event as a compact JSON payload for tray + popover.
pub fn next_event_json(app: &AppHandle) -> Option<Value> {
    let state = app.state::<AppState>();
    let conn = state.db.lock().unwrap();
    let events = db::get_events(&conn).ok()?;
    let mut candidates: Vec<Value> = events
        .into_iter()
        .filter(|ev| {
            // Past events are never eligible for the tray "next event".
            if ev.get("kind").and_then(Value::as_str) == Some("past") {
                return false;
            }
            let rel = ev.get("relative_day_in_event_timezone").and_then(Value::as_str);
            let days = ev
                .get("days_until_event_in_event_timezone")
                .and_then(Value::as_i64)
                .unwrap_or(-9999);
            days >= 0 || matches!(rel, Some("future") | Some("today"))
        })
        .collect();
    candidates.sort_by_key(|ev| {
        ev.get("starts_at_utc")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    });
    let ev = candidates.into_iter().next()?;
    let rsvps = ev.get("rsvps").cloned().unwrap_or(Value::Null);
    Some(json!({
        "meetup_token": ev.get("meetup_token"),
        "name": ev.get("event_name"),
        "city": ev.get("city"),
        "when": ev.get("starts_at_local"),
        "days": ev.get("days_until_event_in_event_timezone"),
        "attending": rsvps.get("attending"),
        "capacity": rsvps.get("capacity"),
        "registered": rsvps.get("registered"),
        "waitlisted": rsvps.get("waitlisted"),
        "cancelled": rsvps.get("cancelled"),
    }))
}
