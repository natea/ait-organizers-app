# CLAUDE.md

Guidance for working in this repository.

## What this is

**AI Tinkerers — Event Mission Control**: a native macOS (Tauri 2) desktop app
that gives AI Tinkerers city organizers an ambient, always-current view of their
events — RSVP funnels, capacity, awaiting-payment stragglers, performance, and a
menubar widget with live counts and change notifications. It is a read-only
consumer of the [AI Tinkerers Agents API](https://aitinkerers.org/api/agents/v1/openapi.yaml).

## Design direction

**`design/DESIGN.md` (and `design/BRAND.md`) is the source of truth for all
visual decisions** — colors, typography, spacing, radius, posture. Do not invent
styling; take it from there. `design/mission-control.html` is the interactive
prototype covering every surface (onboarding, overview, detail, tray popover),
and `design/tokens/` holds the CSS custom properties. Key posture: light Slate-50
canvas, white 16px-radius cards, indigo (`#31439b`) reserved for CTAs and data
accents (never large washes), pill chips/buttons, mono (`ui-monospace`) for all
numeric readouts. `src/styles.css` is ported directly from the prototype.

## Structure

```
src/                     TypeScript frontend (vanilla, no framework), built by Vite
  main.ts                app shell + router (screen switching, live-update wiring)
  api.ts                 thin wrappers over Tauri commands + event subscriptions
  types.ts               API response shapes
  util.ts                esc/fmt/num/byId helpers
  styles.css             ported from design/mission-control.html
  screens/               onboarding.ts, overview.ts, detail.ts
  popover.ts             tray popover window
src-tauri/src/           Rust backend
  lib.rs                 Tauri builder: plugins, state, tray, poll loop, commands
  api.rs                 HTTP client: Bearer auth, envelope unwrap, typed errors, rate headers
  keychain.rs            API key storage (OS keychain only)
  db.rs                  SQLite cache: events, rsvp_summaries, awaiting_payment, performance_snapshots, sync_state
  sync.rs                poll scheduler, poll-diff notifications, tray updates, rate-limit backoff
  commands.rs            Tauri commands bridging the frontend
  state.rs / error.rs    shared state + typed errors
design/                  brand system, prototype, tokens, imagery, logos (design source of truth)
openspec/                spec-driven change workflow (proposal → design → specs → tasks)
openapi/                 vendored OpenAPI contract (regenerate with refresh-openapi)
scripts/refresh-openapi.ts   fetch canonical spec + regenerate src/generated types
```

## Architecture conventions

- **The frontend never calls the network.** All API access and caching live in
  Rust; the UI renders exclusively from the local SQLite cache and stays offline-
  capable. Screens read via Tauri commands and re-render on `sync:updated` /
  `detail:updated` events.
- **API key is keychain-only** — never written to config, logs, or JS state after
  onboarding. Requests attach it Rust-side via the `Authorization: Bearer` header
  (never query params or body — the API rejects those).
- **Read-only API usage.** No write/mutation endpoints. v1 uses `auth/validate`,
  `meetups/upcoming`, `meetups/search`, `rsvps/summary`, `rsvps/awaiting_payment`,
  `meetups/performance`.
- **Graceful degradation.** On `forbidden_api_group` / `forbidden_scope`, mark the
  feature unavailable and show a "not enabled" state instead of erroring — API
  groups are toggled per chapter.
- **Rate-limit aware.** The poller reads `X-RateLimit-*`, honors `Retry-After` on
  429, and backs off exponentially (`sync_state` records backoff windows).

## Dev commands

```bash
bun install
bun run refresh-openapi   # vendor OpenAPI spec + regenerate TS types
bun tauri dev             # launch app; Vite dev server on port 1425 (HMR 1426)
bun run build             # typecheck (tsc --noEmit) + vite build → dist/
cd src-tauri && cargo build
```

The dev server uses **port 1425** (not Tauri's default 1420) to avoid clashing
with other local Tauri apps.

## Stack notes

- Package manager: **bun** (not npm/yarn/pnpm).
- Frontend is **vanilla TypeScript + Vite** — no React/Svelte. TypeScript is the
  language; Vite is the bundler/dev server that serves it in the Tauri webview.
- Rust: `reqwest` (rustls), `rusqlite` (bundled), `keyring`, `tokio`, `chrono`.

## Spec workflow

Changes are developed spec-first under `openspec/changes/<name>/` (proposal,
design, capability specs, tasks). Shipped: `add-event-mission-control-v1`.
Run `/opsx:apply <name>` to implement, `/opsx:archive <name>` to finalize.
