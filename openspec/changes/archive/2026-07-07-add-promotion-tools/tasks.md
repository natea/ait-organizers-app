# Tasks: add-promotion-tools

## 1. Backend — API client (src-tauri/src/api.rs)

- [x] 1.1 Add `social_post_generate` method (POST `/social_posts/generate`) with body `source_type`, `source_ref`, `platform`, `goal`, optional `tone`, `city`; unwrap the `{ok,data,error}` envelope and return raw `data`
- [x] 1.2 Add `event_promo_generate` method (POST `/event_promos/generate`) with body `meetup_token`, `package_type`, `audience`
- [x] 1.3 Add `discussion_topics_generate` method (POST `/meetups/discussion_topics/generate`) with body `meetup_token`
- [x] 1.4 Add `logo_search` method (GET `/logos/search`) with query params `query`, `scope`, `include_co_branded`, `limit`
- [x] 1.5 Use a generation-specific client timeout (~30s, above the ~25s server ceiling) distinct from the 6–8s sync read timeout; map `429` to a rate-limited error carrying `Retry-After` and map `forbidden_*` codes through the existing typed error path

## 2. Backend — draft & job cache (src-tauri/src/db.rs)

- [x] 2.1 Create `promotion_drafts` table `(meetup_token, kind, platform, params_json, result_json, generated_at)` with an upsert keyed by `(meetup_token, kind, platform)`; create on startup if absent
- [x] 2.2 Create `promotion_jobs` table `(id, meetup_token, kind, platform, params_hash, status, started_at, error_code)` with statuses `pending|running|ready|error|timeout`
- [x] 2.3 Add read helpers to fetch the latest draft per `(meetup_token, kind, platform)` and the current job for an action
- [x] 2.4 Add an optional logo-search cache keyed by `(query, scope, include_co_branded)` with a `fetched_at` freshness window

## 3. Backend — commands & job runner (src-tauri/src/commands.rs, sync.rs)

- [x] 3.1 Add `promotion_generate(kind, params)` command that creates/returns a job id, runs the API call on a background task, and returns immediately without blocking the UI
- [x] 3.2 Suppress duplicate kickoffs: if a `pending`/`running` job exists for the same `(meetup_token, kind, platform)`, return the existing job id and issue no new request
- [x] 3.3 On success, upsert `promotion_drafts` and set job `ready`; on `429` honor `Retry-After`; on timeout set job `timeout`; on `forbidden_*` set job `error` with the code
- [x] 3.4 Emit `promotion:job` change events on status transitions (mirroring the existing `sync:updated` pattern); do NOT add generation endpoints to the poll scheduler in `sync.rs`
- [x] 3.5 Add `promotion_cancel(job_id)` command that aborts an in-flight request and drops the action back to its last cached draft
- [x] 3.6 Add `logo_search(query, scope, include_co_branded)` command reading from cache within the freshness window, else fetching

## 4. Frontend — types (src/types.ts, src/generated)

- [x] 4.1 Add types for the four generation payloads/results and the promotion job status enum, aligned with the generated OpenAPI types
- [x] 4.2 Add cache-row types for `promotion_drafts` and the logo-search results consumed by the panel

## 5. Frontend — Promote panel (src/screens, src/api.ts, src/styles.css)

- [x] 5.1 Add a Promote panel to the event detail screen with four actions: Social posts (platform + goal selectors), Promo package (package_type + audience selectors), Discussion topics, Logo search (query input + co-branded toggle)
- [x] 5.2 Wire each action to `promotion_generate`; render progress from `promotion:job` events; disable the Generate button while `pending`/`running`
- [x] 5.3 Render the latest cached draft per action with its `generated_at` and a copy-to-clipboard / export affordance; show Regenerate when a draft exists
- [x] 5.4 Handle `timeout` (retry affordance, keep prior draft visible), `429` (retry, keep draft), and `forbidden_*` ("not enabled for your chapter" non-blocking state)
- [x] 5.5 Style the panel and states per design/DESIGN.md; ensure the panel and cached drafts render offline from cache

## 6. Verification

- [x] 6.1 `tsc` passes with the new types and screen code
- [x] 6.2 `cargo build` and `cargo test` pass for the new api/db/commands code
- [x] 6.3 Mock-drive: kick off each generation against a mock returning after a delay — verify progress shows, draft caches, cached draft renders on revisit, and duplicate clicks are suppressed
- [x] 6.4 Mock-drive failure paths: timeout keeps the prior draft and offers retry; `429` honors `Retry-After`; `forbidden_api_group` shows the non-blocking "not enabled" state while other actions still work
