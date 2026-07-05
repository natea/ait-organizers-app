# Design: add-event-mission-control-v1

## Context

Greenfield repo (only `docs/` exists). The app consumes the AI Tinkerers Agents API — contract in `docs/agents-api.md`, canonical OpenAPI at `https://aitinkerers.org/api/agents/v1/openapi.yaml`. Exploration with a live `city_owner` key confirmed the read endpoints return everything the dashboard needs (RSVP funnels, capacity, awaiting-payment, performance, gallery photos with captions).

API constraints that drive the design:
- Bearer/`X-API-Key` header auth only; keys in query params or body are rejected with 400.
- No pagination on search/list endpoints; responses are capped best-effort with a `truncated` flag.
- Per-key, per-endpoint rate limits (~20–60 rpm at the 1x city-owner tier) with `X-RateLimit-*` response headers and `Retry-After` on 429.
- Sync reads have 6–8s server-side timeouts; all responses use the `{ok, data, error{code}}` envelope.
- Structured authorization errors: `forbidden_role`, `forbidden_scope`, `forbidden_api_group` — chapters can disable API groups independently.

## Goals / Non-Goals

**Goals:**
- Ambient, always-current view of upcoming events for any `city_owner` (or reader) API key.
- Native feel: menubar widget, OS notifications, offline rendering from cache.
- Polite API citizenship: header-aware pacing, exponential backoff with jitter, no redundant fetches.
- Secure key handling: OS keychain only.

**Non-Goals:**
- Any write/mutation endpoints (Attio push, comments, publishing).
- Promo generation, sponsors, speakers, email monitoring, docs chat (future versions).
- Multi-key / multi-account support.
- Windows/Linux polish (build for macOS first; Tauri keeps ports cheap).

## Decisions

**D1 — Tauri 2.x with TypeScript frontend, Rust for privileged operations.**
Rust side owns keychain access (`keyring` crate), the polling scheduler, HTTP calls, and SQLite. The frontend is a pure renderer over cached data plus commands. Alternative considered: Electron — rejected for footprint and because Tauri's tray/notification plugins cover the native surface we need. Frontend tooling uses bun + Vite; keep the UI framework light (React or Svelte — implementer's choice, nothing here demands one).

**D2 — SQLite as the single source of truth for the UI.**
The frontend never calls the API directly; it reads from SQLite (via `tauri-plugin-sql` or a thin Rust command layer) and subscribes to change events emitted after each poll cycle. Rationale: the API forbids pagination and rate-limits per endpoint, so re-fetching per render is both impossible and impolite; a cache also gives offline rendering for free. Tables: `events`, `rsvp_summaries`, `awaiting_payment`, `performance_snapshots`, `sync_state` (per-endpoint last-fetch, rate-limit remaining, backoff-until).

**D3 — Single poll loop with per-endpoint budgets, not per-screen fetching.**
One Rust scheduler ticks (default every 2 minutes for the next upcoming event, 10 minutes for others), fans out the small set of read calls, diffs results against SQLite, writes updates, and emits events. It reads `X-RateLimit-Remaining` and defers when it approaches zero; on 429 it honors `Retry-After` and applies exponential backoff with jitter (base 1s, max 60s) per the doc's recommended client behavior. Alternative: fetch-on-navigation — rejected because the tray widget needs freshness regardless of window state.

**D4 — Notifications come from cache diffs, not API push.**
The API has no webhooks/streaming. Poll-diff on `rsvp_summary` counts (attending, waitlisted) for the next event drives OS notifications ("Boston meetup: 91 → 94 attending"). Debounce so one poll cycle produces at most one notification per event.

**D5 — Typed client generated from the canonical OpenAPI spec.**
Generate TypeScript types (and optionally Rust types) from `https://aitinkerers.org/api/agents/v1/openapi.yaml` at build time, vendored into the repo with a refresh script — the doc explicitly tells consumers to fetch from the canonical URL rather than hand-vendor a stale copy. Envelope handling (`ok`/`error.code`) is wrapped once in the Rust HTTP layer.

**D6 — Graceful degradation on authorization errors.**
`forbidden_api_group` / `forbidden_scope` on a feature's endpoint marks that feature unavailable in `sync_state` and the UI shows a "not enabled for your chapter" state instead of erroring. `auth/validate`'s `enabled_api_groups` is checked at onboarding to preempt most of these.

**D7 — Key storage: OS keychain via Rust `keyring` crate.**
The key never touches the JS side after onboarding entry, never lands in config files or logs, and HTTP requests attach it Rust-side. Onboarding validates with `GET /auth/validate` before persisting.

## Risks / Trade-offs

- [Poll-based freshness: counts can lag up to one interval] → Short interval (2 min) for the next event only; manual refresh button bypasses schedule but still respects rate-limit headers.
- [API is "best-effort" and may truncate lists] → Surface `truncated` flags in the UI (e.g., "showing top 25") rather than implying completeness.
- [Per-chapter API-group toggles mean features silently vary between users] → D6 degradation plus onboarding summary of what's enabled.
- [OpenAPI spec drift vs. shipped app] → Envelope wrapper treats unknown fields as pass-through; CI job re-fetches spec and fails on breaking diffs.
- [Tray + notification behavior differs per OS] → Target macOS first; keep tray logic behind a small trait so ports don't touch business logic.

## Open Questions

- UI framework (React vs Svelte) — implementer's choice at scaffold time.
- Whether `upcoming_events_list` alone suffices for the caller's own events or a `meetup_search`/weblog-scoped query is needed as fallback — verify against the live API during implementation.
