# Proposal: add-rsvp-screening

Stack-rank #3 — RSVP screening / manage attendees (most important organizer tool:
108k RSVP state-change events, 2,151 screening-alert clicks network-wide).

## Why

Screening RSVPs — reviewing who registered and promoting/waitlisting/declining
them — is the single most-used organizer workflow, and Mission Control can't do it
at all today. Organizers still leave the app to manage their list. Adding
screening turns the app from a dashboard into a working tool.

## What Changes

- Add an attendee-management view per event: searchable RSVP list with each
  registrant's assessment/score, status, and history.
- **Write actions** (departure from read-only v1): promote/waitlist/decline via
  RSVP state updates; bulk state update for triage.
- Show the AI screening assessment and score to guide decisions.

## Capabilities

### New Capabilities

- `rsvp-screening`: Review, assess, and change RSVP states for an event's
  attendee list.

## Impact

- Endpoints: read — `rsvp_search`, `rsvp_get`, `rsvp_assessment_get`,
  `rsvp_status_history_list`, `subscriber_score_details_get`; **write** —
  `rsvps/state_update`, `rsvps/bulk_state_update`.
- **First write-capable feature** — requires expanding the app's guardrails
  (currently "no write endpoints"); needs explicit user confirmation for
  mutations and an audit trail. City-owner scope; owner-only actions.
- Larger change: new screen, optimistic updates reconciled against the cache,
  and careful rate-limit handling. Candidate to split proposal vs later design.
