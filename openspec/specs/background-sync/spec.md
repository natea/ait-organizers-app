# background-sync Specification

## Purpose
TBD - created by archiving change add-event-mission-control-v1. Update Purpose after archive.
## Requirements
### Requirement: Scheduled polling with tiered intervals
A single Rust-side scheduler SHALL poll the read endpoints on tiered intervals: the next upcoming event's data every 2 minutes (configurable), all other upcoming events every 10 minutes. The frontend SHALL NOT call the API directly.

#### Scenario: Tiered refresh
- **WHEN** the scheduler ticks
- **THEN** only endpoints whose tier interval has elapsed since their last successful fetch are called

#### Scenario: Manual refresh
- **WHEN** the user triggers manual refresh
- **THEN** the scheduler runs an immediate cycle, still subject to rate-limit header checks

### Requirement: SQLite read cache
All fetched data SHALL be upserted into a local SQLite database (`events`, `rsvp_summaries`, `awaiting_payment`, `performance_snapshots`, `sync_state`), and the UI SHALL render exclusively from this cache. The app SHALL render last-known data when offline.

#### Scenario: Offline rendering
- **WHEN** the network is unavailable
- **THEN** all screens render from cached data with a visible "last synced at" indicator

### Requirement: Rate-limit aware pacing
The sync layer SHALL read `X-RateLimit-Remaining` on every response and defer further calls to that endpoint within the window when remaining approaches zero. On HTTP 429 it SHALL wait at least `Retry-After` seconds and apply exponential backoff with jitter (base 1s, max 60s) on repeated failures.

#### Scenario: 429 backoff
- **WHEN** an endpoint returns 429 with `Retry-After: 34`
- **THEN** no request is made to that endpoint for at least 34 seconds and `sync_state` records the backoff-until time

#### Scenario: Proactive throttle
- **WHEN** `X-RateLimit-Remaining` drops below a small threshold
- **THEN** remaining calls to that endpoint are deferred to the next window

### Requirement: Graceful capability degradation
WHEN an endpoint returns `forbidden_api_group` or `forbidden_scope`, the sync layer SHALL mark that feature unavailable in `sync_state` and stop polling it; the UI SHALL show a "not enabled for your chapter" state for that feature instead of an error.

#### Scenario: Disabled API group
- **WHEN** the chapter's weblog has an API group disabled and its endpoint returns `forbidden_api_group`
- **THEN** the corresponding UI section shows the not-enabled state and no further calls are made to that endpoint until re-validation

### Requirement: Change events to the frontend
After each poll cycle that writes changed rows, the sync layer SHALL emit a change event to the frontend so visible screens re-render from the cache.

#### Scenario: Live update
- **WHEN** a poll cycle changes an event's attending count
- **THEN** an open overview or detail window updates without user action

