mod api;
mod commands;
mod db;
mod error;
mod keychain;
mod state;
mod sync;
mod write_guard;

use std::time::Duration;

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, PhysicalPosition, WindowEvent,
};

use state::{AppState, MAIN_LABEL, POPOVER_LABEL, TRAY_ID};

/// Poll cadence. `meetups/upcoming` returns every visible event in one call,
/// so refreshing the whole set on the 2-minute tier costs ~0.5 rpm — far under
/// the rate cap — and keeps every card's counts fresh (design D3).
const POLL_INTERVAL: Duration = Duration::from_secs(120);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            // Open the SQLite cache in the app data dir.
            let dir = app.path().app_data_dir().expect("app data dir");
            std::fs::create_dir_all(&dir).ok();
            let conn = rusqlite::Connection::open(dir.join("cache.sqlite3"))
                .expect("open sqlite");
            db::init(&conn).expect("init schema");
            app.manage(AppState::new(conn));

            build_tray(app.handle())?;

            // Hide the popover when it loses focus (native menubar behavior).
            if let Some(pop) = app.get_webview_window(POPOVER_LABEL) {
                let pop2 = pop.clone();
                pop.on_window_event(move |ev| {
                    if let WindowEvent::Focused(false) = ev {
                        let _ = pop2.hide();
                    }
                });
            }

            // Closing the main window hides it to the tray instead of quitting.
            if let Some(main) = app.get_webview_window(MAIN_LABEL) {
                let main2 = main.clone();
                main.on_window_event(move |ev| {
                    if let WindowEvent::CloseRequested { api, .. } = ev {
                        api.prevent_close();
                        let _ = main2.hide();
                    }
                });
            }

            // Background poll loop.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                // Initial cycle shortly after launch (if already onboarded).
                let _ = sync::run_cycle(handle.clone(), false).await;
                // Past events fetched once at launch; not part of the poll loop.
                let _ = sync::run_past(handle.clone()).await;
                // Chapter email deliverability: launch + manual refresh only,
                // never the 2-minute loop (specs/email-lifecycle, design D3).
                let _ = sync::fetch_chapter_email(&handle).await;
                let mut ticker = tokio::time::interval(POLL_INTERVAL);
                ticker.tick().await; // consume immediate first tick
                loop {
                    ticker.tick().await;
                    let _ = sync::run_cycle(handle.clone(), false).await;
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::validate_and_store,
            commands::has_key,
            commands::get_identity,
            commands::sign_out,
            commands::get_events,
            commands::get_event_detail,
            commands::fetch_event_detail,
            commands::get_event_email,
            commands::get_send_job_throughput,
            commands::get_chapter_deliverability,
            commands::get_survey_followup,
            commands::fetch_survey_followup,
            commands::refresh_email,
            commands::refresh_now,
            commands::promotion_generate,
            commands::promotion_cancel,
            commands::get_promotion_drafts,
            commands::get_promotion_draft,
            commands::get_promotion_job,
            commands::logo_search,
            commands::sponsor_search,
            commands::get_sponsor_contacts,
            commands::sponsor_contacts_get,
            commands::sponsor_generate,
            commands::sponsor_generation_cancel,
            commands::get_sponsor_drafts,
            commands::get_sponsor_draft,
            commands::get_sponsor_job,
            commands::get_next_event,
            commands::set_notifications_enabled,
            commands::get_notifications_enabled,
            commands::open_main,
            commands::hide_popover,
            commands::get_rsvp_list,
            commands::fetch_rsvp_list,
            commands::get_rsvp_detail,
            commands::fetch_rsvp_detail,
            commands::get_write_audit,
            commands::rsvp_state_update_prepare,
            commands::rsvp_state_update_commit,
            commands::rsvp_bulk_state_update_prepare,
            commands::rsvp_bulk_state_update_commit,
            commands::get_checkin_attendees,
            commands::fetch_checkin_attendees,
            commands::get_checkin_count,
            commands::get_checkin_denials,
            commands::checkin_prepare,
            commands::checkin_commit,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn build_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    let open_i = MenuItem::with_id(app, "open", "Open Mission Control", true, None::<&str>)?;
    let refresh_i = MenuItem::with_id(app, "refresh", "Refresh now", true, None::<&str>)?;
    let signout_i = MenuItem::with_id(app, "signout", "Sign out", true, None::<&str>)?;
    let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open_i, &refresh_i, &signout_i, &quit_i])?;

    let icon = app
        .default_window_icon()
        .cloned()
        .expect("bundled default icon");

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .title("—")
        .tooltip("AI Tinkerers Mission Control")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => {
                let _ = commands::open_main(app.clone());
            }
            "refresh" => {
                let a = app.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = sync::run_cycle(a, true).await;
                });
            }
            "signout" => {
                let _ = commands::sign_out(app.clone());
                let _ = commands::open_main(app.clone());
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            // Left click toggles the popover, positioned under the tray icon.
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                rect,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(pop) = app.get_webview_window(POPOVER_LABEL) {
                    if pop.is_visible().unwrap_or(false) {
                        let _ = pop.hide();
                    } else {
                        let scale = pop.scale_factor().unwrap_or(1.0);
                        let ip = rect.position.to_physical::<f64>(scale);
                        let is = rect.size.to_physical::<f64>(scale);
                        position_popover(&pop, ip.x, ip.y, is.width, is.height);
                        let _ = pop.show();
                        let _ = pop.set_focus();
                    }
                }
            }
        })
        .build(app)?;
    Ok(())
}

/// Place the popover just below the tray icon, right-aligned to it. Inputs are
/// the icon's physical position/size (the event's Rect type isn't nameable).
fn position_popover(pop: &tauri::WebviewWindow, icon_x: f64, icon_y: f64, icon_w: f64, icon_h: f64) {
    let win = pop.outer_size().map(|s| s.width as f64).unwrap_or(320.0);
    let x = (icon_x + icon_w - win).max(8.0);
    let y = icon_y + icon_h + 4.0;
    let _ = pop.set_position(PhysicalPosition::new(x, y));
}
