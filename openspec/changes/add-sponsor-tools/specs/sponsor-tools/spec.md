## ADDED Requirements

### Requirement: Sponsor search

The app SHALL let a city owner search sponsors by name, industry, or city via the
`sponsor_search` endpoint, cache the results in SQLite, and render matches from the
cache. Each match SHALL show the sponsor name, website/domain, city, and short profile.
The search SHALL support optional `city`, `industry`, and `active_only` filters and MUST
respect the API result cap.

#### Scenario: Search returns matches

- **WHEN** a city owner searches for "acme" and the API returns sponsor matches
- **THEN** the app caches the matches and renders sponsor cards from SQLite showing name, domain, city, and short profile

#### Scenario: No matches

- **WHEN** a sponsor search returns zero matches
- **THEN** the Sponsors screen shows an empty "no sponsors found" state distinct from a not-yet-searched state

#### Scenario: Results truncated at the cap

- **WHEN** the sponsor search response is capped at the API maximum of 25 matches
- **THEN** the app indicates that only the top results are shown

### Requirement: Sponsor contact list with masking

The app SHALL let a city owner list contacts for a selected sponsor via the
`sponsor_contact_list` endpoint (by `sponsor_ref`), cache the contacts, and render role,
title, and contact fields from the cache. The app MUST render email and phone fields
exactly as returned by the API and MUST NOT attempt to unmask them; when a field is
masked the app SHALL show a masked indicator with a visibility hint rather than a blank.

#### Scenario: Contacts render from cache

- **WHEN** a city owner opens a sponsor whose contacts have been fetched and cached
- **THEN** the app renders the contacts from SQLite showing role, title, and contact fields without a new network call

#### Scenario: Masked contact fields

- **WHEN** the API returns a contact whose email is masked because email visibility is not enabled for the chapter
- **THEN** the app displays a masked indicator and a hint to enable email visibility, and never displays an unmasked value

#### Scenario: Contact cap respected

- **WHEN** a sponsor has more contacts than the API maximum of 25
- **THEN** the app renders the returned contacts and indicates the list is capped

### Requirement: Generate sponsor research brief

The app SHALL let a city owner generate an AI research brief for an existing sponsor
(`sponsor_ref`) or a free-text company `name` via the `sponsor_research_generate`
endpoint. The generation MUST run without blocking the UI, surface progress while the
call is in flight, and on success cache the `research_summary` as a reusable draft.

#### Scenario: Research brief generated and cached

- **WHEN** a city owner requests a research brief for a sponsor and the API returns a `research_summary`
- **THEN** the app caches the brief as a draft and displays it, and the organizer can reopen it later without regenerating

#### Scenario: Optional grounding inputs

- **WHEN** a city owner supplies an optional `domain` and `city` for the research brief
- **THEN** the app includes them in the request to ground the generated brief

### Requirement: Generate tailored sponsor pitch with event context

The app SHALL let a city owner generate a tailored sponsorship pitch for an existing
sponsor (`sponsor_ref`) or a free-text company `name` via the `sponsor_pitch_generate`
endpoint, including event context (target city, channel, and summarized event details)
in the request `context`. The serialized request body MUST stay within the API's 64 KB
context cap. On success the app SHALL cache the `pitch_text` and any returned variants as
a reusable draft.

#### Scenario: Pitch generated with event context

- **WHEN** a city owner generates a pitch for a sponsor with a selected event and channel
- **THEN** the app includes the event context and channel in the request and caches the returned pitch and variants as a draft

#### Scenario: Context stays within the size cap

- **WHEN** the assembled pitch context would exceed the 64 KB request cap
- **THEN** the app includes only summary fields and truncates free-text context before sending

#### Scenario: Draft reused without regeneration

- **WHEN** a city owner reopens a previously generated pitch draft
- **THEN** the app loads it from the cache without spending a new generation call, and regeneration occurs only on an explicit user action

### Requirement: Asynchronous generation with progress and cancel

The app SHALL run each generation off the UI thread, return a draft handle immediately,
and emit progress states (`queued`, `generating`, `ready`, `failed`) to the frontend,
because generation endpoints are slow (~20s hard timeout) and rate-limited (10 rpm). The
app SHALL let the organizer cancel an in-flight generation and MUST prevent overlapping
generations that would exceed the rate limit.

#### Scenario: Progress surfaced during generation

- **WHEN** a generation kickoff is accepted
- **THEN** the app returns a draft handle immediately and shows progress transitioning from queued to generating without freezing the UI

#### Scenario: Rate limit hit

- **WHEN** a generation request returns a 429 rate-limited response
- **THEN** the app marks the draft failed with a "try again shortly" state rather than a hard error, and does not retry through an alternate path

#### Scenario: Cancel in flight

- **WHEN** the organizer cancels a generation that is still running
- **THEN** the app stops surfacing progress for that draft and does not write a draft body

### Requirement: Gating and scope degradation

Sponsor tools SHALL be gated by the `subscribers_sponsors` API group and available to
city owners only (city-series owners are not authorized). The app MUST treat
`forbidden_api_group` and `forbidden_scope` responses as hard denies and render an
informative disabled state rather than an error toast, and MUST NOT retry a denied
sponsor endpoint through an alternate path.

#### Scenario: API group disabled

- **WHEN** a sponsor endpoint returns `error.code = "forbidden_api_group"`
- **THEN** the Sponsors screen shows a "sponsor tools aren't enabled for this chapter" state and makes no further sponsor calls

#### Scenario: Out-of-scope caller

- **WHEN** a city-series owner or otherwise out-of-scope caller receives `error.code = "forbidden_scope"` from a sponsor endpoint
- **THEN** the app shows a "your role can't use sponsor tools for this chapter" state and does not retry through another endpoint
