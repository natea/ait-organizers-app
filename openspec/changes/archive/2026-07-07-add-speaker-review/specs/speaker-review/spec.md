## ADDED Requirements

### Requirement: List talk proposals per event

The system SHALL display, for a selected event in the organizer's scope, the submitted
talk proposals rendered only from the local SQLite cache, grouped into kanban lanes
proposed → under review → approved derived from each RSVP's `speaker_status` and
`speaker_approval_status`. The cache SHALL be populated from `rsvp_search`
(speaker-tagged) and `rsvp_get`.

#### Scenario: Proposals grouped into kanban lanes

- **WHEN** an organizer opens the speaker pipeline for an event they own that has
  submitted talk proposals
- **THEN** the screen renders each proposal in the lane matching its cached status —
  `submitted` in "proposed", `pending_review` in "under review", and `main_stage` or
  `science_fair` in "approved" — using only cached data

#### Scenario: Declined proposals are visually distinct

- **WHEN** a cached proposal has `speaker_approval_status = sidelined`
- **THEN** the screen shows it in a dimmed/collapsed declined state rather than in one of
  the three primary lanes

#### Scenario: Empty pipeline

- **WHEN** the selected event has no cached talk proposals
- **THEN** the screen shows an empty-state message and no kanban cards

### Requirement: Show ranked speaker candidates with evidence

The system SHALL display a ranked speaker-candidate pool for the organizer's scope,
sourced from `speaker_pipeline_candidates_get` and rendered from the cache. Each candidate
SHALL show its `speaker_fit_score`, `talk_history_summary`, `engagement_signals`,
`recommended_topic_angles`, and `why_now` evidence. The candidate pool SHALL be presented
separately from the review kanban.

#### Scenario: Candidates ranked with supporting evidence

- **WHEN** an organizer views the candidate pool for an event in their scope
- **THEN** candidates are listed in descending `speaker_fit_score` order, each showing its
  talk-history summary, engagement signals, recommended topic angles, and why-now evidence
  from the cache

#### Scenario: Candidate refresh is rate-limited

- **WHEN** a candidate-pool refresh returns a 429 rate-limit response
- **THEN** the screen retains the last cached candidates and shows a non-blocking notice
  that the refresh was unavailable, leaving the review kanban unaffected

### Requirement: Set speaker approval status with confirmation

The system SHALL allow an organizer to approve or decline a talk proposal by writing
`speaker_status` through `rsvps/speaker_proposal_upsert` (`main_stage` for approve,
`sidelined` for decline, `pending_review` to move into review). Every such write MUST
require explicit confirmation before the request is sent and MUST append a local audit
entry on success. The system MUST NOT mutate the cache before the API returns `ok`, and
MUST NOT set `send_speaker_email` or `send_rsvp_email` to true.

#### Scenario: Approve requires and receives confirmation

- **WHEN** an organizer chooses to approve a proposal and confirms the presented action
- **THEN** the system calls `rsvps/speaker_proposal_upsert` with `speaker_status =
  main_stage`, records an audit entry with actor, timestamp, RSVP reference, and change
  summary, and re-fetches the RSVP via `rsvp_get` to update the cached lane

#### Scenario: Decline moves proposal to declined state

- **WHEN** an organizer confirms declining a proposal
- **THEN** the system calls `rsvps/speaker_proposal_upsert` with `speaker_status =
  sidelined`, records an audit entry, and the proposal moves to the declined state after
  the post-write re-sync

#### Scenario: Cancelling confirmation performs no write

- **WHEN** an organizer opens the approve/decline action but cancels the confirmation
- **THEN** no API request is sent, no audit entry is recorded, and the cache is unchanged

#### Scenario: Failed write leaves cache untouched

- **WHEN** an approval write returns a non-`ok` envelope
- **THEN** the cache is not mutated, no success audit entry is recorded, and the error is
  surfaced to the organizer

### Requirement: Upsert a speaker proposal

The system SHALL allow an organizer to create or edit a speaker proposal on an existing
RSVP through `rsvps/speaker_proposal_upsert`, sending at minimum `speaker_title` and
`speaker_description` plus an audit `note`. The write MUST pass through the same
confirmation and audit guardrail and MUST re-sync the affected RSVP on success.

#### Scenario: Create a proposal on an RSVP

- **WHEN** an organizer fills in a title and description for an RSVP that has no proposal
  and confirms the action
- **THEN** the system calls `rsvps/speaker_proposal_upsert` with the provided
  `speaker_title`, `speaker_description`, and audit `note`, records an audit entry, and
  re-fetches the RSVP so the new proposal appears in the pipeline

#### Scenario: Edit an existing proposal

- **WHEN** an organizer changes fields on an existing proposal and confirms
- **THEN** only the changed fields plus the audit `note` are sent to
  `rsvps/speaker_proposal_upsert`, an audit entry is recorded, and the cache reflects the
  updated proposal after re-sync

### Requirement: Contact-field visibility for approved speakers

The system SHALL render a proposer's `phone_number` only when the API includes it in the
payload, and MUST NOT derive or infer phone visibility on the client. When the API omits
`phone_number`, the cache MUST store it as absent and the screen MUST NOT display a phone.

#### Scenario: Phone shown only when API returns it

- **WHEN** the cached RSVP payload for an approved speaker includes `phone_number`
- **THEN** the screen displays the phone number

#### Scenario: Phone hidden when omitted by the API

- **WHEN** the cached RSVP payload for a submitted-but-not-approved proposer omits
  `phone_number`
- **THEN** the screen displays no phone number and stores the field as absent

### Requirement: Degrade gracefully on forbidden responses

The system SHALL treat `forbidden_scope`, `forbidden_role`, and `forbidden_api_group` as
hard denies for both reads and writes, surfacing a scoped message without retrying through
alternate paths and without corrupting the cache.

#### Scenario: Out-of-scope read is degraded

- **WHEN** loading proposals or candidates returns `error.code = forbidden_scope`
- **THEN** the affected panel shows a scoped "not available for your access" state, the
  cache is left unchanged, and no automatic retry is attempted

#### Scenario: Ineligible role blocks a write

- **WHEN** an approval or upsert write returns `error.code = forbidden_role`
- **THEN** the write is aborted with no cache mutation and no success audit entry, and the
  organizer sees a role-denied message with no retry through an alternate endpoint

#### Scenario: Disabled API group is surfaced

- **WHEN** any speaker-review request returns `error.code = forbidden_api_group`
- **THEN** the feature surfaces a disabled-capability state and does not retry
