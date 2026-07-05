# api-auth Specification

## Purpose
TBD - created by archiving change add-event-mission-control-v1. Update Purpose after archive.
## Requirements
### Requirement: API key onboarding and validation
The app SHALL prompt for an AI Tinkerers Agent API key on first launch and validate it via `GET /api/agents/v1/auth/validate` before persisting it. The onboarding screen SHALL display the resolved owner name, caller roles, and enabled API groups from the validate response.

#### Scenario: Valid key
- **WHEN** the user pastes a key and the validate call returns `ok: true`
- **THEN** the app stores the key and shows the resolved identity, roles, and enabled API groups before proceeding to the events overview

#### Scenario: Invalid key
- **WHEN** the validate call fails or returns invalid
- **THEN** the app does not persist the key and shows an actionable error including where to obtain a key (`aitinkerers.org/profile`)

### Requirement: Keychain-only key storage
The app SHALL store the API key exclusively in the operating system keychain. The key MUST NOT appear in config files, logs, frontend state after onboarding, or error reports.

#### Scenario: Key persisted securely
- **WHEN** onboarding completes successfully
- **THEN** the key is retrievable from the OS keychain and no plaintext copy exists in the app's config directory or log output

#### Scenario: Key removal
- **WHEN** the user chooses "Sign out" in settings
- **THEN** the keychain entry is deleted and the app returns to onboarding

### Requirement: Authenticated request handling
All API requests SHALL attach the key via the `Authorization: Bearer` header from the Rust layer, and SHALL NOT place the key in URL query parameters or request bodies. Responses SHALL be unwrapped from the standard `{ok, data, error}` envelope, mapping `error.code` values (`forbidden_role`, `forbidden_scope`, `forbidden_api_group`, `rate_limited`, `not_found`) to typed errors.

#### Scenario: Envelope error mapping
- **WHEN** an endpoint returns `ok: false` with `error.code = "forbidden_api_group"`
- **THEN** the caller receives a typed authorization error rather than a generic failure, so features can degrade per the background-sync capability

