# Proposal: add-email-lifecycle-monitor

Stack-rank #2 — Event email lifecycle (the operational backbone of every chapter).

## Why

Event email is the operational backbone of a chapter, but organizers currently
have no visibility into send health from Mission Control. When an announcement
goes out they want to watch delivery, opens, clicks, and suppression in real time
and catch stuck or failing sends before they hurt an event.

## What Changes

- Add an Email panel per event: send-job status (sent/pending/suppressed),
  delivery accounting, throughput over time, and open/click performance.
- Add a chapter-level deliverability view: sender-domain health, fatigue-risk
  segments, and recent send jobs.
- Read-only monitoring only — no composing or sending in this change.

## Capabilities

### New Capabilities

- `email-lifecycle`: Monitor per-event send jobs and chapter email
  deliverability/engagement.

## Impact

- Endpoints (read-only): `email_send_jobs_summary`, `email_send_jobs_list`,
  `email_send_job_get`, `email_send_job_throughput_get`,
  `email_campaign_performance_get`, `email_deliverability_health_get`,
  `email_fatigue_risk_get`.
- Gated by the `subscribers_sponsors` API group (degrade gracefully when
  disabled); city-owner scope only. Series owners are not authorized.
- New screen + backend fetch/cache; polling should be gentle given per-endpoint
  rate limits.
