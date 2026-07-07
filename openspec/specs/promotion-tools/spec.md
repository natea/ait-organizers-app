# promotion-tools Specification

## Purpose
TBD - created by archiving change add-promotion-tools. Update Purpose after archive.
## Requirements
### Requirement: Generate social post drafts
The Promote panel SHALL let an organizer generate a per-platform social post
package for an event by calling `social_post_generate` with `source_type`,
`source_ref`, `platform` (`linkedin` or `x`), and `goal`. The generated draft
MUST be persisted per event and platform and rendered from the cache. Outputs
are drafts for copy/export only; the app MUST NOT post to any external platform.

#### Scenario: Organizer generates a LinkedIn post
- **WHEN** the organizer picks platform LinkedIn and goal `promote` and clicks Generate for a cached event
- **THEN** the app calls `social_post_generate` with the event's `source_ref`, and on success stores and displays the returned post draft with a copy-to-clipboard action

#### Scenario: Cached draft renders without regenerating
- **WHEN** the organizer reopens the Promote panel for an event that already has a stored social draft for the selected platform
- **THEN** the app renders the stored draft and its `generated_at` timestamp from the cache without issuing a new generation call

#### Scenario: Platform-specific drafts are kept separately
- **WHEN** the organizer generates drafts for both LinkedIn and X for the same event
- **THEN** each platform's latest draft is stored and shown independently, and regenerating one does not overwrite the other

### Requirement: Generate an event promo package
The Promote panel SHALL let an organizer generate a launch-ready event promo
package by calling `event_promo_generate` with the event's `meetup_token`,
`package_type` (`launch`, `reminder`, `final_push`, `recap`, or
`full_campaign`), and `audience`. The returned package MUST be persisted per
event and rendered from the cache. This action MUST NOT mutate any attendee
data.

#### Scenario: Organizer generates a full campaign package
- **WHEN** the organizer selects package type `full_campaign` and audience `general` and clicks Generate
- **THEN** the app calls `event_promo_generate` with the event's `meetup_token`, stores the returned package, and renders its sections with an export/copy affordance

#### Scenario: Regenerate replaces the stored package
- **WHEN** a promo package already exists for the event and the organizer clicks Regenerate with a different package type
- **THEN** the newly generated package replaces the stored package for that event and the displayed `generated_at` updates

### Requirement: Generate discussion topics
The Promote panel SHALL let an organizer generate moderated discussion topics
for an event by calling `discussion_topics_generate` with the event's
`meetup_token`. The returned topics MUST be persisted per event and rendered
from the cache.

#### Scenario: Organizer generates discussion topics
- **WHEN** the organizer clicks Generate discussion topics for a cached event
- **THEN** the app calls `discussion_topics_generate` with the event's `meetup_token` and displays and stores the returned topics list

#### Scenario: Topics render from cache offline
- **WHEN** the app is offline and stored discussion topics exist for the event
- **THEN** the panel renders the stored topics and their `generated_at` without attempting a network call

### Requirement: Logo and brand asset search
The Promote panel SHALL let an organizer search AI Tinkerers logos and brand
assets by calling `logo_search` with a `query`, a `scope` (`smart_match` or
`library`), and an optional `include_co_branded` flag. Results MUST be rendered
for the organizer to reference in co-branded promo. Results MAY be cached by
query parameters since logo search is a lightweight GET rather than a billed
generation.

#### Scenario: Organizer searches for a city logo
- **WHEN** the organizer enters a city name and submits a logo search
- **THEN** the app calls `logo_search` with that query and displays the returned logo results

#### Scenario: Include co-branded logos
- **WHEN** the organizer enables the co-branded option and searches
- **THEN** the app calls `logo_search` with `include_co_branded=true` and the results include co-branded logos

### Requirement: Asynchronous generation with progress and timeout handling
Each promotion generation SHALL run as a tracked asynchronous job that returns
immediately with a job identifier and reports progress state (`pending`,
`running`, `ready`, `error`, `timeout`), because generation calls are slow (up
to ~25s) and rate-limited. The panel MUST show progress while a job runs, MUST use a
client-side timeout above the server ceiling (~30s), and MUST NOT block the UI
or place generation endpoints on the background poll loop. A second kickoff for
the same event, kind, and platform while a job is running MUST be suppressed.

#### Scenario: Progress shown during a slow generation
- **WHEN** a generation job is `pending` or `running`
- **THEN** the panel shows a progress indicator for that action and disables its Generate button until the job resolves

#### Scenario: Generation times out
- **WHEN** a generation job exceeds the client-side timeout without a response
- **THEN** the job is marked `timeout`, the panel shows a retry affordance, and any previously cached draft for that action remains visible

#### Scenario: Duplicate kickoff is suppressed
- **WHEN** the organizer clicks Generate again for an action whose job is already `pending` or `running`
- **THEN** no new request is issued and the existing in-flight job continues

#### Scenario: Rate limited response
- **WHEN** a generation call returns `429`
- **THEN** the app honors `Retry-After`, keeps any cached draft visible, and offers retry rather than marking the action permanently unavailable

### Requirement: Degradation on forbidden generation endpoints
The corresponding Promote action SHALL be marked unavailable with a
non-blocking "not enabled for your chapter" state when a promotion endpoint
returns a `forbidden_role`, `forbidden_scope`, or `forbidden_api_group` error.
Other promotion actions and the rest of the event detail view MUST continue to
work.

#### Scenario: Promotion API group not enabled
- **WHEN** a generation call returns `forbidden_api_group`
- **THEN** that action shows a non-blocking "not enabled for your chapter" state and the remaining promotion actions and event detail render normally

#### Scenario: Insufficient role
- **WHEN** a generation call returns `forbidden_role` or `forbidden_scope`
- **THEN** that action is disabled with an explanatory message and does not surface as a hard error

