//! The write guardrail (specs/rsvp-screening design D2/D3): the first mutation
//! path this app has ever had, and the shared choke point every later write
//! feature (attendance-checkin, speaker-review, networking-connect,
//! media-video-kit) reuses. Every mutation MUST go through a `prepare` →
//! `commit` handshake:
//!
//! - `prepare` binds a short-lived, single-use token to the *exact* intended
//!   mutation payload. No network call happens here.
//! - `commit` is only allowed to proceed (and only then may `commands.rs` call
//!   into `api.rs`) if the caller presents that same token together with an
//!   identical payload. An unknown, expired, already-consumed, or
//!   payload-mismatched (tampered) token is rejected with
//!   `AppError::ConfirmationRequired` — no exceptions, no bypass.
//!
//! This is enforced here in Rust, not in the UI: a confirm-dialog bug or a
//! future caller that forgets to render one still cannot cause a mutation
//! without a valid token bound to that specific change.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::error::{AppError, AppResult};

/// How long a prepared confirmation stays valid before the caller must
/// re-prepare (short-lived per design D2).
const TOKEN_TTL: Duration = Duration::from_secs(300);

/// Hard per-call ceiling for bulk mutations (design D4). Selections larger
/// than this MUST be chunked by the caller into separately confirmed batches.
pub const BULK_CEILING: usize = 100;

struct Pending {
    payload_hash: String,
    expires_at: Instant,
    consumed: bool,
}

/// In-memory registry of prepared-but-not-yet-committed confirmations.
/// Deliberately not persisted to SQLite — these are short-lived handshake
/// state, not the audit trail (that's `write_audit`, which is durable).
#[derive(Default)]
pub struct WriteGuard {
    pending: Mutex<HashMap<String, Pending>>,
}

fn hash_payload(payload: &Value) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    payload.to_string().hash(&mut h);
    format!("{:x}", h.finish())
}

fn new_token() -> String {
    format!("wg_{:x}{:x}", rand::random::<u64>(), rand::random::<u32>())
}

impl WriteGuard {
    /// Bind a new confirmation token to the exact mutation payload the caller
    /// intends to commit. The frontend must echo the identical payload back
    /// to `commit` — any change (including selection order/contents) yields a
    /// different hash and is rejected as tampered.
    pub fn prepare(&self, payload: &Value) -> String {
        let token = new_token();
        self.pending.lock().unwrap().insert(
            token.clone(),
            Pending {
                payload_hash: hash_payload(payload),
                expires_at: Instant::now() + TOKEN_TTL,
                consumed: false,
            },
        );
        token
    }

    /// Validate and consume a token for this exact payload. The token is
    /// consumed on any *valid* commit attempt (payload matched, not expired,
    /// not already used) regardless of what the caller does with the mutation
    /// afterward — a write that may have partially applied must never be
    /// silently replayable by re-presenting the same token (design D6).
    pub fn commit(&self, token: &str, payload: &Value) -> AppResult<()> {
        let mut map = self.pending.lock().unwrap();
        let Some(entry) = map.get_mut(token) else {
            return Err(AppError::ConfirmationRequired(
                "Unknown confirmation token — please re-confirm.".into(),
            ));
        };
        if entry.consumed {
            return Err(AppError::ConfirmationRequired(
                "This confirmation was already used — please re-confirm.".into(),
            ));
        }
        if Instant::now() > entry.expires_at {
            map.remove(token);
            return Err(AppError::ConfirmationRequired(
                "Confirmation expired — please re-confirm.".into(),
            ));
        }
        if entry.payload_hash != hash_payload(payload) {
            return Err(AppError::ConfirmationRequired(
                "The mutation no longer matches what was confirmed — please re-confirm.".into(),
            ));
        }
        entry.consumed = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn prepare_then_commit_with_matching_payload_succeeds() {
        let g = WriteGuard::default();
        let payload = json!({ "action": "rsvp_state_update", "rsvp_ref": "r1", "state": "attending" });
        let token = g.prepare(&payload);
        assert!(g.commit(&token, &payload).is_ok());
    }

    #[test]
    fn unknown_token_is_rejected() {
        let g = WriteGuard::default();
        let payload = json!({ "action": "rsvp_state_update", "rsvp_ref": "r1", "state": "attending" });
        let err = g.commit("not-a-real-token", &payload).unwrap_err();
        assert_eq!(err.code(), "confirmation_required");
    }

    #[test]
    fn reused_token_is_rejected() {
        let g = WriteGuard::default();
        let payload = json!({ "action": "rsvp_state_update", "rsvp_ref": "r1", "state": "denied" });
        let token = g.prepare(&payload);
        assert!(g.commit(&token, &payload).is_ok());
        let err = g.commit(&token, &payload).unwrap_err();
        assert_eq!(err.code(), "confirmation_required");
    }

    #[test]
    fn tampered_payload_is_rejected() {
        let g = WriteGuard::default();
        let payload = json!({ "action": "rsvp_state_update", "rsvp_ref": "r1", "state": "attending" });
        let token = g.prepare(&payload);
        let tampered = json!({ "action": "rsvp_state_update", "rsvp_ref": "r1", "state": "denied" });
        let err = g.commit(&token, &tampered).unwrap_err();
        assert_eq!(err.code(), "confirmation_required");
        // The original, un-tampered payload must still be usable — a rejected
        // tamper attempt must not burn the legitimate confirmation.
        assert!(g.commit(&token, &payload).is_ok());
    }

    #[test]
    fn expired_token_is_rejected() {
        let g = WriteGuard::default();
        let payload = json!({ "action": "rsvp_state_update", "rsvp_ref": "r1", "state": "attending" });
        let token = new_token();
        g.pending.lock().unwrap().insert(
            token.clone(),
            Pending {
                payload_hash: hash_payload(&payload),
                expires_at: Instant::now() - Duration::from_secs(1),
                consumed: false,
            },
        );
        let err = g.commit(&token, &payload).unwrap_err();
        assert_eq!(err.code(), "confirmation_required");
    }

    #[test]
    fn bulk_selection_over_ceiling_is_a_caller_concern_with_a_stable_constant() {
        // The ceiling itself is enforced in commands.rs before a token is ever
        // prepared; this just guards the constant doesn't silently drift.
        assert_eq!(BULK_CEILING, 100);
    }
}
