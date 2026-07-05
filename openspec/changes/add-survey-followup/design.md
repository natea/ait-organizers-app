# Design: add-survey-followup

## Context

Builds on the shipped past-event detail view from `add-past-events-tab`. That
view already renders a recap for a concluded event (held chip, final
Total/Attending/Waitlisted/Cancelled funnel, real door check-in count, frozen
footer) from the SQLite cache; the frontend never calls the API directly. This
change adds a post-event follow-up panel to that same detail view: survey
response coverage plus sentiment/themes, and follow-up email engagement, so an
organizer sees "how did the survey land and did the follow-up get opened" right
next to the event that generated it.

Three read-only endpoints supply the data (all `GET`, all returning the standard
`{ok,data,error{code}}` envelope, all Bearer-authorized):

- `meetups/survey_diagnostic` (`meetup_survey_diagnostic_get`) — per-meetup
  survey state: scheduler eligibility gates, survey presence/open state,
  settings, attendee counts, and survey **email counts** (sent/opened). Scoped
  by `meetup_token` (or `content_page_token`).
- `meetups/survey_report` (`meetup_survey_report`) — survey coverage across
  completed survey-enabled meetups in a lookback window (`days`, default 90). Per
  the OpenAPI it is a report/rollup across meetups, not a per-meetup response
  digest; we use it for response-rate/coverage context and locate our event's row
  by `meetup_token`.
- `analytics/email/campaign_performance` (`email_campaign_performance_get`) —
  campaign engagement (sends/delivered/opens/clicks). Scoped to the event with
  `meetup_token`; we read the follow-up campaign row(s) for that meetup.

Sentiment/themes: the survey endpoints expose counts and settings, not
free-text NLP. Where the API returns aggregate sentiment or theme tallies in the
diagnostic/report payload we surface them; where it does not, the panel shows the
quantitative coverage (response rate, sent/opened) and omits the themes block
rather than fabricating sentiment. This is settled in D3.

## Goals / Non-Goals

**Goals:**
- A follow-up panel on the existing past-event detail: survey response rate +
  any available sentiment/themes, and follow-up email engagement.
- Read-only. Fetched on the same slow cadence as the rest of past-event recap
  data (detail open / manual refresh), cached in SQLite, rendered from cache.
- Reuse the existing detail cache, renderer, and degradation machinery; add one
  bounded panel, not a new screen.
- Graceful, per-section degradation when an endpoint is forbidden or empty.

**Non-Goals:**
- Sending or scheduling follow-up email (a later change may add "send follow-up").
- Network-wide survey dashboards or the cross-meetup `survey_report` rollup as its
  own screen — this change only reads the current event's row/context.
- Free-text NLP or synthesizing sentiment the API does not return.
- Any new polling cadence, notification, or tray behavior for past events.

## Decisions

**D1 — Panel lives on past-event detail, recap-framed, cache-only.**
The panel renders only for `kind='past'` events, below the existing recap
sections, consistent with the frozen-recap framing. The frontend reads a new
cached blob for the event; it does not call the API. Chosen over a separate
"surveys" screen because the insight is per-event and the proposal's value is
"next to the event that generated them."

**D2 — Fetch on detail open + manual refresh only; never on the upcoming poll.**
Follow-up/survey data is frozen recap data. Sync fetches the three endpoints for a
past event when its detail is first opened (and on manual refresh), upserts a
`survey_followup` cache row keyed by `meetup_token`, and never touches them in the
2-minute upcoming loop — mirroring D4 of `add-past-events-tab`. This bounds API
usage: no eager fetch for every past card.

**D3 — Sentiment/themes are opportunistic, not fabricated.**
Response rate and email counts come straight from the diagnostic (attendee counts
+ survey email counts) and the report row. If the payload carries aggregate
sentiment/theme tallies, render them; otherwise omit the themes block. We never
derive sentiment from counts. Alternative (client-side NLP over raw responses)
rejected — the API does not expose raw response text to this app and it would be a
write/compute surface out of scope.

**D4 — Response rate is derived and labeled, guarded against bad denominators.**
Response rate = survey responses ÷ eligible attendees, from the diagnostic's
attendee/response counts. If the denominator is zero/unknown, show the raw
response count without a percentage rather than a divide-by-zero or a fake 0%.
Consistent with the existing "implausible conversion suppressed" rule on past
detail.

**D5 — Follow-up engagement uses `campaign_performance` scoped to the meetup.**
We request `email_campaign_performance_get?meetup_token=…` and read the follow-up
campaign row(s) (sends/delivered/opened/clicked → open rate, click rate). Chosen
over the diagnostic's survey email counts for the *engagement* figures because
campaign_performance is the endpoint of record for opens/clicks; the diagnostic's
email counts feed the survey-invite response-rate context only.

**D6 — One cache row per event, per-source availability flags.**
Store a single `survey_followup` row per `meetup_token` holding the survey summary
JSON, the email-engagement JSON, and a per-source status (`ok` /
`forbidden_api_group` / `forbidden_scope` / `forbidden_role` / `unavailable` /
`empty`). `get_survey_followup(meetup_token)` returns it. The renderer branches
per source so one forbidden endpoint degrades only its sub-section. Mirrors the
existing performance-degradation pattern in `event-detail`.

## Risks / Trade-offs

- [Survey API group disabled for a chapter] → per-source status carries
  `forbidden_api_group`; the panel shows a non-blocking "not available" state for
  that sub-section and the rest of the detail still renders. Same for
  `forbidden_scope`/`forbidden_role`.
- [`survey_report` is a cross-meetup rollup, our event may not be in the window] →
  locate by `meetup_token`; if absent, fall back to the diagnostic's per-meetup
  counts and skip the report-derived context rather than showing a wrong row.
- [Sentiment/themes absent from payload] → omit the themes block (D3); never
  fabricate.
- [Zero/unknown eligible-attendee denominator] → show raw response count, suppress
  the percentage (D4).
- [Extra API calls on every past-detail open] → three calls, only on first open
  per event, then served from cache; not added to the poll loop (D2).
- [Follow-up campaign ambiguous when a meetup has several campaigns] → sum
  meetup-scoped follow-up rows for headline engagement; do not attribute to a
  single campaign we cannot identify.

## Open Questions

- Exact field names for sentiment/theme tallies in the diagnostic/report payload —
  confirm against a live response at implementation; gate the themes block on their
  presence.
- Whether `survey_report` should be called at all for a single event or whether the
  diagnostic alone suffices — default to diagnostic-first, report only for response-
  rate context, decided at implementation against real shapes.
