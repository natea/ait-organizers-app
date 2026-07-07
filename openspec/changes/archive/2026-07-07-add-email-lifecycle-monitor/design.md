# Design: add-email-lifecycle-monitor

## Context

Mission Control is a read-only Tauri desktop app for AI Tinkerers city
organizers. Screens render only from the SQLite cache (`db.rs`) via Tauri
commands (`commands.rs`); the network layer (`api.rs`) is the sole caller of the
Agents API, and `sync.rs` schedules polling with rate-header tracking and 429
backoff. Errors are typed in `error.rs`, and API-group / scope / role denials
already collapse to `is_capability_block()` so a feature can go dark cleanly.

Event email is the operational backbone of a chapter, yet organizers have no
in-app visibility into send health. When an announcement goes out they want to
watch delivery, opens, clicks, and suppression, and catch a stuck or failing
send before it hurts an event. This change adds an Email panel per event plus a
chapter-level deliverability view, sourced from seven read-only endpoints:
`email_send_jobs_summary`, `email_send_jobs_list`, `email_send_job_get`,
`email_send_job_throughput_get`, `email_campaign_performance_get`,
`email_deliverability_health_get`, and `email_fatigue_risk_get`.

All seven are gated by the `subscribers_sponsors` API group and, per the
authorization matrix in `docs/agents-api.md`, are restricted to city owners
(index owners included); city-series owners are not authorized for send-job,
deliverability, or fatigue endpoints. The app therefore treats the whole panel
as a city-owner + `subscribers_sponsors` capability and degrades gracefully when
either is absent.

## Goals / Non-Goals

**Goals:**
- A per-event Email panel: send-job status (sent/pending/suppressed), delivery
  accounting, throughput over time, and open/click performance for that event.
- A chapter-level deliverability view: sender-domain health, fatigue-risk
  segments, and recent send jobs across the caller's weblog scope.
- Reuse the existing cache-only render, typed-error, rate-tracking, and backoff
  machinery; poll gently given per-endpoint rate limits.
- Degrade cleanly (informative, non-alarming empty state) when
  `subscribers_sponsors` is disabled or the caller lacks city-owner scope.

**Non-Goals:**
- Any composing, sending, scheduling, retrying, or mutation of email — this
  change is read-only monitoring only.
- Per-recipient PII surfaces (recipient lists, individual email addresses).
  `email_send_job_recipients_list` and body/recipient fields are out of scope.
- SES system dashboards (`email_send_job_ses_status_get` is index-owner only).
- Series-owner support for the gated endpoints.

## Decisions

**D1 — Two data surfaces, one capability gate.**
The panel splits into an event-scoped view (driven by `meetup_token`:
`email_send_jobs_summary`, `email_campaign_performance_get`, and — for a selected
job — `email_send_job_get` + `email_send_job_throughput_get`) and a chapter-scoped
view (`email_send_jobs_list`, `email_deliverability_health_get`,
`email_fatigue_risk_get`). Both share a single gate: the `subscribers_sponsors`
group must be enabled and the caller must be a city owner. We reuse
`AppError::ForbiddenApiGroup` / `ForbiddenScope` and the existing
`is_capability_block()` so a denial marks the feature blocked and halts polling,
identical to how performance degradation already works in `sync.rs`. Alternative
(gate each endpoint independently) rejected: it multiplies dark-state UI for a
group that is enabled or disabled as a unit.

**D2 — Cache-only render, following the app contract.**
`api.rs` gains one method per endpoint (POST with params in the JSON body, key
via Authorization header only). `sync.rs` fetches and upserts into new `db.rs`
tables; `commands.rs` exposes read commands the new screen calls. The screen
never calls the network directly — it renders whatever is cached, so it works
offline and after a capability block. Mirrors the `add-past-events-tab` and
mission-control patterns already in the repo.

**D3 — Gentle, tiered polling under the documented rate limits.**
Documented limits: send-job endpoints and `email_campaign_performance_get` /
`email_deliverability_health_get` at 20 rpm, `email_fatigue_risk_get` at 15 rpm.
Chapter-level deliverability and fatigue data are slow-moving, so they are
fetched on app launch and manual refresh only (not the 2-minute loop), like past
events. Event send-job status/throughput is only polled while an event's Email
panel is open AND a job is active (status `queued`/`sending`/`active`), on a
gentle cadence (e.g. 30–60s), and stops once all jobs are `completed`/`failed`.
We keep honoring `x-ratelimit-*` headers and 429 `retry_after` backoff via the
existing `record_rate` / `apply_backoff` machinery. Alternative (poll everything
on the main loop) rejected: it would burn the shared rate budget for data that
rarely changes.

**D4 — Active-send throughput is the only "live" surface.**
`email_send_job_get` exposes `send_progress` (observed rate, predicted finish)
and `email_send_job_throughput_get` returns per-bucket `sent_count` with peak /
average rates. These are the one place near-real-time polling earns its cost —
they let an organizer watch a send drain and spot a stall. Once a job is done we
freeze its last snapshot in cache and stop polling it (like frozen past events).

**D5 — Aggregates only; no PII.**
We store and render aggregate counts and rates only: `summary.*_count`,
`status_counts`, campaign `sends/delivered/opens/clicks/bounces/unsubscribes`
and their rates, `sender_domains[]` health, and fatigue **tier summary** counts.
Per-subscriber fatigue rows and any recipient email addresses are intentionally
not cached or shown, keeping the app's read-only, low-PII posture. This also
sidesteps the body/recipient redaction rules on `email_send_job_get`.

**D6 — Degradation copy is specific, not an error.**
When the gate fails, the Email panel shows a plain explanation ("Email
monitoring needs the subscribers group and city-owner access on your key")
rather than a red error, consistent with `onboarding.ts` group chips and the
app's existing graceful-degradation stance. A `truncated` flag on list/fatigue
responses surfaces a "showing recent N" notice so we never imply completeness.

## Risks / Trade-offs

- [Polling active sends adds calls against a shared 20 rpm budget] → only poll
  when the panel is open and a job is active; stop on completion; reuse header
  tracking + 429 backoff so email polling yields to other features.
- [`subscribers_sponsors` disabled or non-city-owner key → whole panel dark] →
  detect via `is_capability_block()`, show specific non-alarming copy, and stop
  re-polling that surface until identity/scope changes (background-sync pattern).
- [Deliverability/fatigue are chapter-wide and can be large] → request bounded
  `limit`, store only aggregate/tier summaries, honor `truncated`, and label the
  view as a recent window, not full history.
- [Campaign_performance permits city-series owners but other endpoints do not] →
  gate the entire panel on the strictest requirement (city owner) so we never
  render a half-authorized surface; document that series owners see the dark
  state.
- [Stale cached counts imply a send is still moving] → store `fetched_at` per
  row, show it, and freeze completed jobs so the UI distinguishes live vs frozen.

## Migration Plan

Additive only: new `api.rs` methods, new `db.rs` tables (created via the existing
schema-init path), new `commands.rs` commands, and a new screen + types + styles.
No existing table or command changes, so no data migration. Rollback is removing
the screen registration and the new tables; the rest of the app is unaffected.

## Open Questions

- Active-send poll cadence (30s vs 60s) — pick the gentlest value that still
  feels live during verification.
- Chapter deliverability scope selector (`weblog_token` vs `city`) when a key
  owns multiple weblogs — default to the caller's primary weblog for v1.
