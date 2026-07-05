# Design: add-promotion-tools

## Context

The app is a read-only Tauri 2 desktop dashboard for AI Tinkerers city
organizers. Screens render only from the SQLite cache; a Rust poll loop
(`sync.rs`) fetches read endpoints and upserts rows, and the frontend
subscribes to change events. Key handling, HTTP, envelope unwrapping, and
rate-limit pacing already live in `api.rs`/`state.rs`; capability degradation
on `forbidden_*` is already modeled in `sync_state`.

This change adds the first **generation** feature. Unlike the existing read
endpoints, the four promotion endpoints are agent-backed AI writes that are
slow and rate-limited:

- `POST /social_posts/generate` (`social_post_generate`) — per-platform social
  post package. Body: `source_type` (meetup|rsvp|content_page|client|sponsor),
  `source_ref` (required), `platform` (linkedin|x, default linkedin), `goal`
  (promote|recap|spotlight|announce|sponsor_thanks, default promote), optional
  `tone`, `city`.
- `POST /event_promos/generate` (`event_promo_generate`) — launch-ready promo
  package. Body: `meetup_token` (required), `package_type`
  (launch|reminder|final_push|recap|full_campaign, default full_campaign),
  `audience` (general|builders|founders|sponsors|students, default general).
- `POST /meetups/discussion_topics/generate` (`discussion_topics_generate`) —
  moderated discussion topics. Body: `meetup_token` (required).
- `GET /logos/search` (`logo_search`) — brand/logo lookup. Query: `query`
  (required), `scope` (smart_match|library, default smart_match),
  `include_co_branded` (default false), `limit` (default 20, max 25).

All four return the standard `{ok, data, error{code}}` envelope and can
respond `401/403/404/429`. Per the proposal, generation calls can take up to
~25s and are rate-limited, so they must be treated as async kickoffs with
visible progress, and the latest drafts must be cached per event so navigating
away and back does not re-spend a slow, throttled call.

This feature is available to city/series owners and does **not** mutate any
attendee data — outputs are drafts the organizer copies or exports.

## Goals / Non-Goals

**Goals:**
- One Promote panel on event detail that kicks off, tracks, and caches the four
  promotion generations for that event.
- Per-platform social post drafts, an event promo package, and AI discussion
  topics, all persisted per event so the latest result renders instantly from
  cache offline.
- Logo/asset lookup for co-branded promo, surfaced in the same panel.
- Async-safe UX: generation runs as a tracked job with progress, cancellation,
  and clear timeout handling; concurrent duplicate kickoffs are prevented.
- Polite API citizenship: reuse existing rate-limit pacing and `forbidden_*`
  degradation; never poll generation endpoints.

**Non-Goals:**
- Any attendee-data mutation, publishing, or sending (no RSVP/state writes, no
  posting to LinkedIn/X, no email dispatch). Drafts are copy/export only.
- Speaker promo banner image generation (`speaker_promo_*`) and content-page
  banner uploads — separate future changes.
- Editing/versioning drafts inside the app beyond storing the latest result.
- Background/scheduled regeneration — generation is user-initiated only.

## Decisions

**D1 — Generation calls are explicit user-initiated jobs, never on the poll
loop.** The existing `sync.rs` scheduler handles cheap, idempotent reads. The
four promotion endpoints are expensive AI writes billed against tight rate
limits, so they run only when the organizer clicks a Promote action. Modeling
them as poll targets would waste the rate budget and could take ~25s per tick.
Alternative considered: fold into sync with a long interval — rejected; there
is no freshness requirement and it would burn the budget silently.

**D2 — A generation-job model with progress states, separate from `sync_state`.**
Each kickoff creates a job row keyed by `(meetup_token, kind, params_hash)`
with status `pending | running | ready | error | timeout`, a `started_at`, and
the request params. `commands.rs` exposes `promotion_generate(kind, params)`
which starts the request on a background task and immediately returns the job
id; the frontend renders progress from job status change events
(`promotion:job`), mirroring the existing `sync:updated` pattern. Rationale:
the frontend must never block on a 25s call, and the panel needs to show
per-kind progress independently.

**D3 — Cache the latest draft per event per kind in SQLite.** A
`promotion_drafts` table stores `(meetup_token, kind, platform, params_json,
result_json, generated_at)` — the raw envelope `data` is persisted verbatim as
`result_json` so unknown/added fields pass through (consistent with the
existing envelope-passthrough approach). Screens render from this table; a new
generation upserts the row for that `(meetup_token, kind, platform)`. Logo
search results are cached by `(query, scope, include_co_branded)` with a short
freshness window since they are cheap GETs, not billed generations. Rationale:
navigating away and back, or restarting the app, must show the last drafts
without re-spending a throttled call, and the app must render offline.

**D4 — One Promote panel on event detail, per-kind sub-actions.** The panel
lives on the existing event detail screen and offers four actions: Social posts
(with platform + goal selectors), Promo package (with package_type + audience),
Discussion topics, and Logo search (free-text query). Each shows its own
last-generated timestamp, a "Generate"/"Regenerate" button, progress, and the
cached result with a copy-to-clipboard / export affordance. Rationale: promo
work is per-event, so co-locating with event detail removes the tool-hopping
step the proposal calls out.

**D5 — Timeout and cancellation are first-class.** The generation request uses
a client-side timeout above the ~25s server ceiling (30s) distinct from the
6–8s sync read timeout. On timeout the job goes to `timeout` with a retry
affordance; the cached prior draft (if any) stays visible. In-flight jobs can
be cancelled from the UI, which aborts the request and drops the job back to
the last cached state. Rationale: a slow/hung generation must never wedge the
panel or hide a previously good draft.

**D6 — Reuse existing degradation on `forbidden_*`.** If a generation endpoint
returns `forbidden_role`/`forbidden_scope`/`forbidden_api_group`, that specific
promotion action is marked unavailable (chapter not enabled / insufficient
role) and shows a non-blocking "not enabled for your chapter" state, exactly
like existing read features. Other promotion actions and the rest of event
detail keep working. Rate-limited (`429`) is distinct: it is transient, honors
`Retry-After`, and offers retry rather than marking the feature unavailable.

**D7 — Idempotent kickoff / duplicate suppression.** While a job for a given
`(meetup_token, kind, platform)` is `pending`/`running`, the button is disabled
and a second click is a no-op that returns the existing job id. Rationale:
prevents double-spending rate budget on an accidental double click.

## Risks / Trade-offs

- [Generation can take ~25s or exceed it] → D5 async job + 30s client timeout,
  progress UI, cached prior draft stays visible, explicit retry on timeout.
- [Tight rate limits; a burst of clicks could exhaust the budget] → D7 duplicate
  suppression, per-kind disabled state while running, `429` honors `Retry-After`
  with backoff (reusing `api.rs` pacing) instead of hammering.
- [Feature silently varies by chapter (API-group toggles)] → D6 per-action
  degradation with an explicit "not enabled" state, not a hard error.
- [Stale drafts could be mistaken for fresh] → D3 stores and shows
  `generated_at`; UI labels drafts with their generation time and offers
  Regenerate.
- [Draft content is model output and may be off-tone or inaccurate] → outputs
  are clearly labeled drafts, copy/export only, never auto-published or sent;
  no attendee data is written.
- [Envelope/spec drift on new generation fields] → D3 persists raw `data` and
  renders known fields, treating unknown fields as pass-through.

## Migration Plan

Additive only: new `promotion_drafts` and `promotion_jobs` tables (created if
absent on startup, alongside existing schema init), new `api.rs` methods, new
`commands.rs` commands, and a new panel on the existing event detail screen. No
existing table or endpoint changes. Rollback is removing the panel and tables;
no data migration is required since drafts are a regenerable cache.

## Open Questions

- Export format for drafts (plain text vs. markdown vs. per-platform copy
  blocks) — default to copy-to-clipboard of the raw draft text for v1.
- Whether logo search results should be cached across events or scoped per
  event — default to a shared cache keyed by query params (D3).
