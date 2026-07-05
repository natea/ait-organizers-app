# Spec: past-events

## ADDED Requirements

### Requirement: Upcoming/Past tab navigation
The events overview SHALL present a segmented control with "Upcoming" and "Past"
tabs. Selecting a tab SHALL show only events of that kind from the cache, and the
active tab SHALL be visually distinct.

#### Scenario: Switch to Past
- **WHEN** the user selects the "Past" tab
- **THEN** the overview renders only cached past events and the Past tab is highlighted

#### Scenario: Switch back to Upcoming
- **WHEN** the user selects the "Upcoming" tab
- **THEN** the overview renders only cached upcoming events

### Requirement: Past events data source and caching
The app SHALL fetch the caller's past events via `meetups/search` with
`status=past` and cache them, marked distinctly from upcoming events. Past events
SHALL be fetched on a slow cadence (app launch and manual refresh) and SHALL NOT
be polled on the upcoming interval.

#### Scenario: Past events cached
- **WHEN** a past-events fetch succeeds
- **THEN** the returned completed events are stored with a past marker and appear under the Past tab

#### Scenario: Upcoming refresh preserves past cache
- **WHEN** the upcoming poll cycle runs and refreshes upcoming events
- **THEN** cached past events are not deleted or altered by that cycle

### Requirement: Past events excluded from live signals
Past events SHALL NOT trigger change notifications and SHALL NOT be considered for
the tray "next event" widget.

#### Scenario: No notification for past events
- **WHEN** a past-events fetch stores or updates a completed event
- **THEN** no OS notification is emitted for that event

#### Scenario: Tray ignores past events
- **WHEN** the tray "next event" is computed
- **THEN** only upcoming (future) events are eligible; past events are never selected

### Requirement: Past event card variant
A past-event card SHALL show a "held" date chip instead of a countdown, a
check-in gauge (checked-in vs capacity) when a check-in count is available, and an
"Attended" figure in place of the "Waitlisted" funnel cell.

#### Scenario: Past card rendering
- **WHEN** a past event with a known held date and final counts is shown
- **THEN** the card shows the held-date chip, an Attended value, and (if capacity known) a checked-in gauge

#### Scenario: Check-in count unavailable
- **WHEN** the checked-in count is not available for a past event
- **THEN** the card falls back to the final attending count, labeled accordingly, without erroring

### Requirement: Past event detail recap
The detail view for a past event SHALL use recap framing: a "held" chip, an
"Attended" row in the RSVP summary, "final" labels, and a footer indicating the
data is frozen and the event is no longer polled.

#### Scenario: Past detail rendering
- **WHEN** the user opens a past event
- **THEN** the detail shows the held chip, an Attended row, final-labeled summary, and a "recap — data frozen" footer

### Requirement: Recap window transparency
WHEN the past-events list is bounded (recap window or API cap), the Past tab SHALL
indicate that only recent past events are shown rather than the full history.

#### Scenario: Bounded past list
- **WHEN** the past-events fetch is limited by the recap window or API cap
- **THEN** the Past tab shows a notice that it lists recent past events, not the complete history
