# Proposal: add-attendance-checkin

Stack-rank #4 — Attendance confirmation + check-in (strong hybrid member/organizer
workflow: 47k attendance prompts, 6,591 confirmations, 6,793 QR scans network-wide).

## Why

Door check-in is a live, day-of-event workflow with heavy usage. Mission Control
now *reports* the final check-in count (from `add-past-events-tab`) but can't
*run* check-in. A native check-in view — especially one that works fast at the
door — is a natural, high-value extension of the app organizers already have open.

## What Changes

- Add a day-of check-in view for the currently-live/next event: attendee list
  with a one-tap "mark attended" (check-in) action and a live checked-in count.
- **Write action**: mark an RSVP as attended/checked-in.
- Show real-time checked-in vs attending progress (reusing the corrected
  `rsvps/summary status=checked_in` count).

## Capabilities

### New Capabilities

- `attendance-checkin`: List attendees and record door check-ins for an event.

## Impact

- Endpoints: read — `rsvp_search`, `rsvps/summary` (`status=checked_in`);
  **write** — `rsvps/mark_attended`.
- Write-capable — same guardrail expansion + confirmation/audit needs as
  `add-rsvp-screening`; pairs naturally with it.
- UX priority is speed and offline tolerance at the door (queue check-ins, sync
  when the network returns).
