## 1. Backend — API client (`src-tauri/src/api.rs`)

- [x] 1.1 Add typed request/response structs for `sponsor_search` (matches: `sponsor_token`, name, website/domain, city, short profile, match metadata) and `sponsor_contact_list` (contacts: role, title, email, linkedin, confidence, masked flags)
- [x] 1.2 Implement `sponsor_search(query, city, industry, active_only, limit)` calling `GET /api/agents/v1/sponsors/search` with the Bearer header, parsing the `{ok,data,error}` envelope
- [x] 1.3 Implement `sponsor_contact_list(sponsor_ref)` calling `GET /api/agents/v1/sponsors/contacts`, preserving API-applied email/phone masking (no unmasking)
- [x] 1.4 Add typed request/response structs for `sponsor_research_generate` (`sponsor_ref`/`name`, optional `domain`, `city`, `target_audience`, `context`; returns `sponsor`, `research_summary`) and `sponsor_pitch_generate` (`sponsor_ref`/`name`, optional `city`, `channel`, `target_audience`, `context`; returns `pitch_text`, variants, rationale)
- [x] 1.5 Implement `sponsor_research_generate(params)` and `sponsor_pitch_generate(params)` POST calls; enforce the 64 KB context cap on the pitch body before sending
- [x] 1.6 Map `forbidden_api_group`, `forbidden_scope`, and `429` responses to typed errors (hard deny, no alternate-path retry)

## 2. Backend — cache schema (`src-tauri/src/db.rs`)

- [x] 2.1 Add `sponsors` table keyed by `sponsor_token` (name, domain, city, short_profile, match metadata, fetched_at)
- [x] 2.2 Add `sponsor_contacts` table keyed by `sponsor_token` + contact id (role, title, email, phone, linkedin, confidence, per-field `masked` flags, fetched_at)
- [x] 2.3 Add `sponsor_drafts` table (`draft_id`, nullable `sponsor_token`, company `name`, `kind` = research|pitch, request fingerprint, body, variants JSON, `status`, created_at/updated_at)
- [x] 2.4 Add read/upsert helpers for sponsors, contacts, and drafts

## 3. Backend — sync + commands (`src-tauri/src/sync.rs`, `src-tauri/src/commands.rs`)

- [x] 3.1 In `sync.rs`, add fetch-and-cache flows writing `sponsor_search` and `sponsor_contact_list` results into their tables
- [x] 3.2 Expose `sponsor_search` command (fetch + cache, return cached rows) and `sponsor_contacts_get` command (return cached rows for a token)
- [x] 3.3 Expose `sponsor_research_generate` and `sponsor_pitch_generate` commands that spawn a background Tokio task, return a `draft_id` immediately, and write the draft on completion
- [x] 3.4 Emit `sponsor_draft_progress` Tauri events with states `queued` → `generating` → `ready` | `failed`; support cancel and prevent overlapping in-flight generations (rate-limit guard)
- [x] 3.5 For pitch generation, assemble event context (target city, channel, summarized event details) from the existing cached events data, staying within the 64 KB cap
- [x] 3.6 Surface `forbidden_api_group` / `forbidden_scope` / `429` to the frontend as degrade states, not errors

## 4. Frontend — types and screen (`src/types.ts`, `src/`)

- [x] 4.1 Add TS types for sponsor, sponsor contact, and sponsor draft mirroring the backend structs (`src/types.ts`)
- [x] 4.2 Build the Sponsors screen: search input + filters, sponsor result cards rendered only from cache
- [x] 4.3 Add sponsor detail with contact list, rendering masked fields as masked chips with a visibility hint (no unmasking)
- [x] 4.4 Add generate controls for research brief and pitch, with progress affordance and cancel wired to `sponsor_draft_progress` events
- [x] 4.5 Add draft list/reopen for a sponsor/company; regeneration only on explicit action
- [x] 4.6 Render degrade states: API-group-disabled vs. out-of-scope, and a rate-limited "try again shortly" state

## 5. Frontend — styling (`src/styles.css`)

- [x] 5.1 Style the Sponsors screen, cards, contact list, masked chips, and draft view per `design/DESIGN.md`
- [x] 5.2 Style the generation progress/cancel affordance and the degrade/empty states

## 6. Verification

- [x] 6.1 Run `tsc` (frontend type-check) clean
- [x] 6.2 Run `cargo build` and `cargo test` clean
- [x] 6.3 Mock-drive the flows: search → contacts (with a masked field) → generate research → generate pitch → reopen cached draft → cancel an in-flight generation
- [x] 6.4 Mock-drive degrade paths: `forbidden_api_group`, `forbidden_scope`, and `429` render the correct states with no alternate-path retry
