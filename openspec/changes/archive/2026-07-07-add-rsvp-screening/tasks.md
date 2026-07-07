## 1. Backend — read path (api.rs, db.rs, sync.rs)

- [x] 1.1 Add typed read methods to `api.rs`: `rsvp_search`, `rsvp_get`, `rsvp_assessment_get`, `rsvp_status_history_list`, `subscriber_score_details_get`, each returning the `{ok,data,error{code}}` envelope with existing typed-error and rate-limit-header handling
- [x] 1.2 Add cache tables/columns in `db.rs` for the attendee list (RSVP row with raw `state`, `registrant_status*` labels, checked_in, score), assessment, status history, and subscriber score detail, keyed by event and rsvp_ref
- [x] 1.3 Add upsert/query functions in `db.rs` for the new tables (per-event list read, per-rsvp detail read)
- [x] 1.4 Extend `sync.rs` to poll the read endpoints for the selected event and populate the new cache tables, marking `forbidden_*`/unavailable sections without blocking the rest of the sync

## 2. Backend — write guardrail (db.rs, api.rs, commands.rs, sync.rs)

- [x] 2.1 Add the append-only `write_audit` table in `db.rs` (timestamp, actor, action, target rsvp_ref(s), from_state, to_state, send_email, confirmed, outcome, error_code) with insert + outcome-update functions
- [x] 2.2 Add `post_json` plus typed `rsvp_state_update` and `rsvp_bulk_state_update` methods to `api.rs` (the only non-GET methods); build the bulk body from a typed `{ rsvp_refs, state, send_email, note }` struct
- [x] 2.3 Add `rsvp_state_update` Tauri command in `commands.rs` that refuses to call the API unless `confirmed = true` (returns `confirmation_required` otherwise), writes an `attempted` audit row before the call, and updates it with the outcome after
- [x] 2.4 Add `rsvp_bulk_state_update` Tauri command enforcing the same confirmation gate, a materialized/enumerated selection, and the per-call ceiling with chunking
- [x] 2.5 Handle write responses in the command layer: on `forbidden_*` abort with no cache change + audit the denial; on `429`/`rate_limited` do not auto-retry, surface `Retry-After`, audit the outcome
- [x] 2.6 Add a priority post-write refresh in `sync.rs` to re-read affected rsvp_ref(s)/event immediately after a successful mutation so the cache converges

## 3. Frontend — attendee screen (types.ts, screens, styles.css)

- [x] 3.1 Add TypeScript types in `src/types.ts` for the RSVP list row, assessment, status history, score detail, mutation commands, confirmation payload, and audit entry
- [x] 3.2 Add `src/api.ts` wrappers that invoke the read and (confirmation-carrying) write Tauri commands
- [x] 3.3 Add a new attendee-management screen under `src/screens/` rendering the searchable list from cache with registrant-facing status labels, assessment, score, and per-registrant detail (history + score breakdown)
- [x] 3.4 Implement single actions (promote/waitlist/decline) with a confirmation dialog that summarizes affected registrant, from→to state, registrant-facing effect, and the visible `send_email` toggle before invoking the write with `confirmed = true`
- [x] 3.5 Implement bulk triage over an explicit selection with an enumerated, count-confirmed dialog and ceiling/chunking behavior
- [x] 3.6 Implement optimistic "pending" state that settles on cache re-read and snaps to the authoritative cached value on divergence
- [x] 3.7 Implement degradation states (non-blocking "not available" for forbidden/unavailable reads) and surface write denials / rate-limit windows to the user
- [x] 3.8 Add styles in `src/styles.css` per `design/DESIGN.md` for the list, detail, confirmation dialog, and pending/degraded/denied states

## 4. Verification

- [x] 4.1 `tsc` passes with no type errors
- [x] 4.2 `cargo build` succeeds
- [x] 4.3 `cargo test` passes, including tests for the confirmation gate (unconfirmed mutation performs no call), audit before/after write, and forbidden/rate-limited write handling
- [x] 4.4 Mock-drive the write path end-to-end against the dev/mock API: confirm a single promote, a bulk decline with chunking, a forbidden denial, and a rate-limited response, verifying audit rows and cache reconciliation
- [x] 4.5 Run `openspec validate add-rsvp-screening` and confirm it passes
