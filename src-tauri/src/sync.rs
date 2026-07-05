use std::sync::atomic::Ordering;

use chrono::{Duration, Utc};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

use crate::api::{ApiClient, RateInfo};
use crate::error::{AppError, AppResult};
use crate::state::{AppState, TRAY_ID};
use crate::{db, keychain};

const UPCOMING_KEY: &str = "upcoming";
const PAST_KEY: &str = "past";
const PAST_LIMIT: u32 = 50;
const MAX_BACKOFF_SECS: i64 = 60;

fn iso_now() -> String {
    Utc::now().to_rfc3339()
}

/// Build a client from the stored key, or `None` if onboarding isn't done.
fn client() -> AppResult<Option<ApiClient>> {
    Ok(keychain::get_key()?.map(ApiClient::new))
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
        Some("backing off after rate limit"),
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
    let Some(api) = client()? else {
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
    let Some(api) = client()? else {
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
    let Some(api) = client()? else {
        return Ok(());
    };

    // Locate the event to derive weblog_token + date bounds.
    let (weblog_token, event_date, paid) = {
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
                (wl, date, paid)
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

    let _ = app.emit("detail:updated", json!({ "meetup_token": meetup_token }));
    Ok(())
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
