## 1. Cache schema (db.rs)

- [ ] 1.1 Add migrations for `media_folders` (folder token, parent token, name, weblog token, event token, note, synced_at)
- [ ] 1.2 Add migrations for `media_files` (file token, folder token, filename, content_type, size, uploader, created_at, note, has_transcript, synced_at)
- [ ] 1.3 Add migrations for `media_transcripts` (file token, transcript_text, transcript_json, synced_at)
- [ ] 1.4 Add migrations for `media_jobs` (file token, job_type [transcript|scale_down], status, attempts, error_detail, updated_at)
- [ ] 1.5 Add migration for per-event `media_availability` flag + reason (extend event/detail cache)
- [ ] 1.6 Add migration for `write_audit` (action, target_token, actor, outcome, created_at) shared write-guardrail log
- [ ] 1.7 Add db.rs read/upsert helpers for the tables above

## 2. API client — read methods (api.rs)

- [ ] 2.1 Add `media_folder_list` and `media_folder_info` methods (parse `{ok,data,error{code}}` envelope)
- [ ] 2.2 Add `media_file_get` method
- [ ] 2.3 Add `media_file_download` method (returns presigned URL)
- [ ] 2.4 Add `media_file_transcript_get` and `media_file_transcript_status` methods
- [ ] 2.5 Add `media_file_scale_down_status` method
- [ ] 2.6 Map `forbidden_role` / `forbidden_scope` / `forbidden_api_group` and `429` to typed errors the caller can branch on

## 3. API client — write methods (api.rs)

- [ ] 3.1 Add `media_file_upload` (enforce 50 MB limit before base64 encode; send filename, content_type, folder_token, body_base64, optional note; emit progress)
- [ ] 3.2 Add `media_folder_create` (name + parent_token or weblog_token)
- [ ] 3.3 Add `media_note_update` (exactly one of file_token/folder_token; empty note clears)
- [ ] 3.4 Add `media_file_transcript_generate` (kickoff)
- [ ] 3.5 Add `media_file_scale_down` (kickoff)

## 4. Sync (sync.rs)

- [ ] 4.1 Resolve and cache the event's media folder subtree via `media_folder_info` + `media_folder_list`, caching files and notes
- [ ] 4.2 On any forbidden media error, set `media_availability = unavailable` with reason instead of failing detail sync
- [ ] 4.3 Targeted folder re-sync after a confirmed write (upload / folder create / note update / scale-down success)
- [ ] 4.4 Poll `media_jobs` status endpoints to a terminal state with back-off on `429`; cache observations; on transcript success fetch and cache transcript

## 5. Commands (commands.rs)

- [ ] 5.1 Command to open the Media view: return cached folder/files/notes/availability for an event
- [ ] 5.2 Command for `media_file_download` returning the presigned URL to open
- [ ] 5.3 Write commands (upload, create folder, update note, start transcription, start scale-down) that require a confirmation flag and write a `write_audit` row
- [ ] 5.4 Command to fetch current job status for a file (drives polling UI)
- [ ] 5.5 Emit upload progress and job-status events to the frontend

## 6. Frontend types (src/types.ts)

- [ ] 6.1 Add types for media folder, file, note, transcript, and job status
- [ ] 6.2 Add `MediaAvailability` (available | unavailable + reason) type
- [ ] 6.3 Add command request/response types for the read and write commands

## 7. Frontend Media view (src/)

- [ ] 7.1 Render the folder tree, file list, notes, and download links from cache
- [ ] 7.2 Render the prominent "not available for your role" panel when availability is unavailable; hide all write controls
- [ ] 7.3 Upload flow: file pick, size pre-check, confirmation, progress indicator, post-success refresh
- [ ] 7.4 Create-folder and edit/clear-note flows with confirmation
- [ ] 7.5 Transcription and scale-down: start (with confirmation), in-progress polling UI, terminal success/failed states with error detail
- [ ] 7.6 Rate-limit / oversize / rejected-write messaging that preserves rendered state

## 8. Styles (src/styles.css)

- [ ] 8.1 Style the Media view (folder tree, file rows, notes) per design/DESIGN.md
- [ ] 8.2 Style the unavailable panel, confirmation dialogs, progress and job-status indicators

## 9. Verification

- [ ] 9.1 `tsc` passes with no type errors
- [ ] 9.2 `cargo build` and `cargo test` pass for src-tauri
- [ ] 9.3 Exercise browse / upload / create-folder / note / transcription / scale-down against the mock drive
- [ ] 9.4 Verify role-gated degradation: forbidden responses render the unavailable panel and leave event detail intact
- [ ] 9.5 Verify `429` back-off on polling and no automatic write retries
- [ ] 9.6 `openspec validate add-media-video-kit` passes
