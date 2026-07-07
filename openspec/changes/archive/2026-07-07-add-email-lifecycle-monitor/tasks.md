# Tasks: add-email-lifecycle-monitor

## 1. Backend — API client (`api.rs`)

- [x] 1.1 Add `email_send_jobs_summary(meetup_token, limit, date_from?, date_to?)` calling `email_send_jobs/summary`
- [x] 1.2 Add `email_send_jobs_list(status?, content_page_token?, limit, date_from?, date_to?)` calling `email_send_jobs/list`
- [x] 1.3 Add `email_send_job_get(token)` and `email_send_job_throughput_get(token, bucket)` calling the respective `email_send_jobs/*` routes
- [x] 1.4 Add `email_campaign_performance_get(meetup_token, ...)` calling `analytics/email/campaign_performance` (aggregate rates only)
- [x] 1.5 Add `email_deliverability_health_get(weblog_token?/city?, date_from?, date_to?)` and `email_fatigue_risk_get(scope, limit)` calling the analytics routes
- [x] 1.6 Ensure every method returns `ApiOk` (data + rate info) and surfaces `forbidden_api_group` / `forbidden_scope` via `AppError` (no new error variants needed)

## 2. Backend — cache (`db.rs`)

- [x] 2.1 Add an `email_send_jobs` table (event/weblog refs, token, subject, status, sent/pending/suppressed counts, delivered_percent, active-rate fields, `fetched_at`)
- [x] 2.2 Add an `email_event_summary` table keyed by `meetup_token` (aggregate summary counts, status_counts, first/last sent, campaign delivery/open/click rates)
- [x] 2.3 Add an `email_throughput` table keyed by send-job token (bucket_start, sent_count, peak/avg rates)
- [x] 2.4 Add an `email_deliverability` table (health score, sender-domain rows, fatigue tier summary — aggregates only, no per-subscriber rows)
- [x] 2.5 Add upsert + `get_*` read helpers; scope retention so refreshing one surface never evicts another; freeze completed jobs (stop overwriting once done)

## 3. Backend — sync (`sync.rs`)

- [x] 3.1 Fetch chapter deliverability, fatigue tier summary, and recent send-job list on app launch and manual refresh only (not the 2-minute loop)
- [x] 3.2 Fetch event send-job summary + campaign performance when an Email panel opens; upsert into cache
- [x] 3.3 Poll `email_send_job_get` + throughput on a gentle cadence only while a panel is open AND a job is active; stop when all jobs are completed/failed
- [x] 3.4 On `forbidden_api_group` / `forbidden_scope` (`is_capability_block()`), mark the surface blocked and stop re-polling it
- [x] 3.5 Record rate headers and apply 429 retry-after backoff for all email calls via the existing `record_rate` / `apply_backoff` machinery

## 4. Backend — commands (`commands.rs`)

- [x] 4.1 Add `get_event_email(meetup_token)` returning cached event summary, send jobs, and campaign rates
- [x] 4.2 Add `get_send_job_throughput(token)` returning cached throughput series + progress
- [x] 4.3 Add `get_chapter_deliverability()` returning cached health, sender-domain rows, and fatigue tier summary
- [x] 4.4 Add `refresh_email(meetup_token?)` to trigger a manual fetch; register all commands in the Tauri handler

## 5. Frontend — types & screen

- [x] 5.1 Add `EmailSummary`, `SendJob`, `Throughput`, `DeliverabilityHealth`, and fatigue-tier-summary types to `types.ts`
- [x] 5.2 Add the per-event Email panel (status/delivery accounting, active-send throughput, open/click performance) rendering only from cache commands
- [x] 5.3 Add the chapter deliverability view (health score, sender-domain rows, fatigue tier summary, recent send jobs) with a recent-window / `truncated` notice
- [x] 5.4 Show specific non-alarming degraded copy when the surface is blocked (subscribers group / city-owner scope); show neutral empty states
- [x] 5.5 Show `fetched_at` and a frozen/final indicator so live vs completed sends are distinguishable

## 6. Frontend — styles

- [x] 6.1 Add Email panel and deliverability styles to `styles.css` following `design/DESIGN.md` (status chips, delivery gauge, throughput sparkline, domain-health rows)

## 7. Verification

- [x] 7.1 `bunx tsc --noEmit` clean
- [x] 7.2 `cargo build` clean and `cargo test` passing
- [x] 7.3 Drive the flow in-browser with mocked Tauri IPC using real endpoint shapes: event panel (summary + throughput + open/click) and chapter deliverability view
- [x] 7.4 Verify graceful degradation: mock `forbidden_api_group` and `forbidden_scope` responses and confirm the panel shows the specific blocked copy and stops re-polling
- [x] 7.5 Verify active-send polling stops on completion and that chapter data is not fetched on the event loop
