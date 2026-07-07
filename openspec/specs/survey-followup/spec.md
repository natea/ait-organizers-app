# survey-followup Specification

## Purpose
TBD - created by archiving change add-survey-followup. Update Purpose after archive.
## Requirements
### Requirement: Post-event survey results on past-event detail
The past-event detail view SHALL show a follow-up panel for a concluded event
that presents the survey response rate and, when the API returns them, aggregate
sentiment and themes. The response rate SHALL be derived from the survey
diagnostic's response and eligible-attendee counts and SHALL be labeled as a
survey response rate. The panel SHALL render only for past (`kind='past'`)
events and SHALL read exclusively from the SQLite cache; the frontend SHALL NOT
call the API directly.

#### Scenario: Survey results available
- **WHEN** the user opens a past event whose cached `survey_followup` row has an
  `ok` survey source with responses and an eligible-attendee count
- **THEN** the panel shows the survey response rate (responses over eligible
  attendees) and the response count, next to the existing recap sections

#### Scenario: Sentiment and themes present in payload
- **WHEN** the cached survey summary includes aggregate sentiment or theme tallies
- **THEN** the panel renders a sentiment/themes block from those tallies

#### Scenario: No sentiment or themes in payload
- **WHEN** the cached survey summary has no sentiment or theme tallies
- **THEN** the themes block is omitted and no sentiment is inferred from counts

#### Scenario: Unknown or zero eligible-attendee denominator
- **WHEN** the eligible-attendee count is zero or unknown
- **THEN** the panel shows the raw survey response count without a percentage
  rather than a zero or divide-by-zero rate

### Requirement: Follow-up email engagement
The follow-up panel SHALL show engagement for the event's follow-up email using
meetup-scoped campaign performance: sends, open rate, and click rate derived from
the cached engagement figures. When a meetup has multiple follow-up campaign
rows, the panel SHALL aggregate them for the headline engagement rather than
attributing engagement to a single unidentified campaign.

#### Scenario: Follow-up engagement available
- **WHEN** the cached `survey_followup` row has an `ok` email source with send and
  open counts for the meetup
- **THEN** the panel shows the follow-up send count, open rate, and click rate

#### Scenario: Multiple follow-up campaigns for the meetup
- **WHEN** the meetup has more than one follow-up campaign row in the cache
- **THEN** the panel aggregates the rows into a single headline engagement figure

### Requirement: Survey and follow-up data source and caching
The app SHALL fetch survey and follow-up data for a past event from
`meetups/survey_diagnostic`, `meetups/survey_report`, and
`analytics/email/campaign_performance` (scoped by `meetup_token`) on detail open
and manual refresh only, and SHALL cache the results in a `survey_followup` row
keyed by `meetup_token` with a per-source status. These endpoints SHALL NOT be
polled on the upcoming interval and SHALL NOT be fetched for upcoming events.

#### Scenario: Fetched on first detail open
- **WHEN** the user opens a past event whose `survey_followup` row is not yet cached
- **THEN** the app fetches the three endpoints, upserts a `survey_followup` row
  keyed by `meetup_token`, and the panel renders from that cached row

#### Scenario: Upcoming poll does not fetch survey data
- **WHEN** the 2-minute upcoming poll cycle runs
- **THEN** no survey or follow-up endpoint is called and no `survey_followup` row
  is created or modified by that cycle

#### Scenario: Report row absent for the event
- **WHEN** `meetups/survey_report` returns no row matching the event's `meetup_token`
- **THEN** the app falls back to the diagnostic's per-meetup counts and omits the
  report-derived context rather than showing a non-matching row

### Requirement: Empty and forbidden degradation
The follow-up panel SHALL degrade per source without blocking the rest of the
detail view. When an endpoint returns a forbidden error (`forbidden_api_group`,
`forbidden_scope`, or `forbidden_role`) or is otherwise unavailable, the affected
sub-section SHALL show a non-blocking "not available" state while other
sub-sections and the existing recap SHALL still render. When an endpoint succeeds
but returns no data, the affected sub-section SHALL show an empty state.

#### Scenario: Survey API group disabled
- **WHEN** the survey source status is `forbidden_api_group`
- **THEN** the survey sub-section shows a non-blocking "not available" state and
  the follow-up engagement sub-section and the rest of the detail still render

#### Scenario: Follow-up engagement out of scope
- **WHEN** the email source status is `forbidden_scope` or `forbidden_role`
- **THEN** the engagement sub-section shows a "not available" state and the survey
  sub-section still renders

#### Scenario: Survey enabled but no responses yet
- **WHEN** the survey source status is `ok` but the response count is zero
- **THEN** the survey sub-section shows an empty "no responses" state rather than a
  fabricated rate

#### Scenario: No follow-up email sent
- **WHEN** the email source returns no follow-up campaign rows for the meetup
- **THEN** the engagement sub-section shows an empty "no follow-up sent" state

