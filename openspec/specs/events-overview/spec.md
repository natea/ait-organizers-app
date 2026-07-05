# events-overview Specification

## Purpose
TBD - created by archiving change add-event-mission-control-v1. Update Purpose after archive.
## Requirements
### Requirement: Upcoming events list
The app SHALL display the caller-scoped upcoming events (sourced from `upcoming_events_list` / `meetup_search` via the sync cache) as cards ordered by start time. Each card SHALL show event name, local start time with timezone, days-until countdown, city, and status.

#### Scenario: Events render from cache
- **WHEN** the user opens the events overview and cached event rows exist
- **THEN** event cards render immediately from SQLite without waiting on a network call

#### Scenario: No upcoming events
- **WHEN** the sync cache contains no upcoming events for the caller's scope
- **THEN** the overview shows an empty state distinguishing "no events scheduled" from "sync not yet completed"

### Requirement: RSVP funnel and capacity display
Each event card SHALL show the RSVP funnel (registered, attending, waitlisted, cancelled) and a capacity gauge (attending vs. capacity) from the event's RSVP data.

#### Scenario: Funnel counts shown
- **WHEN** an event has RSVP data `attending: 91, waitlisted: 23, cancelled: 10, capacity: 150`
- **THEN** the card shows each funnel stage count and a gauge at 91/150

#### Scenario: Missing capacity
- **WHEN** an event has no capacity value
- **THEN** the gauge is omitted and raw counts are still shown

### Requirement: Truncation transparency
WHEN the API response for the events list sets `truncated: true`, the overview SHALL indicate that only the top results are shown.

#### Scenario: Truncated list
- **WHEN** the cached events list was truncated by the API cap
- **THEN** the overview displays a "showing top N events" notice

