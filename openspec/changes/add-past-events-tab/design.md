# Design: add-past-events-tab

## Context

Builds on the shipped `add-event-mission-control-v1` app. The overview currently
renders upcoming events from `meetups/upcoming` cached in SQLite. The prototype
now adds a Past tab. Verified read-only against the live API: `meetups/search`
with `{ "status": "past" }` returns the caller's completed events (no query
required) as full event objects — final `rsvps`, `gallery_preview`,
`weblog_token`, `organizer`, `status: "completed"`, `relative_day_in_event_timezone: "past"`.

## Goals / Non-Goals

**Goals:**
- A second tab listing the caller's recent past events with recap framing.
- Past events cached like upcoming, but frozen (not polled, no notifications).
- Reuse existing card/detail/cache machinery; minimal new surface.

**Non-Goals:**
- Pagination or unbounded history (cap to a recent window, e.g. last 90 days / top N).
- Live updates for concluded events.
- Any new write behavior.

## Decisions

**D1 — Past data source: `meetups/search` with `status=past`.**
Returns caller-scoped past events with the same object shape as upcoming, so the
existing card/detail renderers mostly apply. Chosen over `meetups/performance`
because search returns `gallery_preview` and needs no per-chapter weblog token or
date bounds. Fetch with a bounded `limit` (e.g. 50); surface `truncated`.

**D2 — Checked-in ("Attended") figure.**
Search `rsvps` has `{registered, attending, waitlisted, cancelled, capacity}` but
no check-in count. The prototype's "Attended" = people who actually showed. Use
`meetups/performance` `rsvps.completed` for that number when the performance group
is in scope; otherwise fall back to final `attending` and label accordingly. This
keeps the Past tab useful even where performance is disabled.

**D3 — One `events` table with a `kind` discriminator.**
Add `kind TEXT` (`'upcoming'` | `'past'`) to `events`. `get_events` returns `kind`
per row; the frontend filters by active tab. Critically, `retain_events` (which
deletes rows absent from the latest fetch) must be **scoped by kind** so the
upcoming poll cycle never evicts cached past events and vice-versa. Alternative
(separate tables) rejected — it would duplicate detail-merge and read logic.

**D4 — Past events are fetched on a slow cadence and never polled for change.**
Recap data is frozen. Fetch past events once per launch and on manual refresh
only (not in the 2-minute upcoming loop). Exclude `kind='past'` from poll-diff
notification comparison and from the tray "next event" computation (which already
filters to future events, but must also ignore past rows explicitly).

**D5 — Frontend tab state.**
`overview.ts` owns a `listTab` (`'upcoming' | 'past'`) and renders the `.seg`
control (ported from the prototype). Card rendering branches on `kind`: past →
`held` chip, checked-in gauge, Attended cell; upcoming → unchanged. `detail.ts`
branches similarly (held chip, Attended row, "final"/recap labels, frozen footer).

## Risks / Trade-offs

- [Search may return many past events / truncate] → cap `limit`, show a "recap window" notice, don't imply completeness.
- [Upcoming retain deleting past rows (or vice-versa)] → kind-scoped retention is the core correctness requirement; cover with the verification that an upcoming refresh preserves past rows.
- [Performance out of scope → no check-in number] → graceful fallback to final attending with a clear label, consistent with existing degradation.
- [Past event also appears in upcoming momentarily around start time] → dedupe by `meetup_token`; if a token exists in both, upcoming wins until it flips to past.

## Open Questions

- Recap window size (90 days vs top-N) — pick a sensible default at implementation.
- Whether to enrich each past card with `performance` eagerly (extra calls) or only on detail open — default to detail-only to limit API usage.
