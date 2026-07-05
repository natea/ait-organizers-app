use std::sync::atomic::Ordering;

use serde_json::{json, Value};
use tauri::{AppHandle, Manager, State};

use crate::api::ApiClient;
use crate::error::{AppError, AppResult};
use crate::state::{AppState, MAIN_LABEL, POPOVER_LABEL};
use crate::{db, keychain, sync};

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
    // Kick off an initial sync in the background.
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        let _ = sync::run_cycle(app2, true).await;
    });
    Ok(identity)
}

/// Whether onboarding has completed (a key is in the keychain).
#[tauri::command]
pub fn has_key() -> AppResult<bool> {
    Ok(keychain::get_key()?.is_some())
}

/// Re-validate the stored key to render identity on launch.
#[tauri::command]
pub async fn get_identity() -> AppResult<Value> {
    let Some(key) = keychain::get_key()? else {
        return Err(AppError::NoKey);
    };
    ApiClient::new(key).validate().await
}

/// Delete the key and clear cached data (specs/api-auth sign-out).
#[tauri::command]
pub fn sign_out(app: AppHandle) -> AppResult<()> {
    keychain::delete_key()?;
    let state = app.state::<AppState>();
    {
        let conn = state.db.lock().unwrap();
        let _ = conn.execute_batch(
            "DELETE FROM events; DELETE FROM rsvp_summaries; DELETE FROM awaiting_payment;
             DELETE FROM performance_snapshots; DELETE FROM sync_state;",
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
#[tauri::command]
pub async fn refresh_now(app: AppHandle) -> AppResult<()> {
    sync::run_cycle(app, true).await
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
