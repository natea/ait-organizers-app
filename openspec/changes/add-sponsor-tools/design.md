## Context

Mission Control is a Tauri 2 desktop app for AI Tinkerers city organizers. The Rust
backend (`src-tauri/src/`) talks to the Agents API (`docs/agents-api.md`,
`openapi/openapi.yaml`), caches results in SQLite (`db.rs`), and exposes Tauri
commands (`commands.rs`) that the vanilla-TS frontend (`src/`) renders from. Screens
render **only** from the SQLite cache; the network layer (`api.rs` / `sync.rs`) is the
sole writer of that cache.

Sponsorship funds chapters. Usage is niche but high value per use. This change adds a sponsor
workbench: find sponsors, list their contacts, generate an AI research brief on a
company, and draft a tailored pitch that folds in event context. It is the app's first
feature to call **generation** endpoints; every prior feature is read-only.

Relevant API facts (verified in `openapi/openapi.yaml` and `docs/agents-api.md`):

- Read: `sponsor_search` (`GET /api/agents/v1/sponsors/search`, 30 rpm, 8s; `query`
  required, optional `city`/`industry`/`active_only`/`limit` default 25 max 25) returns
  `matches[]` with `sponsor_token`, name, website/domain, city, short profile, match
  metadata. `sponsor_contact_list` (`GET /api/agents/v1/sponsors/contacts`, 20 rpm, 8s;
  `sponsor_ref` required) returns `contacts[]` (max 25) with role/title/email/linkedin
  and confidence fields.
- Generation: `sponsor_research_generate` and `sponsor_pitch_generate` (both `POST`,
  10 rpm, **20s hard timeout**). Each takes `sponsor_ref`/`sponsor_token` OR `name`,
  plus optional `city`, `context` (hash), `target_audience`; research also takes
  `domain`; pitch also takes `channel`. Research returns `sponsor` + `research_summary`;
  pitch returns `pitch_text` + up to 3 variants and rationale snippets. Pitch context
  JSON is capped at 64 KB.
- Gating: all four are gated by the `subscribers_sponsors` API group and are
  **city-owner scope only** — index/city owners `Y`, city-series owners `N`. Disabled
  group returns `error.code = "forbidden_api_group"`; role-eligible-but-out-of-scope
  returns `forbidden_scope`.
- Contact email fields follow the existing `api_access_allow_email_addresses`
  masking/visibility policy; the API returns already-masked values for non-eligible
  callers.

## Goals / Non-Goals

**Goals:**

- Give city owners an in-app Sponsors screen to search sponsors, view a sponsor's
  contacts (respecting masking), and generate a research brief and a tailored pitch.
- Treat the two ~20s generation calls as long-running, cancelable kickoffs with visible
  progress, so the UI never blocks and the app stays responsive.
- Cache generated drafts locally so an organizer can reopen, re-read, and export a draft
  without re-spending a generation call.
- Degrade gracefully when the API group is off or the caller is out of scope, matching
  the app's existing `forbidden_*` handling.

**Non-Goals:**

- No attendee-data mutations. This feature only reads sponsor/contact data and generates
  drafts; it does not RSVP, email, or write to any attendee record.
- No server-side async job/poll protocol. The generation endpoints are synchronous with
  a 20s timeout (unlike media transcription's `job_token` flow); "async" here means the
  app runs the blocking call off the UI thread, not that the API returns a job token.
- No auto-send of pitches. Drafts are review/export artifacts; delivery stays manual and
  out of app scope.
- No sponsor CRM/pipeline management, sponsor editing, or logo selection (covered by the
  separate hackathon sponsor endpoints).

## Decisions

### 1. Sponsor search + contact list as cached reads

`api.rs` gains `sponsor_search(query, filters)` and `sponsor_contact_list(sponsor_ref)`
methods returning typed structs that mirror the documented output shapes. `sync.rs`
writes results into new `db.rs` tables (`sponsors`, `sponsor_contacts`) keyed by
`sponsor_token`; `commands.rs` exposes `sponsor_search` (triggers a fetch+cache, then
returns cached rows) and `sponsor_contacts_get` (returns cached rows for a token). The
Sponsors screen renders only from cache, consistent with every other screen.

Contact email/phone fields are stored and rendered exactly as the API returns them — the
API applies masking server-side, so the app never unmasks. The cache row carries a
`masked` boolean per field (derived from the value being a mask sentinel) purely so the
UI can show a "masked — enable email visibility for this chapter" hint rather than a
misleading blank.

*Alternative considered:* fetch-and-render without caching contacts. Rejected — it breaks
the cache-only rendering invariant and would re-hit the 20/30 rpm read limits on every
screen revisit.

### 2. Research brief and pitch as draft-generating kickoffs

Because generation is slow (~20s) and rate-limited (10 rpm), the app models each
generation as a **kickoff → in-progress → draft** lifecycle owned by the backend:

- `api.rs` gains `sponsor_research_generate(params)` and `sponsor_pitch_generate(params)`
  that POST the documented request bodies and parse `research_summary` / `pitch_text`
  (+ variants). Both run on a background task (Tokio) so the Tauri command returns a
  `draft_id` immediately and the blocking HTTP call proceeds off the UI thread.
- Progress is surfaced by emitting Tauri events (`sponsor_draft_progress`) with states
  `queued` → `generating` → `ready` | `failed`, so the frontend shows a determinate-ish
  progress affordance and a cancel control during the ~20s window.
- On success the draft is written to a `sponsor_drafts` cache table and a `ready` event
  fires; the screen loads the draft from cache.

Pitch generation always includes **event context** (target city, channel, and an event
payload assembled from the already-cached events data) in the `context` hash, keeping the
serialized body under the 64 KB cap by including only summary fields.

*Alternative considered:* synchronous Tauri command that blocks until the API returns.
Rejected — a 20s blocked command freezes the command channel and gives no progress or
cancel; the event-driven kickoff is the pattern the proposal calls for.

### 3. Draft caching and reuse

`sponsor_drafts` stores `draft_id`, `sponsor_token` (nullable — a draft may target a
free-text company name), `kind` (`research` | `pitch`), request fingerprint, generated
body, variants JSON, `status`, and timestamps. The screen lists prior drafts for a
sponsor/company and lets the organizer reopen one without regenerating. Regeneration is
an explicit user action (it spends a generation call), never automatic on revisit. This
directly serves the "export/share activity is very low, high value per use" profile:
cache the expensive output so a rare, valuable draft is never lost or silently re-billed.

### 4. Gating and contact masking

`commands.rs` treats `forbidden_api_group` and `forbidden_scope` on any sponsor endpoint
as hard denies (no retry through alternate paths), consistent with the existing degrade
handling. The Sponsors screen renders a disabled/empty state — "Sponsor tools aren't
enabled for this chapter" (group off) vs. "Your role can view this chapter but not
sponsor tools" (out of scope / city-series owner) — rather than an error toast. City
owners are the only authorized role; the UI does not offer sponsor tools to series-owner
sessions. Masked contact fields render as a non-editable masked chip with the visibility
hint from Decision 1.

## Risks / Trade-offs

- **20s generation exceeds a comfortable wait** → background kickoff + progress events +
  cancel; draft caching so the wait is paid at most once per distinct request.
- **Rate limits (10 rpm generation, 30/20 rpm reads)** → serialize generation kickoffs
  per session, disable the Generate button while a draft is in flight, and surface `429`
  as a "try again shortly" state rather than a hard failure.
- **Stale cached contacts (email visibility toggled server-side)** → contact rows carry a
  fetched-at timestamp and are refreshed on explicit reload; masking is always taken from
  the latest API response, never inferred or persisted as "unmasked".
- **Generation content quality / hallucination in research briefs** → drafts are labeled
  as AI-generated, unsent, and review-required; no draft is auto-delivered or treated as
  fact by the app.
- **64 KB pitch context cap** → the event-context assembler includes only summary fields
  and truncates free-text context, guarding the request body size before POST.
- **First generation-calling feature** → keep the network surface isolated in `api.rs`
  with typed request/response structs so the read-only invariant elsewhere is unaffected.
