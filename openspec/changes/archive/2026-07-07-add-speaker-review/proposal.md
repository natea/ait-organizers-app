# Proposal: add-speaker-review

Stack-rank #5 — Speaker proposal / speaker review (a high-volume programming workflow).

## Why

Curating speakers — reviewing talk proposals and approving the lineup — is a core
programming task that currently lives entirely outside the app. Bringing the
speaker pipeline into Mission Control lets organizers move candidates from
proposed → approved without context-switching, and surfaces the ranked speaker
candidate pool the API already computes.

## What Changes

- Add a speaker pipeline view: submitted talk proposals per event plus ranked
  speaker candidates with supporting evidence.
- **Write actions**: update speaker approval status (approve/decline), upsert a
  speaker proposal.
- Kanban-style flow: proposed → under review → approved.

## Capabilities

### New Capabilities

- `speaker-review`: Review talk proposals, view ranked speaker candidates, and
  set speaker approval status.

## Impact

- Endpoints: read — `speaker_pipeline_candidates_get`, `rsvp_search`
  (speaker-tagged), `rsvp_get`; **write** — `rsvps/speaker_proposal_upsert`,
  `rsvps/state_update` (speaker_approval_status).
- Write-capable — shares the guardrail/confirmation/audit work with
  `add-rsvp-screening`. Phone-number visibility rules apply to approved speakers.
- Scope: city/series owners for their events.
