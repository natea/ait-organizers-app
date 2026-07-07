use std::sync::atomic::Ordering;

use chrono::Utc;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager, State};

use crate::api::ApiClient;
use crate::error::{AppError, AppResult};
use crate::state::{AppState, MAIN_LABEL, POPOVER_LABEL};
use crate::write_guard::BULK_CEILING;
use crate::{db, keychain, sync};

fn iso_now() -> String {
    Utc::now().to_rfc3339()
}

fn new_id() -> String {
    format!("{:x}{:x}", rand::random::<u64>(), rand::random::<u32>())
}

/// Validate a pasted key against auth/validate, and persist it only on success
/// (specs/api-auth). The key is written straight to the keychain and never
/// returned to or retained by the frontend.
#[tauri::command]
pub async fn validate_and_store(app: AppHandle, key: String) -> AppResult<Value> {
    let key = key.trim().to_string();
    if key.is_empty() {
        return Err(AppError::Unauthorized("Empty key".into()));
    }
    let api = ApiClient::new(key.clone());
    // Unwrapped data shape: { valid, api_key, owner, authorization }.
    let identity = api.validate().await?;
    if identity.get("valid").and_then(Value::as_bool) != Some(true) {
        return Err(AppError::Unauthorized("Key did not validate".into()));
    }
    keychain::store_key(&key)?;
    // Cache in memory so background sync doesn't re-read the keychain per call.
    app.state::<AppState>().set_api_key(Some(key.clone()));
    // Kick off an initial sync (upcoming + past) in the background.
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        let _ = sync::run_cycle(app2.clone(), true).await;
        let _ = sync::run_past(app2).await;
    });
    Ok(identity)
}

/// Whether onboarding has completed (a key is stored). Reads the cached key
/// (keychain at most once per launch).
#[tauri::command]
pub fn has_key(state: State<'_, AppState>) -> AppResult<bool> {
    Ok(state.api_key_cached()?.is_some())
}

/// Re-validate the stored key to render identity on launch.
#[tauri::command]
pub async fn get_identity(state: State<'_, AppState>) -> AppResult<Value> {
    // Extract the owned key before any await (don't hold State across .await).
    let Some(key) = state.api_key_cached()? else {
        return Err(AppError::NoKey);
    };
    ApiClient::new(key).validate().await
}

/// Delete the key and clear cached data (specs/api-auth sign-out).
#[tauri::command]
pub fn sign_out(app: AppHandle) -> AppResult<()> {
    keychain::delete_key()?;
    let state = app.state::<AppState>();
    state.set_api_key(None);
    {
        let conn = state.db.lock().unwrap();
        // NOTE: `write_audit` is deliberately NOT cleared here (specs/rsvp-
        // screening design D3) — it's a durable audit log of every mutation
        // attempt, not a cache, and must survive sign-out.
        let _ = conn.execute_batch(
            "DELETE FROM events; DELETE FROM rsvp_summaries; DELETE FROM awaiting_payment;
             DELETE FROM performance_snapshots; DELETE FROM content_pages;
             DELETE FROM email_send_jobs; DELETE FROM email_event_summary;
             DELETE FROM email_throughput; DELETE FROM email_deliverability;
             DELETE FROM survey_followup; DELETE FROM sync_state;
             DELETE FROM sponsors; DELETE FROM sponsor_search_cache;
             DELETE FROM sponsor_contacts; DELETE FROM sponsor_contacts_meta;
             DELETE FROM sponsor_drafts; DELETE FROM sponsor_jobs;
             DELETE FROM rsvp_rows; DELETE FROM rsvp_detail;",
        );
    }
    // Reset first-sync suppression so re-sign-in doesn't fire stale notifications.
    state.first_sync_done.store(false, Ordering::SeqCst);
    if let Some(tray) = app.tray_by_id(crate::state::TRAY_ID) {
        let _ = tray.set_title(Some("—"));
    }
    Ok(())
}

/// All cached events for the overview (renders from SQLite, offline-safe).
#[tauri::command]
pub fn get_events(state: State<'_, AppState>) -> AppResult<Value> {
    let conn = state.db.lock().unwrap();
    let events = db::get_events(&conn)?;
    let features = db::feature_states(&conn)?;
    Ok(json!({ "events": events, "features": features }))
}

/// Cached detail for one event (fast path; no network).
#[tauri::command]
pub fn get_event_detail(state: State<'_, AppState>, meetup_token: String) -> AppResult<Value> {
    let conn = state.db.lock().unwrap();
    Ok(db::get_event_detail(&conn, &meetup_token)?.unwrap_or(Value::Null))
}

/// Refresh one event's performance + awaiting-payment, then return merged detail.
#[tauri::command]
pub async fn fetch_event_detail(app: AppHandle, meetup_token: String) -> AppResult<Value> {
    sync::fetch_event_detail(&app, &meetup_token).await?;
    let state = app.state::<AppState>();
    let conn = state.db.lock().unwrap();
    Ok(db::get_event_detail(&conn, &meetup_token)?.unwrap_or(Value::Null))
}

/// Manual refresh — an immediate cycle within rate-limit constraints (D3).
/// Refreshes both upcoming and past (past is otherwise only fetched at launch).
#[tauri::command]
pub async fn refresh_now(app: AppHandle) -> AppResult<()> {
    let up = sync::run_cycle(app.clone(), true).await;
    let _ = sync::run_past(app).await;
    up
}

// ── Survey + follow-up (specs/survey-followup) ─────────────────────────────

/// Cached survey + follow-up row for one event (fast path; no network).
#[tauri::command]
pub fn get_survey_followup(state: State<'_, AppState>, meetup_token: String) -> AppResult<Value> {
    let conn = state.db.lock().unwrap();
    Ok(db::get_survey_followup(&conn, &meetup_token)?.unwrap_or(Value::Null))
}

/// Fetch survey diagnostic/report + follow-up campaign performance for one
/// past event, then return the merged cached row. Called on detail open and
/// manual refresh only (never the upcoming poll — sync::fetch_survey_followup
/// itself no-ops for non-past events).
#[tauri::command]
pub async fn fetch_survey_followup(app: AppHandle, meetup_token: String) -> AppResult<Value> {
    sync::fetch_survey_followup(&app, &meetup_token).await?;
    let state = app.state::<AppState>();
    let conn = state.db.lock().unwrap();
    Ok(db::get_survey_followup(&conn, &meetup_token)?.unwrap_or(Value::Null))
}

// ── Email lifecycle (specs/email-lifecycle) ────────────────────────────────

/// Cached email surface for one event (summary, send jobs, campaign rates).
#[tauri::command]
pub fn get_event_email(state: State<'_, AppState>, meetup_token: String) -> AppResult<Value> {
    let conn = state.db.lock().unwrap();
    db::get_event_email(&conn, &meetup_token)
}

/// Cached throughput series + progress for one send job.
#[tauri::command]
pub fn get_send_job_throughput(state: State<'_, AppState>, token: String) -> AppResult<Value> {
    let conn = state.db.lock().unwrap();
    db::get_throughput(&conn, &token)
}

/// Cached chapter deliverability view (health, fatigue tier summary, recent jobs).
#[tauri::command]
pub fn get_chapter_deliverability(state: State<'_, AppState>) -> AppResult<Value> {
    let conn = state.db.lock().unwrap();
    db::get_chapter_deliverability(&conn)
}

/// Manual email fetch. With a `meetup_token` it refreshes that event's send-job
/// summary + campaign + active-send throughput (the panel calls this on open and
/// on the gentle active-send cadence). Without one it refreshes the chapter
/// deliverability surface (launch / manual refresh only, never the poll loop).
#[tauri::command]
pub async fn refresh_email(app: AppHandle, meetup_token: Option<String>) -> AppResult<()> {
    match meetup_token {
        Some(token) => sync::fetch_event_email(&app, &token).await,
        None => sync::fetch_chapter_email(&app).await,
    }
}

// ── Promotion tools (specs/promotion-tools) ────────────────────────────────

/// Kick off (or return the id of an already in-flight) generation job for one
/// promotion action. Returns immediately; progress is reported via the
/// `promotion:job` event (design D2). `platform` is empty for kinds that
/// aren't per-platform (event_promo, discussion_topics).
#[tauri::command]
pub fn promotion_generate(
    app: AppHandle,
    kind: String,
    meetup_token: String,
    platform: Option<String>,
    params: Value,
) -> AppResult<String> {
    sync::promotion_generate(&app, kind, meetup_token, platform.unwrap_or_default(), params)
}

/// Cancel an in-flight generation job; the action falls back to its last
/// cached draft (design D5).
#[tauri::command]
pub fn promotion_cancel(app: AppHandle, job_id: String) -> AppResult<()> {
    sync::promotion_cancel(&app, &job_id)
}

/// All cached promotion drafts for one event (fast path; no network).
#[tauri::command]
pub fn get_promotion_drafts(state: State<'_, AppState>, meetup_token: String) -> AppResult<Value> {
    let conn = state.db.lock().unwrap();
    db::get_promotion_drafts(&conn, &meetup_token)
}

/// The cached draft for one `(meetup_token, kind, platform)`, if any.
#[tauri::command]
pub fn get_promotion_draft(
    state: State<'_, AppState>,
    meetup_token: String,
    kind: String,
    platform: Option<String>,
) -> AppResult<Value> {
    let conn = state.db.lock().unwrap();
    Ok(
        db::get_promotion_draft(&conn, &meetup_token, &kind, platform.unwrap_or_default().as_str())?
            .unwrap_or(Value::Null),
    )
}

/// Current state of one job, in case the frontend missed its event.
#[tauri::command]
pub fn get_promotion_job(state: State<'_, AppState>, job_id: String) -> AppResult<Value> {
    let conn = state.db.lock().unwrap();
    Ok(db::get_promotion_job(&conn, &job_id)?.unwrap_or(Value::Null))
}

/// Logo/brand asset search — a cheap GET cached with a short freshness window,
/// not a tracked generation job (design D3).
#[tauri::command]
pub async fn logo_search(
    app: AppHandle,
    query: String,
    scope: Option<String>,
    include_co_branded: Option<bool>,
    limit: Option<u32>,
) -> AppResult<Value> {
    sync::logo_search(
        &app,
        query,
        scope.unwrap_or_else(|| "smart_match".to_string()),
        include_co_branded.unwrap_or(false),
        limit.unwrap_or(20),
    )
    .await
}

// ── Sponsor tools (specs/sponsor-tools) ────────────────────────────────────

/// Search sponsors (fetch + cache) and return the cached result page. Never
/// bubbles a capability-block/rate-limit error — those are stored on the
/// search-cache row so the screen can render a degrade state (task 3.6).
#[tauri::command]
pub async fn sponsor_search(
    app: AppHandle,
    query: String,
    city: Option<String>,
    industry: Option<String>,
    active_only: Option<bool>,
) -> AppResult<Value> {
    sync::sponsor_search(&app, query, city, industry, active_only.unwrap_or(false)).await
}

/// Cache-only read of one sponsor's contacts (no network) — used for
/// background re-renders once contacts have already been fetched this session.
#[tauri::command]
pub fn get_sponsor_contacts(state: State<'_, AppState>, sponsor_ref: String) -> AppResult<Value> {
    let conn = state.db.lock().unwrap();
    db::get_sponsor_contacts(&conn, &sponsor_ref)
}

/// Fetch + cache contacts for one sponsor (explicit action — selecting a
/// sponsor), replacing the whole cached set, and return the merged view.
#[tauri::command]
pub async fn sponsor_contacts_get(app: AppHandle, sponsor_ref: String) -> AppResult<Value> {
    sync::sponsor_contacts_get(&app, sponsor_ref).await
}

/// Kick off (or return the id of an already in-flight) sponsor research/pitch
/// generation job. Returns immediately; progress is reported via the
/// `sponsor_draft_progress` event. `kind` is `research` or `pitch`.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn sponsor_generate(
    app: AppHandle,
    kind: String,
    sponsor_ref: Option<String>,
    name: Option<String>,
    domain: Option<String>,
    city: Option<String>,
    channel: Option<String>,
    target_audience: Option<String>,
    meetup_token: Option<String>,
    notes: Option<String>,
) -> AppResult<String> {
    sync::sponsor_generate(
        &app,
        kind,
        sync::SponsorGenParams {
            sponsor_ref,
            name,
            domain,
            city,
            channel,
            target_audience,
            meetup_token,
            notes,
        },
    )
}

/// Cancel an in-flight sponsor generation job; the action falls back to its
/// cached drafts.
#[tauri::command]
pub fn sponsor_generation_cancel(app: AppHandle, job_id: String) -> AppResult<()> {
    sync::sponsor_generation_cancel(&app, &job_id)
}

/// All cached drafts for one subject (sponsor_token or free-text company
/// name), newest first; `kind` narrows to `research` or `pitch`.
#[tauri::command]
pub fn get_sponsor_drafts(
    state: State<'_, AppState>,
    sponsor_ref: Option<String>,
    name: Option<String>,
    kind: Option<String>,
) -> AppResult<Value> {
    let conn = state.db.lock().unwrap();
    let subject = db::sponsor_subject_key(sponsor_ref.as_deref(), name.as_deref());
    db::list_sponsor_drafts(&conn, &subject, kind.as_deref())
}

/// One cached draft by id, if any.
#[tauri::command]
pub fn get_sponsor_draft(state: State<'_, AppState>, draft_id: String) -> AppResult<Value> {
    let conn = state.db.lock().unwrap();
    Ok(db::get_sponsor_draft(&conn, &draft_id)?.unwrap_or(Value::Null))
}

/// Current state of one sponsor generation job, in case the frontend missed
/// its event.
#[tauri::command]
pub fn get_sponsor_job(state: State<'_, AppState>, job_id: String) -> AppResult<Value> {
    let conn = state.db.lock().unwrap();
    Ok(db::get_sponsor_job(&conn, &job_id)?.unwrap_or(Value::Null))
}

#[tauri::command]
pub fn get_next_event(app: AppHandle) -> AppResult<Value> {
    Ok(sync::next_event_json(&app).unwrap_or(Value::Null))
}

#[tauri::command]
pub fn set_notifications_enabled(state: State<'_, AppState>, enabled: bool) -> AppResult<()> {
    state.notifications_enabled.store(enabled, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
pub fn get_notifications_enabled(state: State<'_, AppState>) -> AppResult<bool> {
    Ok(state.notifications_enabled.load(Ordering::SeqCst))
}

/// Bring the main window forward (from tray/popover).
#[tauri::command]
pub fn open_main(app: AppHandle) -> AppResult<()> {
    if let Some(win) = app.get_webview_window(MAIN_LABEL) {
        let _ = win.show();
        let _ = win.set_focus();
    }
    if let Some(pop) = app.get_webview_window(POPOVER_LABEL) {
        let _ = pop.hide();
    }
    Ok(())
}

#[tauri::command]
pub fn hide_popover(app: AppHandle) -> AppResult<()> {
    if let Some(pop) = app.get_webview_window(POPOVER_LABEL) {
        let _ = pop.hide();
    }
    Ok(())
}

// ── RSVP screening (specs/rsvp-screening) — first write feature ────────────
// Every mutation below passes through `write_guard`: `_prepare` binds a token
// to the exact intended payload (no network call); `_commit` validates that
// token — rejecting unknown/expired/reused/tampered ones with
// `confirmation_required` — before `api.rs` is ever invoked. This is the
// shared choke point attendance-checkin, speaker-review, networking-connect,
// and media-video-kit will all reuse.

const VALID_RSVP_STATES: [&str; 4] = ["registered", "attending", "waitlisted", "denied"];

fn validate_state(new_state: &str) -> AppResult<()> {
    if VALID_RSVP_STATES.contains(&new_state) {
        Ok(())
    } else {
        Err(AppError::Other(format!(
            "invalid state '{new_state}' — must be one of {VALID_RSVP_STATES:?}"
        )))
    }
}

/// The exact-mutation payload a confirmation token is bound to. `meetup_token`
/// is deliberately excluded — it's bookkeeping for the audit row and the
/// post-write refresh, not part of the mutation's identity.
fn state_update_payload(rsvp_ref: &str, new_state: &str, send_email: bool, note: Option<&str>) -> Value {
    json!({
        "action": "rsvp_state_update",
        "rsvp_ref": rsvp_ref,
        "state": new_state,
        "send_email": send_email,
        "note": note,
    })
}

fn bulk_state_update_payload(rsvp_refs: &[String], new_state: &str, send_email: bool, note: Option<&str>) -> Value {
    let mut refs = rsvp_refs.to_vec();
    refs.sort(); // stable regardless of UI selection order
    json!({
        "action": "rsvp_bulk_state_update",
        "rsvp_refs": refs,
        "state": new_state,
        "send_email": send_email,
        "note": note,
    })
}

/// Cached attendee list for one event (fast path; no network).
#[tauri::command]
pub fn get_rsvp_list(state: State<'_, AppState>, meetup_token: String) -> AppResult<Value> {
    let conn = state.db.lock().unwrap();
    db::get_rsvp_rows(&conn, &meetup_token)
}

/// Fetch + cache the attendee list for one event, then return it. An explicit
/// screen-open/manual-refresh action — never the poll loop.
#[tauri::command]
pub async fn fetch_rsvp_list(app: AppHandle, meetup_token: String) -> AppResult<Value> {
    sync::fetch_rsvp_list(&app, &meetup_token).await?;
    let state = app.state::<AppState>();
    let conn = state.db.lock().unwrap();
    db::get_rsvp_rows(&conn, &meetup_token)
}

/// Cached per-registrant detail (assessment, status history, score breakdown).
#[tauri::command]
pub fn get_rsvp_detail(state: State<'_, AppState>, rsvp_ref: String) -> AppResult<Value> {
    let conn = state.db.lock().unwrap();
    Ok(db::get_rsvp_detail(&conn, &rsvp_ref)?.unwrap_or(Value::Null))
}

/// Fetch + cache one registrant's assessment/history/score, then return it.
#[tauri::command]
pub async fn fetch_rsvp_detail(app: AppHandle, rsvp_ref: String) -> AppResult<Value> {
    sync::fetch_rsvp_detail(&app, &rsvp_ref).await?;
    let state = app.state::<AppState>();
    let conn = state.db.lock().unwrap();
    Ok(db::get_rsvp_detail(&conn, &rsvp_ref)?.unwrap_or(Value::Null))
}

/// Recent write-audit entries for one event (cache-only) — shown alongside the
/// server-side status history so the screen surfaces both (design D3).
#[tauri::command]
pub fn get_write_audit(state: State<'_, AppState>, meetup_token: String, limit: Option<i64>) -> AppResult<Value> {
    let conn = state.db.lock().unwrap();
    db::get_write_audit_for_event(&conn, &meetup_token, limit.unwrap_or(50))
}

/// Step 1 of the write guardrail for a single RSVP: makes NO network call.
/// Binds a confirmation token to the exact mutation and returns a summary
/// (from/to state, registrant-facing effect, email-send choice) for the
/// confirm dialog.
#[tauri::command]
pub fn rsvp_state_update_prepare(
    state: State<'_, AppState>,
    rsvp_ref: String,
    new_state: String,
    send_email: bool,
    note: Option<String>,
) -> AppResult<Value> {
    validate_state(&new_state)?;
    let conn = state.db.lock().unwrap();
    let cached = db::get_rsvp_row(&conn, &rsvp_ref)?;
    drop(conn);
    let from_state = cached.as_ref().and_then(|r| r.get("state").and_then(Value::as_str)).map(str::to_string);
    let registrant_status_label = cached.as_ref().and_then(|r| r.get("registrant_status_label").and_then(Value::as_str)).map(str::to_string);

    let payload = state_update_payload(&rsvp_ref, &new_state, send_email, note.as_deref());
    let token = state.write_guard.prepare(&payload);
    Ok(json!({
        "token": token,
        "action": "rsvp_state_update",
        "rsvp_ref": rsvp_ref,
        "from_state": from_state,
        "to_state": new_state,
        "registrant_status_label": registrant_status_label,
        "send_email": send_email,
        "count": 1,
    }))
}

/// Step 2: validate the token against the identical payload — rejecting any
/// mismatch as `confirmation_required` before this touches the network — then
/// write the `attempted` audit row, call the API, update the audit outcome
/// after, and (on success) run the priority post-write refresh (design D5).
/// On `forbidden_*`/`rate_limited` the mutation is aborted with no cache
/// change beyond the audit row (design D6/D7).
#[tauri::command]
pub async fn rsvp_state_update_commit(
    app: AppHandle,
    token: String,
    meetup_token: String,
    rsvp_ref: String,
    new_state: String,
    send_email: bool,
    note: Option<String>,
) -> AppResult<Value> {
    validate_state(&new_state)?;
    let payload = state_update_payload(&rsvp_ref, &new_state, send_email, note.as_deref());
    let app_state = app.state::<AppState>();
    app_state.write_guard.commit(&token, &payload)?;

    let now = iso_now();
    let from_state = {
        let conn = app_state.db.lock().unwrap();
        db::get_rsvp_row(&conn, &rsvp_ref)?
            .and_then(|r| r.get("state").and_then(Value::as_str).map(str::to_string))
    };
    let audit_id = new_id();
    {
        let conn = app_state.db.lock().unwrap();
        db::insert_write_audit(
            &conn, &audit_id, "rsvp_state_update", Some(&meetup_token), &[rsvp_ref.clone()],
            from_state.as_deref(), Some(&new_state), send_email, true, &now,
        )?;
    }

    let result = sync::rsvp_state_update(&app, &meetup_token, &rsvp_ref, &new_state, send_email, note.as_deref()).await;

    let outcome_now = iso_now();
    let (outcome, error_code) = match &result {
        Ok(()) => ("ok".to_string(), None),
        Err(e) => (e.code().to_string(), Some(e.code().to_string())),
    };
    {
        let conn = app_state.db.lock().unwrap();
        db::update_write_audit_outcome(&conn, &audit_id, &outcome, error_code.as_deref(), &outcome_now)?;
    }
    result?;

    let conn = app_state.db.lock().unwrap();
    Ok(db::get_rsvp_row(&conn, &rsvp_ref)?.unwrap_or(Value::Null))
}

/// Step 1 of the write guardrail for a bulk triage. Enforces the materialized
/// selection ceiling BEFORE a token is ever prepared (design D4) — a
/// selection over the ceiling gets a `ceiling_exceeded` error instead, telling
/// the caller to chunk.
#[tauri::command]
pub fn rsvp_bulk_state_update_prepare(
    state: State<'_, AppState>,
    rsvp_refs: Vec<String>,
    new_state: String,
    send_email: bool,
    note: Option<String>,
) -> AppResult<Value> {
    validate_state(&new_state)?;
    if rsvp_refs.is_empty() {
        return Err(AppError::Other("selection is empty".into()));
    }
    if rsvp_refs.len() > BULK_CEILING {
        return Err(AppError::CeilingExceeded(format!(
            "selection of {} exceeds the per-call ceiling of {BULK_CEILING} — split into chunks",
            rsvp_refs.len(),
        )));
    }
    let payload = bulk_state_update_payload(&rsvp_refs, &new_state, send_email, note.as_deref());
    let token = state.write_guard.prepare(&payload);
    Ok(json!({
        "token": token,
        "action": "rsvp_bulk_state_update",
        "rsvp_refs": rsvp_refs,
        "to_state": new_state,
        "send_email": send_email,
        "count": rsvp_refs.len(),
    }))
}

/// Step 2 of bulk triage: same token-validation gate, ceiling re-checked
/// defensively, audit before/after, no auto-retry on rate limit, priority
/// post-write refresh (re-sweeps the event's list) on success.
#[tauri::command]
pub async fn rsvp_bulk_state_update_commit(
    app: AppHandle,
    token: String,
    meetup_token: String,
    rsvp_refs: Vec<String>,
    new_state: String,
    send_email: bool,
    note: Option<String>,
) -> AppResult<Value> {
    validate_state(&new_state)?;
    if rsvp_refs.len() > BULK_CEILING {
        return Err(AppError::CeilingExceeded(format!(
            "selection of {} exceeds the per-call ceiling of {BULK_CEILING}",
            rsvp_refs.len(),
        )));
    }
    let payload = bulk_state_update_payload(&rsvp_refs, &new_state, send_email, note.as_deref());
    let app_state = app.state::<AppState>();
    app_state.write_guard.commit(&token, &payload)?;

    let now = iso_now();
    let audit_id = new_id();
    {
        let conn = app_state.db.lock().unwrap();
        db::insert_write_audit(
            &conn, &audit_id, "rsvp_bulk_state_update", Some(&meetup_token), &rsvp_refs,
            None, Some(&new_state), send_email, true, &now,
        )?;
    }

    let result = sync::rsvp_bulk_state_update(&app, &meetup_token, &rsvp_refs, &new_state, send_email, note.as_deref()).await;

    let outcome_now = iso_now();
    let (outcome, error_code) = match &result {
        Ok(()) => ("ok".to_string(), None),
        Err(e) => (e.code().to_string(), Some(e.code().to_string())),
    };
    {
        let conn = app_state.db.lock().unwrap();
        db::update_write_audit_outcome(&conn, &audit_id, &outcome, error_code.as_deref(), &outcome_now)?;
    }
    result?;

    Ok(json!({ "updated": rsvp_refs.len() }))
}

// ── Attendance check-in (specs/attendance-checkin) — reuses write_guard ────
// A door tap still passes through the same prepare/commit confirmation gate
// as RSVP screening (design D2), just without a blocking modal in between:
// the frontend calls prepare then immediately commit as one deliberate action
// (the tap itself IS the confirmation — design D2's "distinct control", not a
// dialog). The actual `mark_attended` POST never happens inside `commit` — it
// only enqueues to the durable offline queue; `sync::flush_action_queue` is
// what talks to the network, so a tap never blocks on connectivity.

fn checkin_payload(rsvp_ref: &str) -> Value {
    json!({ "action": "checkin_mark_attended", "rsvp_ref": rsvp_ref })
}

/// Resolve which event the check-in screen targets: an explicit token, or the
/// same live/next-event selection the tray already uses (design D1).
fn resolve_checkin_event(app: &AppHandle, meetup_token: Option<String>) -> Option<String> {
    if let Some(t) = meetup_token.filter(|t| !t.is_empty()) {
        return Some(t);
    }
    sync::next_event_json(app)
        .and_then(|ev| ev.get("meetup_token").and_then(Value::as_str).map(str::to_string))
}

/// Cached attendee list for the resolved event (fast path; no network),
/// annotated with which rows have an unsent check-in still queued.
#[tauri::command]
pub fn get_checkin_attendees(
    state: State<'_, AppState>,
    app: AppHandle,
    meetup_token: Option<String>,
) -> AppResult<Value> {
    let Some(token) = resolve_checkin_event(&app, meetup_token) else {
        return Ok(json!({ "meetup_token": Value::Null, "rows": [], "pending_refs": [] }));
    };
    let conn = state.db.lock().unwrap();
    let mut list = db::get_rsvp_rows(&conn, &token)?;
    let pending = db::pending_checkin_refs(&conn, &token)?;
    if let Value::Object(ref mut map) = list {
        map.insert("pending_refs".into(), json!(pending));
    }
    Ok(list)
}

/// Fetch + cache the attendee list for the resolved event, opportunistically
/// flush any queued check-ins while we're online, then return the merged view.
#[tauri::command]
pub async fn fetch_checkin_attendees(app: AppHandle, meetup_token: Option<String>) -> AppResult<Value> {
    let Some(token) = resolve_checkin_event(&app, meetup_token) else {
        return Ok(json!({ "meetup_token": Value::Null, "rows": [], "pending_refs": [] }));
    };
    sync::fetch_rsvp_list(&app, &token).await?;
    let _ = sync::flush_action_queue(&app).await;
    let state = app.state::<AppState>();
    let conn = state.db.lock().unwrap();
    let mut list = db::get_rsvp_rows(&conn, &token)?;
    let pending = db::pending_checkin_refs(&conn, &token)?;
    if let Value::Object(ref mut map) = list {
        map.insert("pending_refs".into(), json!(pending));
    }
    Ok(list)
}

/// Live checked-in-vs-attending progress for the resolved event (design D5):
/// server `checked_in` total plus any not-yet-synced local check-ins.
#[tauri::command]
pub fn get_checkin_count(app: AppHandle, meetup_token: Option<String>) -> AppResult<Value> {
    let Some(token) = resolve_checkin_event(&app, meetup_token) else {
        return Ok(json!({ "meetup_token": Value::Null, "attending": 0, "checked_in": 0, "pending": 0 }));
    };
    Ok(sync::checkin_progress(&app, &token))
}

/// Terminally-denied check-ins for the resolved event (design D7) — the
/// screen uses this to disable its controls with an explanatory notice.
#[tauri::command]
pub fn get_checkin_denials(state: State<'_, AppState>, app: AppHandle, meetup_token: Option<String>) -> AppResult<Value> {
    let Some(token) = resolve_checkin_event(&app, meetup_token) else {
        return Ok(json!([]));
    };
    let conn = state.db.lock().unwrap();
    Ok(json!(db::checkin_denials(&conn, &token)?))
}

/// Step 1 of the write guardrail for one door check-in: no network call, just
/// binds a confirmation token to the exact `(rsvp_ref)` mutation.
#[tauri::command]
pub fn checkin_prepare(state: State<'_, AppState>, rsvp_ref: String, meetup_token: String) -> AppResult<Value> {
    let payload = checkin_payload(&rsvp_ref);
    let token = state.write_guard.prepare(&payload);
    Ok(json!({
        "token": token,
        "action": "checkin_mark_attended",
        "rsvp_ref": rsvp_ref,
        "meetup_token": meetup_token,
    }))
}

/// Step 2: validate the token, then — unless the RSVP is already checked in
/// or already has an unsent check-in queued (design D4, checked BEFORE
/// touching the audit/queue tables) — write the `attempted` audit row and
/// enqueue the check-in. Returns immediately with the optimistic row; the
/// actual network write happens on the next flush (spawned right after, plus
/// the regular cycle), so a tap never blocks on connectivity (design D3).
#[tauri::command]
pub async fn checkin_commit(app: AppHandle, token: String, rsvp_ref: String, meetup_token: String) -> AppResult<Value> {
    let payload = checkin_payload(&rsvp_ref);
    let app_state = app.state::<AppState>();
    app_state.write_guard.commit(&token, &payload)?;

    let now = iso_now();
    let client_token = new_id();
    let queue_id = new_id();
    let audit_id = new_id();

    let outcome = {
        let conn = app_state.db.lock().unwrap();
        // Insert the attempted audit row first (design D3) so even a crash
        // between here and the enqueue leaves evidence — but only for a
        // genuine new enqueue, not a dedupe no-op (task 2.3).
        let would_dupe = db::get_rsvp_row(&conn, &rsvp_ref)?
            .and_then(|r| r.get("checked_in").and_then(Value::as_bool))
            .unwrap_or(false)
            || db::has_unsent_checkin(&conn, &rsvp_ref)?;
        if !would_dupe {
            db::insert_write_audit(
                &conn, &audit_id, "checkin_mark_attended", Some(&meetup_token), std::slice::from_ref(&rsvp_ref),
                Some("not_checked_in"), Some("checked_in"), false, true, &now,
            )?;
        }
        db::enqueue_checkin_action(&conn, &queue_id, &rsvp_ref, &meetup_token, &client_token, &audit_id, &now)?
    };

    // Opportunistic flush (task 3.2) — spawned so the tap's response is
    // immediate regardless of network conditions (design D3 speed priority).
    if matches!(outcome, db::EnqueueOutcome::Enqueued) {
        let app2 = app.clone();
        tauri::async_runtime::spawn(async move {
            let _ = sync::flush_action_queue(&app2).await;
        });
    }

    let conn = app_state.db.lock().unwrap();
    let row = db::get_rsvp_row(&conn, &rsvp_ref)?.unwrap_or(Value::Null);
    let queued = matches!(outcome, db::EnqueueOutcome::Enqueued);
    Ok(json!({ "row": row, "queued": queued }))
}

// ── Speaker review (specs/speaker-review) — reuses write_guard ─────────────
// The app's third write path. Approve/decline/move-to-review and the
// create/edit-proposal form both funnel through `rsvp_speaker_proposal_upsert`
// (design: "Approval via speaker_proposal_upsert, not state_update") behind
// the same prepare/commit confirmation gate as rsvp-screening and
// attendance-checkin, and reuse the same `write_audit` table — no bespoke
// audit log for this feature.

const VALID_SPEAKER_STATUSES: [&str; 4] = ["pending_review", "main_stage", "science_fair", "sidelined"];

fn validate_speaker_status(status: &str) -> AppResult<()> {
    if VALID_SPEAKER_STATUSES.contains(&status) {
        Ok(())
    } else {
        Err(AppError::Other(format!(
            "invalid speaker_status '{status}' — must be one of {VALID_SPEAKER_STATUSES:?}"
        )))
    }
}

/// The exact-mutation payload a confirmation token is bound to. `meetup_token`
/// is excluded, same rationale as `state_update_payload` — it's bookkeeping
/// for the audit row and post-write refresh, not part of the mutation's identity.
fn speaker_upsert_payload(
    rsvp_ref: &str,
    speaker_title: &str,
    speaker_description: &str,
    speaker_status: Option<&str>,
    note: Option<&str>,
) -> Value {
    json!({
        "action": "speaker_proposal_upsert",
        "rsvp_ref": rsvp_ref,
        "speaker_title": speaker_title,
        "speaker_description": speaker_description,
        "speaker_status": speaker_status,
        "note": note,
    })
}

/// Cached talk-proposal pipeline for one event, grouped into kanban lanes
/// (fast path; no network).
#[tauri::command]
pub fn get_speaker_proposals(state: State<'_, AppState>, meetup_token: String) -> AppResult<Value> {
    let conn = state.db.lock().unwrap();
    db::get_speaker_proposals(&conn, &meetup_token)
}

/// Fetch + cache the talk-proposal pipeline for one event, then return it. An
/// explicit screen-open/manual-refresh action — never the poll loop.
#[tauri::command]
pub async fn fetch_speaker_proposals(app: AppHandle, meetup_token: String) -> AppResult<Value> {
    sync::fetch_speaker_proposals(&app, &meetup_token).await?;
    let state = app.state::<AppState>();
    let conn = state.db.lock().unwrap();
    db::get_speaker_proposals(&conn, &meetup_token)
}

/// Cached ranked candidate pool for the resolved scope (fast path; no network).
#[tauri::command]
pub fn get_speaker_candidates(app: AppHandle, weblog_token: Option<String>) -> AppResult<Value> {
    let scope = weblog_token.unwrap_or_else(|| sync::default_speaker_candidate_scope(&app));
    let state = app.state::<AppState>();
    let conn = state.db.lock().unwrap();
    db::get_speaker_candidates(&conn, &scope)
}

/// Fetch + cache the ranked candidate pool for the resolved scope. On a
/// rate-limit/capability block the last-good cached candidates are returned
/// unchanged, annotated with the degrade reason (task 3.2) — this command
/// never bubbles that as a hard error since the panel is meant to degrade
/// gracefully in place.
#[tauri::command]
pub async fn fetch_speaker_candidates(app: AppHandle, weblog_token: Option<String>) -> AppResult<Value> {
    let scope = weblog_token
        .clone()
        .unwrap_or_else(|| sync::default_speaker_candidate_scope(&app));
    let _ = sync::fetch_speaker_candidates(&app, weblog_token.as_deref()).await;
    let state = app.state::<AppState>();
    let conn = state.db.lock().unwrap();
    db::get_speaker_candidates(&conn, &scope)
}

/// Step 1 of the write guardrail for approve/decline/move-to-review: makes NO
/// network call. Requires the RSVP already be cached (so `speaker_title`/
/// `speaker_description` — required by the upstream endpoint — are available
/// even though only the status is changing), and binds a confirmation token
/// to the exact mutation.
#[tauri::command]
pub fn speaker_approval_prepare(
    state: State<'_, AppState>,
    rsvp_ref: String,
    new_status: String,
    note: Option<String>,
) -> AppResult<Value> {
    validate_speaker_status(&new_status)?;
    let conn = state.db.lock().unwrap();
    let cached = db::get_speaker_proposal(&conn, &rsvp_ref)?
        .ok_or_else(|| AppError::NotFound("proposal not cached — refresh the pipeline first".into()))?;
    drop(conn);

    let speaker_title = cached.get("speaker_title").and_then(Value::as_str).unwrap_or_default().to_string();
    let speaker_description = cached.get("speaker_description").and_then(Value::as_str).unwrap_or_default().to_string();
    let from_lane = cached.get("lane").and_then(Value::as_str).map(str::to_string);
    let from_status = cached.get("speaker_approval_status").and_then(Value::as_str).map(str::to_string);

    let payload = speaker_upsert_payload(&rsvp_ref, &speaker_title, &speaker_description, Some(&new_status), note.as_deref());
    let token = state.write_guard.prepare(&payload);
    Ok(json!({
        "token": token,
        "action": "speaker_proposal_upsert",
        "rsvp_ref": rsvp_ref,
        "speaker_title": speaker_title,
        "speaker_description": speaker_description,
        "from_lane": from_lane,
        "from_status": from_status,
        "to_status": new_status,
        "count": 1,
    }))
}

/// Step 2: validate the token against the identical payload, write the
/// `attempted` audit row, call the API (never with `send_speaker_email`/
/// `send_rsvp_email` true), update the audit outcome after, and (on success)
/// run the priority post-write `rsvp_get` refresh. On `forbidden_*`/
/// `rate_limited` the mutation is aborted with no cache change beyond the
/// audit row.
#[tauri::command]
pub async fn speaker_approval_commit(
    app: AppHandle,
    token: String,
    meetup_token: String,
    rsvp_ref: String,
    new_status: String,
    note: Option<String>,
) -> AppResult<Value> {
    validate_speaker_status(&new_status)?;
    let app_state = app.state::<AppState>();
    let (speaker_title, speaker_description, from_status) = {
        let conn = app_state.db.lock().unwrap();
        let cached = db::get_speaker_proposal(&conn, &rsvp_ref)?
            .ok_or_else(|| AppError::NotFound("proposal not cached — refresh the pipeline first".into()))?;
        (
            cached.get("speaker_title").and_then(Value::as_str).unwrap_or_default().to_string(),
            cached.get("speaker_description").and_then(Value::as_str).unwrap_or_default().to_string(),
            cached.get("speaker_approval_status").and_then(Value::as_str).map(str::to_string),
        )
    };
    let payload = speaker_upsert_payload(&rsvp_ref, &speaker_title, &speaker_description, Some(&new_status), note.as_deref());
    app_state.write_guard.commit(&token, &payload)?;

    let now = iso_now();
    let audit_id = new_id();
    {
        let conn = app_state.db.lock().unwrap();
        db::insert_write_audit(
            &conn, &audit_id, "speaker_proposal_upsert", Some(&meetup_token), &[rsvp_ref.clone()],
            from_status.as_deref(), Some(&new_status), false, true, &now,
        )?;
    }

    let result = sync::speaker_proposal_upsert(
        &app, &meetup_token, &rsvp_ref, &speaker_title, &speaker_description, Some(&new_status), note.as_deref(),
    )
    .await;

    let outcome_now = iso_now();
    let (outcome, error_code) = match &result {
        Ok(()) => ("ok".to_string(), None),
        Err(e) => (e.code().to_string(), Some(e.code().to_string())),
    };
    {
        let conn = app_state.db.lock().unwrap();
        db::update_write_audit_outcome(&conn, &audit_id, &outcome, error_code.as_deref(), &outcome_now)?;
    }
    result?;

    let conn = app_state.db.lock().unwrap();
    Ok(db::get_speaker_proposal(&conn, &rsvp_ref)?.unwrap_or(Value::Null))
}

/// Step 1 of the write guardrail for creating/editing a proposal's
/// title/description (with an optional status change alongside it). Falls
/// back to empty title/description for a brand-new proposal on an RSVP with
/// no cached proposal yet.
#[tauri::command]
pub fn speaker_proposal_prepare(
    state: State<'_, AppState>,
    rsvp_ref: String,
    speaker_title: String,
    speaker_description: String,
    speaker_status: Option<String>,
    note: Option<String>,
) -> AppResult<Value> {
    if let Some(s) = &speaker_status {
        validate_speaker_status(s)?;
    }
    let conn = state.db.lock().unwrap();
    let cached = db::get_speaker_proposal(&conn, &rsvp_ref)?;
    drop(conn);
    let from_lane = cached.as_ref().and_then(|c| c.get("lane").and_then(Value::as_str)).map(str::to_string);

    let payload = speaker_upsert_payload(&rsvp_ref, &speaker_title, &speaker_description, speaker_status.as_deref(), note.as_deref());
    let token = state.write_guard.prepare(&payload);
    Ok(json!({
        "token": token,
        "action": "speaker_proposal_upsert",
        "rsvp_ref": rsvp_ref,
        "speaker_title": speaker_title,
        "speaker_description": speaker_description,
        "from_lane": from_lane,
        "to_status": speaker_status,
        "count": 1,
    }))
}

/// Step 2: same token-validation gate as `speaker_approval_commit`, for the
/// create/edit-proposal form.
#[tauri::command]
pub async fn speaker_proposal_commit(
    app: AppHandle,
    token: String,
    meetup_token: String,
    rsvp_ref: String,
    speaker_title: String,
    speaker_description: String,
    speaker_status: Option<String>,
    note: Option<String>,
) -> AppResult<Value> {
    if let Some(s) = &speaker_status {
        validate_speaker_status(s)?;
    }
    let payload = speaker_upsert_payload(&rsvp_ref, &speaker_title, &speaker_description, speaker_status.as_deref(), note.as_deref());
    let app_state = app.state::<AppState>();
    app_state.write_guard.commit(&token, &payload)?;

    let now = iso_now();
    let from_status = {
        let conn = app_state.db.lock().unwrap();
        db::get_speaker_proposal(&conn, &rsvp_ref)?
            .and_then(|c| c.get("speaker_approval_status").and_then(Value::as_str).map(str::to_string))
    };
    let audit_id = new_id();
    {
        let conn = app_state.db.lock().unwrap();
        db::insert_write_audit(
            &conn, &audit_id, "speaker_proposal_upsert", Some(&meetup_token), &[rsvp_ref.clone()],
            from_status.as_deref(), speaker_status.as_deref(), false, true, &now,
        )?;
    }

    let result = sync::speaker_proposal_upsert(
        &app, &meetup_token, &rsvp_ref, &speaker_title, &speaker_description, speaker_status.as_deref(), note.as_deref(),
    )
    .await;

    let outcome_now = iso_now();
    let (outcome, error_code) = match &result {
        Ok(()) => ("ok".to_string(), None),
        Err(e) => (e.code().to_string(), Some(e.code().to_string())),
    };
    {
        let conn = app_state.db.lock().unwrap();
        db::update_write_audit_outcome(&conn, &audit_id, &outcome, error_code.as_deref(), &outcome_now)?;
    }
    result?;

    let conn = app_state.db.lock().unwrap();
    Ok(db::get_speaker_proposal(&conn, &rsvp_ref)?.unwrap_or(Value::Null))
}

// ── Networking / Connect (specs/networking-connect) — reuses write_guard ───
// The app's fourth write feature. Every mutation (post/reply, reaction
// toggle, attachment upload, DM) passes through the same prepare/commit
// confirmation gate as rsvp-screening, attendance-checkin, and speaker
// review, and reuses `write_audit` — no bespoke audit path.

const VALID_REACTION_TYPES: [&str; 13] = [
    "thumbs_up", "thumbs_down", "love", "haha", "fire", "100", "trophy",
    "pizza", "rocket", "robot", "rainbow", "salute", "gen_ai",
];

fn validate_reaction_type(t: &str) -> AppResult<()> {
    if VALID_REACTION_TYPES.contains(&t) {
        Ok(())
    } else {
        Err(AppError::Other(format!(
            "invalid reaction_type '{t}' — must be one of {VALID_REACTION_TYPES:?}"
        )))
    }
}

fn validate_post_content(content: &str) -> AppResult<()> {
    if content.trim().is_empty() {
        return Err(AppError::Other("post content is required".into()));
    }
    if content.chars().count() > 10_000 {
        return Err(AppError::Other("post content exceeds the 10000 character limit".into()));
    }
    Ok(())
}

/// Image-URL caps (spec: "rejects the write if more than four URLs or a URL
/// over 2048 characters is supplied") — enforced before a token is ever
/// prepared, so an over-cap request never even reaches `write_guard`.
fn validate_image_urls(urls: &[String]) -> AppResult<()> {
    if urls.len() > 4 {
        return Err(AppError::Other(format!(
            "{} image URLs exceeds the maximum of 4",
            urls.len()
        )));
    }
    for u in urls {
        if u.chars().count() > 2048 {
            return Err(AppError::Other("image URL exceeds the 2048 character limit".into()));
        }
        if !(u.starts_with("http://") || u.starts_with("https://")) {
            return Err(AppError::Other("image URL must be a public http(s) URL".into()));
        }
    }
    Ok(())
}

fn post_create_payload(
    board_key: &str,
    content: &str,
    title: Option<&str>,
    reply_to_post_token: Option<&str>,
    image_urls: &[String],
) -> Value {
    json!({
        "action": "message_board_post_create",
        "board_key": board_key,
        "content": content,
        "title": title,
        "reply_to_post_token": reply_to_post_token,
        "image_urls": image_urls,
    })
}

fn reaction_toggle_payload(board_key: &str, post_token: &str, reaction_type: &str) -> Value {
    json!({
        "action": "message_board_reaction_toggle",
        "board_key": board_key,
        "post_token": post_token,
        "reaction_type": reaction_type,
    })
}

fn attachment_upload_payload(board_key: &str, image_url: &str) -> Value {
    json!({ "action": "message_board_attachment_upload", "board_key": board_key, "image_url": image_url })
}

fn direct_message_payload(client_refs: &[String], emails: &[String], content: &str) -> Value {
    let mut refs = client_refs.to_vec();
    refs.sort();
    let mut ems = emails.to_vec();
    ems.sort();
    json!({ "action": "direct_message_post_create", "client_refs": refs, "emails": ems, "content": content })
}

/// Cached accessible boards (fast path; no network).
#[tauri::command]
pub fn get_networking_boards(state: State<'_, AppState>) -> AppResult<Value> {
    let conn = state.db.lock().unwrap();
    db::get_boards(&conn)
}

/// Fetch + cache boards and the Attention inbox, then return the cached
/// boards list. An explicit screen-open/manual-refresh action.
#[tauri::command]
pub async fn refresh_networking(app: AppHandle) -> AppResult<Value> {
    sync::fetch_networking(&app).await?;
    let state = app.state::<AppState>();
    let conn = state.db.lock().unwrap();
    db::get_boards(&conn)
}

/// Cached messages for one board (fast path; no network).
#[tauri::command]
pub fn get_board_messages(state: State<'_, AppState>, board_key: String) -> AppResult<Value> {
    let conn = state.db.lock().unwrap();
    db::get_board_messages(&conn, &board_key)
}

/// Fetch + cache one board's messages, optionally filtered by
/// mentioned_me/needs_response, then return the cached (post-filter) view.
#[tauri::command]
pub async fn fetch_board_messages(
    app: AppHandle,
    board_key: String,
    mentioned_me: Option<bool>,
    needs_response: Option<bool>,
) -> AppResult<Value> {
    sync::fetch_board_messages(&app, &board_key, mentioned_me.unwrap_or(false), needs_response.unwrap_or(false)).await?;
    let state = app.state::<AppState>();
    let conn = state.db.lock().unwrap();
    db::get_board_messages(&conn, &board_key)
}

/// Cached thread (fast path; no network).
#[tauri::command]
pub fn get_thread(state: State<'_, AppState>, board_key: String, root_post_token: String) -> AppResult<Value> {
    let conn = state.db.lock().unwrap();
    Ok(db::get_thread(&conn, &board_key, &root_post_token)?.unwrap_or(Value::Null))
}

/// Fetch + cache one thread (open, or the focus-based/interval refresh of an
/// already-open thread), then return it from cache.
#[tauri::command]
pub async fn fetch_thread(app: AppHandle, board_key: Option<String>, post_token: String) -> AppResult<Value> {
    sync::fetch_thread(&app, board_key.as_deref(), &post_token).await?;
    let state = app.state::<AppState>();
    let conn = state.db.lock().unwrap();
    // The thread may have resolved a different root_post_token than the
    // matched post we opened with — read back whichever board this post's
    // cached message row belongs to, defaulting to the caller's board_key.
    let bk = board_key.unwrap_or_default();
    Ok(db::get_thread(&conn, &bk, &post_token)?.unwrap_or(Value::Null))
}

/// Cached cross-board Attention inbox (fast path; no network). `reason`
/// narrows to "mentioned_me" or "needs_response"; omit for both.
#[tauri::command]
pub fn get_flagged_posts(state: State<'_, AppState>, reason: Option<String>) -> AppResult<Value> {
    let conn = state.db.lock().unwrap();
    db::get_flagged_posts(&conn, reason.as_deref())
}

/// Fetch + cache the Attention inbox, then return it from cache.
#[tauri::command]
pub async fn refresh_flagged_posts(app: AppHandle, reason: Option<String>) -> AppResult<Value> {
    sync::fetch_networking_flagged(&app).await?;
    let state = app.state::<AppState>();
    let conn = state.db.lock().unwrap();
    db::get_flagged_posts(&conn, reason.as_deref())
}

/// Step 1 of the write guardrail for creating a post/reply: makes NO network
/// call. Validates content/image-URL caps BEFORE a token is ever prepared
/// (spec: "rejects the write if more than four URLs...").
#[tauri::command]
pub fn post_create_prepare(
    state: State<'_, AppState>,
    board_key: String,
    content: String,
    title: Option<String>,
    reply_to_post_token: Option<String>,
    image_urls: Option<Vec<String>>,
) -> AppResult<Value> {
    validate_post_content(&content)?;
    let urls = image_urls.unwrap_or_default();
    validate_image_urls(&urls)?;
    let payload = post_create_payload(&board_key, &content, title.as_deref(), reply_to_post_token.as_deref(), &urls);
    let token = state.write_guard.prepare(&payload);
    Ok(json!({
        "token": token,
        "action": "message_board_post_create",
        "board_key": board_key,
        "content": content,
        "title": title,
        "reply_to_post_token": reply_to_post_token,
        "image_urls": urls,
        "count": 1,
    }))
}

/// Step 2: validate the token against the identical payload, write the
/// `attempted` audit row, call the API, update the audit outcome, and (on
/// success) trigger the targeted re-sync (design: "Writes trigger a targeted
/// re-sync, not a full cycle").
#[tauri::command]
pub async fn post_create_commit(
    app: AppHandle,
    token: String,
    board_key: String,
    content: String,
    title: Option<String>,
    reply_to_post_token: Option<String>,
    image_urls: Option<Vec<String>>,
) -> AppResult<Value> {
    validate_post_content(&content)?;
    let urls = image_urls.unwrap_or_default();
    validate_image_urls(&urls)?;
    let payload = post_create_payload(&board_key, &content, title.as_deref(), reply_to_post_token.as_deref(), &urls);
    let app_state = app.state::<AppState>();
    app_state.write_guard.commit(&token, &payload)?;

    let now = iso_now();
    let audit_id = new_id();
    {
        let conn = app_state.db.lock().unwrap();
        db::insert_write_audit(
            &conn, &audit_id, "message_board_post_create", None, &[board_key.clone()],
            None, Some("posted"), false, true, &now,
        )?;
    }

    let result = sync::networking_post_create(&app, &board_key, &content, title.as_deref(), reply_to_post_token.as_deref(), &urls).await;

    let outcome_now = iso_now();
    let (outcome, error_code) = match &result {
        Ok(_) => ("ok".to_string(), None),
        Err(e) => (e.code().to_string(), Some(e.code().to_string())),
    };
    {
        let conn = app_state.db.lock().unwrap();
        db::update_write_audit_outcome(&conn, &audit_id, &outcome, error_code.as_deref(), &outcome_now)?;
    }
    result
}

/// Step 1 of the write guardrail for a reaction toggle. Per design, reactions
/// stay lightweight (single-line preview) but still flow through the same
/// gate — no bespoke shortcut.
#[tauri::command]
pub fn reaction_toggle_prepare(
    state: State<'_, AppState>,
    board_key: String,
    post_token: String,
    reaction_type: String,
) -> AppResult<Value> {
    validate_reaction_type(&reaction_type)?;
    let payload = reaction_toggle_payload(&board_key, &post_token, &reaction_type);
    let token = state.write_guard.prepare(&payload);
    Ok(json!({
        "token": token,
        "action": "message_board_reaction_toggle",
        "board_key": board_key,
        "post_token": post_token,
        "reaction_type": reaction_type,
        "count": 1,
    }))
}

/// Step 2: same token-validation gate, audit before/after, targeted re-sync
/// of the affected board on success.
#[tauri::command]
pub async fn reaction_toggle_commit(
    app: AppHandle,
    token: String,
    board_key: String,
    post_token: String,
    reaction_type: String,
) -> AppResult<Value> {
    validate_reaction_type(&reaction_type)?;
    let payload = reaction_toggle_payload(&board_key, &post_token, &reaction_type);
    let app_state = app.state::<AppState>();
    app_state.write_guard.commit(&token, &payload)?;

    let now = iso_now();
    let audit_id = new_id();
    {
        let conn = app_state.db.lock().unwrap();
        db::insert_write_audit(
            &conn, &audit_id, "message_board_reaction_toggle", None, &[post_token.clone()],
            None, Some(reaction_type.as_str()), false, true, &now,
        )?;
    }

    let result = sync::networking_reaction_toggle(&app, &board_key, &post_token, &reaction_type).await;

    let outcome_now = iso_now();
    let (outcome, error_code) = match &result {
        Ok(_) => ("ok".to_string(), None),
        Err(e) => (e.code().to_string(), Some(e.code().to_string())),
    };
    {
        let conn = app_state.db.lock().unwrap();
        db::update_write_audit_outcome(&conn, &audit_id, &outcome, error_code.as_deref(), &outcome_now)?;
    }
    result
}

/// Step 1 of the write guardrail for uploading an image attachment by URL.
#[tauri::command]
pub fn attachment_upload_prepare(state: State<'_, AppState>, board_key: String, image_url: String) -> AppResult<Value> {
    validate_image_urls(std::slice::from_ref(&image_url))?;
    let payload = attachment_upload_payload(&board_key, &image_url);
    let token = state.write_guard.prepare(&payload);
    Ok(json!({
        "token": token,
        "action": "message_board_attachment_upload",
        "board_key": board_key,
        "image_url": image_url,
        "count": 1,
    }))
}

/// Step 2: same token-validation gate, audit before/after. No cache mutation
/// on success — the returned attachment_token/image_url is meant to be passed
/// into a subsequent `post_create_prepare`/`commit`.
#[tauri::command]
pub async fn attachment_upload_commit(app: AppHandle, token: String, board_key: String, image_url: String) -> AppResult<Value> {
    validate_image_urls(std::slice::from_ref(&image_url))?;
    let payload = attachment_upload_payload(&board_key, &image_url);
    let app_state = app.state::<AppState>();
    app_state.write_guard.commit(&token, &payload)?;

    let now = iso_now();
    let audit_id = new_id();
    {
        let conn = app_state.db.lock().unwrap();
        db::insert_write_audit(
            &conn, &audit_id, "message_board_attachment_upload", None, &[board_key.clone()],
            None, None, false, true, &now,
        )?;
    }

    let result = sync::networking_attachment_upload(&app, &board_key, &image_url).await;

    let outcome_now = iso_now();
    let (outcome, error_code) = match &result {
        Ok(_) => ("ok".to_string(), None),
        Err(e) => (e.code().to_string(), Some(e.code().to_string())),
    };
    {
        let conn = app_state.db.lock().unwrap();
        db::update_write_audit_outcome(&conn, &audit_id, &outcome, error_code.as_deref(), &outcome_now)?;
    }
    result
}

/// Step 1 of the write guardrail for a direct message. Requires at least one
/// resolved recipient (`client_refs` or `emails`) — the prepare preview shows
/// them so the organizer confirms exactly who receives it.
#[tauri::command]
pub fn direct_message_prepare(
    state: State<'_, AppState>,
    client_refs: Option<Vec<String>>,
    emails: Option<Vec<String>>,
    content: String,
) -> AppResult<Value> {
    let refs = client_refs.unwrap_or_default();
    let ems = emails.unwrap_or_default();
    if refs.is_empty() && ems.is_empty() {
        return Err(AppError::Other("at least one recipient (client_refs or emails) is required".into()));
    }
    validate_post_content(&content)?;
    let payload = direct_message_payload(&refs, &ems, &content);
    let token = state.write_guard.prepare(&payload);
    Ok(json!({
        "token": token,
        "action": "direct_message_post_create",
        "client_refs": refs,
        "emails": ems,
        "content": content,
        "count": refs.len() + ems.len(),
    }))
}

/// Step 2: same token-validation gate, audit before/after, targeted re-sync
/// (the affected DM board, or the boards list if the API didn't echo one back).
#[tauri::command]
pub async fn direct_message_commit(
    app: AppHandle,
    token: String,
    client_refs: Option<Vec<String>>,
    emails: Option<Vec<String>>,
    content: String,
) -> AppResult<Value> {
    let refs = client_refs.unwrap_or_default();
    let ems = emails.unwrap_or_default();
    if refs.is_empty() && ems.is_empty() {
        return Err(AppError::Other("at least one recipient (client_refs or emails) is required".into()));
    }
    validate_post_content(&content)?;
    let payload = direct_message_payload(&refs, &ems, &content);
    let app_state = app.state::<AppState>();
    app_state.write_guard.commit(&token, &payload)?;

    let now = iso_now();
    let audit_id = new_id();
    let mut targets = refs.clone();
    targets.extend(ems.clone());
    {
        let conn = app_state.db.lock().unwrap();
        db::insert_write_audit(
            &conn, &audit_id, "direct_message_post_create", None, &targets,
            None, Some("posted"), false, true, &now,
        )?;
    }

    let result = sync::networking_direct_message_post(&app, &refs, &ems, &content).await;

    let outcome_now = iso_now();
    let (outcome, error_code) = match &result {
        Ok(_) => ("ok".to_string(), None),
        Err(e) => (e.code().to_string(), Some(e.code().to_string())),
    };
    {
        let conn = app_state.db.lock().unwrap();
        db::update_write_audit_outcome(&conn, &audit_id, &outcome, error_code.as_deref(), &outcome_now)?;
    }
    result
}
