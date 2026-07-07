# Proposal: add-survey-followup

Stack-rank #7 — Survey + post-event follow-up (large outbound volume, moderate action depth).

## Why

Post-event surveys and follow-up are high-volume but the insight is scattered.
Mission Control can close the loop on the past-events recap (from
`add-past-events-tab`) by showing survey results and follow-up engagement right
next to the event that generated them, so organizers learn what worked without
digging through email tools.

## What Changes

- Add a post-event follow-up panel on past-event detail: survey diagnostic +
  report (response rate, sentiment/themes) and follow-up email engagement.
- Read-only in this change; a later change could add "send follow-up".

## Capabilities

### New Capabilities

- `survey-followup`: Show post-event survey results and follow-up engagement for
  a concluded event.

## Impact

- Endpoints (read-only): `meetups/survey_diagnostic`, `meetups/survey_report`,
  `email_campaign_performance_get` (follow-up sends).
- Extends the past-event detail from `add-past-events-tab`; no writes.
- Gated by the relevant API group; degrade gracefully when unavailable.
