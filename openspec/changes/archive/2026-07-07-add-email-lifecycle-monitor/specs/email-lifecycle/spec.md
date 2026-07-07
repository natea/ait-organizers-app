# Spec: email-lifecycle

## ADDED Requirements

### Requirement: Per-event send-job status and delivery accounting
The app SHALL provide a per-event Email panel that renders, from the SQLite
cache, aggregate send-job delivery for the event sourced from
`email_send_jobs_summary` (scoped by `meetup_token`) and the event's send jobs
from `email_send_jobs_list`. The panel MUST show sent, pending, and suppressed
counts, intended-recipient count, and per-job status (queued, sending,
completed, failed) with delivered percent. The panel MUST NOT expose any write,
compose, retry, or send action, and MUST NOT display individual recipient email
addresses.

#### Scenario: Event send summary rendered from cache
- **WHEN** the user opens the Email panel for an event whose send-job summary is cached
- **THEN** the panel shows sent, pending, suppressed, and intended-recipient counts and a list of the event's send jobs with status and delivered percent

#### Scenario: No email activity for the event
- **WHEN** an event has no cached send jobs
- **THEN** the panel shows a neutral "no email sent for this event yet" empty state rather than an error

### Requirement: Active-send throughput monitoring
The app SHALL surface near-real-time progress for an active send job using
`email_send_job_get` (send progress, observed rate, predicted finish) and
`email_send_job_throughput_get` (per-bucket sent counts with peak and average
rates). Throughput polling SHALL occur only while the Email panel is open and at
least one job is active (queued/sending/active), SHALL use a gentle cadence, and
SHALL stop once all jobs are completed or failed. A completed job's last snapshot
SHALL be frozen in cache and no longer polled.

#### Scenario: Watching an active send drain
- **WHEN** the Email panel is open and a send job is active
- **THEN** the app polls throughput on a gentle cadence and updates the sent-per-bucket series, observed rate, and predicted finish

#### Scenario: Polling stops when the send completes
- **WHEN** all of an event's send jobs have reached completed or failed
- **THEN** the app stops polling throughput for that event and shows the frozen final counts

### Requirement: Event open/click performance
The app SHALL render campaign open and click performance for an event from
`email_campaign_performance_get` (scoped by `meetup_token`), showing delivery
rate, open rate, and click rate as aggregate rates. The app SHALL store and
display only aggregate campaign counts and rates, never per-recipient data.

#### Scenario: Event email performance shown
- **WHEN** campaign performance for an event is cached
- **THEN** the panel shows delivery rate, open rate, and click rate for the event's sends

#### Scenario: Performance unavailable but send data present
- **WHEN** campaign performance is not available for an event but send-job data is
- **THEN** the panel still shows send/delivery accounting and omits the open/click section without erroring

### Requirement: Chapter deliverability and fatigue-risk view
The app SHALL provide a chapter-level view rendering sender-domain deliverability
health from `email_deliverability_health_get` (health score, per-domain
sent/delivered/bounce/complaint/unsubscribe rates and status), a fatigue-risk
tier summary from `email_fatigue_risk_get`, and recent send jobs from
`email_send_jobs_list`. This view SHALL be fetched on app launch and manual
refresh only (not on the event polling interval). The app SHALL store and render
only aggregate deliverability metrics and fatigue **tier summary** counts; it
MUST NOT store or display per-subscriber fatigue rows or subscriber email
addresses.

#### Scenario: Deliverability health rendered
- **WHEN** the chapter deliverability data is cached
- **THEN** the view shows the health score, per-sender-domain rates with status, and the fatigue-risk tier summary

#### Scenario: Bounded results labeled as a recent window
- **WHEN** a deliverability or send-job response sets a `truncated` flag or is limited to a recent window
- **THEN** the view indicates it shows recent results, not the complete history

#### Scenario: Chapter view not polled on the event loop
- **WHEN** the event polling interval runs
- **THEN** it does not fetch chapter deliverability or fatigue data, which refresh only on launch and manual refresh

### Requirement: Gentle polling within rate limits
The app SHALL honor the documented per-endpoint rate limits and rate-limit
response headers for all email-lifecycle calls, and SHALL apply backoff on a 429
response using the returned retry-after before retrying. Email polling SHALL
yield to the shared rate budget rather than starve other features.

#### Scenario: Rate headers tracked
- **WHEN** an email-lifecycle call returns rate-limit headers
- **THEN** the app records the remaining/reset values and paces subsequent calls accordingly

#### Scenario: Backoff on 429
- **WHEN** an email-lifecycle call returns a 429 with a retry-after
- **THEN** the app waits at least the retry-after duration before that surface polls again

### Requirement: Graceful degradation on forbidden API group or scope
The app SHALL treat the Email panel as gated by the `subscribers_sponsors` API
group and city-owner scope. WHEN an email-lifecycle call returns
`forbidden_api_group` or `forbidden_scope`, the app SHALL mark the feature
blocked, stop re-polling that surface, and show a specific, non-alarming
explanation instead of a red error. Series owners without city-owner scope SHALL
see this degraded state.

#### Scenario: Subscribers group disabled
- **WHEN** an email-lifecycle call returns `forbidden_api_group`
- **THEN** the Email panel shows that it needs the subscribers group enabled on the key and stops polling that surface

#### Scenario: Caller lacks city-owner scope
- **WHEN** an email-lifecycle call returns `forbidden_scope` (e.g. a series-owner key)
- **THEN** the Email panel shows that it needs city-owner access and does not present partial or fabricated email data
