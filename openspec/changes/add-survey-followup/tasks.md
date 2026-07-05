# Tasks: add-survey-followup

## 1. Backend — API client

- [ ] 1.1 Add `survey_diagnostic(meetup_token)` to `api.rs` calling `meetups/survey_diagnostic` with `{ meetup_token }`
- [ ] 1.2 Add `survey_report(meetup_token, days)` to `api.rs` calling `meetups/survey_report` (default `days=90`), used only for the event's response-rate context
- [ ] 1.3 Add `email_campaign_performance(meetup_token)` to `api.rs` calling `analytics/email/campaign_performance` scoped by `meetup_token`
- [ ] 1.4 Map `forbidden_api_group` / `forbidden_scope` / `forbidden_role` and empty responses to a per-source status enum (`ok` / `forbidden_*` / `unavailable` / `empty`) instead of hard-failing the whole fetch

## 2. Backend — cache

- [ ] 2.1 Add a `survey_followup` table in `db.rs` keyed by `meetup_token` holding the survey summary JSON, email-engagement JSON, and per-source status columns
- [ ] 2.2 Add `upsert_survey_followup(meetup_token, …)` and `get_survey_followup(meetup_token)` in `db.rs`
- [ ] 2.3 Ensure `survey_followup` rows are only created for `kind='past'` events and are not evicted by upcoming retention

## 3. Backend — sync & command

- [ ] 3.1 In `sync.rs`, fetch the three endpoints for a past event on detail open and manual refresh only; derive survey response rate (guard zero/unknown denominator) and aggregate meetup-scoped follow-up campaign rows into headline engagement
- [ ] 3.2 In `sync.rs`, exclude these endpoints from the 2-minute upcoming poll loop and never fetch them for upcoming events
- [ ] 3.3 Locate the event in `survey_report` by `meetup_token`; on no match, fall back to diagnostic counts and drop report-derived context
- [ ] 3.4 Add a `get_survey_followup` command in `commands.rs` returning the cached row for a `meetup_token`

## 4. Frontend — types & rendering

- [ ] 4.1 Add `SurveyFollowup` types (survey summary, sentiment/themes optional, email engagement, per-source status) to `types.ts`
- [ ] 4.2 Extend the past-event panel in `screens/detail.ts` to load `get_survey_followup` on open and render the survey response-rate + optional themes sub-section and the follow-up engagement sub-section, recap-framed, below existing sections
- [ ] 4.3 Render per-source degradation: "not available" for `forbidden_*`/`unavailable`, empty states for `empty`, and suppress the percentage when the denominator is unknown/zero
- [ ] 4.4 Add follow-up panel styles to `styles.css` per `design/DESIGN.md` (reuse existing recap/section tokens)

## 5. Verification

- [ ] 5.1 `bunx tsc --noEmit` clean
- [ ] 5.2 `cargo build` and `cargo test` clean
- [ ] 5.3 Drive the flow in-browser with mocked Tauri IPC using real survey/diagnostic/campaign shapes: survey results, follow-up engagement, empty states, and forbidden per-source degradation
- [ ] 5.4 Confirm the upcoming poll cycle does not fetch survey/follow-up endpoints and does not create or modify `survey_followup` rows
