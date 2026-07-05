# Spec: attendance-checkin

## ADDED Requirements

### Requirement: Attendee list for the live/next event
The check-in screen SHALL present the attendee list for a single event — the
currently-live event, or the next upcoming event when none is live — using the
same event selection as the tray "next event". The list SHALL be sourced from
`rsvp_search` filtered by that event's `meetup_token`, cached in SQLite, and
rendered from the cache so it remains available offline. Each row SHALL show the
attendee's name, registrant-facing status, and current check-in state
(`checked_in` / `checked_in_at`).

#### Scenario: Live event attendees shown
- **WHEN** the organizer opens the check-in screen and an event is live
- **THEN** the screen renders that event's cached attendee list with each attendee's name, status, and check-in state

#### Scenario: Falls back to next event
- **WHEN** the organizer opens the check-in screen and no event is currently live
- **THEN** the screen renders the attendee list for the next upcoming event

#### Scenario: List available offline
- **WHEN** the network is unavailable and the attendee list was previously cached
- **THEN** the screen still renders the cached attendee list

### Requirement: Record a door check-in
The screen SHALL provide a one-tap "mark attended" control per attendee that
records a door check-in via `rsvps/mark_attended`. Recording a check-in is a
mutation and SHALL follow the app's write guardrail: it MUST be a deliberate,
explicit check-in action and MUST produce a local audit-trail entry
(rsvp_ref, event, timestamp, result) in addition to the server-side
`rsvp_status_history_list` entry. The row SHALL reflect the check-in optimistically
the moment the action is taken.

#### Scenario: One-tap check-in
- **WHEN** the organizer taps "mark attended" on an un-checked-in attendee
- **THEN** the attendee's row immediately shows a checked-in state and a `mark_attended` action is recorded for that RSVP

#### Scenario: Audit trail recorded
- **WHEN** a check-in is recorded
- **THEN** a local audit entry with the rsvp_ref, event, timestamp, and result is written and the server appends a `rsvp_status_history_list` entry

### Requirement: Live checked-in count
The screen SHALL show live progress of checked-in versus attending, reusing the
`rsvps/summary` count with `status=checked_in` as the checked-in total. Between
summary refreshes, the displayed count SHALL be adjusted by the number of
locally-recorded check-ins that have not yet synced, so the organizer's taps are
reflected immediately, and SHALL reconcile to the server figure once the queue
drains and the summary refreshes.

#### Scenario: Count reflects a new check-in
- **WHEN** the organizer checks in an attendee
- **THEN** the checked-in count increments immediately even before the mutation syncs

#### Scenario: Count reconciles with server
- **WHEN** the pending check-ins have synced and `rsvps/summary status=checked_in` refreshes
- **THEN** the displayed checked-in count matches the server summary total

### Requirement: Offline queue and later sync
A door check-in SHALL always be written to a durable local action queue first and
then flushed to `rsvps/mark_attended` by the sync scheduler. Queued check-ins
MUST survive app restart. When offline or rate-limited, queued check-ins SHALL
remain pending and SHALL be flushed when connectivity returns or the backoff
window elapses, without the organizer re-entering them.

#### Scenario: Check-in queued while offline
- **WHEN** the organizer checks in an attendee while the network is unavailable
- **THEN** the check-in is stored in the durable action queue and the row shows a checked-in (pending sync) state

#### Scenario: Queue flushes when network returns
- **WHEN** connectivity returns and a sync cycle runs
- **THEN** each pending check-in is POSTed to `rsvps/mark_attended` and marked sent on success

#### Scenario: Queue survives restart
- **WHEN** the app is restarted with unsent check-ins in the queue
- **THEN** those check-ins are still present and are flushed on the next sync cycle

#### Scenario: Rate-limited flush retries later
- **WHEN** a flush receives `429 rate_limited` with a `Retry-After`
- **THEN** the affected queue rows stay pending and are retried after the backoff window rather than failing

### Requirement: Idempotent check-in (no double check-in)
The system SHALL NOT record a duplicate check-in for an attendee. Before
enqueueing, the app MUST NOT enqueue a check-in for an RSVP that is already
`checked_in` or that already has an unsent queue row. Each queued action MUST
carry a stable client token so retries are safe, and a flush response indicating
the RSVP was already attended SHALL be treated as success.

#### Scenario: Double-tap does not double-queue
- **WHEN** the organizer taps "mark attended" twice on the same attendee
- **THEN** only one check-in action is enqueued for that RSVP

#### Scenario: Already-checked-in attendee
- **WHEN** the organizer taps "mark attended" on an attendee already marked checked in
- **THEN** no new check-in is enqueued

#### Scenario: Retry of an already-attended RSVP
- **WHEN** a queued check-in is flushed but the server reports the RSVP is already attended
- **THEN** the queue row is resolved as sent and no duplicate is created

### Requirement: Degradation on forbidden scope
The system SHALL treat a `403` `forbidden_scope`, `forbidden_role`, or
`forbidden_api_group` response from `rsvps/mark_attended` as a hard deny that MUST
NOT be retried through alternate paths. The affected queue row SHALL be marked failed,
the attendee's optimistic check-in SHALL be reverted, and the screen SHALL surface
a clear non-retrying message and disable the check-in controls with an explanation.

#### Scenario: Forbidden scope reverts the check-in
- **WHEN** a flush of a check-in returns `403 forbidden_scope`
- **THEN** the queue row is marked failed, the attendee's row reverts to not-checked-in, and a "not permitted for your scope" message is shown

#### Scenario: Controls degrade on hard deny
- **WHEN** check-ins are denied by a `forbidden_*` error for the event
- **THEN** the check-in controls are disabled with an explanatory notice and are not retried
