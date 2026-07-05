use crate::error::{AppError, AppResult};

/// The API key lives only in the OS keychain (specs/api-auth). It never
/// touches config files, logs, or the JS side after onboarding entry.
const SERVICE: &str = "org.aitinkerers.missioncontrol";
const ACCOUNT: &str = "agents-api-key";

fn entry() -> AppResult<keyring::Entry> {
    keyring::Entry::new(SERVICE, ACCOUNT).map_err(AppError::from)
}

pub fn store_key(key: &str) -> AppResult<()> {
    entry()?.set_password(key).map_err(AppError::from)
}

/// Returns the stored key, or `None` when onboarding has not completed.
pub fn get_key() -> AppResult<Option<String>> {
    match entry()?.get_password() {
        Ok(k) => Ok(Some(k)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(AppError::from(e)),
    }
}

pub fn delete_key() -> AppResult<()> {
    match entry()?.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(AppError::from(e)),
    }
}
