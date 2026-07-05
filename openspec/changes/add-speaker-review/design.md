## Context

AI Tinkerers Mission Control is a read-only Tauri 2 desktop app for city organizers.
Screens render exclusively from the local SQLite cache (`src-tauri/src/db.rs`), which
is populated by `sync.rs` calling the Agents API (`api.rs`) and exposed to the frontend
through Tauri commands (`commands.rs`). The frontend is vanilla TS + Vite in `src/`,
styled from `design/DESIGN.md`.

Speaker curation — reviewing submitted talk proposals and approving the lineup — lives
entirely outside the app today, and it is a high-volume programming task network-wide.
The API already computes a ranked speaker-candidate pool
(`speaker_pipeline_candidates_get`) and exposes talk submissions through `rsvp_search`
(with `speaker_status` / `speaker_approval_status`) and `rsvp_get`.

This is the app's first **write** feature alongside `add-rsvp-screening`. Both must reuse
a single shared write guardrail: explicit per-action confirmation plus a local audit
trail. Approving a speaker also affects **contact-field visibility**: per the Contact
Field Visibility Policy (`docs/agents-api.md`), a city/series owner may see an approved
upcoming speaker's `phone_number`, but not that of a merely-submitted proposer.

Constraints:
- Screens read only from the SQLite cache; writes go through the API and then re-sync.
- Envelope is `{ok, data, error{code}}`; degradation codes include `forbidden_scope`,
  `forbidden_role`, `forbidden_api_group`.
- Scope is city/series owners for their own events.
- Rate limits: `speaker_pipeline_candidates_get` 15 rpm; `rsvp_search`/`rsvp_get` per
  their existing limits.

## Goals / Non-Goals

**Goals:**
- Surface, per event, the submitted talk-proposal pipeline and the ranked speaker
  candidate pool with supporting evidence, rendered from the cache.
- Provide a kanban-style pipeline view with lanes: proposed → under review → approved.
- Support two write actions, each behind the shared confirmation + audit guardrail:
  - Set speaker approval status (approve → `main_stage`; decline → `sidelined`) via
    `rsvps/speaker_proposal_upsert`.
  - Upsert a speaker proposal (title/description and optional fields) via
    `rsvps/speaker_proposal_upsert`.
- Degrade gracefully on `forbidden_scope` / `forbidden_role` / `forbidden_api_group`
  without corrupting the cache.
- Respect the phone-number visibility rule: show phone only for approved speakers in
  caller-visible scope, exactly as the API returns it.

**Non-Goals:**
- Bulk approve/decline across many proposals (single-RSVP actions only this change).
- Sending speaker or RSVP notification emails by default (`send_speaker_email` and
  `send_rsvp_email` default `false`; email is out of scope for this change).
- Changing the raw RSVP state via `rsvps/state_update` for approval — approval status
  is driven through `speaker_proposal_upsert`'s `speaker_status`, which is the field
  `rsvp_search` reports back as `speaker_approval_status`.
- Editing candidate scoring or the ranking model (read-only consumption).
- Science-fair proposal-type workflows beyond passing through existing fields.

## Decisions

### Kanban lanes proposed → under review → approved
Map API speaker states onto three lanes so organizers see the whole funnel:
- **proposed**: `speaker_status = submitted` and `speaker_approval_status` is unset /
  `not_approved` with no review marker.
- **under review**: `speaker_approval_status = pending_review`.
- **approved**: `speaker_approval_status` in {`main_stage`, `science_fair`}.
Declined proposals (`sidelined`) render as a collapsed/dimmed state, not a primary lane.
*Rationale:* mirrors the enum the API already exposes, so no client-side state invented
beyond presentation. *Alternative considered:* a free-form status column — rejected
because it would drift from the API's authoritative enum.

### Ranked candidate pool as a distinct panel, not a lane
`speaker_pipeline_candidates_get` returns *future*-speaker candidates (people, not
existing proposals) with `speaker_fit_score`, `talk_history_summary`,
`engagement_signals`, `recommended_topic_angles[]`, `why_now[]`, and `refs`. These are
recommendations to recruit, not items in the review funnel, so they live in a separate
"Candidate pool" panel beside the kanban. *Rationale:* keeps the mutable review funnel
(RSVP-backed) cleanly separate from read-only recommendations. Cached in its own table
keyed by event scope.

### Approval via `speaker_proposal_upsert`, not `state_update`
Approve/decline sets `speaker_status` (`main_stage` for approve, `sidelined` for decline;
`pending_review` to move into review). `rsvps/state_update` changes the RSVP *state*
(registered/attending/waitlisted/denied) and is the wrong tool for approval; the proposal
brief's "state_update (speaker_approval_status)" is satisfied by the `speaker_status`
field on `speaker_proposal_upsert`, which is what `rsvp_search` echoes as
`speaker_approval_status`. Email sends stay off (`send_speaker_email`/`send_rsvp_email`
default `false`). *Alternative considered:* calling `state_update` too, to auto-move the
RSVP to attending on approval — deferred to keep the write surface minimal and avoid
unintended registrant emails.

### Upsert path is the same endpoint
Creating/editing a proposal and approving it are both `speaker_proposal_upsert` calls;
`speaker_title` + `speaker_description` are the only required fields. The screen sends
only the fields the organizer changed plus an audit `note`. *Rationale:* one write path
to guardrail and test.

### Shared confirmation + audit guardrail (reused from add-rsvp-screening)
Every write goes through the common guardrail:
1. The command builds a described action (endpoint, RSVP ref, human summary of the change).
2. The UI shows an explicit confirmation the organizer must accept.
3. On confirm, `api.rs` performs the write; on `ok`, an audit row is appended locally
   (actor, timestamp, endpoint, rsvp_ref, summary, resulting status) and the affected
   RSVP is re-fetched via `rsvp_get` to refresh the cache. On non-`ok`, no cache mutation
   and the error code is surfaced.
*Rationale:* a single implementation shared with `add-rsvp-screening` keeps write safety
uniform. This change consumes the guardrail; it does not redefine it.

### Contact-visibility handling
The client never derives phone visibility itself. It renders `phone_number` only when the
API includes it in the payload (which the API restricts to approved upcoming speakers in
caller-visible scope). Cache stores the field as-received (absent when omitted). After an
approval, the post-write `rsvp_get` may begin returning the phone; that flows through
naturally. *Rationale:* keeps the policy authoritative on the server; the app cannot leak
a phone the API withheld.

### Read-then-write freshness
Before showing the confirmation for an approval, the screen relies on the cached
`speaker_approval_status`; the post-write re-sync reconciles any drift. No optimistic
mutation is persisted before the API confirms `ok`.

## Risks / Trade-offs

- **Stale approval state causes a confusing confirmation** → the confirmation summarizes
  the intended target status, and the post-write `rsvp_get` re-sync corrects the lane
  immediately; a full pipeline re-sync is available on demand.
- **Phone number leaking to an ineligible viewer** → the app never computes visibility;
  it renders only what the API returns, and stores absent when omitted. Approving a
  speaker legitimately unlocks the phone via the server policy.
- **Accidental email sends** → `send_speaker_email` / `send_rsvp_email` are pinned to
  `false` in this change; there is no UI path that sets them true.
- **Write hits `forbidden_scope`/`forbidden_role`/`forbidden_api_group`** → the write is
  aborted with no cache change; the screen shows a scoped, non-retrying message (per the
  contract, these are hard denies and must not be retried through alternate paths).
- **Rate-limit (429) on candidate refresh** → candidate panel keeps last-good cached data
  and shows a "refresh unavailable, showing cached" note; the review funnel is unaffected.
- **Approve/decline mapping ambiguity** (`main_stage` vs `science_fair`) → this change
  approves to `main_stage` only; science-fair routing is left to the API's existing
  fields and is out of scope, documented as a non-goal.
