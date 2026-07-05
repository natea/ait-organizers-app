# Proposal: add-media-video-kit

Stack-rank #9 — Media upload / video kit (important for post-event content, lower
observed event usage: 387 uploads, 301 complete, 119 folders network-wide).

## Why

Post-event media (photos, talk recordings) is valuable content but its handling
lives in a separate media tool. Mission Control can host a lightweight media kit
per event — browse the event's folder, upload files, kick off transcription and
video scale-down — so the post-event content workflow starts from the same place
organizers review the recap.

## What Changes

- Add a Media view on event detail: browse the event's media folder/files, view
  notes, and get download links.
- **Write actions**: upload files (base64, ≤50 MB), create folders, initiate
  transcription and video scale-down, poll their async status.

## Capabilities

### New Capabilities

- `media-video-kit`: Browse, upload, and process an event's media (transcription,
  scale-down) from the app.

## Impact

- Endpoints: read — `media_folder_list`, `media_folder_info`, `media_file_get`,
  `media_file_download`, `media_transcript_get/status`; **write** —
  `media_file_upload`, `media_folder_create`, `media_transcript_generate`,
  `media_file_scale_down`, `media_note_update`.
- Access is via the **Media API group** (index owners / `index_video_editor`);
  city owners are NOT authorized — so for most organizers this degrades to
  "not available." Reconsider priority against the audience (city owners).
- Large uploads + async jobs need progress UI and status polling.
