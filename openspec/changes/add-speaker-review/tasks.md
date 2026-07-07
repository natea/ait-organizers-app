## 1. Backend — API client (api.rs)

- [x] 1.1 Add read call for `speaker_pipeline_candidates_get` (GET
  `/api/agents/v1/recommendations/speakers/pipeline`) with `limit` and `weblog_token`,
  parsing `candidates[]` (`client_token`, `sample_*` tokens, `name`, `email`, `home_city`,
  `matched_cities`, `speaker_fit_score`, `talk_history_summary`, `engagement_signals`,
  `recommended_topic_angles`, `why_now`, `refs`), `filters`, and `truncated`.
- [x] 1.2 Add read call for `rsvp_search` (speaker-tagged) parsing `speaker_status`,
  `speaker_approval_status`, registrant-facing status fields, and optional `phone_number`.
- [x] 1.3 Add read call for `rsvp_get` used for post-write re-sync of a single RSVP.
- [x] 1.4 Add write call for `rsvps/speaker_proposal_upsert` (POST) supporting
  `rsvp_ref`/`rsvp_id`, `speaker_title`, `speaker_description`, optional proposal fields,
  `speaker_status` enum, `note`, with `send_speaker_email`/`send_rsvp_email` pinned false.
- [x] 1.5 Parse the `{ok, data, error{code}}` envelope and map `forbidden_scope`,
  `forbidden_role`, `forbidden_api_group`, and `429` into typed errors (no auto-retry).

## 2. Backend — cache (db.rs)

- [x] 2.1 Add tables/columns for cached talk proposals per event, storing raw
  `speaker_status`, `speaker_approval_status`, and optional `phone_number` (absent when
  the API omits it).
- [x] 2.2 Add a table for the ranked candidate pool keyed by event/scope with score and
  evidence fields.
- [x] 2.3 Add a local audit-log table (actor, timestamp, endpoint, rsvp_ref, change
  summary, resulting status) shared with the write guardrail.
- [x] 2.4 Add read queries that group proposals into proposed / under review / approved /
  declined for the pipeline screen.

## 3. Backend — sync (sync.rs)

- [x] 3.1 Sync speaker proposals for the selected event via `rsvp_search` into the cache.
- [x] 3.2 Sync the candidate pool via `speaker_pipeline_candidates_get`; on 429 keep
  last-good cached candidates and flag refresh-unavailable.
- [x] 3.3 Implement single-RSVP re-sync via `rsvp_get` after a successful write.
- [x] 3.4 Ensure forbidden/error responses leave the cache unchanged and are surfaced.

## 4. Backend — commands (commands.rs)

- [x] 4.1 Add commands to load cached proposals (by lane) and the candidate pool for an event.
- [x] 4.2 Add a write command for approve/decline/move-to-review that builds a described
  action, requires the shared confirmation guardrail, calls
  `speaker_proposal_upsert` with the mapped `speaker_status`, writes the audit row on `ok`,
  and triggers `rsvp_get` re-sync.
- [x] 4.3 Add a write command to upsert a speaker proposal (title/description + changed
  fields + `note`) through the same guardrail and re-sync.
- [x] 4.4 Ensure both write commands never set `send_speaker_email`/`send_rsvp_email` true
  and do not mutate the cache before an `ok` response.

## 5. Frontend — types (types.ts)

- [x] 5.1 Add TS types for cached talk proposals, kanban lane grouping, and the candidate
  pool matching the cached shapes.
- [x] 5.2 Add types for the write actions (approve/decline/upsert payloads) and audit entries.

## 6. Frontend — speaker pipeline screen (src/)

- [x] 6.1 Build the kanban view with lanes proposed → under review → approved, rendered
  only from the cache, with a declined/dimmed state and an empty state.
- [x] 6.2 Build the candidate-pool panel showing ranked candidates with evidence and the
  refresh-unavailable notice.
- [x] 6.3 Wire the approve/decline action through the shared confirmation guardrail and
  show the audit result; cancel performs no write.
- [x] 6.4 Wire the create/edit proposal form through the same confirmation + audit path.
- [x] 6.5 Render `phone_number` only when present in the cached payload; never derive
  visibility client-side.
- [x] 6.6 Show scoped degradation states for `forbidden_scope` / `forbidden_role` /
  `forbidden_api_group` without retrying.

## 7. Frontend — styles (styles.css)

- [x] 7.1 Add kanban, candidate-pool, confirmation, and degradation styles per
  `design/DESIGN.md`.

## 8. Verification

- [x] 8.1 Run `tsc` (type-check) clean.
- [x] 8.2 Run `cargo build` and `cargo test` for the Rust backend.
- [x] 8.3 Mock-drive the read + write flows (list proposals, view candidates, approve,
  decline, upsert) against the mock API, verifying confirmation, audit rows, post-write
  re-sync, phone visibility, and forbidden/429 degradation.
