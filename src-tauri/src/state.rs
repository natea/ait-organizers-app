use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

use rusqlite::Connection;

/// Shared application state. The SQLite connection is the single source of
/// truth for the UI (design D2); the frontend never calls the API directly.
pub struct AppState {
    pub db: Mutex<Connection>,
    /// True once the first successful sync has populated counts, so poll-diff
    /// notifications are suppressed for the initial fill (specs/tray-notifications).
    pub first_sync_done: AtomicBool,
    /// User toggle for OS notifications.
    pub notifications_enabled: AtomicBool,
    /// Guards against overlapping sync cycles (manual refresh + timer).
    pub syncing: AtomicBool,
}

impl AppState {
    pub fn new(db: Connection) -> Self {
        Self {
            db: Mutex::new(db),
            first_sync_done: AtomicBool::new(false),
            notifications_enabled: AtomicBool::new(true),
            syncing: AtomicBool::new(false),
        }
    }
}

pub const TRAY_ID: &str = "mc-tray";
pub const POPOVER_LABEL: &str = "popover";
pub const MAIN_LABEL: &str = "main";
