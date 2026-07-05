## Context

Mission Control is a Tauri 2 desktop app for AI Tinkerers city organizers. Every
screen renders **only** from the local SQLite cache (`db.rs`); `sync.rs` pulls
from the Agents API (`api.rs`) into the cache, and `commands.rs` exposes Tauri
commands to the TypeScript frontend. The app is **read-only today** — no screen
issues a mutating API call.

This change adds a per-event **Media view** (video kit) on the event detail
screen: browse the event's media folder and files, read/download them, and — for
authorized callers — upload files, create folders, edit sticky notes, and kick
off asynchronous transcription and video scale-down jobs. It is the app's **first
write feature**, so it must establish the write path (confirmation + audit)
without changing the read-from-cache rendering rule.

The relevant endpoints live in the **Media API group** (`docs/agents-api.md`
authorization table, lines ~283–302; rate limits ~454–474; shapes in
`openapi/openapi.yaml` under `/api/agents/v1/media/...`):

- Read: `media_folder_list`, `media_folder_info`, `media_file_get`,
  `media_file_download`, `media_file_transcript_get`,
  `media_file_transcript_status`, `media_file_scale_down_status`.
- Write: `media_file_upload`, `media_folder_create`,
  `media_file_transcript_generate`, `media_file_scale_down`, `media_note_update`.

The proposal's short names `media_transcript_get/status/generate` map to the
file-oriented endpoints this app calls: `media_file_transcript_get`,
`media_file_transcript_status`, and `media_file_transcript_generate` (the
`POST .../media/files/generate_transcript` alias documented at
`docs/agents-api.md:2516`). We use the file-oriented forms everywhere because the
UI always operates on a specific file token.

### Critical authorization caveat

The Media API group is authorized for **index owners** and, for most endpoints,
`index_video_editor` — **city owners are NOT authorized** (the "City owner"
columns read `N`; transcription generation is index-owner-only). Mission
Control's primary audience is city organizers, so for most users **every media
endpoint returns `forbidden_role` / `forbidden_api_group`**. This is not an edge
case for our users; it is the common case. The design therefore treats clean,
prominent role-gated degradation as a first-class requirement, and flags the
underlying priority question (see Open Questions).

## Goals / Non-Goals

**Goals:**

- Add a Media view to event detail that browses the event's folder tree, files,
  notes, and download links, rendered from the SQLite cache like every other
  screen.
- Support the write actions (upload ≤50 MB base64, create folder, edit note,
  start transcription, start scale-down) behind the app's write guardrail
  (explicit confirmation + local audit-log entry).
- Show live progress for large uploads and poll async job status
  (transcription, scale-down) until terminal.
- Degrade cleanly and prominently to a "Media isn't available for your role"
  state when the API returns `forbidden_role` / `forbidden_scope` /
  `forbidden_api_group`, without breaking the rest of event detail.
- Handle rate limits (`429`) and oversize uploads with actionable messaging.

**Non-Goals:**

- No media browsing outside an event's folder (no global `media_file_search` /
  `media_transcript_search` surface in this change).
- No file/folder delete, move, or transcript delete (`media_file_delete`,
  `media_file_move`, `media_transcript_delete` are out of scope).
- No in-app media playback, thumbnail generation, or transcript editing beyond
  sticky notes; download links hand off to the OS/browser.
- No change to the read-from-cache rendering rule or to auth/token handling.
- No offline queueing of writes — writes require connectivity and are issued
  live.

## Decisions

### 1. Media is an event-scoped view driven by the folder–meetup link

`media_folder_info` returns a folder's associated meetup (event token, name,
date). Sync resolves the event's media folder via that association and caches the
folder subtree. **Rationale:** keeps the feature anchored to the event the
organizer is already looking at, and avoids exposing a network-wide media
browser. **Alternative considered:** a standalone top-level Media screen using
`media_file_search` — rejected as broader scope and weaker event context.

### 2. Reads flow through cache; writes are live then re-sync

Browsing renders only from cached rows (`media_folders`, `media_files`,
`media_transcripts`, `media_jobs`). Writes call `api.rs` directly, and on success
trigger a targeted re-sync of the affected folder so the cache reflects the new
state. **Rationale:** preserves the "screens render only from SQLite" invariant
while keeping post-write UI correct. **Alternative:** optimistic local insertion
before confirmation — rejected as risking cache/server divergence on partial
failure.

### 3. Write guardrail: confirm + audit, reused for every mutation

Each write (upload, folder create, note update, transcription start, scale-down
start) requires an explicit confirmation step in the UI, and on issue writes a
row to a local `write_audit` log (action, target token, timestamp, actor,
outcome). **Rationale:** this is the app's first write surface; a single reusable
guardrail keeps mutations auditable and hard to trigger accidentally, matching
the API's own audit-logging expectations. **Alternative:** silent immediate
writes — rejected; unacceptable for a first write feature.

### 4. Upload: base64 in the Rust layer, progress surfaced to the UI

The frontend hands a chosen file path to a Tauri command; `api.rs` reads the
file, enforces the 50 MB limit **before** encoding, base64-encodes the body, and
POSTs `media_file_upload` with `filename`, `content_type`, `folder_token`,
`body_base64`, optional `note`. Progress is emitted to the frontend (read/encode,
then upload) so large files show a determinate-then-indeterminate indicator.
**Rationale:** keeps large binary handling and the size guard in Rust; the size
check must precede encoding because base64 inflates payloads ~33%. **Alternative:**
encode in TS — rejected (memory pressure, weaker guard placement).

### 5. Async jobs: kick off, then poll status to a terminal state

Transcription and scale-down return immediately; the app polls
`media_file_transcript_status` / `media_file_scale_down_status` on an interval
until `success` or `failed`, caching each observation in `media_jobs`. Polling
respects the status endpoints' generous limits (60 rpm) and backs off on `429`.
On `success`, a targeted re-sync pulls the transcript / scaled-file metadata.
**Rationale:** the API is explicitly async with poll endpoints; caching job state
lets the view survive app restarts mid-job. **Alternative:** fire-and-forget with
manual refresh — rejected as poor UX for multi-minute jobs.

### 6. Role gating is detected at sync time and cached as view state

When any media call returns `forbidden_role` / `forbidden_scope` /
`forbidden_api_group`, sync records a `media_availability = unavailable` flag
(with reason) for the event instead of erroring the whole detail sync. The Media
view reads that flag and renders a prominent "not available for your role" panel;
write controls are hidden, not merely disabled. **Rationale:** the forbidden case
is the common case for city owners, so it must be a designed state, not an error
toast. **Alternative:** hide the Media tab entirely when forbidden — rejected;
organizers should understand *why* it is unavailable and that an index
owner / `index_video_editor` can use it.

## Risks / Trade-offs

- **[Primary audience can't use the core write actions]** City owners — the app's
  main users — are not authorized for the Media group, so for them this ships as a
  read-blocked, write-blocked "unavailable" panel. → Mitigation: make degradation
  prominent and honest; treat the priority question as open (below). This risk is
  significant enough that it may warrant deprioritizing the change; surfaced here
  and in the proposal.
- **[First write path in a read-only app]** Introduces mutation, confirmation, and
  audit plumbing that did not exist. → Mitigation: single reusable guardrail
  (Decision 3), writes isolated to explicit commands, no change to read rendering.
- **[Large uploads hit timeouts / limits]** `media_file_upload` is 10 rpm, 30s
  timeout, 50 MB cap. → Mitigation: enforce size before encode, surface progress,
  map `429` and `413`-style errors to clear retry guidance.
- **[Async jobs never terminate or fail silently]** Transcription/scale-down may
  fail or stall. → Mitigation: poll to an explicit terminal state, cache attempt
  count and error detail from the status endpoints, show failed state with reason.
- **[Cache/server divergence after partial write failure]** A write that
  half-succeeds could leave stale cache. → Mitigation: targeted re-sync only on
  confirmed success; on failure, no cache mutation.

## Migration Plan

Additive only. New cache tables (`media_folders`, `media_files`,
`media_transcripts`, `media_jobs`, `write_audit`) are created by `db.rs`
migrations; no existing tables change. The Media view is a new section on the
existing event detail screen and is inert (renders the unavailable panel) when
the role is not authorized. Rollback = drop the new tables and remove the view;
no other screen depends on them.

## Open Questions

- **Prioritization:** Given city owners (the primary audience) cannot use any
  media endpoint, should this change ship now as an index-owner / `index_video_editor`
  feature, be deferred, or be re-scoped? This is the central open decision and is
  called out in the proposal's Impact section.
- Should a successful role check be cached long enough to avoid re-probing on
  every event open, and if so for how long, given roles can change server-side?
- Do we need per-event write rate awareness (e.g. surfacing the 50/day
  transcription cap) before the user starts a job, or is reactive `429` handling
  sufficient for v1?
