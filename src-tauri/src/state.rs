use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

use rusqlite::Connection;

use crate::error::AppResult;
use crate::keychain;
use crate::write_guard::WriteGuard;

/// Shared application state. The SQLite connection is the single source of
/// truth for the UI (design D2); the frontend never calls the API directly.
pub struct AppState {
    pub db: Mutex<Connection>,
    /// In-memory copy of the API key. The keychain remains the durable store,
    /// but it is read at most once per launch and cached here so background
    /// sync (which builds a client on every API call) doesn't re-hit the
    /// keychain — that repeated access is what triggers the macOS "allow"
    /// prompt over and over on unsigned/dev builds.
    api_key: Mutex<Option<String>>,
    /// True once the first successful sync has populated counts, so poll-diff
    /// notifications are suppressed for the initial fill (specs/tray-notifications).
    pub first_sync_done: AtomicBool,
    /// User toggle for OS notifications.
    pub notifications_enabled: AtomicBool,
    /// Guards against overlapping sync cycles (manual refresh + timer).
    pub syncing: AtomicBool,
    /// In-flight promotion-generation tasks, keyed by job id, so
    /// `promotion_cancel` can abort the background request (specs/promotion-tools,
    /// design D5). Entries are removed once the task finishes or is cancelled.
    pub promo_jobs: Mutex<HashMap<String, tauri::async_runtime::JoinHandle<()>>>,
    /// In-flight sponsor research/pitch generation tasks, keyed by job id
    /// (specs/sponsor-tools) — the same cancellable-job pattern as
    /// `promo_jobs`, kept as a separate registry since the two features have
    /// independent job tables and id spaces.
    pub sponsor_jobs: Mutex<HashMap<String, tauri::async_runtime::JoinHandle<()>>>,
    /// The write guardrail (specs/rsvp-screening design D2/D3) — the shared
    /// prepare/commit confirmation gate every write feature reuses.
    pub write_guard: WriteGuard,
}

impl AppState {
    pub fn new(db: Connection) -> Self {
        Self {
            db: Mutex::new(db),
            api_key: Mutex::new(None),
            first_sync_done: AtomicBool::new(false),
            notifications_enabled: AtomicBool::new(true),
            syncing: AtomicBool::new(false),
            promo_jobs: Mutex::new(HashMap::new()),
            sponsor_jobs: Mutex::new(HashMap::new()),
            write_guard: WriteGuard::default(),
        }
    }

    /// The API key, from the in-memory cache; on a miss, read the keychain
    /// exactly once and cache it. Returns `None` before onboarding.
    pub fn api_key_cached(&self) -> AppResult<Option<String>> {
        {
            let cache = self.api_key.lock().unwrap();
            if cache.is_some() {
                return Ok(cache.clone());
            }
        }
        // Cache miss — one keychain read, then memoize (may be the only
        // keychain access of the whole session).
        let key = keychain::get_key()?;
        *self.api_key.lock().unwrap() = key.clone();
        Ok(key)
    }

    /// Update the cached key (after onboarding stores it) or clear it (sign-out).
    pub fn set_api_key(&self, key: Option<String>) {
        *self.api_key.lock().unwrap() = key;
    }
}

pub const TRAY_ID: &str = "mc-tray";
pub const POPOVER_LABEL: &str = "popover";
pub const MAIN_LABEL: &str = "main";
