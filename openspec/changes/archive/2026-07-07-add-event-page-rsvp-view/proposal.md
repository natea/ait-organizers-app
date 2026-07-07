# Proposal: add-event-page-rsvp-view

Stack-rank #1 — Public event page + RSVP flow (the highest-usage surface across the network).

## Why

The event page and its RSVP flow are where nearly all attendee activity happens,
but Mission Control currently shows only aggregate RSVP counts. Organizers can't
see the actual public event page content or the RSVP funnel in one place without
opening the website. Surfacing the rendered event page plus the full RSVP flow
makes the app the single place to check "what attendees see and how they're
converting."

## What Changes

- Add an event-page view to the event detail: rendered public content page body
  (markdown/HTML) alongside the live RSVP funnel and page-traffic figures.
- Show the RSVP flow breakdown (registered → attending → waitlisted → cancelled →
  checked-in) with the correct semantics established in `add-past-events-tab`.
- Deep link to the live event URL; read-only (no page editing in this change).

## Capabilities

### New Capabilities

- `event-page-view`: Render an event's public content page and RSVP-flow summary
  inside the app's event detail.

## Impact

- Endpoints (read-only): `content_page_get` (article body + metadata),
  `content_page_metrics_get` (email/traffic), `meetups/performance`,
  `rsvps/summary` (incl. `status=checked_in`).
- Frontend: extend `src/screens/detail.ts` with an event-page panel; backend
  `api.rs`/`sync.rs`/`db.rs` gain a content-page fetch + cache.
- Fits the read-only v1 posture — no writes. Depends on the existing detail
  cache from `add-event-mission-control-v1`.
