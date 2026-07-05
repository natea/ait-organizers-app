## Context

Mission Control is a Tauri 2 desktop app for AI Tinkerers city organizers. Every
screen today renders exclusively from the local SQLite cache that `sync.rs`
populates by polling read-only Agents API endpoints; `api.rs` only ever issues
`GET` requests, and the app advertises a "no write endpoints" posture. RSVP
screening — reviewing registrants and promoting / waitlisting / declining them —
is the single most-used organizer workflow (108k RSVP state-change events and
2,151 screening-alert clicks network-wide), and the app cannot do it at all.

This change adds the first attendee-management screen and, with it, the **first
write-capable feature in the app's history**. Introducing mutations to an app
built entirely around a read cache is the central design problem: writes must not
silently diverge from what the user sees, must never fire without an explicit
human decision, and must leave a durable record of what was changed and by whom.

Relevant endpoints (see `openapi/openapi.yaml`, all under
`/api/agents/v1/`, Bearer auth, `{ok,data,error{code}}` envelope):

- Read: `rsvp_search`, `rsvp_get`, `rsvp_assessment_get`,
  `rsvp_status_history_list`, `subscriber_score_details_get`.
- Write: `rsvps/state_update` (single; `state ∈ {registered, attending,
  waitlisted, denied}`, `send_email` default true, optional `note`),
  `rsvps/bulk_state_update` (permissive body, no registered MCP schema).

Key API semantics that shape the design:
- `rsvp_search` returns raw `rsvp.state` for internal decisions **and** separate
  `registrant_status` / `registrant_status_label` / `registrant_status_text`
  fields for registrant-facing display; internal `denied` is surfaced externally
  as "waitlisted". The UI MUST show registrant-facing labels to the user but key
  mutations off raw state.
- Authorization is city-owner scope, owner-only actions. Denials arrive as
  `forbidden_scope` (in scope-role, wrong resource), `forbidden_role` (role not
  eligible), or `forbidden_api_group` (group disabled). All three are hard denies.
- Rate limits: HTTP 429 + `error.code = "rate_limited"`, with `Retry-After` and
  `X-RateLimit-*` headers already parsed by `api.rs`.

## Goals / Non-Goals

**Goals:**
- A searchable per-event attendee list rendered from the cache, showing each
  registrant's AI assessment, engagement score, current status, and status
  history.
- Single-RSVP state changes (promote → `attending`, waitlist → `waitlisted`,
  decline → `denied`) and bulk triage across a selection.
- A write guardrail that is an explicit, opt-in departure from the read-only
  posture: mandatory user confirmation before any mutation, a durable audit
  trail of every attempted write, and optimistic UI reconciled against the cache.
- Graceful, non-blocking degradation when authorization or rate limits block a
  read or a write.

**Non-Goals:**
- Sending custom emails, editing talk content, or any mutation beyond RSVP state
  (`send_email` toggles the *standard* status email only).
- Auto-triage, AI-driven auto-decisions, or acting without a human in the loop.
- Making writes the default posture for the rest of the app; other screens stay
  read-only. This change adds a write *path*, not a write *default*.
- Offline queuing of mutations for later replay (writes require live connectivity;
  see Open Questions).

## Decisions

### D1. Writes go through a dedicated, explicitly-gated command path in `commands.rs`

Read data continues to flow cache → screen. Writes are a **separate, opt-in
lane**: the frontend never calls the API directly; it invokes new Tauri commands
(`rsvp_state_update`, `rsvp_bulk_state_update`) that are the *only* place `api.rs`
issues a non-GET request. `api.rs` gains `post_json` plus typed `rsvp_state_update`
/ `rsvp_bulk_state_update` methods; every other method stays GET-only, preserving
the "reads can't mutate" invariant at the type level.

*Why not let screens reconcile writes themselves?* Centralizing mutation in
commands keeps the guardrail (confirm gate + audit write) impossible to bypass
from the UI and keeps `api.rs` the single mutation choke point.

### D2. Mandatory confirmation gate, enforced in the backend, not just the UI

A mutation command MUST NOT reach `api.rs` unless the call carries an explicit
confirmation token/flag that the command layer validates. The frontend renders a
confirmation dialog summarizing the exact change (who, from-state → to-state,
whether an email will send, and for bulk the full affected count) and only on
user approval re-invokes the command with `confirmed = true`. A command invoked
without confirmation returns a `confirmation_required` error and performs no
network call. Enforcing this server-side (in the Rust command) means a UI bug or
a future caller cannot skip the gate.

*Why backend-enforced?* The whole point of the guardrail is that no code path
mutates without a human decision; a UI-only check is one refactor away from being
bypassed.

### D3. Audit trail written before and after every mutation attempt

`db.rs` gains an append-only `write_audit` table (id, timestamp, actor/api-key
fingerprint, action, target rsvp_ref(s), from_state, to_state, send_email,
confirmation flag, request outcome, error_code). The command writes an
`attempted` row *before* the API call and updates it with the `outcome` (success
/ error code) *after*, so even a crash mid-call leaves evidence. Because the API
side also records status changes (visible via `rsvp_status_history_list`), the
local audit is the *client-side* record of intent; the server history is the
authoritative record of effect. The attendee screen surfaces both.

*Why append-only + local?* The read cache is derived and disposable; the audit
trail is not. It must survive cache rebuilds and record attempts the server never
saw (denied, rate-limited, network failure).

### D4. Single vs bulk share one code path with a hard bulk ceiling

`rsvp_bulk_state_update` is driven from an explicit selection the user built (no
"select all matching filter" without materializing the list). The confirmation
dialog for bulk shows the exact count and enumerates targets; a configurable
ceiling (e.g. 100 per call) caps a single bulk action, and selections above it
must be chunked, each chunk individually confirmed. Single-RSVP update is the
same path with a one-item selection. The permissive bulk body is constructed by
`api.rs` from a typed Rust struct (`{ rsvp_refs: [...], state, send_email, note }`)
so the client never sends an unbounded/ambiguous payload despite the schema
allowing it.

*Why a ceiling and enumerated selection?* The dominant catastrophic risk is an
accidental mass mutation (a mis-click on "select all", a fat-fingered bulk
decline). Forcing a materialized, bounded, count-confirmed selection makes mass
mutation a deliberate act, not an accident.

### D5. Optimistic UI reconciled against the cache, not trusted

On confirmed mutation the screen may optimistically reflect the new state
(marked "pending"), but the write is only *settled* when the next targeted
re-read (`rsvp_get` for single, `rsvp_search` for the affected event on bulk)
confirms the server state in the cache. `sync.rs` gains a **priority
post-write refresh**: immediately after a successful mutation the command
requests an out-of-band refresh of the affected rsvp_ref(s)/event so the cache
converges in seconds rather than on the next poll cycle. If the re-read disagrees
with the optimistic state, the UI snaps to the cached (authoritative) value and
flags the divergence.

*Why not trust the write response?* The cache is the single source of truth for
rendering everywhere else in the app; letting writes paint UI that the cache
hasn't confirmed would reintroduce exactly the drift the read-only architecture
was built to avoid.

### D6. Rate-limit handling reuses the existing header-aware client

`api.rs` already parses `Retry-After` and `X-RateLimit-*`. Writes honor them: on
`429`/`rate_limited` a mutation is **not** silently retried (retrying a mutation
risks double-application); instead the command surfaces the `Retry-After` to the
UI, records a `rate_limited` audit outcome, and lets the user re-confirm after the
window. Reads (search/history/assessment) may back off and retry automatically
with jitter as they do today. During bulk chunking the client proactively
throttles as `X-RateLimit-Remaining` approaches zero.

*Why no auto-retry on writes?* A mutation that may have partially applied must not
be blindly replayed; a human re-confirm after the cooldown is safer than a
duplicate state change.

### D7. Degradation on forbidden_* and role denials

A `forbidden_scope` / `forbidden_role` / `forbidden_api_group` on a **read**
renders the affected section (assessment, score, history) as a non-blocking "not
available" state while the rest of the screen renders. The same code on a
**write** aborts the mutation, leaves the cache untouched, records the denial in
the audit trail, and shows the user why the action was refused. Denials are hard
— no alternate-path retry (per the API contract).

## Risks / Trade-offs

- **Accidental mass mutation via bulk triage.** → Materialized+enumerated
  selection, per-call ceiling with forced chunking, exact-count confirmation
  dialog, backend-enforced confirmation gate (D2/D4), and a full audit trail (D3)
  so any errant bulk action is at least reconstructable.
- **Optimistic UI diverging from server truth.** → Writes settle only on cache
  re-read; UI snaps to authoritative cache value on disagreement (D5).
- **Unintended registrant emails.** `state_update` sends the standard email by
  default. → The confirmation dialog states explicitly whether an email will be
  sent; `send_email` is surfaced as a visible toggle, defaulting per action but
  never hidden.
- **Showing the wrong status to the wrong audience.** Internal `denied` reads as
  "waitlisted" to registrants. → UI keys mutations off raw `rsvp.state` but
  displays `registrant_status_*` labels, and the confirm dialog names both the
  internal state change and the registrant-facing effect.
- **Double-application on retry after 429 / network error.** → No automatic retry
  of writes; human re-confirm after the rate-limit window (D6), with the audit
  row recording the ambiguous outcome.
- **Guardrail erosion over time.** Adding a write lane invites future features to
  reuse it loosely. → The confirm gate and audit write live in the command layer
  and `api.rs` is the sole mutation choke point, so new writes inherit the
  guardrail by construction (D1).

## Migration Plan

1. Ship read-only additions first: new read methods in `api.rs`, cache tables in
   `db.rs`, sync coverage in `sync.rs`, and the attendee screen rendering from
   cache. At this stage the feature is a richer read-only view — no new risk.
2. Add the `write_audit` table and the confirmation-gated commands, still with the
   write UI disabled, and exercise them against the mock/dev API.
3. Enable the write UI (promote / waitlist / decline, then bulk) behind the
   confirmation gate.
4. **Rollback:** the write path is additive and isolated. Disabling the mutation
   commands (or hiding the write UI) reverts the app to its read-only posture with
   no schema rollback needed; the `write_audit` table is inert when unused.

## Open Questions

- Should denied/rate-limited mutations be queued for later user-driven retry, or
  always require a fresh manual action? (Current design: fresh manual action; no
  queue — see Non-Goals.)
- Exact bulk ceiling (100 assumed) and whether it should be tier-aware from
  `X-RateLimit-Tier`.
- Whether the audit trail should be exportable/surfaced network-wide or remain a
  local, per-installation record.
