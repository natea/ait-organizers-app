# Design: add-attendance-checkin

## Context

Builds on the shipped `add-event-mission-control-v1` app and its `add-past-events-tab`
extension. Mission Control today is strictly READ-ONLY: `api.rs` only issues GET
requests, screens render from the SQLite cache in `db.rs`, and `sync.rs` polls
on a fixed cadence. The app already fetches `rsvps/summary` and a checked-in count
(`api.rs::rsvp_checked_in_count`) to *report* attendance on the Past tab, but it
cannot *record* a check-in.

Door check-in is the highest-frequency day-of workflow. This change adds the first
attendance WRITE — `rsvps/mark_attended` — and the at-the-door screen that drives
it. Verified against `openapi/openapi.yaml`:

- `POST /api/agents/v1/rsvps/mark_attended` — body `{ rsvp_ref }` or `{ rsvp_id }`;
  sets `confirmed_at` and appends a `rsvp_status_history_list` audit entry.
  Returns the standard `{ ok, data, error{ code } }` envelope; can return `403`
  (`forbidden_scope` / `forbidden_role` / `forbidden_api_group`) and `429`
  (`rate_limited`, with `Retry-After` / `error.details.retry_after`).
- `GET /api/agents/v1/rsvps/rsvp_search` — per-RSVP `checked_in` boolean,
  `checked_in_at`, `rsvp_ref`/token, `registrant_status*` fields. Filter by
  `meetup_token` and `status`.
- `GET /api/agents/v1/rsvps/summary` — grouped counts using the same filters;
  `status=checked_in` yields the live checked-in total without paging rows.

The door is a hostile network environment (venue Wi-Fi, cellular dead zones), so
the two hard requirements are SPEED (one tap to check someone in) and OFFLINE
TOLERANCE (a check-in must never be lost because the network blipped).

## Goals / Non-Goals

**Goals:**
- A day-of check-in screen for the live/next event: searchable attendee list with
  one-tap "mark attended" and a live checked-in-vs-attending progress figure.
- Record door check-ins via `rsvps/mark_attended`, reusing the write guardrail
  established by `add-rsvp-screening` (explicit confirmation for mutations +
  audit trail).
- Queue check-ins locally when offline or rate-limited and flush them to the API
  when connectivity returns — with idempotency so no attendee is checked in twice.

**Non-Goals:**
- QR-code scanning / camera capture (this is the manual-lookup path the API
  description calls out: "arrived late, missed QR scan").
- Un-check-in / reversal of an attendance mark (mark_attended is additive only;
  corrections stay in the web UI).
- Editing RSVP state (promote/waitlist/decline) — that is `add-rsvp-screening`.
- Multi-device conflict resolution beyond server-side idempotency (see Risks).

## Decisions

**D1 — One-tap mark-attended, list scoped to the live/next event.**
The screen loads the attendee list for a single event — the currently-live event,
falling back to the next upcoming event (reuse `sync::next_event_json`'s
selection). Attendees come from `rsvp_search` filtered by that `meetup_token`,
cached so the list is available offline. Each row shows name, `registrant_status`,
and a check-in control that is a single tap. Tapping issues one confirmed
`mark_attended` for that RSVP — no multi-step form, because speed at the door is
the whole point. Alternative (a bulk multi-select flow) was rejected as the
primary path: check-ins arrive one attendee at a time as people walk in.

**D2 — Write guardrail reuse (confirmation + audit), tuned for at-the-door speed.**
This reuses the same mutation guardrail as `add-rsvp-screening`: writes require
explicit user confirmation and produce an audit trail. Because a per-tap modal
would defeat door speed, the confirmation is expressed as a deliberate,
unambiguous check-in action (a distinct control that flips the row to a pending
"checking in…" state) rather than a blocking dialog, and the app keeps a local
audit log of every queued/sent mark_attended (rsvp_ref, event, timestamp, result)
in addition to the server-side `rsvp_status_history_list` entry the endpoint
writes. `api.rs` gains its first write method; the guardrail lives at that
boundary so no other read path can accidentally mutate.

**D3 — Offline action queue in SQLite, flushed by sync.rs.**
Add an `action_queue` table in `db.rs` holding pending mutations:
`(id, kind, rsvp_ref, meetup_token, client_token, created_at, status, attempts, last_error)`.
A door tap ALWAYS writes to this queue first and immediately reflects the row as
checked-in in the UI (optimistic), whether or not the network is up. `sync.rs`
gains a `flush_action_queue` step (run on each cycle and on manual refresh, and
opportunistically right after enqueue when online) that pops pending rows,
POSTs `mark_attended`, and marks them `sent` on success. This is the same
cache-first pattern the rest of the app uses, extended to writes. Alternative
(direct synchronous POST on tap, queue only on failure) was rejected: it makes
the offline path a special case instead of the default, which is exactly the
case that must never drop a check-in.

**D4 — Idempotency via a stable client token; server-side dedupe on RSVP state.**
Each queued action carries a `client_token` (UUID minted at tap time). The flush
is safe to retry because `mark_attended` is idempotent on the target: an RSVP
already `checked_in` stays checked in. The app additionally guards locally —
before enqueueing, if the cached RSVP is already `checked_in` (or an unsent queue
row exists for that `rsvp_ref`), it does not enqueue a duplicate. On flush, a
success OR a response indicating the RSVP was already attended both resolve the
queue row to `sent`. This gives no-double-check-in across app restarts, flaky
retries, and double-taps.

**D5 — Live checked-in count reuses `rsvps/summary status=checked_in`.**
The progress figure (checked-in vs attending) reuses the corrected
`rsvps/summary` count with `status=checked_in`, the same source the Past tab uses,
so the door screen and the recap agree. Between summary refreshes, the count is
adjusted optimistically by the number of locally-confirmed-but-unsynced check-ins
so the organizer sees their taps reflected immediately.

**D6 — Rate-limit handling reuses the existing backoff.**
`mark_attended` shares the app's `429` handling: on `rate_limited`, respect
`Retry-After` / `error.details.retry_after` via the existing `apply_backoff` /
`in_backoff` helpers in `sync.rs`. A rate-limited flush leaves the queue row
`pending` (not failed) and retries after the backoff window with exponential
backoff + jitter, so a burst of door taps drains safely instead of hammering.

**D7 — Degradation on forbidden_scope.**
If `mark_attended` returns `403 forbidden_scope` / `forbidden_role` /
`forbidden_api_group`, the check-in is a hard deny (per docs/agents-api.md §"do
not retry through alternate paths"): the queue row is marked `failed`, the UI
reverts that row's optimistic check-in and surfaces a clear, non-retrying
"can't check in — not permitted for your scope" state, and the check-in controls
degrade to disabled with an explanatory notice — mirroring how read screens
degrade on `forbidden_*`.

## Risks / Trade-offs

- **Double check-in from concurrent devices / double-taps** → Mitigated by D4:
  local pre-enqueue dedupe (already-checked-in or existing unsent row), stable
  `client_token`, and server idempotency (already-attended treated as success).
  Worst case is a redundant no-op POST, never a duplicate attendance record.

- **Offline conflict — RSVP state changed server-side while queued** (e.g.
  cancelled between tap and flush) → The flush surfaces the server response; a
  non-idempotent failure (`404`/`400`) marks the row `failed` with `last_error`
  and reverts the optimistic UI, rather than silently succeeding. Attendance is
  never fabricated locally beyond what the server accepts.

- **Optimistic count drift** → The live figure = server `summary` +
  unsynced local check-ins. Once the queue drains and summary refreshes, the two
  reconcile. Transient over/under-count is bounded by the unsent queue depth and
  labeled as live/pending, not final.

- **Lost queue on crash** → The queue is durable in SQLite (not in-memory), so a
  crash or force-quit at the door preserves pending check-ins; they flush on next
  launch.

- **First write path widens blast radius** → Contained by D2: the single write
  method lives behind the confirmation+audit guardrail in `api.rs`; every read
  path stays GET-only.

## Migration Plan

Additive. New `action_queue` table is created by `db::init` (CREATE TABLE IF NOT
EXISTS); existing installs pick it up on next launch with no backfill. No schema
changes to existing tables. Rollback = revert the change; any unsent queue rows
are inert (never read by the read-only screens). No data migration required.

## Open Questions

- Should the audit log be user-visible in-app (a "checked in by you at HH:MM"
  history), or is the server-side `rsvp_status_history_list` sufficient for v1?
- Retention/cleanup policy for `sent` queue rows (prune after N days vs keep as
  the local audit trail).
