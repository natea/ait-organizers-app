# rsvp-screening Specification

## Purpose
TBD - created by archiving change add-rsvp-screening. Update Purpose after archive.
## Requirements
### Requirement: Searchable attendee list

The system SHALL provide a per-event attendee-management view that renders a
searchable, filterable RSVP list from the local SQLite cache. Each row SHALL show
the registrant's identity, current status (using registrant-facing status labels),
AI screening assessment and engagement score, and access to that RSVP's status
history. The list MUST render only from cached data populated by sync.

#### Scenario: List renders from cache

- **WHEN** the user opens the attendee-management view for a cached event
- **THEN** the RSVP list renders each registrant with name, registrant-facing
  status label, assessment, and score, sourced entirely from the cache

#### Scenario: Free-text and status search

- **WHEN** the user types a query or selects a status filter
- **THEN** the list narrows to matching cached RSVPs without leaving the screen

#### Scenario: Assessment or score unavailable

- **WHEN** the assessment or score endpoint returned an authorization or
  availability error during sync for a registrant
- **THEN** that row shows a non-blocking "not available" indicator for the missing
  field and the rest of the row and list still render

### Requirement: RSVP status history and score detail

The system SHALL let the user open a single registrant's detail showing the
append-only status history (`rsvp_status_history_list`), the full AI assessment
(`rsvp_assessment_get`), and the engagement score breakdown
(`subscriber_score_details_get`), rendered from the cache.

#### Scenario: History and score shown

- **WHEN** the user opens a registrant's detail
- **THEN** the view shows the newest-first status-change history, the assessment,
  and the score breakdown from the cache

### Requirement: Single RSVP state update

The system SHALL allow the user to change one RSVP's state to promote
(`attending`), waitlist (`waitlisted`), or decline (`denied`) via
`rsvps/state_update`. The request MUST key off the raw `rsvp.state` for the
decision while the UI displays registrant-facing labels. The user MUST be able to
see and control whether the standard status-change email is sent (`send_email`)
before confirming.

#### Scenario: Promote a registrant

- **WHEN** the user selects "Promote" on a waitlisted registrant and confirms
- **THEN** the system calls `rsvps/state_update` with `state = attending` and the
  chosen `send_email` value, and records the action in the audit trail

#### Scenario: Email-send choice is explicit

- **WHEN** the confirmation dialog for a state change is shown
- **THEN** it states whether a registrant email will be sent and lets the user
  toggle `send_email` before confirming

#### Scenario: Optimistic update reconciled against cache

- **WHEN** a single state update succeeds
- **THEN** the row shows the new state as pending and settles only after a
  targeted re-read confirms the state in the cache; if the cache disagrees, the UI
  snaps to the cached value and flags the divergence

### Requirement: Bulk RSVP triage

The system SHALL allow the user to change the state of an explicitly selected set
of RSVPs in one action via `rsvps/bulk_state_update`. The selection MUST be a
materialized, enumerable list (not an unbounded "all matching" filter). A single
bulk action MUST NOT exceed the configured per-call ceiling; larger selections
MUST be chunked, with each chunk confirmed separately.

#### Scenario: Bulk decline a selection

- **WHEN** the user selects multiple registrants, chooses "Decline", and confirms
  the enumerated set
- **THEN** the system calls `rsvps/bulk_state_update` with the selected
  `rsvp_refs` and `state = denied`, and records the bulk action in the audit trail

#### Scenario: Selection exceeds the per-call ceiling

- **WHEN** the user's selection exceeds the per-call ceiling
- **THEN** the system splits the action into bounded chunks and requires
  confirmation for each chunk before sending it

### Requirement: Mandatory confirmation before any mutation

The system SHALL NOT issue any write request to the API unless the user has
explicitly confirmed that specific mutation. The confirmation MUST be enforced in
the backend command layer, not solely in the UI, and the confirmation prompt MUST
summarize the exact change: affected registrant(s) and count, from-state →
to-state, the registrant-facing effect, and whether an email will be sent.

#### Scenario: Unconfirmed mutation is refused

- **WHEN** a state-update or bulk-state-update command is invoked without an
  explicit confirmation flag
- **THEN** the command returns a `confirmation_required` error and performs no API
  call and no cache change

#### Scenario: Confirmation summarizes the exact change

- **WHEN** the user initiates a promote, waitlist, decline, or bulk action
- **THEN** a confirmation prompt shows the affected count, the internal state
  change, the registrant-facing effect, and the email-send choice before any write

### Requirement: Write audit trail

The system SHALL record every mutation attempt in an append-only local audit log
that survives cache rebuilds. Each entry MUST capture timestamp, actor, action,
target RSVP reference(s), from-state, to-state, email-send choice, confirmation,
and outcome (success or error code). An entry MUST be written before the API call
and updated with the outcome after it, so attempts that fail, are denied, or are
rate-limited are still recorded.

#### Scenario: Successful mutation is audited

- **WHEN** a confirmed state change succeeds
- **THEN** the audit log contains an entry for the action with its targets,
  from/to states, and a success outcome

#### Scenario: Denied or failed mutation is audited

- **WHEN** a confirmed mutation is refused or fails (authorization, rate limit, or
  network error)
- **THEN** the audit log contains an entry recording the attempt and the failure
  outcome, and the cache is left unchanged

### Requirement: Degradation on forbidden scope or role

The system SHALL treat `forbidden_scope`, `forbidden_role`, and
`forbidden_api_group` as hard denials with no alternate-path retry. On a read,
the affected section MUST degrade to a non-blocking "not available" state while
the rest of the view renders. On a write, the mutation MUST be aborted, the cache
left untouched, the denial recorded in the audit trail, and the reason shown to
the user.

#### Scenario: Read degrades on forbidden response

- **WHEN** a read for assessment, score, or history returns `forbidden_scope`,
  `forbidden_role`, or `forbidden_api_group`
- **THEN** that section shows a non-blocking "not available" state and the rest of
  the attendee view still renders

#### Scenario: Write aborts on forbidden response

- **WHEN** a mutation returns `forbidden_scope`, `forbidden_role`, or
  `forbidden_api_group`
- **THEN** the mutation is aborted with no cache change, the denial is written to
  the audit trail, and the user is shown why the action was refused

### Requirement: Rate-limit handling for mutations

The system SHALL honor rate-limit signals on writes. On a `429` / `rate_limited`
response the system MUST NOT automatically retry the mutation; it MUST surface the
`Retry-After` window to the user, record a rate-limited outcome in the audit
trail, and require a fresh user confirmation to retry. During bulk chunking the
system SHALL throttle proactively as `X-RateLimit-Remaining` approaches zero.

#### Scenario: Mutation is rate-limited

- **WHEN** a state-update or bulk-state-update returns `429` / `rate_limited`
- **THEN** the mutation is not auto-retried, the `Retry-After` window is shown to
  the user, the outcome is audited, and a new confirmation is required to retry

