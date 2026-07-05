# Proposal: add-past-events-tab

## Why

Mission Control v1 shows only upcoming events, so organizers can't review how
their recent meetups actually turned out (final attendance, check-ins, traffic)
without leaving the app. The updated prototype (`design/mission-control.html`)
adds an **Upcoming / Past** segmented control to the overview with past-event
recap variants — this change implements it.

## What Changes

- Add an **Upcoming / Past** segmented tab to the events overview.
- Add a past-events data source: `meetups/search` with `status: "past"` (returns
  the caller's completed events with final RSVP counts, gallery, and organizer).
- Past events are cached but **not polled** — recap data is frozen at last sync,
  and past events never trigger change notifications or claim the tray "next event".
- Past **card** variant: a "held" date chip instead of a countdown, a check-in
  gauge (checked-in / capacity), and an **Attended** funnel cell in place of
  Waitlisted.
- Past **detail** variant: "held" chip, an **Attended** row in the RSVP summary,
  "final" recap labels, and a "data frozen / no longer polled" footer.
- Extend the SQLite cache so upcoming and past events coexist without the
  upcoming poll cycle evicting cached past events.
- Out of scope: pagination/infinite scroll of past events, editing past events,
  any write endpoints.

## Capabilities

### New Capabilities

- `past-events`: Upcoming/Past tab navigation, past-event data source and
  non-polled caching, and the past-event card and detail recap variants.

### Modified Capabilities

None — the v1 specs are not yet archived to `openspec/specs/`, so past-event
behavior is captured as a self-contained new capability rather than a delta.

## Impact

- Frontend: `src/screens/overview.ts` (segmented control + tab state + past card
  variant), `src/screens/detail.ts` (past recap variant), `src/styles.css`
  (`.seg`, `.count.held` from the prototype), `src/types.ts`.
- Backend: `src-tauri/src/api.rs` (`past_events`), `db.rs` (`kind` column;
  kind-scoped reads and retention), `sync.rs` (slow-cadence past fetch, excluded
  from notifications and tray), `commands.rs` (expose `kind`).
- External API: adds `meetups/search` (`status=past`); optionally
  `meetups/performance` `rsvps.completed` for the checked-in figure. Read-only.
