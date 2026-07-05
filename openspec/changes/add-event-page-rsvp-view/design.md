# Design: add-event-page-rsvp-view

## Context

Builds on the shipped `add-event-mission-control-v1` app and its detail cache.
The event detail view (`src/screens/detail.ts`) already renders, from the SQLite
cache, an RSVP summary (`rsvp_summary`), event metadata, awaiting-payment rows,
and a `meetup_performance_get` traffic/RSVP trend. The `add-past-events-tab`
change established the RSVP-flow semantics this change reuses: the API's
`registered` field is unreliable, Total is derived from
attending + waitlisted + cancelled, and the true door count comes from
`rsvps/summary` with `status=checked_in` — never from performance "completed
RSVPs".

What is missing is the thing attendees actually see: the public content page
(the rendered event/article body) and a single consolidated RSVP-flow summary
sitting next to it. Network-wide this is the dominant surface, yet organizers must leave the app and open the website to
read their own page. This change adds a read-only event-page panel to the detail
view: rendered public content page body plus a full RSVP funnel and page-traffic
figures, with a deep link to the live URL.

The app is READ-only. All four endpoints are GETs and this change adds no writes.
Frontend screens render exclusively from the SQLite cache via Tauri commands; the
Rust layer owns all HTTP, envelope unwrapping, and the `Authorization: Bearer`
header (`api-auth`). Authorization failures must degrade gracefully to a
"not enabled" state (`background-sync`, `api-auth`), because `content_pages` and
`meetups` are separate API groups from `rsvps` and any of them may be forbidden
for a given key.

## Goals / Non-Goals

**Goals:**
- Surface the rendered public content page body (markdown/HTML) inside the event
  detail, alongside the RSVP-flow summary and page-traffic figures.
- Show the full RSVP flow (registered → attending → waitlisted → cancelled →
  checked-in) with the semantics established in `add-past-events-tab`.
- Deep link to the live event/content page URL.
- Reuse the existing detail cache, `rsvp_summary`, and `meetup_performance_get`
  wiring; add only a content-page fetch + cache.
- Degrade each sub-section independently when its API group/scope is forbidden.

**Non-Goals:**
- Any page editing, publishing, or write behavior (content_page body/theme/og
  endpoints are explicitly out of scope).
- Sanitizing or executing arbitrary remote HTML/JS — the body is rendered as
  trusted-but-inert content (see Risks).
- New polling cadence for content pages beyond the existing detail refresh.
- Pagination or comment threads for the content page.

## Decisions

**D1 — Content page source: `content_page_get`, keyed by content page token.**
Fetch the full visible page (body markdown, plain text, author metadata,
editorial status, live URL) via `content_page_get`. The endpoint accepts a
`content_page_token` (preferred) or a fallback `slug`. Events that have an
associated content page expose that token/slug through the existing event object;
when neither is resolvable the panel shows a "no public page" empty state rather
than calling the endpoint. Chosen over scraping the public URL because it returns
structured body + metadata in the standard envelope and respects key scoping.

**D2 — Page traffic figures: reuse `meetup_performance_get`, add
`content_page_metrics_get` for email metrics.** The detail cache already stores
`meetup_performance_get` (page-view traffic + completed-RSVP conversion) from
`add-event-mission-control-v1`; the event-page panel reuses that cached row for
its traffic figures rather than issuing a new call. `content_page_metrics_get`
(email sends/opens/clicks for the page) is fetched only when a content page token
exists and the metrics scope is enabled, and is treated as an optional
enrichment. This keeps API usage minimal and reuses existing wiring.

**D3 — RSVP flow: one `rsvp_summary` call grouped by status, plus a
`status=checked_in` count.** Render the funnel from `rsvp_summary` with
`group_by=status` (registered/attending/waitlisted/cancelled) and obtain the true
door count from `rsvp_summary` with `status=checked_in` — consistent with
`add-past-events-tab`'s detail recap. Total is derived
(attending + waitlisted + cancelled); a bare "Registered" figure is treated as
unreliable and is not the headline. Conversion (checked-in ÷ attending, or the
performance conversion) is shown only when it is a valid fraction (≤ 100%),
matching the existing "implausible conversion suppressed" rule.

**D4 — Cache a content page alongside the event, keyed by the event.** Add a
`content_pages` cache table (or columns on the detail cache) in `db.rs` keyed by
`meetup_token`, storing body, plain text, author/editorial metadata, live URL,
and a fetched-at timestamp. A `get_event_page` Tauri command in `commands.rs`
returns the cached page + metrics for a given event; `detail.ts` renders only
from that command output. The content page is fetched on detail open / detail
refresh (the existing slow cadence), not on the 2-minute upcoming loop.
Alternative (fetch on every detail render, no cache) rejected — it would break
the "render only from cache" rule and add avoidable API calls.

**D5 — Independent per-section degradation.** `content_pages`, `meetups`, and
`rsvps` are distinct API groups. Each of the three panel sections (page body,
traffic/email metrics, RSVP flow) degrades on its own: a `forbidden_api_group` /
`forbidden_scope` (or availability) error on one endpoint renders a non-blocking
"not enabled" state for that section while the others still render. This follows
the `api-auth` typed-error mapping and the existing performance "not available"
degradation.

**D6 — Body rendering is inert.** The content page body is markdown/HTML authored
on aitinkerers.org. The panel renders it as read-only content with scripts and
remote form submission disabled; the live URL deep link is the escape hatch for
full interactivity. This avoids turning the desktop app into an arbitrary web
runtime.

## Risks / Trade-offs

- [Rendering remote HTML/markdown could execute untrusted script] → render inert
  (no script execution, no active form posting); treat body as display-only and
  send users to the live URL for anything interactive (D6).
- [`content_pages` group forbidden for many organizer keys] → page-body section
  degrades to a "not enabled" state independently; RSVP flow and traffic still
  render (D5). The panel is useful even with only the `rsvps` group.
- [Not every event has a content page] → resolve token/slug first; show a
  "no public page" empty state instead of a failed fetch (D1).
- [`registered` unreliable / conversion > 100%] → derive Total, headline on
  attending + checked-in, suppress implausible conversion — reuse
  `add-past-events-tab` semantics (D3).
- [Extra API calls per detail open] → reuse the already-cached
  `meetup_performance_get` row; fetch `content_page_get` /
  `content_page_metrics_get` only on detail open/refresh and cache them (D2, D4).
- [Content page body drifts from live page] → show a fetched-at timestamp and the
  live-URL deep link so organizers can confirm against the source.

## Open Questions

- Whether the event object reliably carries a `content_page_token`/`slug`, or
  whether a lookup step is needed to resolve it — confirm at implementation and
  fall back to the "no public page" state when unresolved.
- Whether to cache the rendered HTML or re-render markdown client-side on each
  read — default to storing the raw body and rendering on read.
