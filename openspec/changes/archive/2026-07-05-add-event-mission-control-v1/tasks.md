# Tasks: add-event-mission-control-v1

## 1. Project scaffold

- [x] 1.1 Scaffold Tauri 2.x app with bun + Vite + TypeScript frontend; verify `bun tauri dev` opens a window
- [x] 1.2 Add Rust dependencies: `keyring`, `reqwest`, `rusqlite` (or tauri-plugin-sql), `serde`; add tray and notification plugins
- [x] 1.3 Add spec-refresh script that fetches `https://aitinkerers.org/api/agents/v1/openapi.yaml` and generates TypeScript types; vendor generated output

## 2. API client & auth (specs/api-auth)

- [x] 2.1 Implement Rust HTTP layer: Bearer header injection, `{ok, data, error}` envelope unwrap, typed error codes (`forbidden_role`, `forbidden_scope`, `forbidden_api_group`, `rate_limited`, `not_found`), rate-limit header capture
- [x] 2.2 Implement keychain storage commands (store/retrieve/delete key) using `keyring`; ensure key never crosses to JS after onboarding
- [x] 2.3 Build onboarding screen: key entry → `auth/validate` → show owner, roles, enabled API groups; persist only on success; sign-out flow deletes keychain entry
- [x] 2.4 Verify no plaintext key in config dir or logs after onboarding (grep of project, app data dir, and build output: no key literal; access path is keychain-only)

## 3. Sync engine & cache (specs/background-sync)

- [x] 3.1 Create SQLite schema: `events`, `rsvp_summaries`, `awaiting_payment`, `performance_snapshots`, `sync_state`
- [x] 3.2 Implement poll scheduler with tiered intervals calling `meetups/upcoming`, `rsvps/summary`, `rsvps/awaiting_payment`, `meetups/performance` (upcoming is a single bulk call refreshed every 2 min; per-event scoped detail fetched on demand)
- [x] 3.3 Implement rate-limit pacing: proactive throttle on low `X-RateLimit-Remaining`, `Retry-After` honor on 429, exponential backoff with jitter; record backoff-until in `sync_state`
- [x] 3.4 Implement poll-diff upserts and frontend change-event emission (`sync:updated`, `detail:updated`)
- [x] 3.5 Implement capability degradation: mark features unavailable on `forbidden_*` errors, stop polling them, expose state to UI
- [x] 3.6 Manual refresh command that runs an immediate cycle within rate-limit constraints

## 4. Events UI (specs/events-overview, specs/event-detail)

- [x] 4.1 Events overview: cards from cache with name, local time, countdown, city, status; empty state distinguishing "no events" from "not yet synced"
- [x] 4.2 RSVP funnel + capacity gauge on cards; handle missing capacity; `truncated` notice
- [x] 4.3 Event detail: RSVP summary groups, metadata, gallery photos with captions
- [x] 4.4 Awaiting-payment section (paid events only) with count badge
- [x] 4.5 Performance panel from aggregate row (page views / completed / conversion); non-blocking "not available" state on degraded endpoint
- [x] 4.6 "Last synced at" indicator and offline rendering from cache

## 5. Tray & notifications (specs/tray-notifications)

- [x] 5.1 Menubar item with next event's attending count + days-until; popover with funnel and open-detail link; idle state when no events
- [x] 5.2 Poll-diff notifications on attending/waitlisted changes (old → new), max one per event per cycle
- [x] 5.3 First-sync and post-sign-in suppression; notification on/off setting

## 6. Verification

- [x] 6.1 End-to-end: real API key used to probe every endpoint (validate, upcoming, summary, performance, awaiting) confirming live shapes; full UI flow (onboarding → identity → overview → detail available + degraded → popover) driven in-browser against real response shapes. Note: `meetups/upcoming` currently returns no future events for the caller's scope, so a live app run shows the empty state.
- [x] 6.2 Kill network → screens render from cache with stale indicator (UI reads exclusively from SQLite; `getEvents` fallback + "Not synced yet" indicator)
- [x] 6.3 429 handling: `Retry-After` parsed from response, backoff recorded in `sync_state`, backoff window respected before retry (exponential w/ 60s cap)
- [x] 6.4 Grep build artifacts and app config dir for the API key → no plaintext occurrences
