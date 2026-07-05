# event-detail Specification

## Purpose
TBD - created by archiving change add-event-mission-control-v1. Update Purpose after archive.
## Requirements
### Requirement: Event drill-down view
Selecting an event SHALL open a detail view showing the grouped RSVP summary (`rsvp_summary`), event metadata (venue city, organizer, event URL), and gallery preview photos with captions when available.

#### Scenario: Detail renders for a cached event
- **WHEN** the user selects an event card
- **THEN** the detail view renders the RSVP summary groups, metadata, and any gallery photos from the cache

### Requirement: Awaiting-payment list
For paid events, the detail view SHALL show the awaiting-payment RSVPs (`rsvp_awaiting_payment_list`) with attendee name and RSVP timestamp so organizers can follow up.

#### Scenario: Paid event with stragglers
- **WHEN** the event has an active payment link and awaiting-payment rows exist
- **THEN** the detail view lists them under an "Awaiting payment" section with a count badge

#### Scenario: Free event
- **WHEN** the event has no active payment link
- **THEN** the awaiting-payment section is hidden

### Requirement: Event performance metrics
The detail view SHALL show event performance data (`meetup_performance_get`): page traffic and RSVP trend over time, rendered as a simple time-series chart.

#### Scenario: Performance data available
- **WHEN** performance snapshots exist in the cache for the event
- **THEN** a traffic/RSVP trend chart renders with the snapshot timestamps

#### Scenario: Performance endpoint unavailable
- **WHEN** the performance endpoint returned an authorization or availability error during sync
- **THEN** the section shows a non-blocking "not available" state and the rest of the detail view still renders

