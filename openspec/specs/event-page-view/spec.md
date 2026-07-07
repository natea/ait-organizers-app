# event-page-view Specification

## Purpose
TBD - created by archiving change add-event-page-rsvp-view. Update Purpose after archive.
## Requirements
### Requirement: Public event page rendering
The event detail view SHALL render the event's public content page — the body
authored on aitinkerers.org (markdown/HTML), the page title, and author/editorial
metadata — from the cache via a Tauri command. The page SHALL be fetched with
`content_page_get` (using the event's content page token, or the fallback slug)
on detail open and detail refresh, cached keyed to the event, and rendered
read-only. The rendered body MUST be inert: scripts SHALL NOT execute and forms
SHALL NOT post from within the app. A deep link to the live event/content page URL
SHALL be shown.

#### Scenario: Content page available
- **WHEN** the user opens an event whose content page is cached
- **THEN** the detail view renders the page title, body, author/editorial metadata, and a deep link to the live URL

#### Scenario: Event has no public page
- **WHEN** the event has no resolvable content page token or slug
- **THEN** the panel shows a "no public page" empty state and does not call `content_page_get`

#### Scenario: Body is inert
- **WHEN** the content page body contains scripts or forms
- **THEN** the app renders the body as read-only content without executing scripts or posting forms, and the live URL is the path to full interactivity

### Requirement: RSVP flow summary
The detail view SHALL present the full RSVP flow for the event —
registered → attending → waitlisted → cancelled → checked-in — using
`rsvp_summary`. The registered/attending/waitlisted/cancelled counts SHALL come
from `rsvp_summary` grouped by status, and the checked-in count SHALL come from
`rsvp_summary` with `status=checked_in` (NOT from performance "completed RSVPs").
The Total SHALL be derived as attending + waitlisted + cancelled; a bare
"Registered" value SHALL NOT be presented as the headline because the API's
`registered` field is unreliable. Conversion SHALL be shown only when it is a
valid fraction (≤ 100%).

#### Scenario: Full funnel rendered
- **WHEN** the RSVP summary for the event is cached
- **THEN** the detail view shows attending, waitlisted, cancelled, a derived Total, and a real checked-in figure from `status=checked_in`

#### Scenario: Checked-in source is authoritative
- **WHEN** both a performance "completed RSVPs" value and a `status=checked_in` count exist
- **THEN** the checked-in figure shown is the `status=checked_in` count, not the performance value

#### Scenario: Implausible conversion suppressed
- **WHEN** the computed conversion exceeds 100%
- **THEN** the conversion figure is hidden rather than displayed

### Requirement: Page traffic and email metrics
The detail view SHALL show page-traffic figures for the event from the cached
`meetup_performance_get` row (page views and completed-RSVP conversion) and,
when a content page token is present and the metrics scope is enabled, email
metrics (sends, opens, clicks) from `content_page_metrics_get`. These figures
SHALL be read from the cache; the app SHALL NOT expose raw analytics events or
visitor-level details.

#### Scenario: Traffic figures rendered
- **WHEN** a cached `meetup_performance_get` row exists for the event
- **THEN** the panel shows page-view traffic and completed-RSVP conversion alongside the RSVP flow

#### Scenario: Email metrics available
- **WHEN** the event has a content page token and `content_page_metrics_get` returned metrics
- **THEN** the panel shows sends, opens, and clicks for the page

#### Scenario: Email metrics absent
- **WHEN** the event has no content page token or no metrics were returned
- **THEN** the email-metrics figures are omitted and the rest of the panel still renders

### Requirement: Graceful degradation of event-page sections
The three event-page sections (page body, traffic/email metrics, and RSVP flow) SHALL each degrade independently. When a section's endpoint returns an
authorization error (`forbidden_api_group` or `forbidden_scope`) or is otherwise
unavailable during sync, that section SHALL render a non-blocking "not enabled"
state while the other sections still render.

#### Scenario: Content pages group forbidden
- **WHEN** `content_page_get` returns `forbidden_api_group` for the caller's key
- **THEN** the page-body section shows a "not enabled" state and the RSVP flow and traffic sections still render

#### Scenario: RSVP scope forbidden
- **WHEN** `rsvp_summary` returns a `forbidden_scope` error
- **THEN** the RSVP-flow section shows a "not enabled" state and the page body still renders

#### Scenario: Metrics endpoint unavailable
- **WHEN** `content_page_metrics_get` or `meetup_performance_get` is unavailable during sync
- **THEN** the traffic/email-metrics section shows a non-blocking "not available" state and the rest of the detail view renders

