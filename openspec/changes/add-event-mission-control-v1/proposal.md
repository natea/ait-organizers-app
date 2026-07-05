# Proposal: add-event-mission-control-v1

## Why

AI Tinkerers city organizers monitor RSVPs, waitlists, payment stragglers, and event health through the web UI, which requires actively remembering to check and clicking through several pages per event. The AI Tinkerers Agents API (`https://aitinkerers.org/api/agents/v1`) now exposes everything needed for a live, ambient view of event health — this v1 packages it as a native Tauri desktop app so any `city_owner` can watch their upcoming events without babysitting a browser tab.

## What Changes

- New Tauri 2.x desktop app (TypeScript frontend, Rust shell) — this repo currently has no application code.
- API-key onboarding flow: paste key → validate via `GET /auth/validate` → display resolved identity, role, and enabled API groups. Key is stored in the OS keychain, never in plaintext config.
- Events overview screen: upcoming events for the caller's scope with countdown, capacity gauge, and RSVP funnel (registered / attending / waitlisted / cancelled).
- Event detail screen: grouped RSVP summary, awaiting-payment list, event performance trend, and gallery preview photos.
- Menubar/tray widget: next event's live attending count and days-until, with native notifications when RSVP counts change (poll-diff).
- Rate-limit-aware background poller with local SQLite read cache (API has no pagination and per-key rate limits; app must render from cache and back off on 429s).
- Out of scope for v1: promo generation, sponsor workbench, speaker pipeline, email send monitoring, docs chat, and all write/mutation endpoints.

## Capabilities

### New Capabilities

- `api-auth`: API key onboarding, validation, keychain storage, and authenticated request handling (headers, error envelope, rate-limit header awareness).
- `events-overview`: List upcoming events in the caller's scope with RSVP funnel and capacity display.
- `event-detail`: Per-event drill-down with RSVP summary, awaiting-payment list, performance metrics, and photos.
- `background-sync`: Polite polling scheduler with SQLite cache, poll-diff change detection, and 429/backoff handling.
- `tray-notifications`: Menubar widget and native notifications driven by cached event data changes.

### Modified Capabilities

None — greenfield project with no existing specs.

## Impact

- New codebase: Tauri 2.x shell, TypeScript frontend built with bun, Rust commands for keychain and polling.
- External dependency: AI Tinkerers Agents API (contract in `docs/agents-api.md`; canonical OpenAPI at `https://aitinkerers.org/api/agents/v1/openapi.yaml`). Read-only endpoints used: `auth/validate`, `upcoming_events_list`, `meetup_search`, `rsvp_summary`, `rsvp_awaiting_payment_list`, `meetup_performance_get`.
- Feature availability varies by chapter: API groups are toggled per weblog, so the app must degrade gracefully on `forbidden_api_group` / `forbidden_scope` errors rather than fail.
- No server-side changes; the app is a pure API consumer.
