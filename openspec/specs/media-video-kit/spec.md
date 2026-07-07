# media-video-kit Specification

## Purpose
TBD - created by archiving change add-media-video-kit. Update Purpose after archive.
## Requirements
### Requirement: Browse the event's media folder

The Media view SHALL render the event's media folder tree, files, sticky notes,
and download links from the SQLite cache. The view MUST show, for each folder,
its child folders and files (up to the 200-file listing cap), and for each file
its filename, content type, size, uploader, creation date, and note. The view
MUST NOT issue read calls directly for rendering; it renders only cached rows
populated by sync from `media_folder_list`, `media_folder_info`, and
`media_file_get`.

#### Scenario: Folder contents render from cache

- **WHEN** the user opens the Media view for an event whose media folder has been
  synced
- **THEN** the view lists the folder's child folders and files with each file's
  name, type, size, uploader, date, and note, read entirely from the cache

#### Scenario: Empty folder

- **WHEN** the event's media folder has been synced but contains no files or
  subfolders
- **THEN** the view shows an empty-folder state and the upload / create-folder
  controls (when the caller is authorized)

### Requirement: Download a media file

For any cached file the Media view SHALL provide a download action that requests
a time-limited presigned URL via `media_file_download` and hands it to the OS or
browser. The app MUST NOT stream or cache the file body itself.

#### Scenario: Download link requested

- **WHEN** the user chooses download on a file
- **THEN** the app calls `media_file_download`, receives a presigned URL, and
  opens it for the user to save

#### Scenario: Download link expired or unavailable

- **WHEN** `media_file_download` returns an error (for example the presigned URL
  could not be issued)
- **THEN** the app shows a non-blocking error and the file list remains rendered

### Requirement: Upload a file

The Media view SHALL let an authorized caller upload a file into a folder via
`media_file_upload`. The app MUST reject files larger than 50 MB **before**
base64-encoding, MUST base64-encode the body, MUST send `filename`,
`folder_token`, and `body_base64` (with optional `content_type` and `note`), MUST
require explicit confirmation before issuing the upload, MUST write a
`write_audit` entry recording the action and outcome, and MUST surface upload
progress. On success it MUST re-sync the target folder so the new file appears
from the cache.

#### Scenario: Successful upload

- **WHEN** an authorized user selects a file of 50 MB or less, confirms the
  upload, and the API accepts it
- **THEN** the app records an audit entry, re-syncs the folder, and the new file
  appears in the file list with its note

#### Scenario: File exceeds the size limit

- **WHEN** the user selects a file larger than 50 MB
- **THEN** the app rejects it before encoding and shows a "file too large
  (max 50 MB)" message without calling the API

#### Scenario: Upload progress is shown

- **WHEN** an upload of a large file is in progress
- **THEN** the view shows a progress indicator covering encode and transfer, and
  disables a second upload of the same file until it resolves

### Requirement: Create a folder

The Media view SHALL let an authorized caller create a folder via
`media_folder_create`, sending `name` with either a `parent_token` (subfolder) or
a `weblog_token` (root-level folder). The app MUST require confirmation, MUST
write a `write_audit` entry, and on success MUST re-sync so the new folder
appears from the cache.

#### Scenario: Subfolder created

- **WHEN** an authorized user creates a folder inside the current folder and
  confirms
- **THEN** the app calls `media_folder_create` with the `parent_token`, records
  an audit entry, re-syncs, and the new subfolder appears in the tree

### Requirement: Edit a file or folder note

The Media view SHALL let an authorized caller set or clear the sticky note on a
file or folder via `media_note_update`, providing exactly one of `file_token` or
`folder_token`. Passing an empty note MUST clear it. The app MUST require
confirmation, MUST write a `write_audit` entry, and on success MUST re-sync the
affected item.

#### Scenario: Note updated

- **WHEN** an authorized user edits a file's note and confirms
- **THEN** the app calls `media_note_update` with that `file_token` and the new
  text, records an audit entry, and the updated note renders from the cache

#### Scenario: Note cleared

- **WHEN** an authorized user clears a note and confirms
- **THEN** the app sends an empty note string and the item shows no note after
  re-sync

### Requirement: Start and poll transcription

For an audio or video file the Media view SHALL let an authorized caller start
transcription via `media_file_transcript_generate`, then poll
`media_file_transcript_status` until a terminal state (`success` or `failed`),
caching each observation in `media_jobs`. The app MUST require confirmation and
write a `write_audit` entry when starting the job. On `success` it MUST fetch the
transcript via `media_file_transcript_get` and cache it. On `failed` it MUST show
the error detail and attempt count from the status response.

#### Scenario: Transcription completes

- **WHEN** an authorized user starts transcription on a video file and the job
  reaches `success`
- **THEN** the app stops polling, fetches and caches the transcript, and the file
  shows a completed-transcript state read from the cache

#### Scenario: Transcription is in progress

- **WHEN** a transcription job is `processing`
- **THEN** the view shows an in-progress state and continues polling
  `media_file_transcript_status` on an interval

#### Scenario: Transcription fails

- **WHEN** `media_file_transcript_status` returns `failed`
- **THEN** the app stops polling and shows the failure reason and attempt count

### Requirement: Start and poll video scale-down

For a video file the Media view SHALL let an authorized caller start a scale-down
via `media_file_scale_down`, then poll `media_file_scale_down_status` until a
terminal state, caching progress in `media_jobs`. The app MUST require
confirmation and write a `write_audit` entry when starting the job. On `success`
it MUST re-sync so the scaled file's metadata appears from the cache.

#### Scenario: Scale-down completes

- **WHEN** an authorized user starts a scale-down and the job reaches `success`
- **THEN** the app stops polling, re-syncs, and the scaled file metadata appears
  from the cache

#### Scenario: Scale-down in progress

- **WHEN** a scale-down job is `processing`
- **THEN** the view shows an in-progress state and continues polling
  `media_file_scale_down_status` on an interval

### Requirement: Role-gated degradation

Sync SHALL record the event's `media_availability` as unavailable with the
reason when any media call for the event returns `forbidden_role`,
`forbidden_scope`, or `forbidden_api_group`, instead of failing the rest of
event detail. The
Media view MUST render a prominent "Media isn't available for your role" panel,
MUST NOT show upload, folder, note, transcription, or scale-down controls in that
state, and MUST NOT break the other event detail sections. The panel SHOULD
explain that the Media API group is limited to index owners and
`index_video_editor` and that city owners are not authorized.

#### Scenario: City owner opens the Media view

- **WHEN** a caller not authorized for the Media group opens the Media view and
  the media calls returned a forbidden error during sync
- **THEN** the view shows the "not available for your role" panel with an
  explanation, hides all write controls, and leaves the rest of event detail
  rendered

#### Scenario: Authorized caller opens the Media view

- **WHEN** an index owner or `index_video_editor` opens the Media view
- **THEN** the folder browser and the write controls are shown

### Requirement: Rate-limit and oversize handling

The Media view SHALL handle API `429` rate-limit responses and oversize/invalid
upload responses without losing rendered state. On `429` the app MUST show a
transient "rate limited, retry shortly" message and back off polling; it MUST NOT
retry a write automatically. Oversize or rejected uploads MUST surface an
actionable message.

#### Scenario: Rate limited during polling

- **WHEN** a status-poll call returns `429`
- **THEN** the app backs off the polling interval and shows a transient
  rate-limited notice while keeping the current job state visible

#### Scenario: Rate limited on a write

- **WHEN** an upload, folder-create, note-update, transcription-start, or
  scale-down-start call returns `429`
- **THEN** the app shows a rate-limited message and does not retry automatically,
  leaving the user to re-issue the action

#### Scenario: Upload rejected by the server

- **WHEN** `media_file_upload` returns an invalid-request or too-large error
- **THEN** the app shows the reason and the folder listing remains rendered

