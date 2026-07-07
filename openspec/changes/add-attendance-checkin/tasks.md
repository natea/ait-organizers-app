# Tasks: add-attendance-checkin

## 1. Backend — API write path

- [x] 1.1 Add a `mark_attended(rsvp_ref)` write method to `api.rs` (first POST; `POST /api/agents/v1/rsvps/mark_attended` with body `{ rsvp_ref }`), returning the `{ ok, data, error{ code } }` envelope
- [x] 1.2 Route the write through the shared mutation guardrail (explicit-confirmation contract + audit) so no read path can POST
- [x] 1.3 Map responses: `200` → success, `403 forbidden_scope|forbidden_role|forbidden_api_group` → hard deny, `429 rate_limited` → retryable with `Retry-After`/`error.details.retry_after`
- [x] 1.4 Add/confirm attendee-list read: `rsvp_search` filtered by `meetup_token` (per-RSVP `rsvp_ref`, `checked_in`, `checked_in_at`, `registrant_status*`)
- [x] 1.5 Add/confirm live count read: `rsvps/summary` with `status=checked_in` for the event

## 2. Backend — cache & offline action queue (db.rs)

- [x] 2.1 Add a `checkin_attendees` cache table (or reuse an rsvp cache) keyed by `meetup_token` + `rsvp_ref` storing name, status, `checked_in`, `checked_in_at`, raw_json
- [x] 2.2 Add an `action_queue` table `(id, kind, rsvp_ref, meetup_token, client_token, created_at, status, attempts, last_error)` created in `db::init` (CREATE TABLE IF NOT EXISTS)
- [x] 2.3 Add `enqueue_action` with pre-enqueue dedupe: skip if the RSVP is already `checked_in` or an unsent queue row exists for that `rsvp_ref`; mint a stable `client_token`
- [x] 2.4 Add `pending_actions` / `mark_action_sent` / `mark_action_failed` helpers
- [x] 2.5 Add a query for unsynced check-in count per event (to adjust the optimistic live count)

## 3. Backend — sync flush (sync.rs)

- [x] 3.1 Add `flush_action_queue` that pops pending rows, POSTs `mark_attended`, and marks them sent on success (including "already attended" as success)
- [x] 3.2 Call flush from `run_cycle` and manual refresh, and opportunistically right after enqueue when online
- [x] 3.3 On `429`, keep rows pending and reuse `apply_backoff`/`in_backoff` (exponential backoff + jitter) instead of failing
- [x] 3.4 On `403 forbidden_*`, mark the row failed with `last_error` and do not retry
- [x] 3.5 Refresh the `status=checked_in` summary after a successful flush so the live count reconciles

## 4. Backend — commands (commands.rs)

- [x] 4.1 Add `get_checkin_attendees(meetup_token)` returning the cached list for the live/next event (reuse `next_event_json` selection when no token given)
- [x] 4.2 Add `checkin_attendee(rsvp_ref, meetup_token)` command: enqueue the action, return the optimistic new state
- [x] 4.3 Add `get_checkin_count(meetup_token)` returning server `checked_in` + unsynced local count
- [x] 4.4 Register the new commands in the Tauri handler

## 5. Frontend — check-in screen

- [x] 5.1 Add a check-in screen under `src/screens` that loads the live/next event's attendee list from the cache
- [x] 5.2 Render each attendee row with a one-tap "mark attended" control and per-row checked-in / pending-sync / failed states
- [x] 5.3 Show the live checked-in-vs-attending progress figure, incremented optimistically on tap
- [x] 5.4 Optimistic update on tap → call `checkin_attendee`; revert the row on a `forbidden_*` hard deny and show a non-retrying message
- [x] 5.5 Degrade controls to disabled with an explanatory notice when the event returns a `forbidden_*` deny; add offline/empty/loading states
- [x] 5.6 Add attendee/check-in/queue-state fields to `types.ts`
- [x] 5.7 Add check-in screen styles to `styles.css` per `design/DESIGN.md` (row control, pending/failed badges, progress bar)

## 6. Verification

- [x] 6.1 `bunx tsc --noEmit` clean
- [x] 6.2 `cargo build` clean and `cargo test` for enqueue dedupe / idempotency and queue persistence
- [x] 6.3 Drive the flow in-browser with mocked Tauri IPC using real `rsvp_search` / `rsvps/summary` shapes: list loads, one-tap check-in, live count increments
- [x] 6.4 Offline drill: check in with the network down → queued + optimistic; restore network → flush marks sent; verify no double check-in on double-tap and across restart
- [x] 6.5 Mock-drive a `403 forbidden_scope` and a `429 rate_limited` to confirm hard-deny revert and backoff-retry behavior
