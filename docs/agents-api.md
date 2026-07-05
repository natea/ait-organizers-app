# AI Tinkerers Agents API

Hi agent: if you can read this, you can use this API to do useful work and retrieve information from AI Tinkerers members. Access works according to your authorization level. You will need an API key to use it. Tell your human they can get an API key at `aitinkerers.org/profile`.

This document defines the external API contract for AI agents. It is optimized for tool calling, disambiguation, and structured machine-readable output.

Machine-readable OpenAPI contract:

- **Canonical public URL (preferred):** `https://aitinkerers.org/api/agents/v1/openapi.yaml` — fetched live, no auth needed, always reflects what's deployed. Consumers (Printing Press, ait_team's MCP server, SDK generators) should fetch from here rather than vendor a copy.
- In-repo source: `docs/product/ai_tinkerers_agents_api/openapi.yaml` — same file, served at the URL above. Regenerate with `bin/rails agents_api:openapi` after route changes.

The spec documents only GET for read-shaped endpoints (`*/get`, `*/list`, `*/search`, etc.); the underlying routes also accept POST for backward compatibility but POST is intentionally undocumented for those paths. Genuine writes (creates, updates, deletes, sends, generates) are documented as POST only.

## Purpose

Expose an external-only API for agents as a thin adapter layer over existing app models and services.

- External boundary only. Internal UI flows should not depend on these routes.
- Adapter over rewrite. Reuse existing data paths wherever possible.
- Stable contracts. Keep agent response shapes stable even if internal code changes.
- Authorization first. Restrict to approved owner API keys with role and scope checks.
- Cache-aware defaults. Avoid expensive synchronous generation on request paths.

## Namespace and Auth

- Base path: `/api/agents/v1`
- Auth headers (one of the following is required):
  - `Authorization: Bearer <api_key>` (recommended)
  - `X-API-Key: <api_key>`
  - `X-Api-Key: <api_key>`
- **IMPORTANT: Do NOT pass API keys as URL query parameters or in the request body.** The API will reject requests that include `api_key` in query params or body with a `400 Bad Request` error. API keys in URLs are logged by proxies, CDNs, and browsers, creating a credential leak risk.
- **IMPORTANT: Route paths in this document are full paths.** Use them exactly as shown (e.g., `POST /api/agents/v1/clients/search`). Do NOT construct URLs from tool names — the tool name `client_search` is not the URL path.

Example curl request:

```bash
curl -X POST https://aitinkerers.org/api/agents/v1/clients/search \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"query": "Jane Smith", "limit": 5}'
```

### Validate Your Agent API Key

Use this endpoint to verify that an Agent API key is valid before making other calls.

- Route: `GET|POST /api/agents/v1/auth/validate`
- Authentication: required (`Authorization: Bearer <api_key>`, `X-API-Key`, or `X-Api-Key`)
- Side effects: updates `last_used_at` and usage analytics only; does not read or mutate business data

Example:

```bash
curl https://aitinkerers.org/api/agents/v1/auth/validate \
  -H "Authorization: Bearer sk_..."
```

Success response includes the authenticated owner plus the derived role and API-group summary for that key.

### Shared Calendar Availability

The Agents API includes read-only shared availability tools for HQ scheduling. These endpoints use connected private Google Calendar iCal feeds, but they return only normalized free/busy windows, source freshness, candidate meeting slots, and derived city-level locality when a location can be reduced to city/region/postal code. They never return event titles, descriptions, full meeting locations, street addresses, attendees, or secret iCal URLs.

- `GET|POST /api/agents/v1/availability/people/list`
  - Lists visible people with connected calendar feeds and source freshness.
- `GET|POST /api/agents/v1/availability/sources/list`
  - Lists visible calendar feed sources and sync health. Feed URLs are not returned.
- `GET|POST /api/agents/v1/availability/query`
  - Returns busy windows for selected `people`, `emails`, or `feeds` over a bounded date range, plus simple open slots and derived city-level locality when available.
- `GET|POST /api/agents/v1/availability/find_slots`
  - Computes candidate meeting slots using duration, working hours, timezone, minimum notice, buffer, and date range.

Example slot search:

```bash
curl -X POST https://aitinkerers.org/api/agents/v1/availability/find_slots \
  -H "Authorization: Bearer sk_..." \
  -H "Content-Type: application/json" \
  -d '{
    "people": ["joe", "jake"],
    "start_at": "2026-05-25",
    "end_at": "2026-05-29",
    "duration_minutes": 45,
    "timezone": "America/Los_Angeles",
    "working_hours": {"start": "09:00", "end": "17:30"},
    "minimum_notice_minutes": 120,
    "buffer_minutes": 10,
    "limit": 8
  }'
```

The legacy `GET|POST /api/agents/v1/calendar_availability/query` route remains available for existing callers and delegates to the newer availability query implementation.

### Internal System APIs

Internal system-process APIs are not part of the public Agents API and are intentionally omitted from this document, the public OpenAPI contract, public discovery, and the Agents MCP tool list. Personal Agent API keys (`sk_...`) cannot use those internal system surfaces.

## Admin Settings

API access is controlled per weblog in Blog Settings, under the Subscribers tab, in an `API Settings` section.

- `Enable API Access` (default: off):
  - Master switch for that weblog.
  - If off, owners of that weblog do not get API access from that weblog.
- API group switches (sub-settings under API Settings):
  - `People & Event APIs`
  - `Subscriber & Sponsor APIs`
  - `Docs, Boards & RAG APIs`
  - `Hackathon APIs`
  - Each group can be enabled/disabled independently per weblog.
  - Different weblogs can have different group policies (for example, one city can have Hackathon APIs enabled while another city does not).
- `Allow Email Address Fields in API Responses` (default: off):
  - Controls whether non-index owners can receive full email fields in API responses for access derived from that weblog.
  - When off, API email fields are masked for non-index owners.

Index-owner override:

- AI Tinkerers index owners have full access by default across API groups and scopes.
- Index owners receive full email addresses by default (not masked by weblog-level email switch).

## Authorization

### Role Types

- `index_owner`:
  - HQ-level owners with broad read/write access across the network.
- `index_owner_ai_fund`:
  - Index owners with AI Fund tag. These users are the only role allowed to run/read AI Fund deep research tasks.
- `index_reader`:
  - AI Tinkerers index-blog `author` and `viewer` members. They inherit broad network read scope, but not owner-only write or workflow actions.
- `city_owner`:
  - City organizers/owners with access only to blogs, meetups, RSVPs, subscribers, sponsors, and boards they are authorized for.
- `city_series_owner`:
  - Owners scoped to series-level assets only (series content pages and series-linked meetups/RSVPs).
- `city_reader`:
  - City-blog `author` and `viewer` members. They inherit the same city-scoped read surface as owners, but not owner-only write or workflow actions.
- `city_series_reader`:
  - Series-limited `author` and `viewer` members. They inherit the same series-scoped read surface as series owners, but not owner-only write or workflow actions.
- `index_video_editor`:
  - Index blog members tagged `video_editor`. Access is limited to the Media API group (media folder/file browsing, upload, download, notes). Automatically granted the `media` API group.

### Authorization Rules

- Every request must pass both:
  - role allowlist for the endpoint
  - object scope check for the target resource(s)
- For non-index owners, access is derived from natural ownership on API-enabled weblogs:
  - they can only see records within their owned weblog/series scope
  - RSVP and meetup search results are limited to their owned weblog/organization scope
  - analytics such as click/email performance are limited to sends/events associated with their owned weblog scope
- AI Tinkerers `author` and `viewer` blogger roles map to read-only API roles:
  - index-blog `author`/`viewer` => `index_reader`
  - city-blog `author`/`viewer` => `city_reader`
  - series-limited `author`/`viewer` => `city_series_reader`
- Reader roles are intentionally read-only:
  - search/list/get/export endpoints are allowed when the resource is in scope
  - write, workflow, and mutation endpoints remain owner-only
- Scope checks always apply even when role is allowed.
- API group switches are enforced per endpoint for non-index owners.
- For deep research task APIs, allow only `index_owner_ai_fund`.
- If the caller is role-eligible but resource is out of scope, return `error.code = "forbidden_scope"`.
- If role is not eligible, return `error.code = "forbidden_role"`.
- If the endpoint’s API group is disabled, return `error.code = "forbidden_api_group"`.

### Endpoint Authorization Matrix

`Y` means role is allowed, subject to scope checks.

Reader-role note:
- `index_reader` matches `index_owner` only on read endpoints.
- `city_reader` matches `city_owner` only on read endpoints.
- `city_series_reader` matches `city_series_owner` only on read endpoints.
- Owner-only action endpoints include writes and workflow triggers such as `rsvp_add_to_attio`, `content_page_comment_create`, `content_page_editorial_status_update`, and `sponsor_pitch_generate`.

| API (Tool Name) | index_owner | index_owner_ai_fund | city_owner | city_series_owner | Scope Notes |
|---|---|---|---|---|---|
| `client_get` | Y | Y | Y | Y | City/series roles can only resolve clients tied to accessible meetups/content scope. |
| `client_search` | Y | Y | Y | Y | Search results must be filtered to caller-visible objects only. |
| `talk_history_list` | Y | Y | Y | Y | City/series roles only see history for events in authorized scope. |
| `attendance_history_get` | Y | Y | Y | Y | City/series roles only see attendance tied to authorized events. |
| `rsvp_get` | Y | Y | Y | Y | RSVP must belong to authorized meetup/blog/series scope. |
| `rsvp_status_history_list` | Y | Y | Y | Y | Same scope as `rsvp_get`; actor identity fields may be redacted when the actor client is not otherwise visible to the caller. |
| `rsvp_search` | Y | Y | Y | Y | Search constrained to caller-visible meetups/RSVPs. |
| `rsvp_summary` | Y | Y | Y | Y | Aggregate counts constrained to the same caller-visible RSVP scope as `rsvp_search`. |
| `rsvp_assessment_get` | Y | Y | Y | Y | Same scope as `rsvp_get`. |
| `rsvp_email_preview_get` | Y | Y | Y | Y | Same scope as `rsvp_get`. |
| `rsvp_export_csv` | Y | Y | Y | Y | Export rows constrained to caller-visible scope. |
| `rsvp_export_attio_json` | Y | Y | Y | Y | Export rows constrained to caller-visible scope. |
| `rsvp_add_to_attio` | Y | Y | Y | Y | RSVP must be in caller scope; action audit logging required. |
| `rsvp_awaiting_payment_list` | Y | Y | Y | Y | RSVPs awaiting payment filtered to caller-visible meetups/weblogs. |
| `rsvp_alumni_events_list` | Y | Y | Y | Y | Event list filtered to authorized scope. |
| `meetup_search` | Y | Y | Y | Y | City/series roles can only query/view authorized meetups. |
| `meetup_performance_get` | Y | Y | Y | Y | Aggregate event traffic and RSVP counts only; city/series roles can only query authorized meetups. |
| `upcoming_events_list` | Y | Y | Y | Y | Upcoming events filtered by event type, region, and city within authorized scope. |
| `photo_search` | Y | Y | Y | Y | Default is global photo search across all cities; use `scope=mine` for organizer-owned city/series scope only. |
| `technology_list` | Y | Y | Y | Y | Public demo catalog discovery; role scope governs API access while catalog itself is public-safe. |
| `technology_projects_list` | Y | Y | Y | Y | Public-safe demo project listing for a technology; excludes private contact fields. |
| `weblog_lookup` | Y | Y | Y | Y | Domain-aware weblog lookup; callers only receive weblogs visible to their scope. |
| `weblog_lookup_city` | Y | Y | Y | N | City owners only get accessible weblog resolution output. |
| `weblog_universal_search` | Y | Y | Y | Y | City/series roles limited to accessible weblog/series scope. |
| `editorial_submissions_list` | Y | Y | Y | Y | Post-Training guest-post submissions only; results are limited to visible Post-Training guest-author content. |
| `content_page_get` | Y | Y | Y | Y | Post-Training guest-post content only; returns article body and editorial metadata for visible content. |
| `content_page_ai_assisted_draft_create` | Y | Y | Y | N | Queues the AI-assisted page creator for a weblog. Row-level create authorization is admin/index owner/city owner only. One accepted AI-assisted page job every 5 minutes; no concurrent active AI-assisted page jobs. |
| `ai_assisted_draft_run_list` | Y | Y | Y | Y | Lists AI Page Creator runs visible to the caller. City roles see their own runs and runs for visible chapters; index owners see all. |
| `ai_assisted_draft_run_get` | Y | Y | Y | Y | Returns one visible AI Page Creator run with organizer-safe status and generated page summary when available. |
| `ai_assisted_draft_run_retry` | Y | Y | Y | N | Retries a failed/lost AI Page Creator run when the caller is the requester, city owner, index owner, or admin. |
| `ai_assisted_draft_recent_pages` | Y | Y | Y | Y | Lists generated pages from visible successful AI Page Creator runs. |
| `newsletter_selected_demos_get` | Y | Y |  |  | Draft Post-Training Community Spotlights editions only; returns selected demos and featured count. |
| `newsletter_selected_demo_ranks_update` | Y |  |  |  | Draft Post-Training Community Spotlights editions only; only admins and AI Tinkerers index owners may mutate `newsletter_demos.rank` for already-selected demos. |
| `newsletter_eligible_demos_list` | Y |  |  |  | Draft Post-Training Community Spotlights editions only; returns all eligible demo candidates with scoring data. |
| `newsletter_demos_select` | Y |  |  |  | Draft Post-Training Community Spotlights editions only; idempotent selection of demos for newsletter inclusion. |
| `newsletter_demos_deselect` | Y |  |  |  | Draft Post-Training Community Spotlights editions only; removes demos from selection with notification protection. |
| `newsletter_notifications_send` | Y |  |  |  | Draft Post-Training Community Spotlights editions only; sends feature notification emails to selected demos. |
| `content_page_comments_list` | Y | Y | Y | Y | Private editorial comments on visible Post-Training guest posts only. |
| `content_page_comment_create` | Y | Y | Y | Y | Creates private editorial comments on visible Post-Training guest posts only; notifications follow existing workflow. |
| `content_page_comment_resolve` | Y | Y | Y | Y | Resolves a visible private editorial comment thread. Passing a reply token resolves the root thread comment. |
| `content_page_comment_reopen` | Y | Y | Y | Y | Reopens a visible private editorial comment thread. Passing a reply token reopens the root thread comment. |
| `content_page_public_comments_list` | Y | Y | Y | Y | Reader-comment inspection for visible Post-Training guest posts only. |
| `content_page_editorial_status_update` | Y | Y | Y | Y | Mutates editorial status for visible Post-Training guest posts only; preserves existing notifications and audit logging. |
| `content_page_body_update` | Y | Y | Y | Y | Saves a new draft ContentPageVersion for any editable ContentPage (meetup pages, blog posts, docs, newsletter editions). Per-row authorization via `content_page_editable_by_api?`: index owner OR city owner of the page's weblog OR city-series owner of the page's series OR direct page author. Does not publish. |
| `content_page_body_publish` | Y | Y | Y | Y | Promotes a specific ContentPageVersion to live (republish). Requires the page to already have been published at least once. Does not re-fire announcement emails. Same row-level authorization as `content_page_body_update`. |
| `content_page_metrics_get` | Y | Y | Y | Y | Email performance metrics for visible content pages only. |
| `subscriber_search` | Y | Y | Y | N | City owners only for their weblog(s); series owners not authorized. |
| `subscriber_growth_stats_get` | Y | Y | Y | N | City owners only for their weblog(s); series owners not authorized. |
| `subscriber_opt_out_metrics_get` | Y | Y | Y | N | City owners only for their weblog(s); series owners not authorized. |
| `email_campaign_performance_get` | Y | Y | Y | Y | Campaign rows and metrics are filtered to caller-visible sends/events/content scope only. |
| `email_deliverability_health_get` | Y | Y | Y | N | City owners only for their weblog(s); series owners not authorized. |
| `email_fatigue_risk_get` | Y | Y | Y | N | City owners only for subscribers in authorized weblog scope; series owners not authorized. |
| `email_send_jobs_summary` | Y | Y | Y | N | Aggregate sent/intended/pending/suppressed counts for caller-visible meetup or content page send jobs. |
| `email_send_jobs_list` | Y | Y | Y | N | List send jobs scoped to caller-visible weblogs. Filter by status, date range, content page. |
| `email_send_job_get` | Y | Y | Y | N | Detailed send job status including delivery accounting, send progress, suppression breakdown. |
| `email_send_job_recipients_list` | Y | Y | Y | N | Paginated recipient list for a send job. Status filtering. Email fields follow masking policy. |
| `email_send_job_throughput_get` | Y | Y | Y | N | Send throughput time series for a job. Bucket by minute, 5min, or hour. |
| `email_send_jobs_compare` | Y | Y | Y | N | Compare up to 10 send jobs side by side with delivery and engagement metrics. |
| `email_send_job_ses_status_get` | Y | Y | N | N | SES quota status, active sends, stuck jobs, system alerts. Index owners only. |
| `subscriber_talk_history_get` | Y | Y | Y | N | City owners only for subscribers tied to their weblog scope. |
| `subscriber_score_details_get` | Y | Y | Y | N | City owners only for subscribers tied to their weblog scope. |
| `newsletter_spotlight_candidates_get` | Y | Y | Y | Y | Candidates and evidence must be scoped to caller-visible meetups/content/subscribers only. |
| `speaker_pipeline_candidates_get` | Y | Y | Y | Y | Candidate pool is constrained to authorized city/weblog/series scope, including city-name filters. |
| `sponsor_search` | Y | Y | Y | N | City owners only for sponsor objects in their authorized scope. |
| `sponsor_contact_list` | Y | Y | Y | N | City owners only for sponsor objects in their authorized scope. |
| `sponsor_pitch_generate` | Y | Y | Y | N | City owners only for sponsor/event context in authorized scope. |
| `sponsor_research_generate` | Y | Y | Y | N | City owners only; generates AI research summary for a sponsor or company. |
| `job_search` | Y | Y | N | N | Paid job ads only; restricted to index-owner roles. |
| `job_ad_data_get` | Y | Y | N | N | Full job ad detail + click analytics; restricted to index-owner roles. |
| `message_board_search` | Y | Y | Y | Y | Returns only boards the API-key user can access (membership/visibility constrained). |
| `message_board_messages_list` | Y | Y | Y | Y | Board must be caller-accessible; message rows are constrained to visible posts only. |
| `message_board_thread_get` | Y | Y | Y | Y | Fetches one visible thread by message-center URL or post token; board access is always enforced. |
| `message_board_post_search` | Y | Y | Y | Y | Access requires membership/visibility for the target board. |
| `message_board_post_create` | Y | Y |  |  | Caller must be a member of the target board. Creates posts and replies with optional image attachments. |
| `message_board_reaction_toggle` | Y | Y |  |  | Caller must have access to the target board. Toggles emoji reactions on posts. |
| `message_board_attachment_upload` | Y | Y |  |  | Caller must be a member of the target board. Uploads an image from URL for later use in posts. |
| `rag_chunk_search` | Y | Y | Y | Y | Chunk/object filtering must enforce object visibility and role scope. |
| `rag_chunks_get` | Y | Y | Y | Y | Returned chunks must be filtered to caller-visible objects only. |
| `docs_chat` | Y | Y | Y | Y | Access to private/fund docs is still restricted by document policy. |
| `docs_find` | Y | Y | Y | Y | Results include only docs visible to the caller under document policy. |
| `doc_get` | Y | Y | Y | Y | Retrieval is limited by document visibility policy and path validation. |
| `docs_edit` | Y | Y | Y | Y | Edits only API-published docs owned by the caller. Requires `base_sha256` and refuses stale or ambiguous replacements. |
| `docs_comments_list` | Y | Y | Y | Y | Lists visible comments and replies for a repo-backed or API-published doc. |
| `docs_comments_search` | Y | Y | Y | Y | Searches visible doc comments by comment body or inline selected text. |
| `docs_comment_create` | Y | Y | Y | Y | Creates a document comment or reply. Replies use `parent_note_token`; top-level comments may include inline text anchor metadata. |
| `docs_comment_resolve` | Y | Y | Y | Y | Resolves a visible document comment thread. Passing a reply token resolves the root thread comment. |
| `docs_comment_reopen` | Y | Y | Y | Y | Reopens a visible document comment thread. Passing a reply token reopens the root thread comment. |
| `docs_comment_delete` | Y | Y | Y | Y | Soft-deletes a caller-authored document comment. |
| `docs_publish` | Y | Y | Y | Y | Publishes a markdown/HTML document to the caller's published docs storage. Accepts optional `visibility` tier (`private`/`index_owner`/`city_owner`/`public`/`members`/`city:<slug>`), `members` email list, and `suppress_member_notifications`. |
| `docs_unpublish` | Y | Y | Y | Y | Deletes a published document from the caller's storage. Also destroys the `PublishedDoc` row and any `DocMembership` rows for that doc. |
| `docs_published_list` | Y | Y | Y | Y | Lists all published documents for the authenticated API client. Each row includes `visibility`, `cities`, `members_count`, `pub_token`, `content_type`. |
| `docs_published_get` | Y | Y | Y | Y | Reads visibility tier, cities, and full members list for a single doc the caller has published. Restricted to the publisher (returns 404 otherwise). |
| `docs_visibility` | Y | Y | Y | Y | Changes the visibility tier of a doc the caller has already published. Cannot retarget another publisher's docs. |
| `docs_members_add` | Y | Y | Y | Y | Adds emails to a doc's `members` list (additive on top of any base tier). Stub Clients are created for unknown emails, mirroring the docs UI flow. Accepts `suppress_member_notifications`. |
| `docs_members_remove` | Y | Y | Y | Y | Removes emails from a doc's `members` list. |
| `weblog_list` | Y | Y | N | N | List recently provisioned/launched weblogs. Index owners only. |
| `meetup_time_series_get` | Y | Y | Y | Y | Return event frequency time series for a weblog. |
| `global_hackathon_list` | Y | Y | Y | Y | List global hackathon series with series-level stats and per-city hackathon completeness metadata. |
| `global_hackathon_cities_list` | Y | Y | Y | Y | List participating city hackathons for a series, including location and per-field missing flags. |
| `content_brand_scrub_analyze` | Y | Y | Y | Y | No object lookup required; standard role check still applies. |
| `restricted_content_brand_scrub_analyze` | Y | Y | N | N | Restricted policy surface; limited to index-owner roles. |
| `logo_search` | Y | Y | Y | Y | Global asset scope. Returns logos visible across the network. |
| `social_post_generate` | Y | Y | Y | Y | Generates social media post drafts from meetup/rsvp/content/client/sponsor sources. |
| `event_promo_generate` | Y | Y | Y | Y | Generates event promotion package for a meetup. |
| `discussion_topics_generate` | Y | Y | Y | Y | Generates AI discussion topics for a meetup based on its content. |
| `media_file_search` | Y | Y | N | N | Search files across all folders by name, uploader, note. `index_video_editor` also allowed. |
| `media_transcript_search` | Y | Y | N | N | Search across transcript text content. `index_video_editor` also allowed. |
| `media_folder_list` | Y | Y | N | N | Browse media folders and files. `index_video_editor` also allowed. |
| `media_folder_create` | Y | Y | N | N | Create a new folder or subfolder. `index_video_editor` also allowed. |
| `media_file_get` | Y | Y | N | N | Get metadata for a media file. `index_video_editor` also allowed. |
| `media_file_download` | Y | Y | N | N | Get presigned download URL. `index_video_editor` also allowed. |
| `media_file_upload` | Y | Y | N | N | Upload a file (base64, max 50 MB). `index_video_editor` also allowed. |
| `media_folder_info` | Y | Y | N | N | Get folder info including associated meetup. `index_video_editor` also allowed. |
| `media_note_update` | Y | Y | N | N | Update sticky note on file or folder. `index_video_editor` also allowed. |
| `media_file_transcript_generate` | Y | Y | N | N | File-oriented route to initiate async transcription of audio/video. Index owners only. |
| `media_file_transcript_get` | Y | Y | N | N | File-oriented route to get transcript JSON and plain text. `index_video_editor` also allowed. |
| `media_file_transcript_status` | Y | Y | N | N | File-oriented route to poll transcription status. `index_video_editor` also allowed. |
| `media_file_scale_down` | Y | Y | N | N | Initiate async video scale-down. `index_video_editor` also allowed. |
| `media_file_scale_down_status` | Y | Y | N | N | Poll video scale-down status. `index_video_editor` also allowed. |
| `media_file_move` | Y | Y | N | N | Move a file to a different accessible folder, including across accessible AIT weblogs. `index_video_editor` also allowed. |
| `media_file_delete` | Y | Y | N | N | Delete a media file and its S3 object. `index_video_editor` also allowed. |
| `media_file_render` | Y | Y | N | N | Render full content of a text/markdown/JSON file. `index_video_editor` also allowed. |
| `media_transcript_get` | Y | Y | N | N | Get full transcript JSON for a media file. `index_video_editor` also allowed. |
| `media_transcript_generate` | Y | Y | N | N | Initiate async transcription of audio/video. Index owners only. |
| `media_transcript_status` | Y | Y | N | N | Poll transcription status. `index_video_editor` also allowed. |
| `media_transcript_delete` | Y | Y | N | N | Delete transcript data while keeping the media file. `index_video_editor` also allowed. |
## Contact Field Visibility Policy

- `email` follows the existing `api_access_allow_email_addresses` masking/visibility behavior.
- `social_links` (`linkedin_url`, `github_url`, `twitter_handle`, `twitter_url`) is returned only when one of these role/scope conditions is met:
  - caller is an admin or AI Tinkerers index owner, or
  - caller is a full city owner, the target person is connected to that owned chapter's API-visible subscriber/RSVP scope, and that weblog's `api_access_allow_social_links` switch is enabled.
- `social_links` is never returned to city readers, index readers, or series-limited city roles by default. It is also omitted when the target person has no stored social links.
- `phone_number` is only returned when all of the following are true:
  - the target person has a phone number on file, and
  - one of these role conditions is met:
    - caller is an admin or AI Tinkerers index owner (always allowed), or
    - caller is a city owner/city series owner and target person is a blog owner or AI Tinkerers index owner, or
    - caller is a city owner/city series owner and target person is an approved upcoming speaker (main stage or science fair) in caller-visible scope.
- In all other cases, `phone_number` is omitted from API payloads.

## Operational Safety Limits

These limits are part of the API contract and are required for production safety.

- Authorization is mandatory on every request. There is no unauthenticated or bypass mode.
- Best-effort API behavior:
  - Search/disambiguation endpoints return only the top-ranked subset.
  - No pagination support for search/list endpoints.
  - If `page`, `per_page`, or `cursor` is provided to a non-export search/list endpoint, return `error.code = "unsupported_parameter"`.
  - If more data exists than the cap, response includes `data.truncated = true`.
- Default payload limits:
  - JSON body max: `64 KB`
  - `query` max length: `200` chars
  - `note` max length: `2,000` chars
  - Any free-text filter/token field max length: `200` chars unless specified below
  - Array inputs default max length: `25`
- Default timeout model:
  - Sync read/search endpoints: hard timeout `8s`
  - Sync mutation endpoints: hard timeout `10s`
  - Generation endpoints: hard timeout `20s`
  - Async kickoff endpoints must return quickly with job metadata and never hold open long-running work
- Default rate limit model:
  - Per API key, per endpoint.
  - On exceed: HTTP `429` + `error.code = "rate_limited"`.
  - Daily caps apply only where explicitly listed in the Endpoint Limits Matrix.

### Endpoint Limits Matrix

All limits below are per API key.

| API (Tool Name) | Rate Limit | Timeout | Request Caps | Response Caps / Behavior |
|---|---|---|---|---|
| `client_get` | 60 rpm | 6s | one identifier input | single record only |
| `client_search` | 30 rpm | 8s | `query<=200`, `limit` default `25`, max `25` (clamped) | top `25` matches max, best-effort, `truncated` flag |
| `client_profile_search` | 20 rpm | 10s | profile filters, `query<=400`, `limit` default `25`, max `200` | scoped profile-answer matches plus optional facets |
| `talk_history_list` | 30 rpm | 8s | `limit` default `50`, max `100` (clamped) | max `100` rows |
| `attendance_history_get` | 30 rpm | 8s | `limit` default `50`, max `100` (clamped) | max `100` events |
| `rsvp_get` | 60 rpm | 6s | one RSVP identifier | single record only |
| `rsvp_status_history_list` | 30 rpm | 8s | one RSVP selector (`rsvp_token` or `rsvp_id`), `limit` default `50`, max `200` (clamped), optional `cursor` | max `200` history events per page, newest-first, returns `next_cursor` when more history exists |
| `rsvp_search` | 30 rpm | 8s | `query<=200`, no pagination params, `limit` default `25`, max `25` (clamped) | top `25` matches max, best-effort, `truncated` flag |
| `rsvp_summary` | 30 rpm | 8s | same filters as `rsvp_search`, `group_by` optional, `limit` default `100`, max `500` groups | exact count plus optional grouped counts; no row paging required |
| `rsvp_assessment_get` | 30 rpm | 8s | one RSVP identifier | single payload |
| `rsvp_email_preview_get` | 20 rpm | 10s | one RSVP identifier | single preview payload |
| `rsvp_export_csv` | 4 rpm | 6s (kickoff) | filter payload only, no pagination params | async export, max `5,000` rows/export |
| `rsvp_export_attio_json` | 4 rpm | 6s (kickoff) | filter payload only, no pagination params | async export, max `2,000` rows/export |
| `rsvp_add_to_attio` | 12 rpm | 10s | `note<=2000` | single mutation result |
| `rsvp_awaiting_payment_list` | 20 rpm | 8s | `meetup_token` optional, `weblog_token` optional, `limit` default `100`, max `500` | max `500` rows with RSVP, client, and meetup data |
| `rsvp_alumni_events_list` | 30 rpm | 8s | one RSVP identifier, implicit cap | max `50` events |
| `meetup_search` | 30 rpm | 8s | `query<=200`, `limit` default `25`, max `25` (clamped) | top `25` matches max, best-effort |
| `upcoming_events_list` | 30 rpm | 8s | `limit` default `25`, max `100`, optional `region<=120`, `city<=120`, `event_type<=40` | max `100` upcoming events, `truncated` flag |
| `photo_search` | 30 rpm | 8s | `query<=200`, `limit` default `50`, max `100` (clamped), optional `scope` (`all`/`mine`) + metadata filters, `exclude_text_overlays` default `true` | top `100` photos max, best-effort, `truncated` flag |
| `technology_list` | 40 rpm | 8s | `query<=200`, `limit` default `100`, max `250`, `offset` max `10,000` | top `250` technologies max, ranked by indexed project count |
| `technology_projects_list` | 40 rpm | 8s | one technology selector, `limit` default `25`, max `100`, `offset` max `20,000` | top `100` public-safe project rows max, `truncated` flag |
| `weblog_lookup_city` | 30 rpm | 6s | `city<=120` chars | top `10` matches max |
| `weblog_universal_search` | 20 rpm | 8s | `query<=200`, `object_types<=10` | max `25` total results across groups |
| `editorial_submissions_list` | 20 rpm | 8s | optional filters: `editorial_status`, `author_query`, `series_token`, `date_from`, `date_to`, `include_published`, `limit` default `50`, max `100` | max `100` guest-post submissions, `truncated` flag |
| `content_page_get` | 30 rpm | 8s | one selector: `content_page_token` or `slug` | single article payload including body markdown/text |
| `content_page_ai_assisted_draft_create` | 12 rpm admission attempts; enqueue guard accepts max 1 job / 5 min | 8s kickoff | `weblog_token`, `instructions<=12000`, optional `type` (`event`/`content`) | 202 with queued job metadata; 409 if another AI-assisted page job is active; 429 if the 5-minute enqueue guard is still cooling down |
| `ai_assisted_draft_run_list` | 30 rpm | 8s | optional filters: `city`, `weblog_token`, `state`, `stage`, `client_email`, `since`, `mine`, `limit` default `25`, max `100` | visible run rows with safe diagnostics and retry guidance |
| `ai_assisted_draft_run_get` | 30 rpm | 8s | run `token` | one visible run with safe diagnostics, instructions preview, and generated page summary |
| `ai_assisted_draft_run_retry` | 10 rpm | 8s | run `token` | 202 with queued retry job metadata or a structured retry blocker |
| `ai_assisted_draft_recent_pages` | 30 rpm | 8s | optional filters: `city`, `weblog_token`, `since`, `limit` default `25`, max `100` | generated page title, URL, summary, and originating run token |
| `newsletter_selected_demos_get` | 30 rpm | 8s | one selector: `content_page_token` or `slug` | selected demo rows + featured count for one draft newsletter edition |
| `newsletter_selected_demo_ranks_update` | 12 rpm | 10s | one selector: `content_page_token` or `slug`; `ranked_rsvp_tokens[]` max `50` | updated selected demo rows after a rank-only mutation |
| `newsletter_eligible_demos_list` | 10 rpm | 15s | one selector: `content_page_token` or `slug` | eligible demo candidates with scoring, nomination data, selection status |
| `newsletter_demos_select` | 12 rpm | 10s | one selector: `content_page_token` or `slug`; `rsvp_tokens[]` max `50` | newly selected + already selected tokens, updated demo list |
| `newsletter_demos_deselect` | 12 rpm | 10s | one selector: `content_page_token` or `slug`; `rsvp_tokens[]` max `50`, optional `force` boolean | deselected + refused tokens (notification protection) |
| `newsletter_notifications_send` | 5 rpm | 30s | one selector: `content_page_token` or `slug`; optional `rsvp_tokens[]` max `50` | per-demo notification status with deduplication |
| `content_page_comments_list` | 20 rpm | 8s | one selector: `content_page_token` or `slug` | full private editorial thread for one article |
| `content_page_comment_create` | 12 rpm | 10s | one selector: `content_page_token` or `slug`, `content<=2000`, optional `parent_note_token` | single mutation result + created comment |
| `content_page_comment_resolve` | 12 rpm | 8s | one selector: `content_page_token` or `slug`, `note_token` required | resolved root thread comment plus article metadata |
| `content_page_comment_reopen` | 12 rpm | 8s | one selector: `content_page_token` or `slug`, `note_token` required | reopened root thread comment plus article metadata |
| `content_page_public_comments_list` | 20 rpm | 8s | one selector: `content_page_token` or `slug` | full public comment thread for one article |
| `content_page_editorial_status_update` | 12 rpm | 10s | one selector: `content_page_token` or `slug`, `status` required, optional `note<=2000` | single mutation result + refreshed article |
| `content_page_body_update` | 12 rpm | 15s | one selector: `content_page_token`, `content_page_id`, or `slug`; `body_markdown<=200000`, optional `title<=400`, optional `expected_live_version_token`, optional `note<=2000`; request body cap 512 KB | created draft version + refreshed article + current live/latest version tokens |
| `content_page_body_publish` | 6 rpm | 15s | one selector: `content_page_token`, `content_page_id`, or `slug`; `version_token` required, optional `expected_current_live_token`, optional `note<=2000` | promoted version + previous live version token |
| `content_page_metrics_get` | 20 rpm | 8s | one selector: `content_page_token` or `slug` | single metrics payload |
| `subscriber_search` | 30 rpm | 8s | `query<=200`, `limit` default `25`, max `25` (clamped) | top `25` matches max, best-effort |
| `subscriber_growth_stats_get` | 20 rpm | 8s | date window max `730` days | max `365` buckets |
| `subscriber_opt_out_metrics_get` | 20 rpm | 8s | date window max `730` days | max `365` buckets |
| `email_campaign_performance_get` | 20 rpm | 8s | date window max `730` days overall; campaign row mode (`include_campaigns=true`) is further limited by selector breadth: `365` days for `content_page_token|meetup_token`, `180` for `series_token`, `31` for `weblog_token|weblog_tokens`; campaign row `limit` default `50`, max `100`, `offset` max `10000`, optional `sort`, `sort_dir`, `min_sends`, `include_campaigns`, `include_trends`, `include_summary`, selectors `weblog_token|weblog_tokens|content_page_token|meetup_token|series_token` | paginated campaign rows, optional `campaign_pagination`, optional max `365` trend buckets, optional summary |
| `email_deliverability_health_get` | 20 rpm | 8s | date window max `365` days | max `25` alerts, max `25` sender-domain rows, max `25` risky segments |
| `email_fatigue_risk_get` | 15 rpm | 8s | date window max `180` days, `limit` default `100`, max `200` (clamped) | max `200` subscribers + tier summary, `truncated` flag |
| `email_send_jobs_summary` | 20 rpm | 8s | `meetup_token` or `content_page_token` required; date window max `365` days; `limit` default `10`, max `50` | aggregate summary + recent send job rows |
| `email_send_jobs_list` | 20 rpm | 8s | date window max `365` days, `limit` default `25`, max `100` (clamped), `status` filter | max `100` send job rows with delivery accounting |
| `email_send_job_get` | 30 rpm | 8s | one `token` required | single send job with full progress, suppression, pipeline detail |
| `email_send_job_recipients_list` | 15 rpm | 8s | one `token` required, `limit` default `50`, max `200`, `offset` max `10000` | max `200` recipient rows per page |
| `email_send_job_throughput_get` | 20 rpm | 8s | one `token` required, `bucket` in (`minute`, `5min`, `hour`) | max `1440` throughput buckets + peak/avg rates |
| `email_send_jobs_compare` | 10 rpm | 10s | `tokens[]` max `10` required | max `10` comparison rows with engagement rates |
| `email_send_job_ses_status_get` | 10 rpm | 8s | no params | SES quota, active jobs, stuck jobs, alerts |
| `subscriber_talk_history_get` | 30 rpm | 8s | one selector, implicit cap | max `100` rows |
| `subscriber_score_details_get` | 30 rpm | 8s | one selector | single payload |
| `newsletter_spotlight_candidates_get` | 10 rpm | 12s | date window max `365` days, `limit` default `25`, max `50` (clamped) | max `50` ranked candidates + evidence snippets |
| `speaker_pipeline_candidates_get` | 15 rpm | 10s | `city_names<=50`, date window max `730` days, `limit` default `50`, max `100` (clamped) | max `100` ranked candidates, `truncated` flag |
| `sponsor_search` | 30 rpm | 8s | `query<=200`, `limit` default `25`, max `25` (clamped) | top `25` matches max, best-effort |
| `sponsor_contact_list` | 20 rpm | 8s | one sponsor selector | max `25` contacts |
| `sponsor_pitch_generate` | 10 rpm | 20s | context JSON max `64 KB` | one pitch + up to `3` variants |
| `sponsor_research_generate` | 10 rpm | 20s | `sponsor_token` or `name` required, optional `domain<=160`, `city<=120` | single research summary |
| `job_search` | 20 rpm | 8s | `query<=200`, `limit` default `25`, max `50` (clamped), filters: `city`, `company`, `status`, `sort` | top `50` paid jobs max, `truncated` flag |
| `job_ad_data_get` | 20 rpm | 8s | one ad selector (`ad_token`), optional `click_people_limit` max `1000` | single ad detail payload + click-people list |
| `message_board_search` | 20 rpm | 8s | `query<=200`, `limit` default `25`, max `50` (clamped) | top `50` boards max, visibility constrained to caller-accessible boards |
| `message_board_messages_list` | 20 rpm | 8s | `board_key` required, `query<=200`, `limit` default `30`, max `50` (clamped), optional `before_post_token` | top `50` messages max, optional thread expansion per message |
| `message_board_thread_get` | 20 rpm | 10s | `url` or `post_token` required, `thread_limit` default `200`, max `500`, `content_limit` default `8000`, max `20000` | one thread expanded from root through visible replies |
| `message_board_post_search` | 20 rpm | 8s | `query<=200` optional, `limit` default `25`, max `25` (clamped), filters: `mentioned_me`, `needs_response` | top `25` matches max, optional thread expansion, caller scope only |
| `message_board_post_create` | 10 rpm | 15s | `board_key` required, `content<=10000` required, `reply_to_post_token` optional, `image_urls<=4` optional | single post with optional attachments |
| `message_board_reaction_toggle` | 20 rpm | 8s | `board_key` required, `post_token` required, `reaction_type` required | toggles reaction, returns current reactions summary |
| `message_board_attachment_upload` | 10 rpm | 15s | `board_key` required, `image_url<=2048` required | single attachment token for use in `post_create` |
| `rag_chunk_search` | 20 rpm | 8s | `query<=400`, `object_types<=10`, `object_refs<=25`, `top_k<=25` | max `25` chunks, best-effort vector/text ranking |
| `rag_chunks_get` | 30 rpm | 8s | `chunk_ids<=25` | max `25` chunks |
| `docs_chat` | 10 rpm | 20s | `question<=400`, `chat_history<=12` messages | single answer payload with metadata |
| `docs_find` | 20 rpm | 8s | `query<=200`, `limit` default `20`, max `20` (clamped) | top `20` docs max, best-effort, `truncated` flag |
| `doc_get` | 30 rpm | 8s | `doc_path<=255` | one doc payload, `content_text` truncated at `500,000` bytes |
| `docs_edit` | 20 rpm | 8s | `doc_path<=200`, `base_sha256` required, `operations<=10`, literal `find`/`replace` only, optional `dry_run` | edited doc metadata plus old/new SHA-256 and match previews |
| `docs_comments_list` | 30 rpm | 8s | `doc_path<=255`, optional `author_client_token`, optional `limit` default `100`, max `200` | flat active comment/reply list with anchor metadata |
| `docs_comments_search` | 30 rpm | 8s | `doc_path<=255`, `query<=200`, optional `author_client_token`, optional `limit` default `100`, max `200` | filtered comment/reply list |
| `docs_comment_create` | 20 rpm | 8s | `doc_path<=255`, `content<=2000`, optional `parent_note_token`, optional `anchor.selected_text` | created comment/reply plus doc metadata |
| `docs_comment_resolve` | 20 rpm | 8s | `doc_path<=255`, `note_token` required | resolved root thread comment plus doc metadata |
| `docs_comment_reopen` | 20 rpm | 8s | `doc_path<=255`, `note_token` required | reopened root thread comment plus doc metadata |
| `docs_comment_delete` | 20 rpm | 8s | `doc_path<=255`, `note_token` required | soft-delete confirmation |
| `docs_publish` | 20 rpm | 10s | `path<=200`, `content` required (max 2 MB), `.md`/`.html` only, optional `visibility` (string or array), optional `cities` (array), optional `members` (email array), optional `suppress_member_notifications` boolean | single doc record with `visibility`, `cities`, `members_count`, URL |
| `docs_unpublish` | 20 rpm | 6s | `path<=200` | confirmation of deletion |
| `docs_published_list` | 30 rpm | 8s | no params | all published docs for the caller, sorted by modified date, with tier metadata per row |
| `docs_published_get` | 30 rpm | 6s | `path<=200` required | single doc record with `visibility`, `cities`, `members_count`, `members[]`, content metadata |
| `docs_visibility` | 30 rpm | 6s | `path<=200` required, `visibility` required, optional `cities` array | updated doc record |
| `docs_members_add` | 20 rpm | 6s | `path<=200` required, `emails` array (max 50) required, optional `suppress_member_notifications` boolean | resolved members list with `added` flag per email |
| `docs_members_remove` | 20 rpm | 6s | `path<=200` required, `emails` array (max 50) required | list of removed emails |
| `weblog_list` | 20 rpm | 8s | `status<=40`, `region<=120`, `limit` default `50` | list of weblog objects with status and region |
| `meetup_time_series_get` | 20 rpm | 8s | date window max `730` days | max `365` buckets |
| `global_hackathon_list` | 20 rpm | 8s | `query<=200`, `location_status<=40`, `limit` default `25`, max `25` | top `25` series max, optional embedded per-city hackathon audit rows |
| `global_hackathon_cities_list` | 20 rpm | 8s | `hackathon_slug<=180` required, `query<=200`, `location_status<=40`, `limit` default `100`, max `200` | top `200` city rows max with hackathon/location missing flags |
| `content_brand_scrub_analyze` | 10 rpm | 20s | `text<=12,000` chars | one analysis + up to `5` rewrites |
| `restricted_content_brand_scrub_analyze` | 8 rpm | 20s | `text<=12,000` chars | one analysis + up to `5` rewrites |
| `logo_search` | 20 rpm | 8s | `query<=200`, `scope` in (`smart_match`,`library`), `include_co_branded` boolean, `limit` default `20`, max `25` | top `25` matches max, best-effort vector/text ranking |
| `social_post_generate` | 12 rpm | 20s | `source_type<=40` required, `source_ref<=180` required, `platform<=40`, `goal<=40`, `tone<=120` | single generated post draft |
| `event_promo_generate` | 8 rpm | 25s | `meetup_token<=160` required, `package_type<=40`, `audience<=40` | meetup data + promo content artifacts |
| `discussion_topics_generate` | 8 rpm | 25s | `meetup_token<=160` required | meetup data + array of topic strings |
| `media_folder_list` | 30 rpm | 8s | `folder_token` optional | max 200 files per listing |
| `media_folder_create` | 10 rpm | 8s | `name` required, `parent_token` or `weblog_token` required | single folder record |
| `media_file_get` | 60 rpm | 6s | `file_token` required | single record |
| `media_file_download` | 20 rpm | 10s | `file_token` required | presigned URL, 1hr expiry |
| `media_file_upload` | 10 rpm | 30s | `filename`, `folder_token`, `body_base64` required; max 50 MB | single record |
| `media_folder_info` | 30 rpm | 6s | `folder_token` required | single record + meetup info |
| `media_note_update` | 20 rpm | 6s | `file_token` or `folder_token` + `note` required | single record |
| `media_file_search` | 20 rpm | 10s | `query<=200` required, optional `content_type<=20`, `has_transcript`, `folder_token<=60`, `weblog_token<=60`, `limit` default `20`, max `100` | max `100` files, multi-field search |
| `media_transcript_search` | 15 rpm | 15s | `query<=500` required, optional `content_type<=20`, `folder_token<=60`, `weblog_token<=60`, `limit` default `20`, max `100` | max `100` files with transcript snippets |
| `media_file_transcript_generate` | 5 rpm + 50/day | 6s (kickoff) | `file_token<=60` required | async transcription start, audio/video only |
| `media_file_transcript_get` | 30 rpm | 10s | `file_token<=60` required | transcript JSON and cached plain text |
| `media_file_transcript_status` | 60 rpm | 6s | `file_token<=60` required | transcription status + metadata |
| `media_file_scale_down` | 10 rpm | 10s | `file_token<=60` required | async job start, video files only |
| `media_file_scale_down_status` | 60 rpm | 6s | `file_token<=60` required | status + scaled file details on completion |
| `media_file_move` | 20 rpm | 6s | `file_token<=60` required, optional `folder_token<=60` | single file record in new location |
| `media_file_delete` | 10 rpm | 6s | `file_token<=60` required | confirmation of deletion + S3 cleanup |
| `media_file_render` | 30 rpm | 10s | `file_token<=60` required | full file content (text/md/JSON only, max 5 MB) |
| `media_transcript_get` | 30 rpm | 10s | `file_token<=60` required | full transcript JSON from S3 |
| `media_transcript_generate` | 5 rpm + 50/day | 6s (kickoff) | `file_token<=60` required | async transcription start, audio/video only |
| `media_transcript_status` | 60 rpm | 6s | `file_token<=60` required | transcription status + metadata |
| `media_transcript_delete` | 10 rpm | 6s | `file_token<=60` required | clears transcript data, keeps file |
### Rate Limit Headers

Endpoint rows list the manager-tier baseline. Effective limits are scaled by the authenticated API key owner's role:

| Caller tier | Multiplier | Example for `rsvp_search` |
|---|---:|---:|
| Attendee / valid API key without an API manager role | 0.5x | 15 rpm |
| City owner, city reader, series owner/reader, and other manager roles | 1x | 30 rpm |
| Index owner | 3x | 90 rpm |
| Admin | 10x | 300 rpm |

The same multiplier applies to optional daily caps. Every API response includes headers so clients can pace themselves proactively:

| Header | Description | Example |
|---|---|---|
| `X-RateLimit-Limit` | The effective RPM cap for this endpoint and caller tier | `30` |
| `X-RateLimit-Remaining` | Requests remaining in the current 1-minute window | `22` |
| `X-RateLimit-Reset` | UTC epoch timestamp (seconds) when the current window resets | `1709402460` |
| `X-RateLimit-Tier` | Caller tier used to calculate the effective cap | `manager` |

When a rate limit is exceeded (HTTP `429`), the response also includes:

| Header | Description | Example |
|---|---|---|
| `Retry-After` | Seconds to wait before retrying | `34` |

The `429` JSON body includes `error.details.retry_after` (integer seconds) matching the header value.

**Recommended client behavior:** On receiving a `429`, wait at least `Retry-After` seconds before retrying. For general resilience, use exponential backoff with jitter (e.g., `min(base * 2^attempt + random_jitter, max_wait)`). A reasonable starting point: base=1s, max_wait=60s. Well-behaved clients should also monitor `X-RateLimit-Remaining` and throttle proactively as it approaches zero.

## Standard Request/Response Shape

### Request

All endpoints accept JSON.

### Success Envelope

```json
{
  "ok": true,
  "request_id": "req_...",
  "data": {},
  "resolution": {
    "matched_by": "client_token",
    "matched_id": "client_..."
  },
  "warnings": []
}
```

### Error Envelope

```json
{
  "ok": false,
  "request_id": "req_...",
  "error": {
    "code": "not_found",
    "message": "No client found for provided identifier(s).",
    "details": {}
  }
}
```

## Multi-Identifier Lookup Policy

Many endpoints accept multiple identifier options so different agents can call the same API with whatever they already know.

- Preferred identifier order for exact lookup:
  1. token
  2. numeric id
  3. email
- Name is treated as a search input, not an exact lookup key.
- If multiple exact identifiers are provided and disagree, return `error.code = "identifier_conflict"`.
- If name/email search yields multiple plausible matches, return:
  - `ok: true`
  - `data.matches[]`
  - `data.needs_disambiguation: true`

## Common Selector Objects

- `client_ref`:
  - `client_token` OR `email`
- `rsvp_ref`:
  - `rsvp_token` OR `rsvp_id`
- `meetup_ref`:
  - `meetup_token` OR `meetup_id` OR `event_url`
- `subscriber_ref`:
  - `subscriber_token` OR (`email` + `weblog_token`)
- `sponsor_ref`:
  - `sponsor_token` OR (`name` + optional `city`)

## API Catalog

Each API below includes a function name, tool name, one-sentence description, flexible inputs, and expected outputs.

Current catalog size: `199` endpoints/tools.

### 1) ClientGet

- Tool name: `client_get`
- Route: `POST /api/agents/v1/clients/get`
- Authentication: required (owner API key via `Authorization: Bearer <api_key>`, `X-API-Key`, or `X-Api-Key`)
- Signed-in identity: required (API key must resolve to an active authorized owner identity; browser session cookie is not required)
- What it does: Fetch one client profile using exact identifiers.
- Input options:
  - `client_token`
  - `email`
- Output:
  - `client`: `client_token`, `name`, `email`, optional `phone_number`, `title`, `short_bio`, location fields
  - `subscriber_tags[]`
  - `rsvp_activity_summary`
  - `attio_summary`
  - `fund_screening_summary`
  - `resolution`

### 2) ClientSearch

- Tool name: `client_search`
- Route: `POST /api/agents/v1/clients/search`
- Authentication: required (owner API key via `Authorization: Bearer <api_key>`, `X-API-Key`, or `X-Api-Key`)
- Signed-in identity: required (API key must resolve to an active authorized owner identity; browser session cookie is not required)
- What it does: Search clients by fuzzy text and return ranked candidates for disambiguation.
- Input options:
  - `query` (name/email/company/title text)
  - optional filters: `city`, `tag`, `has_presented`, `date_from`, `date_to`, `limit`
- Output:
  - `matches[]` with `client_token`, `name`, `email`, optional `social_links`, optional `phone_number`, `title`, `short_bio`, `city`, `match_score`, `match_reasons[]`
  - `needs_disambiguation` (boolean)

### 2a) ClientProfileSearch

- Tool name: `client_profile_search`
- Route: `POST /api/agents/v1/clients/profile_search`
- Authentication: required (owner API key via `Authorization: Bearer <api_key>`, `X-API-Key`, or `X-Api-Key`)
- Signed-in identity: required (API key must resolve to an active authorized owner identity; browser session cookie is not required)
- What it does: Search people by structured `/profile` networking answers. The search starts from the caller-visible client scope, so city managers only search people visible through their authorized weblogs/series while index owners can search their broader authorized scope.
- Input options:
  - `query` (optional keyword search over profile text fields)
  - `filters` object: `employment_any`, `employment_all`, `help_with_any`, `help_with_all`, `looking_for_any`, `looking_for_all`, `intro_preference_any`, `contact_preference_any`, `role_any`, `city`, `has_presented`, `exclude_contact_closed`
  - `intent` preset: `recruiting`, `hiring`, `sponsor_matching`, `warm_intros`
  - `text_fields[]`: `bio`, `skills`, `interests`, `investing`, `startup`, `projects`
  - `facets[]`: `employment`, `help_with`, `looking_for`, `intro_preference`, `contact_preference`, `role`, `city`
  - `limit`, `offset`
- Output:
  - `matches[]` with standard client fields plus `profile_answers`, `matched_fields[]`, `match_reasons[]`, `match_score`, and `refs`
  - `facets` with scoped counts for requested profile fields
  - `resolved_query`, `total_count`, `truncated`, pagination offsets

### 3) TalkHistoryList

- Tool name: `talk_history_list`
- Route: `POST /api/agents/v1/rsvps/talk_history`
- Authentication: required (owner API key via `Authorization: Bearer <api_key>`, `X-API-Key`, or `X-Api-Key`)
- Signed-in identity: required (API key must resolve to an active authorized owner identity; browser session cookie is not required)
- What it does: Return a client’s demo/talk RSVP history.
- Input options:
  - `client_token` OR `email`
  - optional: `include_unapproved`, `limit`
- Output:
  - `client`
  - `talk_rsvps[]` with `rsvp_token`, talk title/description, event URL, talk URL, tech stack, ratings, screening/Attio state

### 4) AttendanceHistoryGet

- Tool name: `attendance_history_get`
- Route: `POST /api/agents/v1/attendance_history`
- Authentication: required (owner API key via `Authorization: Bearer <api_key>`, `X-API-Key`, or `X-Api-Key`)
- Signed-in identity: required (API key must resolve to an active authorized owner identity; browser session cookie is not required)
- What it does: Return event attendance history for a single person with RSVP and speaking-slot semantics.
- Input options:
  - `client_token` OR `email`
  - optional: `ai_tinkerers_network_only`, `limit`
- Output:
  - `client`
  - `attendance_events[]` with event metadata, RSVP state, speaking-slot status, sponsor/presenter markers, optional feedback summary

### 5) RsvpGet

- Tool name: `rsvp_get`
- Route: `POST /api/agents/v1/rsvps/get`
- Authentication: required (owner API key via `Authorization: Bearer <api_key>`, `X-API-Key`, or `X-Api-Key`)
- Signed-in identity: required (API key must resolve to an active authorized owner identity; browser session cookie is not required)
- What it does: Fetch one RSVP with related client and meetup context.
- Input options:
  - `rsvp_token`
  - `rsvp_id`
- Output:
  - `rsvp`
  - `client`
  - `meetup`
  - `resolution`

### 5a) RsvpStatusHistoryList

- Tool name: `rsvp_status_history_list`
- Route: `POST /api/agents/v1/rsvps/status_history`
- Authentication: required (owner API key via `Authorization: Bearer <api_key>`, `X-API-Key`, or `X-Api-Key`)
- Signed-in identity: required (API key must resolve to an active authorized owner identity; browser session cookie is not required)
- What it does: Return append-only RSVP status history newest-first for one RSVP.
- Input options:
  - `rsvp_token` OR `rsvp_id`
  - optional: `limit`, `cursor`
- Output:
  - `events[]` with `event_id`, `rsvp_token`, `changed_at`, `from_status`, `to_status`, `actor_type`, optional `actor_token`, optional `actor_name`, `source`, optional `reason`, optional `request_id`
  - `next_cursor`
  - `resolution`

### 6) RsvpSearch

- Tool name: `rsvp_search`
- Route: `POST /api/agents/v1/rsvps/search`
- Authentication: required (owner API key via `Authorization: Bearer <api_key>`, `X-API-Key`, or `X-Api-Key`)
- Signed-in identity: required (API key must resolve to an active authorized owner identity; browser session cookie is not required)
- What it does: Search/filter RSVPs for demos and screening workflows.
- Input options:
  - text: `query`
  - optional filters: `city`, `region`, `meetup_token`, `date_from`, `date_to`, `speaker_status`, `status`, `rsvp_tag`, `payment_status`, `sort`, `limit`
  - explicit time filters: `time_field`, `time_from`, `time_to`, plus `created_from`/`created_to` as aliases for `time_field: "rsvp_created_at"`
  - identifier filters: `meetup_tokens[]` (array, max 50), `client_token`, `client_tokens[]` (array, max 50), `email`, `emails[]` (array, max 50), `weblog_token`
  - When both singular and array forms are given (e.g., `meetup_token` + `meetup_tokens[]`), they are merged into one filter.
  - Backward compatibility: `date_from` and `date_to` filter `meetups.start_time` unless `time_field` is provided. With `time_field`, the same dates filter that timestamp field.
  - Supported `time_field` values: `event_start_at`, `rsvp_created_at`, `rsvp_updated_at`, `speaker_submitted_at`, `speaker_approved_at`, `checked_in_at`.
  - `speaker_status` filters speaker proposals: `submitted`, `approved`, `not_approved`, `submitted_not_approved`, `pending_review`, `sidelined`, `withdrawn`, `main_stage`, or `science_fair`.
  - `status` is for RSVP/attendance state (`attending`, `waitlisted`, `denied`, `checked_in`, `no_show`, etc.). For speaker proposal questions, prefer `speaker_status`.
  - `rsvp_tag` filters by RSVP role/tag values such as `sponsor`, `organizer`, `speaker`, `volunteer`, `venue`, `mentor`, `judge`, `finalist`, `winner`, `gold`, `silver`, or `bronze`.
- Output:
  - `results[]` (RSVP summaries, capped)
  - RSVP summaries include `rsvp.created_at` and `rsvp.updated_at` for timestamp-filter verification.
  - RSVP summaries include `rsvp.speaker_status` and `rsvp.speaker_approval_status` so agents can distinguish submitted-but-not-approved speaker proposals from approved main-stage/science-fair speakers.
  - RSVP summaries include displayable RSVP `tags`, `tagged_sponsor`, `tagged_organizer`, `sponsor_contact_sharing_opted_in`, and `sponsor_contact_share_opted_in_at` for event operations diagnostics.
  - paid-event RSVP summaries include `rsvp.stripe_payment_completed`; when payment is completed they also include `rsvp.receipt_url`, the canonical printable receipt URL for attendee expense reimbursement
  - applied filters
- Agent guidance — common recipes:

  | Intent | Recommended params |
  |--------|-------------------|
  | Submitted speaker proposals for an event | `meetup_token: "<token>", speaker_status: "submitted", sort: "recent"` |
  | Approved speakers for an event | `meetup_token: "<token>", speaker_status: "approved"` |
  | Submitted but not approved speakers for an event | `meetup_token: "<token>", speaker_status: "not_approved"` |
  | Unscreened demos for next week's meetup | `meetup_token: "<token>", speaker_status: "pending_review", sort: "recent"` |
  | Recent demo submissions across a city | `city: "seattle", sort: "recent", limit: 25` |
  | RSVPs created in the last 24 hours | `time_field: "rsvp_created_at", time_from: "<24-hours-ago-iso8601>", time_to: "<now-iso8601>", sort: "recent"` |
  | Find past presenters to re-invite | `city: "seattle", speaker_status: "approved", date_from: "2025-01-01", date_to: "2025-12-31"` |
  | Audit denied RSVPs | `meetup_token: "<token>", status: "denied"` |
  | Search for a specific person's RSVPs | `query: "Jane Smith"` |
  | Look up RSVPs by exact email | `email: "jane@example.com"` |
  | RSVPs across multiple meetups | `meetup_tokens: ["<token1>", "<token2>"]` |
  | RSVPs for a specific client | `client_token: "<token>"` |
  | Sponsor representatives for an event | `meetup_token: "<token>", rsvp_tag: "sponsor"` |
  | Scope RSVPs to a city by weblog | `weblog_token: "<token>", status: "approved"` |

  - After finding an RSVP, chain with `rsvp_assessment_get` for the AI screening report.
  - Use `rsvp_export_csv` with the same filters to export results for spreadsheet review.
  - Use `weblog_token` instead of `city` when you have an exact weblog token from a prior call (avoids fuzzy name matching).

#### Related Aggregate: RsvpSummary

- Tool name: `rsvp_summary`
- Route: `POST /api/agents/v1/rsvps/summary`
- What it does: Count RSVPs using the same filters as `rsvp_search` without paging through rows.
- Input options:
  - same filter set as `rsvp_search`
  - optional `group_by`: `status`, `speaker_status`, `speaker_approval_status`, `city`, `meetup`, `day`, `week`, or `month`
- Output:
  - `total_count`
  - `groups[]` when `group_by` is provided
  - applied filters
- Example: for “RSVP records created in the last 24 hours,” call `rsvp_summary` with `time_field: "rsvp_created_at"`, `time_from`, and `time_to`.
- Example: for “How many speakers are approved for this event and how many are not?”, call `rsvp_summary` with `meetup_token`, `speaker_status: "submitted"`, and `group_by: "speaker_approval_status"`. The groups will contain `approved` and `not_approved`.

### 7) RsvpAssessmentGet

- Tool name: `rsvp_assessment_get`
- Route: `POST /api/agents/v1/rsvps/assessment`
- Authentication: required (owner API key via `Authorization: Bearer <api_key>`, `X-API-Key`, or `X-Api-Key`)
- Signed-in identity: required (API key must resolve to an active authorized owner identity; browser session cookie is not required)
- What it does: Return assessment details for one RSVP.
- Input options:
  - `rsvp_token` OR `rsvp_id`
- Output:
  - structured assessment payload
  - model metadata and timestamps when available

### 8) RsvpEmailPreviewGet

- Tool name: `rsvp_email_preview_get`
- Route: `POST /api/agents/v1/rsvps/email_preview`
- Authentication: required (owner API key via `Authorization: Bearer <api_key>`, `X-API-Key`, or `X-Api-Key`)
- Signed-in identity: required (API key must resolve to an active authorized owner identity; browser session cookie is not required)
- What it does: Return RSVP-centric email preview content for agent review.
- Input options:
  - `rsvp_token` OR `rsvp_id`
- Output:
  - `subject`
  - `preview_html`
  - `preview_text`

### 9) RsvpExportCsv

- Tool name: `rsvp_export_csv`
- Route: `POST /api/agents/v1/rsvps/export_csv`
- Authentication: required (owner API key via `Authorization: Bearer <api_key>`, `X-API-Key`, or `X-Api-Key`)
- Signed-in identity: required (API key must resolve to an active authorized owner identity; browser session cookie is not required)
- What it does: Export RSVP search scope to CSV.
- Input options:
  - same filter set as `rsvp_search`
- Output:
  - direct file response or `export_job` metadata with download URL/status

### 10) RsvpExportAttioJson

- Tool name: `rsvp_export_attio_json`
- Route: `POST /api/agents/v1/rsvps/export_attio_json`
- Authentication: required (owner API key via `Authorization: Bearer <api_key>`, `X-API-Key`, or `X-Api-Key`)
- Signed-in identity: required (API key must resolve to an active authorized owner identity; browser session cookie is not required)
- What it does: Queue Attio-format export for RSVP scope.
- Input options:
  - same filter set as `rsvp_search`
- Output:
  - `job_token`
  - queue state

### 11) RsvpAddToAttio

- Tool name: `rsvp_add_to_attio`
- Route: `POST /api/agents/v1/rsvps/add_to_attio`
- Authentication: required (owner API key via `Authorization: Bearer <api_key>`, `X-API-Key`, or `X-Api-Key`)
- Signed-in identity: required (API key must resolve to an active authorized owner identity; browser session cookie is not required)
- What it does: Push one RSVP lead to Attio.
- Input options:
  - `rsvp_token` OR `rsvp_id`
  - `note` (required)
- Output:
  - `state`
  - `deal_id`
  - `attio_url`

### 15) RsvpAlumniEventsList

- Tool name: `rsvp_alumni_events_list`
- Route: `POST /api/agents/v1/rsvps/alumni_events`
- Authentication: required (owner API key via `Authorization: Bearer <api_key>`, `X-API-Key`, or `X-Api-Key`)
- Signed-in identity: required (API key must resolve to an active authorized owner identity; browser session cookie is not required)
- What it does: List prior related events for a presenter RSVP.
- Input options:
  - `rsvp_token` OR `rsvp_id`
- Output:
  - `events[]` with event identity, date, city, URL

### 15a) RsvpAwaitingPaymentList

- Tool name: `rsvp_awaiting_payment_list`
- Route: `POST /api/agents/v1/rsvps/awaiting_payment`
- Authentication: required (owner API key)
- What it does: List RSVPs that are awaiting payment, scoped by meetup and/or weblog.
- Input options:
  - optional: `meetup_token`, `weblog_token`, `limit` (default 100, max 500)
- Output:
  - `results[]` with `rsvp`, `client`, `meetup` for each awaiting-payment RSVP
  - `count`

### 16) MeetupSearch

- Tool name: `meetup_search`
- Route: `POST /api/agents/v1/meetups/search`
- Authentication: required (owner API key via `Authorization: Bearer <api_key>`, `X-API-Key`, or `X-Api-Key`)
- Signed-in identity: required (API key must resolve to an active authorized owner identity; browser session cookie is not required)
- What it does: Search meetups by city, name, date window, and fuzzy query.
- Input options:
  - `query` (optional)
  - optional filters: `city`, `region`, `date_from`, `date_to`, `status`, `limit`
  - identifier filters: `weblog_token` (scope to a city by exact weblog token), `meetup_tokens[]` (array, max 50 — filter to specific meetups)
- Output:
  - `matches[]` with `meetup_token`, `weblog_token`, `content_page_token`, `event_name`, `event_type`, `starts_at`, `ends_at`, `timezone`, `city`, `region`, `country`, `location`, `event_url`, `status`, `rsvps` (registered/attending/waitlisted/cancelled/capacity), `organizer` (`client_token`, name/email/title/company), `refs`, match metadata
- Agent guidance — common recipes:

  | Intent | Recommended params |
  |--------|-------------------|
  | Upcoming events in a city | `city: "seattle", status: "upcoming"` |
  | Past events in a region last quarter | `region: "north_america", status: "past", date_from: "2025-10-01", date_to: "2025-12-31"` |
  | Find a specific event by name | `query: "LLM agents night"` |
  | All events across the network this month | `date_from: "2026-03-01", date_to: "2026-03-31"` |
  | Check capacity for an upcoming event | `meetup_token: "<token>"` — then read `rsvps.registered` vs `rsvps.capacity` |
  | Scope to a city by weblog token | `weblog_token: "<token>", status: "upcoming"` |
  | Fetch details for known meetups | `meetup_tokens: ["<token1>", "<token2>"]` |

  - Chain with `rsvp_search` (using `meetup_token`) to see who signed up, or `email_campaign_performance_get` (using `meetup_token`) to see how event emails performed.
  - Use `weblog_lookup_city` first if you have fuzzy city text (e.g., "SF" → resolved weblog token), then scope meetup searches with `weblog_token`.

### 16a) UpcomingEventsList

- Tool name: `upcoming_events_list`
- Route: `POST /api/agents/v1/meetups/upcoming`
- Authentication: required (owner API key)
- What it does: List future/upcoming events filtered by event type, region, and city.
- Input options:
  - optional: `limit` (default 25, max 100), `region`, `city`, `event_type` (one of: `dinner`, `hackathon`, `meetup`)
- Output:
  - `events[]` with serialized meetup search rows
  - `truncated`, `count`

### 16b) MeetupPerformanceGet

- Tool name: `meetup_performance_get`
- Route: `POST /api/agents/v1/meetups/performance`
- Authentication: required (owner API key)
- What it does: Return one aggregate row per meetup for a weblog and event date range, with RSVP counts and page-view traffic.
- Input options:
  - required: `weblog_token`, `date_from`, `date_to`
  - optional: `traffic_from`, `traffic_to`, `limit` (default 100, max 200)
- Output:
  - `events[]` with `meetup_token`, `content_page_token`, `event_name`, `starts_at`, `event_url`
  - `rsvps` aggregate counts: `registered`, `attending`, `waitlisted`, `cancelled`, `completed`, `capacity`
  - `traffic.page_views`
  - `conversion.completed_rsvps_per_page_view`
  - `summary` totals for event count, page views, completed RSVPs, and conversion
- Privacy: returns aggregate counts only. It does not expose raw `Event` rows, visitor IPs, user agents, clients, or individual analytics records.

### 17) WeblogLookupCity

- Tool name: `weblog_lookup_city`
- Route: `POST /api/agents/v1/weblogs/lookup_city`
- Authentication: required (owner API key via `Authorization: Bearer <api_key>`, `X-API-Key`, or `X-Api-Key`)
- Signed-in identity: required (API key must resolve to an active authorized owner identity; browser session cookie is not required)
- What it does: Resolve city text into weblog/network context.
- Input options:
  - `city` or lookup text
- Output:
  - resolved weblog/network object and confidence metadata

### 17a) WeblogLookup

- Tool name: `weblog_lookup`
- Route: `POST /api/agents/v1/weblogs/lookup`
- Authentication: required (owner API key)
- What it does: Resolve a domain or URL to a visible weblog.
- Input options:
  - `domain` (for example `ee.dream.page`)
- Output:
  - `weblog` with `weblog_token`, `weblog_name`, `city`, `domain`, `ai_tinkerers`, and `refs`

### 18) WeblogUniversalSearch

- Tool name: `weblog_universal_search`
- Route: `POST /api/agents/v1/weblogs/universal_search`
- Authentication: required (owner API key via `Authorization: Bearer <api_key>`, `X-API-Key`, or `X-Api-Key`)
- Signed-in identity: required (API key must resolve to an active authorized owner identity; browser session cookie is not required)
- What it does: Run broad search over a weblog/network scope.
- Input options:
  - `query`
  - optional: `weblog_token`, `object_types[]`
  - `object_types: ["weblog"]` can match by weblog name, city, token, or domain.
- Output:
  - grouped result sets by object type (capped)
  - total counts and truncation metadata

### 19) SubscriberSearch

### Chaining Conventions

- List/search rows include the primary token needed for the next call whenever available.
- Many list/search rows now also include a `refs` object containing related identifiers commonly needed for follow-up calls.
- Agent clients should preserve those tokens when presenting options back to users instead of paraphrasing away the identifiers.

### Guest Post Editorial APIs

- Tool name: `editorial_submissions_list`
- Route: `POST /api/agents/v1/content_pages/editorial/list`
- What it does: List Post-Training guest-post submissions in editorial flow for editors and owners.
- Input options:
  - optional: `editorial_status` (`draft`, `ready_for_review`, `editing_complete`)
  - optional: `author_query`, `series_token`, `date_from`, `date_to`, `include_published`, `limit`
- Output:
  - `submissions[]` with article metadata, author info, workflow state, and comment counts
  - each row includes `content_page_token` plus `refs` (`content_page_token`, `slug`, `weblog_token`, `series_token`, `blogger_token`, `author_client_token`)
  - each row includes `available_actions[]` for common editorial follow-up calls
  - `truncated`

- Tool name: `content_page_get`
- Route: `POST /api/agents/v1/content_pages/get`
- What it does: Return the full article payload for a guest post so CLI tools can analyze the draft body.
- Input options:
  - `content_page_token`
  - OR `slug`
- Output:
  - article metadata
  - `body_markdown`
  - `body_text`
  - author + editorial metadata
  - `refs` and `available_actions[]` for chaining into comments/status/metrics

- Tool name: `content_page_ai_assisted_draft_create`
- Route: `POST /api/agents/v1/content_pages/ai_assisted_draft/create`
- What it does: Queue the same AI-assisted page creator used by the web UI for a target weblog. The API call returns immediately; the generated draft and completion email are handled by the existing Sidekiq pipeline.
- Input options:
  - `weblog_token` (required)
  - `instructions` (required, max 12,000 chars)
  - optional: `type` (`event` or `content`)
- Safeguards:
  - accepts at most one AI-assisted page draft job every 5 minutes
  - rejects with HTTP 409 `already_running` while another AI-assisted page job is queued/running
  - rejects with HTTP 429 `rate_limited` during the 5-minute cooldown
- Output:
  - `queued: true`
  - `job` with Sidekiq `jid` and `queue`
  - `weblog`
  - `requested_type`
  - `safeguards`

- Tool name: `ai_assisted_draft_run_list`
- Route: `POST /api/agents/v1/ai_assisted_draft_runs`
- What it does: List AI Page Creator runs visible to the caller with safe status, retryability, current stage, and generated page summary when available.
- Input options:
  - optional: `city`, `weblog_token`, `state`, `stage`, `client_email`, `since`, `mine`, `limit`
- Output:
  - `summary` counts by state, active, and retryable
  - `runs[]` with run token, state, stage, weblog, requester visibility, diagnosis, actions, and generated page summary

- Tool name: `ai_assisted_draft_run_get`
- Route: `POST /api/agents/v1/ai_assisted_draft_runs/:token`
- What it does: Get one visible AI Page Creator run for status checks and chaining.
- Input options:
  - `token` (required)
- Output:
  - `run` with instructions preview/full allowed payload, safe diagnostics, retry guidance, and generated page metadata

- Tool name: `ai_assisted_draft_run_retry`
- Route: `POST /api/agents/v1/ai_assisted_draft_runs/:token/retry`
- What it does: Retry a failed or lost AI Page Creator run when the caller is authorized for that run.
- Input options:
  - `token` (required)
- Output:
  - `requeued: true`
  - `job` with Sidekiq `jid` and `queue`
  - updated `run`

- Tool name: `ai_assisted_draft_recent_pages`
- Route: `POST /api/agents/v1/ai_assisted_draft_runs/recent_pages`
- What it does: Return pages generated by visible successful AI Page Creator runs.
- Input options:
  - optional: `city`, `weblog_token`, `since`, `include_content_summary`, `limit`
- Output:
  - `pages[]` with page token, title, slug, canonical URL, summary, word count, originating run token, and weblog metadata

- Tool name: `newsletter_selected_demos_get`
- Route: `POST /api/agents/v1/newsletters/selected_demos/get`
- What it does: Return the demos currently selected for a draft Post-Training Community Spotlights edition.
- Input options:
  - `content_page_token`
  - OR `slug`
- Output:
  - newsletter edition metadata
  - `featured_demo_count`
  - `ranked_demo_count`
  - `demos[]` with `rank`, `sort_index`, RSVP, client, and meetup payloads

- Tool name: `newsletter_selected_demo_ranks_update`
- Route: `POST /api/agents/v1/newsletters/selected_demos/ranks/update`
- What it does: Update only `newsletter_demos.rank` for demos already selected in a draft Post-Training Community Spotlights edition. Only admins and AI Tinkerers index owners are allowed to call this endpoint.
- Input options:
  - `content_page_token`
  - OR `slug`
  - `ranked_rsvp_tokens[]` in desired order; ranks are reassigned to `1..N`
- Output:
  - refreshed newsletter edition metadata
  - `featured_demo_count`
  - `ranked_demo_count`
  - refreshed `demos[]`

- Tool name: `newsletter_eligible_demos_list`
- Route: `POST /api/agents/v1/newsletters/eligible_demos/list`
- What it does: List all eligible demo candidates for newsletter selection within the selection window, with scoring data and nomination counts.
- Access policy: index owners only.
- Input options:
  - `content_page_token`
  - OR `slug`
- Output:
  - newsletter edition metadata
  - `selection_window` with `start`, `end`
  - `total_eligible`, `total_selected`, `total_notified`
  - `demos[]` with `rsvp_token`, demo title, scores, nomination counts, selection status

- Tool name: `newsletter_demos_select`
- Route: `POST /api/agents/v1/newsletters/selected_demos/select`
- What it does: Idempotent selection of one or more demos for newsletter inclusion. Reports newly selected and already-selected tokens separately.
- Access policy: index owners only.
- Input options:
  - `content_page_token`
  - OR `slug`
  - `rsvp_tokens[]` (max 50)
- Output:
  - newsletter edition metadata
  - `newly_selected[]`, `already_selected[]`
  - `featured_demo_count`, `ranked_demo_count`
  - refreshed `demos[]`

- Tool name: `newsletter_demos_deselect`
- Route: `POST /api/agents/v1/newsletters/selected_demos/deselect`
- What it does: Remove demos from newsletter selection. Refuses to deselect already-notified demos unless `force=true`.
- Access policy: index owners only.
- Input options:
  - `content_page_token`
  - OR `slug`
  - `rsvp_tokens[]` (max 50)
  - optional: `force` (boolean, overrides notification protection)
- Output:
  - newsletter edition metadata
  - `deselected[]`, `not_selected[]`
  - `refused[]` with `rsvp_token`, `speaker_name`, `reason`, `notified_at` (when notification protection blocks deselection)
  - `featured_demo_count`, `ranked_demo_count`
  - refreshed `demos[]`

- Tool name: `newsletter_notifications_send`
- Route: `POST /api/agents/v1/newsletters/notifications/send`
- What it does: Send feature notification emails to selected demos that haven't been notified yet. Uses durable deduplication via MailLog.
- Access policy: index owners only.
- Input options:
  - `content_page_token`
  - OR `slug`
  - optional: `rsvp_tokens[]` (max 50; if omitted, sends to all pending featured demos)
- Output:
  - newsletter edition metadata
  - `sent_count`, `already_notified_count`, `skipped_count`, `error_count`
  - `notifications[]` with `rsvp_token`, `speaker_name`, `email`, `status` (`sent`/`already_notified`/`skipped`/`error`), `notified_at`, `reason`

- Tool name: `content_page_comments_list`
- Route: `POST /api/agents/v1/content_pages/comments/list`
- What it does: Return the private editorial comment thread for a guest post.
- Output:
  - editorial `content_page`
  - `comments[]` where each note/reply includes `refs.note_token`, `refs.parent_note_token`, and `refs.author_client_token`

- Tool name: `content_page_comment_create`
- Route: `POST /api/agents/v1/content_pages/comments/create`
- What it does: Create a private editorial comment or reply for a guest post; notifications follow the existing `content_page_participants` workflow.
- Input options:
  - `content_page_token`
  - OR `slug`
  - `content`
  - optional: `parent_note_token`

- Tool name: `content_page_comment_resolve`
- Route: `POST /api/agents/v1/content_pages/comments/resolve`
- What it does: Resolve a private editorial comment thread for a guest post. If `note_token` is a reply, the root thread comment is resolved.
- Input options:
  - `content_page_token`
  - OR `slug`
  - `note_token`

- Tool name: `content_page_comment_reopen`
- Route: `POST /api/agents/v1/content_pages/comments/reopen`
- What it does: Reopen a resolved private editorial comment thread for a guest post. If `note_token` is a reply, the root thread comment is reopened.
- Input options:
  - `content_page_token`
  - OR `slug`
  - `note_token`

- Tool name: `content_page_public_comments_list`
- Route: `POST /api/agents/v1/content_pages/public_comments/list`
- What it does: Inspect reader-visible public comments on a guest post.
- Output:
  - editorial `content_page`
  - `public_comments[]` where each comment/reply includes `refs.comment_token`, `refs.parent_comment_token`, and `refs.author_client_token`

- Tool name: `content_page_editorial_status_update`
- Route: `POST /api/agents/v1/content_pages/editorial_status/update`
- What it does: Advance or move back editorial status using the same notification + audit pipeline as the editor portal.
- Input options:
  - `content_page_token`
  - OR `slug`
  - `status`
  - optional: `note`

### ContentPage Theme Operations

Use these endpoints for page theme changes. Do not call browser-only routes such as `/content_page/:token/update_theme`; those routes are session/UI endpoints and may be challenged by Cloudflare.

- Tool name: `content_page_themes_list`
- Route: `GET|POST /api/agents/v1/content_pages/themes/list`
- What it does: List themes available to the authenticated caller, respecting admin-only and index-owner-only theme visibility. If a content page ref is provided, the response also includes the page and its current theme.
- Input options:
  - optional: one of `content_page_token`, `content_page_id`, or `slug`

- Tool name: `content_page_theme_update`
- Route: `POST /api/agents/v1/content_pages/theme/update`
- What it does: Set or clear a ContentPage theme through the Agents API with API-key auth, scope checks, row-level authorization, theme visibility rules, rate limits, and audit logging.
- Input options:
  - one of: `content_page_token`, `content_page_id`, or `slug`
  - one of: `theme`, `theme_key`, or `clear: true`
- Output:
  - `content_page` with refreshed `theme`
  - `previous_theme`
  - `current_theme`
- Errors: `invalid_request` (missing/unknown theme), `forbidden_scope` (caller cannot edit the page or cannot use that theme), `not_found`

### ContentPage Body Editing (save draft + publish + revert)

For the end-to-end story (model invariants, `ever_live` row preservation, notification-driven revert flow, conflict resolution UI), see **`docs/product/content_page_body_editing_and_revert.md`**. This section documents the API surface only.

Three endpoints let an agent rewrite any ContentPage's body markdown (meetup event pages, blog posts, newsletter editions, docs — anything backed by ContentPage), publish it as a new live version, or revert to a prior version. Authorization matches the web editor: the caller must be an index owner, a city owner whose weblog owns the page, a city-series owner whose series the page belongs to, or the page's direct author.

Save and publish are intentionally separate. `content_page_body_update` only writes a draft version; it does not change what the public sees. `content_page_body_publish` promotes a specific draft version to live. Both enforce optimistic concurrency via version tokens — stale tokens return HTTP 409 rather than silently overwriting another agent's or editor's work.

Publishing via this API is a **republish**: it flips `is_live` on ContentPageVersion, preserves `published_at`, does NOT toggle `ContentPage#is_live`, and does NOT re-fire announcement emails. First-time publish of a page still requires the web UI — calling publish on a page that has never been live returns 409.

- Tool name: `content_page_body_update`
- Route: `POST /api/agents/v1/content_pages/body/update`
- What it does: Save a new draft ContentPageVersion for the page. The draft is not visible to the public until promoted via `content_page_body_publish`.
- Request body cap: 512 KB (body_markdown itself capped at 200,000 characters).
- Input options:
  - One of: `content_page_token`, `content_page_id`, or `slug`
  - `body_markdown` (required, markdown text)
  - optional: `title` (max 400 chars)
  - optional: `expected_live_version_token` (the `version_token` of the current live version as last known by the caller — if it no longer matches, the call returns 409 `version_conflict` with the current live token in details)
  - optional: `note` (free-form audit note, max 2000 chars)
- Output:
  - `content_page` (editorial serialization of the refreshed page)
  - `draft_version` with `version_token`, `is_live: false`, `title`, timestamps
  - `live_version_token` (what the public still sees)
  - `latest_version_token`
  - `has_unsaved_changes: true`
  - `body_markdown_length`
- Errors: `invalid_request` (missing body, oversized), `forbidden_scope` (caller cannot edit this page), `not_found`, `version_conflict` (stale `expected_live_version_token`)

- Tool name: `content_page_body_publish`
- Route: `POST /api/agents/v1/content_pages/body/publish`
- What it does: Promote a specific ContentPageVersion (previously created via `content_page_body_update` or via the web editor) to `is_live: true`, demoting the prior live version. Refreshes meetup panel caches when the page backs a meetup. Does not re-fire announcement emails.
- Preconditions: the target page must already have been published at least once (`ContentPage#is_live` true AND `published_at` set). First-time publish must go through the web UI.
- Input options:
  - One of: `content_page_token`, `content_page_id`, or `slug`
  - `version_token` (required — the ContentPageVersion to promote; must belong to the named content page)
  - optional: `expected_current_live_token` (if set and does not match the current live version, returns 409)
  - optional: `note` (audit note)
- Output:
  - `content_page` (refreshed)
  - `live_version` (the newly-live version)
  - `previous_live_version_token`
  - `promoted: true` on success, or `already_live: true` if the target was already live (no-op)
- Errors: `invalid_request`, `forbidden_scope`, `not_found` (version belongs to a different content_page), `version_conflict` (first publish required, or stale `expected_current_live_token`)

Typical agent flow:

1. Call `content_page_get` to read the current body and capture the current live `version_token`.
2. Call `content_page_body_update` with the new `body_markdown` and `expected_live_version_token` set to the token from step 1. Capture the returned `draft_version.version_token`.
3. Optionally show the draft to a human for approval.
4. Call `content_page_body_publish` with the `version_token` from step 2 and `expected_current_live_token` set to the live token from step 1.

#### Reverting to a prior version

Every save (from the web editor or from `content_page_body_update`) writes a new `ContentPageVersion` row. Old versions are **never deleted** — they stay in `content_page_versions` indefinitely, with only one row carrying `is_live: true` at any time. Any version that has ever been live is flagged `ever_live: true` and is protected from the web editor's "discard unsaved changes" sweep.

Two primitives can make an older version live again:

1. `content_page_body_publish` — the generic forward/backward publish. Accepts any `version_token` on the page, optional `expected_current_live_token`. Use this when re-publishing your own draft or when you explicitly want to move to a specific version regardless of direction.
2. `content_page_body_revert` (**preferred for rollbacks**) — see the dedicated section below. Requires `expected_current_live_token`, refuses to move forward in time, and logs a distinct `content_page_body_reverted` Event so agent rollbacks are queryable as a group.

Revert flow:

1. Call `content_page_get` with `include_version_history: true` to list prior versions with their `version_token`, `created_at`, and author metadata.
2. Pick the `version_token` you want to make live again (e.g. the version immediately before an agent's edit).
3. Call `content_page_body_revert` with `target_version_token` set to that token and `expected_current_live_token` set to whatever is currently live. The 409 `version_conflict` response will fire if someone else republished in the meantime — which is what you want, because it prevents silently clobbering work you haven't seen.
4. The previously-live version becomes a normal (non-live) history row; you can forward-revert to it later by calling `content_page_body_publish` with its token.

Because revert never mutates or deletes old version rows, the history remains an accurate audit trail even after multiple round trips. If an agent republishes, a human reverts, and another agent republishes again, all four versions remain in `content_page_versions` with timestamps and actor attribution via `Event` log entries (`content_page_body_draft_created`, `content_page_body_published`, `content_page_body_reverted`).

- Tool name: `content_page_body_revert`
- Route: `POST /api/agents/v1/content_pages/body/revert`
- What it does: Promote an older ContentPageVersion back to `is_live: true`, demoting the current live version. Designed for rollbacks — refuses to move forward in time. Refreshes meetup panel caches when the page backs a meetup. Does not re-fire announcement emails.
- Preconditions: the target page must already have been published at least once. `target_version_token` must not be the current live version. `target_version` must be older than (created before) the current live version.
- Input options:
  - One of: `content_page_token`, `content_page_id`, or `slug`
  - `target_version_token` (required — the older ContentPageVersion to restore; must belong to the named content page)
  - `expected_current_live_token` (**required** — not optional like on publish; revert must surface races rather than silence them)
  - optional: `note` (audit note, max 2000 chars — strongly encouraged, e.g. "reverting agent edit that dropped the sponsor mention")
- Output:
  - `content_page` (refreshed)
  - `live_version` (the newly-live — now-restored — version)
  - `previous_live_version_token` (what just got demoted)
  - `reverted: true` on success
- Errors:
  - `invalid_request` — target_version is already live, is newer than the current live version, or references a different content_page
  - `forbidden_scope` — caller cannot edit this page (or the page has been emailed and the caller is author-scope only)
  - `not_found` — no version with that `target_version_token` exists on this page
  - `version_conflict` — stale `expected_current_live_token`, or page has never been live
- Rate limit: 3 rpm (tighter than publish's 6 rpm — revert should be rare and deliberate).

#### Authorization

Both endpoints are allowlisted to `ALL_STANDARD_ROLES`; per-request authorization is enforced at row level via `content_page_editable_by_api?`.

For pages that have already been distributed to subscribers via email (i.e. `content_page.emails_sent.any?`), the API collapses the editable check to **admin / index owner / city owner / city-series owner only** — the direct-author fallback is disabled, and a blogger author of such a page receives `forbidden_scope`. Authors must use the web UI (which surfaces the "this page has been emailed; changes will not be re-sent" warning inline) to edit a page that has already gone out to subscribers. This prevents an author-scoped API key from silently rewriting a newsletter edition whose sent copy is now immutable in subscriber inboxes.

Rate limits: `content_page_body_update` 12 rpm, `content_page_body_publish` 6 rpm.

- Tool name: `content_page_metrics_get`
- Route: `POST /api/agents/v1/content_pages/metrics/get`
- What it does: Return send/open/click metrics for the guest post email distribution.
- Output:
  - editorial `content_page` with `refs`
  - metrics payload

- Tool name: `subscriber_search`
- Route: `POST /api/agents/v1/subscribers/search`
- Authentication: required (owner API key via `Authorization: Bearer <api_key>`, `X-API-Key`, or `X-Api-Key`)
- Signed-in identity: required (API key must resolve to an active authorized owner identity; browser session cookie is not required)
- What it does: Search subscribers for disambiguation and targeting.
- Input options:
  - `query`
  - optional: `weblog_token`, `status`, `tag`, `limit`
- Output:
  - `matches[]` with `subscriber_token`, `client_token`, `email`, optional `social_links`, name fields (when available), tags, and `refs`

### 20) SubscriberGrowthStatsGet

- Tool name: `subscriber_growth_stats_get`
- Route: `POST /api/agents/v1/subscribers/growth_stats`
- Authentication: required (owner API key via `Authorization: Bearer <api_key>`, `X-API-Key`, or `X-Api-Key`)
- Signed-in identity: required (API key must resolve to an active authorized owner identity; browser session cookie is not required)
- What it does: Return subscriber growth time series for a weblog.
- Input options:
  - `weblog_token`
  - optional: `date_from`, `date_to`, `bucket` (`day`/`week`/`month`)
- Output:
  - `series[]` points and summary deltas

### 21) SubscriberOptOutMetricsGet

- Tool name: `subscriber_opt_out_metrics_get`
- Route: `POST /api/agents/v1/subscribers/opt_out_metrics`
- Authentication: required (owner API key via `Authorization: Bearer <api_key>`, `X-API-Key`, or `X-Api-Key`)
- Signed-in identity: required (API key must resolve to an active authorized owner identity; browser session cookie is not required)
- What it does: Return unsubscribe/opt-out metrics for a weblog.
- Input options:
  - `weblog_token`
  - optional: `date_from`, `date_to`, `bucket`
- Output:
  - opt-out metrics series and aggregate rates

### 22) SubscriberTalkHistoryGet

- Tool name: `subscriber_talk_history_get`
- Route: `POST /api/agents/v1/subscribers/talk_history`
- Authentication: required (owner API key via `Authorization: Bearer <api_key>`, `X-API-Key`, or `X-Api-Key`)
- Signed-in identity: required (API key must resolve to an active authorized owner identity; browser session cookie is not required)
- What it does: Return talk/demo history for one subscriber.
- Input options:
  - `subscriber_token`
  - OR (`email` + `weblog_token`)
  - OR `client_token` (if linked)
- Output:
  - `subscriber`
  - `talk_history[]`
  - `resolution`

### 23) SubscriberScoreDetailsGet

- Tool name: `subscriber_score_details_get`
- Route: `POST /api/agents/v1/subscribers/score_details`
- Authentication: required (owner API key via `Authorization: Bearer <api_key>`, `X-API-Key`, or `X-Api-Key`)
- Signed-in identity: required (API key must resolve to an active authorized owner identity; browser session cookie is not required)
- What it does: Return subscriber scoring factors and detail fields.
- Input options:
  - `subscriber_token`
  - OR (`email` + `weblog_token`)
- Output:
  - score detail payload
  - feature-level breakdown where available

### 24) SponsorSearch

- Tool name: `sponsor_search`
- Route: `POST /api/agents/v1/sponsors/search`
- Authentication: required (owner API key via `Authorization: Bearer <api_key>`, `X-API-Key`, or `X-Api-Key`)
- Signed-in identity: required (API key must resolve to an active authorized owner identity; browser session cookie is not required)
- What it does: Search sponsors by name/domain/city and return ranked matches.
- Input options:
  - `query`
  - optional filters: `city`, `industry`, `active_only`, `limit`
- Output:
  - `matches[]` with `sponsor_token`, sponsor name, website/domain, city, short profile, match metadata

### 25) SponsorContactList

- Tool name: `sponsor_contact_list`
- Route: `POST /api/agents/v1/sponsors/contacts`
- Authentication: required (owner API key via `Authorization: Bearer <api_key>`, `X-API-Key`, or `X-Api-Key`)
- Signed-in identity: required (API key must resolve to an active authorized owner identity; browser session cookie is not required)
- What it does: Return contact candidates for one sponsor.
- Input options:
  - `sponsor_token`
  - OR (`name` + optional `city`)
  - optional contact filters
- Output:
  - `contacts[]` (capped) with role/title/email/linkedin and confidence fields when present

### 26) SponsorPitchGenerate

- Tool name: `sponsor_pitch_generate`
- Route: `POST /api/agents/v1/sponsors/pitch_generate`
- Authentication: required (owner API key via `Authorization: Bearer <api_key>`, `X-API-Key`, or `X-Api-Key`)
- Signed-in identity: required (API key must resolve to an active authorized owner identity; browser session cookie is not required)
- What it does: Generate a sponsor pitch draft from sponsor and event context.
- Input options:
  - `sponsor_token` OR (`name` + optional `city`)
  - event/context payload
- Output:
  - `pitch_text`
  - optional variants and rationale snippets

### 26a) SponsorResearchGenerate

- Tool name: `sponsor_research_generate`
- Route: `POST /api/agents/v1/sponsors/research_generate`
- Authentication: required (owner API key)
- What it does: Generate an AI research summary for a sponsor or company.
- Access policy: index owners and city owners only.
- Input options:
  - `sponsor_token` OR `name` (required if no token)
  - optional: `domain`, `city`, `target_audience`, `context` (hash)
- Output:
  - `sponsor` (if token provided)
  - `research_summary`

### 27) MessageBoardSearch

- Tool name: `message_board_search`
- Route: `POST /api/agents/v1/message_boards/search`
- Authentication: required (owner API key via `Authorization: Bearer <api_key>`, `X-API-Key`, or `X-Api-Key`)
- Signed-in identity: required (API key must resolve to an active authorized owner identity; browser session cookie is not required)
- What it does: Search/list message boards that the API-key user can actually access.
- Input options:
  - optional `query`
  - optional `include_direct_messages` (boolean, default `true`)
  - optional `include_unread` (boolean, default `false`)
  - optional `limit` (default `25`, max `50`)
- Output:
  - `boards[]` (capped) with `board_key`, title, board metadata, URL, and optional unread count

### 27b) MessageBoardMessagesList

- Tool name: `message_board_messages_list`
- Route: `POST /api/agents/v1/message_boards/messages/list`
- Authentication: required (owner API key via `Authorization: Bearer <api_key>`, `X-API-Key`, or `X-Api-Key`)
- Signed-in identity: required (API key must resolve to an active authorized owner identity; browser session cookie is not required)
- What it does: Pull recent messages from one accessible board with optional mention/needs-response filters.
- Input options:
  - `board_key` (required)
  - optional `query`
  - optional `before_post_token` (cursor-like pagination by post token)
  - optional filters: `mentioned_me`, `needs_response`, `date_from`, `date_to`, `days_back`
  - optional thread options: `include_thread` (default `false`), `thread_limit` (default `40`, max `80`)
  - optional `limit` (default `30`, max `50`)
- Output:
  - `board`
  - `messages[]` (capped) with message metadata, snippet/content, mention flags, and optional `thread` payload
  - message and thread `content_text` values are capped at 8,000 characters per post by default

### 27c) MessageBoardThreadGet

- Tool name: `message_board_thread_get`
- Route: `POST /api/agents/v1/message_boards/threads/get`
- Authentication: required (owner API key via `Authorization: Bearer <api_key>`, `X-API-Key`, or `X-Api-Key`)
- Signed-in identity: required (API key must resolve to an active authorized owner identity; browser session cookie is not required)
- What it does: Fetch one message-board thread directly from a `message_center` URL or a `board_key` plus `post_token`.
- Input options:
  - optional `url` containing `board=` or `topic=` plus `post=`, or a `#board:post` fragment
  - optional `board_key` when `url` is omitted or does not identify the board
  - `post_token` required unless provided in `url`
  - optional `thread_limit` (default `200`, max `500`)
  - optional `content_limit` per post (default `8000`, max `20000`)
  - optional `include_attachments` (default `true`)
  - optional `include_reactions` (default `true`)
- Output:
  - `board`
  - `matched_post`
  - `thread` with `root_post_token`, `matched_post_token`, `truncated`, and ordered `posts[]`
  - `refs` with `board_key`, `post_token`, and `root_post_token`

### 27d) MessageBoardPostSearch

- Tool name: `message_board_post_search`
- Route: `POST /api/agents/v1/message_boards/posts/search`
- Authentication: required (owner API key via `Authorization: Bearer <api_key>`, `X-API-Key`, or `X-Api-Key`)
- Signed-in identity: required (API key must resolve to an active authorized owner identity; browser session cookie is not required)
- What it does: Search posts across caller-accessible boards (or a specific board) with mention and needs-response filters.
- Input options:
  - optional `board_key` (when omitted, searches caller-accessible boards)
  - optional `query`
  - optional filters: `mentioned_me`, `needs_response`, `date_from`, `date_to`, `days_back`
  - optional thread options: `include_thread` (default `true`), `thread_limit` (default `40`, max `80`)
  - optional `limit` (default `25`, max `25`)
- Output:
  - `matches[]` (capped) with post metadata, snippets, mention/needs-response flags, and optional thread payload
  - message and thread `content_text` values are capped at 8,000 characters per post by default

### 28) MessageBoardPostCreate

- Tool name: `message_board_post_create`
- Route: `POST /api/agents/v1/message_boards/posts/create`
- Authentication: required (owner API key via `Authorization: Bearer <api_key>`, `X-API-Key`, or `X-Api-Key`)
- Signed-in identity: required (API key must resolve to an active authorized owner identity; browser session cookie is not required)
- What it does: Create a new post or reply on a message board. Caller must be a member of the board. Optionally attach images by URL.
- Input options:
  - `board_key` (required) - the board to post to
  - `content` (required, max 10000 chars) - the post text
  - optional `reply_to_post_token` - token of an existing post to reply to (must be in the same board)
  - optional `image_urls[]` (max 4) - array of public image URLs to attach
- Output:
  - `post_token`, `board_key`, `content`, `author`, `posted_at`, `parent_post_token`, `url`, `attachments[]`, `refs`

### 29) MessageBoardReactionToggle

- Tool name: `message_board_reaction_toggle`
- Route: `POST /api/agents/v1/message_boards/reactions/toggle`
- Authentication: required (owner API key via `Authorization: Bearer <api_key>`, `X-API-Key`, or `X-Api-Key`)
- Signed-in identity: required (API key must resolve to an active authorized owner identity; browser session cookie is not required)
- What it does: Toggle an emoji reaction on a post. If the caller already has a reaction of the specified type, it is removed; otherwise it is added.
- Input options:
  - `board_key` (required)
  - `post_token` (required)
  - `reaction_type` (required) - one of: `thumbs_up`, `thumbs_down`, `love`, `haha`, `fire`, `100`, `trophy`, `pizza`, `rocket`, `robot`, `rainbow`, `salute`, `gen_ai`
- Output:
  - `post_token`, `board_key`, `reaction_type`, `action` ("added" or "removed"), `reactions` (object with reaction type counts)

### 30) MessageBoardAttachmentUpload

- Tool name: `message_board_attachment_upload`
- Route: `POST /api/agents/v1/message_boards/attachments/upload`
- Authentication: required (owner API key via `Authorization: Bearer <api_key>`, `X-API-Key`, or `X-Api-Key`)
- Signed-in identity: required (API key must resolve to an active authorized owner identity; browser session cookie is not required)
- What it does: Upload an image from a public URL for later attachment to a post. Returns an attachment token. This is optional since `message_board_post_create` accepts `image_urls` directly.
- Input options:
  - `board_key` (required)
  - `image_url` (required, max 2048 chars) - public URL of the image to upload
- Output:
  - `attachment_token`, `image_url` (processed imgix URL), `board_key`

### 31) RagChunkSearch

- Tool name: `rag_chunk_search`
- Route: `POST /api/agents/v1/rag_chunks/search`
- Authentication: required (owner API key via `Authorization: Bearer <api_key>`, `X-API-Key`, or `X-Api-Key`)
- Signed-in identity: required (API key must resolve to an active authorized owner identity; browser session cookie is not required)
- What it does: Run vector/text search over structured RAG chunks across major object types.
- Input options:
  - `query` (required)
  - optional:
    - `object_types[]` (for example `client`, `rsvp`, `meetup`, `subscriber`, `sponsor`)
    - `object_refs[]` (limit search to specific objects)
    - `top_k`
    - `min_score`
    - `include_payload` (default `true`)
- Output:
  - `chunks[]` where each chunk includes:
    - `chunk_id`
    - `object_type`
    - `object_token`
    - `score`
    - `text`
    - `summary`
    - `payload_json` (structured machine-readable metadata)
    - `citations[]` (URLs/tokens where applicable)
- Agent guidance — usage tips:
  - Valid `object_types`: `client`, `rsvp`, `meetup`, `subscriber`, `sponsor` — use these to narrow search to a specific domain.
  - Start with `top_k: 10` and `min_score: 0.7` for focused results; lower `min_score` to `0.5` for broader exploratory searches.
  - Use `object_refs[]` to search within a specific person or event's data (e.g., all chunks about a particular client).
  - **When to use this vs other endpoints:**
    - Use `rag_chunk_search` when you need cross-domain semantic search (e.g., "who talked about LLM agents?" spans clients, RSVPs, and meetups).
    - Use `docs_chat` or `docs_find` when the answer lives in documentation, not in people/event data.
    - Use `client_search` or `rsvp_search` when you already know you're looking for a person or RSVP and have a name/keyword.
  - The `citations[]` field links chunks back to source records — use these to chain into detail endpoints (e.g., `client_get`, `rsvp_get`).

  | Intent | Recommended params |
  |--------|-------------------|
  | Find people who worked on a topic | `query: "RAG pipelines", object_types: ["client", "rsvp"], top_k: 10` |
  | Everything about a specific person | `object_types: ["client"], object_refs: ["client_token_here"]` |
  | Cross-domain search for a theme | `query: "computer vision", top_k: 20, min_score: 0.5` |
  | Sponsors related to a technology | `query: "vector databases", object_types: ["sponsor"], top_k: 10` |

### 29) RagChunksGet

- Tool name: `rag_chunks_get`
- Route: `POST /api/agents/v1/rag_chunks/get`
- Authentication: required (owner API key via `Authorization: Bearer <api_key>`, `X-API-Key`, or `X-Api-Key`)
- Signed-in identity: required (API key must resolve to an active authorized owner identity; browser session cookie is not required)
- What it does: Retrieve structured RAG chunks directly by chunk id or object reference.
- Input options:
  - `chunk_ids[]`
  - OR (`object_type` + `object_token`)
  - optional: `include_embeddings` (default `false`)
- Output:
  - `chunks[]` with canonical chunk JSON payload and object linkage metadata

### 30) DocsChat

- Tool name: `docs_chat`
- Route: `POST /api/agents/v1/docs/chat`
- Authentication: required (owner API key via `Authorization: Bearer <api_key>`, `X-API-Key`, or `X-Api-Key`)
- Signed-in identity: required (API key must resolve to an active authorized owner identity; browser session cookie is not required)
- What it does: Runs the same documentation Q&A backend used by the docs UI and returns a final answer.
- Input options:
  - `question` (required)
  - optional: `chat_history[]`, `section`, `context_mode` (`documentation` or `current_document`), `doc_path`, `research_mode`, `budget_policy`, `max_latency_ms`
- Output:
  - `answer_markdown`
  - `answer_html`
  - `meta` (router/research metadata)
- Agent guidance — mode selection and tips:
  - **`context_mode: "documentation"`** (default) — searches across all indexed docs to answer the question. Use this for general "how does X work?" questions.
  - **`context_mode: "current_document"`** — answers about a specific doc. Requires `doc_path` (e.g., `"product/architecture.md"`). Use when the user is reading a doc and asks a follow-up about it.
  - **`research_mode: true`** — enables deeper multi-step research for complex questions. Increases latency but improves answer quality. Use for "compare X and Y" or "what are all the ways we handle Z?" style questions.
  - **`budget_policy`** — controls how much compute to spend. Leave as default for most queries; set to a lower budget for quick lookups and higher for thorough research.
  - **`section`** — scope the search to a doc section/folder (e.g., `"hackathon"`, `"operations"`). Use when you know the answer lives in a specific area.
  - **`chat_history[]`** — pass up to 12 prior `{role, content}` messages for multi-turn conversations. Always include the most recent exchange for context continuity.

  | Intent | Recommended params |
  |--------|-------------------|
  | Quick factual lookup | `question: "What is the default Sidekiq queue?"` |
  | Deep research question | `question: "How does the RAG pipeline process new content?", research_mode: true` |
  | Question about a specific doc | `question: "What are the required fields?", context_mode: "current_document", doc_path: "product/job_ad_system.md"` |
  | Scoped to a topic area | `question: "How do reminder emails work?", section: "operations"` |
  | Follow-up in a conversation | `question: "What about error handling?", chat_history: [{role: "user", content: "How does email sending work?"}, {role: "assistant", content: "...previous answer..."}]` |

### 31) DocsFind

- Tool name: `docs_find`
- Route: `POST /api/agents/v1/docs/find`
- Authentication: required (owner API key via `Authorization: Bearer <api_key>`, `X-API-Key`, or `X-Api-Key`)
- Signed-in identity: required (API key must resolve to an active authorized owner identity; browser session cookie is not required)
- What it does: Searches internal docs and returns ranked matching documents.
- Input options:
  - `query` (required)
  - optional: `limit` (default `20`, max `20`), `path_prefix`
- Output:
  - `matches[]` with `doc_path`, `title`, `description`, `match_score`, `matched_by`
  - `truncated`

### 32) DocGet

- Tool name: `doc_get`
- Route: `POST /api/agents/v1/docs/get`
- Authentication: required (owner API key via `Authorization: Bearer <api_key>`, `X-API-Key`, or `X-Api-Key`)
- Signed-in identity: required (API key must resolve to an active authorized owner identity; browser session cookie is not required)
- What it does: Retrieves one documentation file by path (or inferred path) and returns full normalized text content plus the raw file `sha256` used by `docs_edit`.
- Input options:
  - `doc_path` (required): relative docs path or `/docs/...` URL
- Output:
  - `doc` with `doc_path`, `title`, `description`, `format`, `content_text`, `truncated`, `bytes`, `sha256`, `line_count`, `modified_at`, `source_url`

### 32a) DocsEdit

- Tool name: `docs_edit`
- Route: `POST /api/agents/v1/docs/edit`
- Authentication: required (owner API key via `Authorization: Bearer <api_key>`, `X-API-Key`, or `X-Api-Key`)
- Signed-in identity: required (API key must resolve to an active authorized owner identity; browser session cookie is not required)
- What it does: Applies deterministic literal text replacements to an API-published document owned by the caller. This does not edit repo-backed docs or docs published by another client.
- Reliability model:
  - Call `doc_get` first and pass the returned `doc.sha256` as `base_sha256`.
  - The server rejects stale edits with `edit_conflict` when the current file hash differs.
  - Each `find` is literal text, not regex.
  - Default `occurrence` is `exactly_one`; if the text appears more than once, the server rejects with `ambiguous_match` and returns match previews.
  - Use a 1-based integer occurrence only after reviewing previews, or use `all` when replacing every occurrence is intentional.
- Input options:
  - `doc_path` (required): logical published-doc path such as `aitfund/q2-2026-draft-v2.html`, `docs/aitfund/q2-2026-draft-v2.html`, or the caller's own `pub/{token}/...` path
  - `base_sha256` (required): raw SHA-256 from `doc_get`
  - optional: `dry_run` (default `false`)
  - `operations[]` (required, max `10`), each with:
    - `op`: `replace_text` (optional; default)
    - `find`: required non-empty literal text
    - `replace`: required literal replacement text; empty string deletes
    - `occurrence`: optional `exactly_one`, `all`, or a 1-based integer
- Output:
  - `doc` metadata for the published doc
  - `dry_run`, `changed`, `old_sha256`, `new_sha256`, `bytes_before`, `bytes_after`, `operations_applied`
  - `operations[]` with `match_count`, `applied_count`, and line/column match previews

Example:

```bash
./scripts/ait-api.sh /api/agents/v1/docs/edit '{
  "doc_path": "aitfund/q2-2026-draft-v2.html",
  "base_sha256": "b7f4...",
  "dry_run": true,
  "operations": [
    {
      "op": "replace_text",
      "find": "Working outline for the next LP memo",
      "replace": "Working draft of the May LP memo",
      "occurrence": "exactly_one"
    }
  ]
}'
```

### 32b) DocsCommentCreate

- Tool name: `docs_comment_create`
- Route: `POST /api/agents/v1/docs/comments/create`
- Authentication: required (owner API key via `Authorization: Bearer <api_key>`, `X-API-Key`, or `X-Api-Key`)
- Signed-in identity: required (API key must resolve to an active authorized owner identity; browser session cookie is not required)
- What it does: Creates a comment on a documentation file. Pass `parent_note_token` to reply to an existing comment thread.
- Access: resolves the document with the same docs API visibility rules, then checks the `DocFile` comment visibility for the API key owner.
- Input options:
  - `doc_path` (required)
  - `content` (required, max `2000`)
  - optional: `parent_note_token` to reply within an existing comment thread
  - optional: `anchor` for a top-level inline comment. Include at least `selected_text`; stored fields may also include `prefix`, `suffix`, offsets, and `anchor_version`.
- Output:
  - `doc` with `doc_path`, `comment_file_path`, `title`, `source_url`
  - `comment` with `token`, `content`, `parent_note_token`, `root_note_token`, `author`, timestamps, `refs`, `replies_count`, and inline `anchor` metadata when applicable
  - `count`

Example reply:

```bash
./scripts/ait-api.sh /api/agents/v1/docs/comments/create '{
  "doc_path": "aitfund/q2-2026-draft-v2.html",
  "parent_note_token": "cn_abc123",
  "content": "Addressed this in the portfolio section."
}'
```

### 32c) DocsCommentsList

- Tool name: `docs_comments_list`
- Route: `POST /api/agents/v1/docs/comments/list`
- Authentication: required (owner API key via `Authorization: Bearer <api_key>`, `X-API-Key`, or `X-Api-Key`)
- Signed-in identity: required (API key must resolve to an active authorized owner identity; browser session cookie is not required)
- What it does: Lists active comments on a documentation file, including comments on API-published docs that only live in server storage.
- Access: resolves the document with the same docs API visibility rules, then checks the `DocFile` comment visibility for the API key owner.
- Input options:
  - `doc_path` (required)
  - optional: `author_client_token` (or `client_token` alias) to restrict results to one commenter
  - optional: `limit` (default `100`, max `200`)
- Output:
  - `doc` with `doc_path`, `comment_file_path`, `title`, `source_url`
  - `comments[]` as a flat list of active comments and replies, each with `token`, `content`, `parent_note_token`, `root_note_token`, `author`, timestamps, `refs`, and `replies_count`
  - inline comments include `inline_comment: true` and an `anchor` object with `selected_text`, `anchor_digest`, `reattach_status`, `resolved`, and the stored anchor `payload`
  - `count`, `total_count`, `truncated`

### 32d) DocsCommentsSearch

- Tool name: `docs_comments_search`
- Route: `POST /api/agents/v1/docs/comments/search`
- Authentication: required (owner API key via `Authorization: Bearer <api_key>`, `X-API-Key`, or `X-Api-Key`)
- Signed-in identity: required (API key must resolve to an active authorized owner identity; browser session cookie is not required)
- What it does: Searches active comments on one documentation file by comment body or inline selected-text anchor.
- Input options:
  - `doc_path` (required)
  - `query` (required)
  - optional: `author_client_token` (or `client_token` alias)
  - optional: `limit` (default `100`, max `200`)
- Output:
  - same shape as `docs_comments_list`, with `query` and optional `author_client_token` echoed in `data`

### 32e) DocsCommentDelete

- Tool name: `docs_comment_delete`
- Route: `POST /api/agents/v1/docs/comments/delete`
- Authentication: required (owner API key via `Authorization: Bearer <api_key>`, `X-API-Key`, or `X-Api-Key`)
- Signed-in identity: required (API key must resolve to an active authorized owner identity; browser session cookie is not required)
- What it does: Soft-deletes a document comment authored by the API key owner.
- Input options:
  - `doc_path` (required)
  - `note_token` (required)
- Output:
  - `doc`
  - `note_token`
  - `deleted`
  - `had_replies`
  - `count`

### 32f) DocsCommentResolve

- Tool name: `docs_comment_resolve`
- Route: `POST /api/agents/v1/docs/comments/resolve`
- Authentication: required (owner API key via `Authorization: Bearer <api_key>`, `X-API-Key`, or `X-Api-Key`)
- Signed-in identity: required (API key must resolve to an active authorized owner identity; browser session cookie is not required)
- What it does: Resolves a document comment thread. If `note_token` is a reply, the root comment thread is resolved.
- Access: resolves the document with the same docs API visibility rules, then checks the `DocFile` comment visibility for the API key owner.
- Input options:
  - `doc_path` (required)
  - `note_token` (required)
- Output:
  - `doc`
  - `note_token`
  - `root_note_token`
  - `resolved`
  - `comment` with the resolved root comment payload, including `resolved_at` and `resolved_by`

Example:

```bash
./scripts/ait-api.sh /api/agents/v1/docs/comments/resolve '{
  "doc_path": "aitfund/q2-2026-draft-v2.html",
  "note_token": "cn_abc123"
}'
```

### 32g) DocsCommentReopen

- Tool name: `docs_comment_reopen`
- Route: `POST /api/agents/v1/docs/comments/reopen`
- Authentication: required (owner API key via `Authorization: Bearer <api_key>`, `X-API-Key`, or `X-Api-Key`)
- Signed-in identity: required (API key must resolve to an active authorized owner identity; browser session cookie is not required)
- What it does: Reopens a resolved document comment thread. If `note_token` is a reply, the root comment thread is reopened.
- Access: resolves the document with the same docs API visibility rules, then checks the `DocFile` comment visibility for the API key owner.
- Input options:
  - `doc_path` (required)
  - `note_token` (required)
- Output:
  - `doc`
  - `note_token`
  - `root_note_token`
  - `resolved`
  - `comment` with the reopened root comment payload

### 33) ContentBrandScrubAnalyze

- Tool name: `content_brand_scrub_analyze`
- Route: `POST /api/agents/v1/content/brand_scrub/analyze`
- Authentication: required (owner API key via `Authorization: Bearer <api_key>`, `X-API-Key`, or `X-Api-Key`)
- Signed-in identity: required (API key must resolve to an active authorized owner identity; browser session cookie is not required)
- What it does: Analyze content against brand rules and return edits/suggestions.
- Input options:
  - `text` (required)
  - optional: `channel`, `tone`, `target_audience`
- Output:
  - rule hits
  - rewritten suggestions
  - risk flags and confidence

### 34) RestrictedContentBrandScrubAnalyze

- Tool name: `restricted_content_brand_scrub_analyze`
- Route: `POST /api/agents/v1/restricted_content/brand_scrub/analyze`
- Authentication: required (owner API key via `Authorization: Bearer <api_key>`, `X-API-Key`, or `X-Api-Key`)
- Signed-in identity: required (API key must resolve to an active authorized owner identity; browser session cookie is not required)
- What it does: Analyze restricted/high-sensitivity content using stricter policy heuristics.
- Input options:
  - `text` (required)
  - optional: `channel`, `audience`, `stage`
- Output:
  - restricted-policy rule hits
  - rewritten suggestions
  - risk flags and confidence

### 35) WeblogList

- Tool name: `weblog_list`
- Route: `POST /api/agents/v1/weblogs/list`
- Authentication: required (owner API key)
- What it does: List weblogs (chapters) by creation date. Useful for finding recently provisioned chapters, counting chapters by region, or getting a full network roster.
- Input options:
  - `date_from` (YYYY-MM-DD)
  - `date_to` (YYYY-MM-DD)
  - `status` (`provisioned` or `launched`)
  - `region` (e.g. `Africa`, `Europe`, `Latin America`, `Asia`, `North America`, `Middle East`, `Oceania`) — filters to chapters in the given region
  - `limit`
- Output:
  - `weblogs[]` with `weblog_token`, `city`, `region`, `country`, `timezone`, `weblog_name`, `one_line_description`, `created_at`, `status`, `subscriber_count`, `domain`, `organizer` (name/email/title/company)

### 36) MeetupTimeSeriesGet

- Tool name: `meetup_time_series_get`
- Route: `POST /api/agents/v1/meetups/time_series`
- Authentication: required (owner API key)
- What it does: Return a time series of meetup counts and attendance for a specific weblog.
- Input options:
  - `weblog_token` (required)
  - `date_from`, `date_to`, `bucket` (`day`/`week`/`month`)
- Output:
  - `series[]` with `bucket_start`, `bucket_end`, `label`, `meetup_count`, `attending_count` per bucket
  - `summary` with `total_meetups` and `total_attending`

### 37) GlobalHackathonList

- Tool name: `global_hackathon_list`
- Route: `POST /api/agents/v1/global_hackathons/list`
- Authentication: required (owner API key)
- What it does: List global hackathon series and, by default, include per-city hackathon completeness rows suitable for questions like “which cities are missing location information?”
- Input options:
  - `query` (optional)
  - `location_status` (optional; one of `all`, `missing_address`, `missing_any`, `complete`)
  - `include_hackathons` (optional boolean, default `true`)
  - `limit`
- Output:
  - `global_hackathons[]`
    - `summary`: `hackathon_slug`, `name`, `description`, `creator`, `city_count`, `starts_at_first`, `starts_at_last`, `stats`, and missing-count summaries
    - `hackathons[]` (when `include_hackathons=true`): city-level rows with:
      - meetup metadata (`city_name`, `city_slug`, `meetup_token`, `event_name`, `starts_at`, `event_url`)
      - hackathon metadata (`hackathon_token`, `hackathon_portal_url`)
      - `hackathon_fields` (all Hackathon model columns)
      - `hackathon_missing_flags` (per-column booleans)
      - `team_role_status` (`judges`, `organizers`, `volunteers`, `mentors`, `sponsors`, `participant teams` counts + missing booleans)
      - `location_audit` (`is_virtual`, `meeting_url`, inferred address data, missing-location booleans/reasons)
      - `essential_missing_flags` + flattened `missing_flags[]`

### 38) GlobalHackathonCitiesList

- Tool name: `global_hackathon_cities_list`
- Route: `POST /api/agents/v1/global_hackathons/cities`
- Authentication: required (owner API key)
- What it does: Return participating city hackathons for one global series with detailed completeness flags, including physical address/location auditing.
- Input options:
  - `hackathon_slug` (required)
  - `location_status` (optional; one of `all`, `missing_address`, `missing_any`, `complete`)
  - `query` (optional city/title filter)
  - `limit`
- Output:
  - `global_hackathon` summary (`hackathon_slug`, `name`, `description`, creator/stats/date-range metadata)
  - `cities[]` with the same city-level structure as `global_hackathon_list.hackathons[]`
  - `filters` and `truncated`

### 39) GlobalHackathonCityAttach

- Tool name: `global_hackathon_city_attach`
- Route: `POST /api/agents/v1/global_hackathons/cities/attach`
- Authentication: required (owner API key with hackathon API write access)
- What it does: Attach an existing hackathon record to an existing global hackathon series by setting the hackathon's `global_hackathon_id`.
- Authorization: requires global management access for the target series. Read-only users, city-only organizers, and sponsor-only users cannot call it.
- Input options:
  - `hackathon_slug` (required): target global hackathon slug.
  - `hackathon_token` or `meetup_token` (one required): existing hackathon/event to attach.
  - `dry_run` (optional boolean, default `false`): preview without saving.
  - `allow_reassign` (optional boolean, default `false`): required when moving a hackathon from another global hackathon.
  - `sync_boards` (optional boolean, default `true`): sync global organizer boards after saving.
- Output:
  - `dry_run`, `changed`, and `action` (`would_attach`, `attached`, or `already_attached`)
  - target `global_hackathon` summary
  - optional `previous_global_hackathon`
  - attached `city` row in the same shape returned by `global_hackathon_cities_list`
  - `refs` with `hackathon_slug`, `meetup_token`, `weblog_token`, `content_page_token`, and `hackathon_token`
- Reassignment safety: if the hackathon is already attached to a different global hackathon and `allow_reassign` is not true, the endpoint returns `409 conflict` with `error.code=global_hackathon_reassignment_requires_confirmation`.

### Hackathon Sponsor Logo Endpoints

#### `hackathon_sponsors_get`

- Route: `GET|POST /api/agents/v1/hackathons/sponsors`
- Authentication: required (owner API key with hackathon API read access)
- What it does: Return detected hackathon sponsors, venue sponsor, selected logo and click URL for each sponsor, inferred/configured logo and click URLs, and whether the caller can update sponsor logo selections.
- Input options:
  - `hackathon_token` or `meetup_token` (one required)
- Output:
  - `hackathon` and `meetup` summary refs
  - `sponsors[]` with `label`, `selected_logo_url`, `logo_url`, `selected_click_url`, `click_url`, `tracked_click_url`, `configured_logo_url`, `configured_click_url`, `inferred_logo_url`, `inferred_click_url`, `selected_source`, and `configured`
  - `marquee_sponsors[]`, `secondary_sponsors[]`, optional `venue`
  - `logo_config` metadata and `can_write`

#### `hackathon_sponsor_logo_update`

- Route: `POST /api/agents/v1/hackathons/sponsors/logo`
- Authentication: required (owner API key). The endpoint also enforces per-hackathon write authorization: admin, AI Tinkerers index owner, hackathon manager, or owning chapter weblog owner.
- What it does: Change the selected logo URL and/or sponsor click URL for one sponsor on one hackathon. The selected sponsor logo config is stored on the hackathon and returned by `hackathon_sponsors_get`.
- Input options:
  - `hackathon_token` or `meetup_token` (one required)
  - `sponsor_label` (required; `label` alias accepted by REST)
  - `logo_url` (`http` or `https`; optional when updating an existing sponsor row)
  - `click_url` (`http` or `https` when present; optional; pass an empty string to clear an existing click URL)
  - At least one of `logo_url` or `click_url` is required.
- Output:
  - Same payload as `hackathon_sponsors_get`, after the update.

### 40) EmailCampaignPerformanceGet

- Tool name: `email_campaign_performance_get`
- Route: `POST /api/agents/v1/analytics/email/campaign_performance`
- Authentication: required (owner API key)
- What it does: Return campaign-level email performance metrics for scoped sends, with optional dashboard trend lines/summary blocks and paginated/sortable campaign leaderboard rows.
- Input options:
  - scope selectors: `weblog_token`, `weblog_tokens[]`, `meetup_token`, `content_page_token`, `series_token`
  - optional filters: `date_from`, `date_to`, `campaign_type` (`meetup`, `content_page`, `newsletter`, `all`), `group_by` (`campaign`, `day`, `week`)
  - campaign row controls: `limit`, `offset`, `sort`, `sort_dir`, `min_sends`
  - payload controls: `include_campaigns`, `include_trends`, `include_summary`
- Output:
  - `campaigns[]` with `campaign_key`, `campaign_label`, `sent_at`, `sends`, `delivered`, `delivery_rate`, `opens`, `open_rate`, `clicks`, `click_rate`, `bounces`, `bounce_rate`, `unsubscribes`, `unsubscribe_rate`
  - `campaign_pagination` with `offset`, `limit`, `total_count`, `returned_count`, `has_more`, `sort`, `sort_dir`, `min_sends`
  - `trends[]` with per-bucket counts/rates for send, delivery, open, click, bounce, unsubscribe
  - `summary` totals and weighted rates
  - `meta` with matched mail-log count and applied selectors
- Agent guidance — common recipes:

  | Intent | Recommended params |
  |--------|-------------------|
  | How did our last newsletter perform? | `campaign_type: "newsletter", weblog_token: "<token>", sort: "sent_at", sort_dir: "desc", limit: 1` |
  | Event email performance for a meetup | `meetup_token: "<token>", campaign_type: "meetup"` |
  | Series-only leaderboard | `weblog_token: "<token>", series_token: "<series_token>", include_trends: false, sort: "open_rate_pct", sort_dir: "desc"` |
  | Weekly send trends for a dashboard | `weblog_token: "<token>", group_by: "week", include_campaigns: false` |
  | Open-rate leaderboard for large sends | `weblog_token: "<token>", include_trends: false, sort: "open_rate_pct", sort_dir: "desc", min_sends: 10000` |
  | Paginate through all campaigns | `weblog_token: "<token>", include_trends: false, limit: 100, offset: 100` |
  | Performance for a specific content page | `content_page_token: "<token>"` |

  - Use `include_campaigns: false` when you only need chart data for a dashboard and want to keep payloads small.
  - Use `include_trends: false` for leaderboard/analytics queries so the response budget goes to campaign rows instead of time buckets.
  - Campaign row mode (`include_campaigns=true`) enforces narrower date windows for broader selectors to keep retrieval fast: `365` days for `content_page_token`/`meetup_token`, `180` days for `series_token`, and `31` days for weblog-only queries.
  - The `summary` field gives overall weighted averages — use it for quick "how are we doing?" answers without parsing individual campaigns.
  - The `trends[]` array is best for charting or spotting patterns (open rate declining, bounce rate spiking, etc.).
  - Chain with `email_deliverability_health_get` if bounce/complaint rates look high, or `email_fatigue_risk_get` if unsubscribe rates are climbing.

### 40) EmailDeliverabilityHealthGet

- Tool name: `email_deliverability_health_get`
- Route: `POST /api/agents/v1/analytics/email/deliverability_health`
- Authentication: required (owner API key)
- What it does: Surface deliverability risk by detecting bounce/complaint spikes, sender-domain issues, and risky audience slices.
- Input options:
  - scope selectors: `weblog_token` OR `city`
  - optional filters: `date_from`, `date_to`, `sender_domain`, `from_email`, `campaign_type`, `include_segments`
- Output:
  - `health_score` (0-100)
  - `alerts[]` with `severity`, `code`, `message`, `metric`, `current_value`, `baseline_value`, `delta_pct`, `window`
  - `sender_domains[]` with `domain`, `sent`, `delivered`, `bounce_rate`, `complaint_rate`, `unsubscribe_rate`, `status`
  - `risky_segments[]` with `segment_key`, `segment_label`, `sent`, `bounce_rate`, `complaint_rate`, `unsubscribe_rate`, `risk_score`, `reasons[]`

### 41) EmailFatigueRiskGet

- Tool name: `email_fatigue_risk_get`
- Route: `POST /api/agents/v1/analytics/email/fatigue_risk`
- Authentication: required (owner API key)
- What it does: Score subscriber-level email fatigue risk from send cadence and engagement decline.
- Input options:
  - scope selectors: `weblog_token` OR `city`
  - optional filters: `date_from`, `date_to`, `tag`, `subscriber_status`, `min_sent_last_30d`, `limit`
- Output:
  - `subscribers[]` with `subscriber_token`, `email`, `name`, `fatigue_score` (0-100), `fatigue_tier` (`low`, `medium`, `high`, `critical`), `drivers`, `recommended_action`
  - `summary` with counts by fatigue tier, average fatigue score, and evaluated totals
  - `truncated`

### 42) NewsletterSpotlightCandidatesGet

- Tool name: `newsletter_spotlight_candidates_get`
- Route: `POST /api/agents/v1/recommendations/newsletter/spotlights`
- Authentication: required (owner API key)
- What it does: Rank likely strong newsletter spotlight candidates with supporting evidence.
- Input options:
  - scope selectors: `weblog_token` (optional), `city` (optional), `series_token` (optional)
  - optional filters: `date_from`, `date_to`, `topic`, `candidate_types[]` (`demo`, `member`), `limit`, `include_evidence`
- Output:
  - `candidates[]` with `candidate_type`, `client_token`, optional `rsvp_token`, `meetup_token`, `weblog_token`, `content_page_token`, `name`, `city`, `score`, `evidence[]`, `relevance_tags[]`, `suggested_angle`, `source_refs[]`, and `refs`
  - member candidates also include `sample_rsvp_token`, `sample_meetup_token`, `sample_weblog_token`, and `sample_content_page_token`
  - `summary` with evaluated pool counts and score distribution

### 43) SpeakerPipelineCandidatesGet

- Tool name: `speaker_pipeline_candidates_get`
- Route: `POST /api/agents/v1/recommendations/speakers/pipeline`
- Authentication: required (owner API key)
- What it does: Return ranked future-speaker candidates from RSVP/talk/engagement history with city-aware targeting.
- Input options:
  - city targeting: `city_names[]` (explicit multi-city input; max `50`)
  - optional scope selectors: `weblog_token`, `series_token`, `region`
  - optional filters: `date_from`, `date_to`, `topics[]`, `experience_level`, `min_prior_talks`, `limit`
- Output:
  - `candidates[]` with `client_token`, `sample_rsvp_token`, `sample_meetup_token`, `sample_weblog_token`, `sample_content_page_token`, `name`, `email`, `home_city`, `matched_cities[]`, `speaker_fit_score`, `talk_history_summary`, `engagement_signals`, `recommended_topic_angles[]`, `why_now[]`, and `refs`
  - `filters` and `truncated`

### 44) PhotoSearch

- Tool name: `photo_search`
- Route: `POST /api/agents/v1/photos/search`
- Authentication: required (owner API key via `Authorization: Bearer <api_key>`, `X-API-Key`, or `X-Api-Key`)
- Signed-in identity: required (API key must resolve to an active authorized owner identity; browser session cookie is not required)
- What it does: Search meetup-associated photos with full metadata filters and return rich per-photo metadata payloads.
- Input options:
  - selectors: `meetup_token` or `event_key` (aliases), optional `uploader_client_token`
  - scope controls: `scope` (`all` default, `mine` for organizer-owned city/series scope), optional aliases `my_photos` / `mine_only`
  - text/location filters: `query`, `city`, `region`
  - recency/date filters: `days_back`, `date_from`, `date_to`
  - metadata filters: `women_facing_forward`, `people_facing_forward`, `women_present`, `people_present`, `has_caption`, `has_text`, `is_high_quality`, `is_beautiful`, `blurred`, `with_analysis`, `uploaded_by_people`, `exclude_text_overlays` (default true), `scene_tags_any[]`, `scene_tags_all[]`, `image_type`
  - result controls: `sort` (`recent`, `oldest`, `quality`, `women_facing_forward`), `limit`
- Output:
  - `photos[]` each with:
    - identity: `photo_token`, `photo_id`, `event_key`, `meetup_token`
    - `urls` (`imgix_url`, `s3_url`, `original_url`) — **`imgix_url` is a fully hosted, publicly accessible image URL.** Use it directly in `<img>` tags, markdown images, or any context that accepts a URL. Do NOT download, re-upload, or proxy these URLs; they are served via a global CDN and are ready to use as-is.
    - `dimensions` (`width`, `height`, `aspect_ratio`)
    - `uploader` (client payload)
    - `meetup` (meetup payload)
    - `weblog` (`weblog_token`, `city`, `name`)
    - `metadata` dump including caption, scene tags/activity, people/women/facing-forward flags, quality flags, counters, timestamps, and optional `image_analysis_json`
  - `filters`
  - `truncated`
- Defaults and behavior notes:
  - `exclude_text_overlays` defaults to `true` — photos with significant overlaid text (banners, promotional graphics misclassified as photos) are filtered out. Set to `false` to include them.
  - `uploaded_by_people` distinguishes attendee uploads from organizer uploads. When `true`, photos uploaded by blog owners/authors on the associated weblog are excluded, returning only attendee-contributed photos.
- Agent guidance — common recipes:

  | Intent | Recommended params |
  |--------|-------------------|
  | Best photos from a city's recent events | `city: "<city>", days_back: 30, is_beautiful: true, sort: "quality", limit: 10` |
  | Event recap gallery | `meetup_token: "<token>", sort: "recent", is_high_quality: true` |
  | Diverse community photos with women | `women_facing_forward: true, is_high_quality: true, sort: "women_facing_forward", limit: 20` |
  | Presentation or stage shots | `scene_tags_any: ["presentation", "stage"], people_facing_forward: true` |
  | Social media post image | `is_beautiful: true, is_high_quality: true, sort: "quality", limit: 5` |
  | Attendee-uploaded photos (not organizer) | `uploaded_by_people: true, sort: "recent"` |
  | Cross-city crowd and networking shots | `people_present: true, is_high_quality: true, scene_tags_any: ["crowd", "audience", "networking"], sort: "quality"` |
  | Search photos by concept or keyword | `query: "rooftop networking", is_high_quality: true, sort: "quality"` |

### 45) LogoSearch

- Tool name: `logo_search`
- Route: `POST /api/agents/v1/logos/search`
- Authentication: required (owner API key via `Authorization: Bearer <api_key>`, `X-API-Key`, or `X-Api-Key`)
- Signed-in identity: required (API key must resolve to an active authorized owner identity; browser session cookie is not required)
- What it does: Search the logo database using semantic matching (RAG) and return logo assets with rich AI-generated metadata so the agent can select the perfect variant for its visual context.
- Input options:
  - `query` (text, required)
  - optional filters: `scope` (`smart_match` default, or `library`), `include_co_branded` (boolean, default false), `limit` (default 20)
- Output:
  - `matches[]` where each match includes:
    - `id`
    - `token`
    - `text_content` (OCR text, e.g., "Anthropic")
    - `caption`
    - `imgix_url` (Raw image URL — **fully hosted, publicly accessible, CDN-served.** Use directly in `<img>` tags, markdown, or anywhere a URL is accepted. Do NOT download, re-upload, or proxy these URLs.)
    - `padded_imgix_url` (URL with automatic transparent padding applied — same hosting; use directly)
    - `thumbnail_light_url` (same hosting; use directly)
    - `thumbnail_dark_url` (same hosting; use directly)
    - `dimensions`: `{ width, height }`
    - `metadata`: `{ brand_name, description, design_style, white_background_fit, is_on_dark_background, primary_colors, image_type, is_co_branded }`
    - `markdown_snippet`
    - `padded_markdown_snippet`
  - `needs_disambiguation`
  - `search_options`: `{ scope, include_co_branded, limit }`
- Agent guidance — usage tips:
  - Use `imgix_url` for raw/uncropped placement; use `padded_imgix_url` when placing logos on colored or busy backgrounds where the logo needs breathing room.
  - Use `thumbnail_dark_url` when your page/slide has a dark background; use `thumbnail_light_url` for light backgrounds.
  - Check `metadata.is_on_dark_background` and `metadata.primary_colors` to avoid color clashes with your layout.
  - Use `markdown_snippet` or `padded_markdown_snippet` for quick insertion into markdown content (newsletters, docs, chat responses).
  - Set `include_co_branded: true` when looking for partnership or co-branded logo variants.
  - Use `scope: "library"` to browse the full logo collection; use `scope: "smart_match"` (default) for best semantic relevance.
- Agent guidance — common recipes:

  | Intent | Recommended params | Post-processing tip |
  |--------|-------------------|---------------------|
  | Find Anthropic logo for a sponsor slide | `query: "Anthropic", limit: 5` | Use `padded_imgix_url` for slide placement; check `primary_colors` to match theme |
  | NVIDIA logo for a dark mode site | `query: "NVIDIA", limit: 5` | Filter results using `metadata.is_on_dark_background` or use `thumbnail_dark_url` |
  | Google Gemini logo | `query: "Google Gemini", limit: 5` | Use `thumbnail_light_url` or `thumbnail_dark_url` based on background |
  | OpenAI logo for a light background | `query: "OpenAI", limit: 10` | Filter results client-side using `metadata.is_on_dark_background: false`; use `thumbnail_light_url` |

### 46) TechnologyList

- Tool name: `technology_list`
- Route: `POST /api/agents/v1/technologies/list`
- Authentication: required (owner API key)
- What it does: List technologies with currently indexed public demos/projects.
- Input options:
  - optional filters: `query`, `limit`, `offset`
- Output:
  - `technologies[]` with `technology_slug`, `technology_name`, `canonical_url`, `project_count`
  - `truncated`
  - `next_offset`

### 47) TechnologyProjectsList

- Tool name: `technology_projects_list`
- Route: `POST /api/agents/v1/technologies/projects`
- Authentication: required (owner API key)
- What it does: Return public-safe demo project rows for a single technology.
- Input options:
  - one selector: `technology_slug` or `technology_name` (also accepts `technology`)
  - optional filters: `city`, `date_from`, `date_to`, `demo_type` (`main_stage`/`science_fair`), `limit`, `offset`
- Output:
  - `technology` identity payload
  - `projects[]` with project/demo title, description, links, technology labels, event metadata, and stage/science-fair flags
  - `truncated`
  - `next_offset`

### 48) JobSearch

- Tool name: `job_search`
- Route: `POST /api/agents/v1/jobs/search`
- Authentication: required (owner API key)
- What it does: Search paid job ads with status/sort/filter controls.
- Access policy: index-owner roles only.
- Input options:
  - optional filters: `query`, `city`, `company`, `status` (`active`, `archived`, `all`), `sort`, `limit`
- Output:
  - `results[]` with ad summary fields (`ad_token`, title/company/location/status, payment/publication metadata, and `raw_attributes`)
  - `count`
  - `truncated`
  - `filters`

### 49) JobAdDataGet

- Tool name: `job_ad_data_get`
- Route: `POST /api/agents/v1/jobs/ad_data`
- Authentication: required (owner API key)
- What it does: Return full ad detail plus current performance metrics and click-people analytics.
- Access policy: index-owner roles only.
- Input options:
  - `ad_token` (required)
  - optional: `click_people_limit` (max 1000)
- Output:
  - ad detail payload (`ad`, `ad_token`)
  - `current_stats`
  - `current_stats_graphic` (`rendered_image_url`, `render_stats_path`)
  - `performance_interpretation_plain_english`
  - `click_people[]` with client/context/geo metadata

### 50) EmailSendJobsSummary

- Tool name: `email_send_jobs_summary`
- Route: `POST /api/agents/v1/email_send_jobs/summary`
- Authentication: required (owner API key)
- What it does: Return aggregate email send-job delivery counts for a caller-visible meetup/event or content page.
- Access policy: city managers/owners and index owners see only jobs in their visible weblog scope.
- Input options:
  - one of `meetup_token` or `content_page_token` required
  - optional: `date_from`, `date_to` (max 365-day window), `limit` (default 10, max 50)
- Output:
  - `target` event/page/weblog identifiers
  - `summary` with `send_jobs_count`, `sent_count`, `intended_recipient_count`, `pending_count`, `suppressed_count`, `pre_send_excluded_count`, `status_counts`, `first_sent_at`, `last_sent_at`
  - `answer` concise natural-language sentence suitable for Ashley replies
  - recent `send_jobs[]` rows using the normal send-job summary shape

### 51) EmailSendJobsList

- Tool name: `email_send_jobs_list`
- Route: `POST /api/agents/v1/email_send_jobs/list`
- Authentication: required (owner API key)
- What it does: List email send jobs with filtering by status, date range, and content page.
- Input options:
  - optional: `status` (`queued`, `sending`, `completed`, `failed`, `active`, `all`), `sort` (`created_at`, `started_at`, `finished_at`), `sort_dir` (`asc`, `desc`), `from_date`, `to_date`, `days_back` (max 365), `content_page_token`, `limit` (default 25, max 100)
- Output:
  - `send_jobs[]` with `token`, `subject`, `status`, `distribution_option`, delivery counts (`sent_count`, `pending_count`, `suppressed_count`), `delivered_percent`, `done`, content page and weblog refs, timestamps
  - active jobs include `observed_send_rate_per_minute`, `predicted_finish_at`
  - `total_count`, `truncated`

### 52) EmailSendJobGet

- Tool name: `email_send_job_get`
- Route: `POST /api/agents/v1/email_send_jobs/get`
- Authentication: required (owner API key)
- What it does: Get detailed information about a single email send job including audience summary, send progress, suppression breakdown, and drip wave count.
- Input options:
  - `token` (required)
- Output:
  - full `send_job` payload with `audience_summary`, `suppressed_reason_counts`, `send_progress` (rates, predicted finish window, batch info), `recipient_pipeline`, `drip_wave_count` (number of times the job was reopened for new subscribers)

### 53) EmailSendJobRecipientsList

- Tool name: `email_send_job_recipients_list`
- Route: `POST /api/agents/v1/email_send_jobs/recipients/list`
- Authentication: required (owner API key)
- What it does: Paginated list of recipients for a send job with status filtering.
- Input options:
  - `token` (required)
  - optional: `status` (`pending`, `sent`, `failed`, `all`), `limit` (default 50, max 200), `offset` (max 10000)
- Output:
  - `recipients[]` with name, email (follows masking policy), status, timestamps
  - `total_count`, `truncated`, `pipeline` (`recipient_table` or `legacy`)

### 54) EmailSendJobThroughputGet

- Tool name: `email_send_job_throughput_get`
- Route: `POST /api/agents/v1/email_send_jobs/throughput`
- Authentication: required (owner API key)
- What it does: Get time-series throughput data for a send job with configurable bucketing.
- Input options:
  - `token` (required)
  - optional: `bucket` (`minute`, `5min`, `hour`)
- Output:
  - `throughput[]` with `bucket_start`, `sent_count`
  - `peak_rate_per_minute`, `average_rate_per_minute`, `total_sent`

### 55) EmailSendJobsCompare

- Tool name: `email_send_jobs_compare`
- Route: `POST /api/agents/v1/email_send_jobs/compare`
- Authentication: required (owner API key)
- What it does: Compare up to 10 send jobs side-by-side with delivery and engagement metrics.
- Input options:
  - `tokens[]` (required, max 10)
- Output:
  - `comparisons[]` with `token`, `subject`, delivery counts, `open_rate_pct`, `click_rate_pct`, `bounce_rate_pct`, `duration_seconds`, `average_send_rate_per_minute`

### 56) EmailSendJobSesStatusGet

- Tool name: `email_send_job_ses_status_get`
- Route: `POST /api/agents/v1/email_send_jobs/ses_status`
- Authentication: required (owner API key)
- What it does: Get current SES quota status, active/stuck send jobs, and system alerts.
- Access policy: index owners only.
- Input options: none
- Output:
  - `ses_quota` with rate limits, 24-hour send cap, utilization
  - `active_send_jobs[]` with observed rates and pending counts
  - `stuck_jobs[]` with staleness info
  - `recent_failure_count`
  - `alerts[]` with `severity`, `code`, `message`

### 56) DocsPublish

- Tool name: `docs_publish`
- Route: `POST /api/agents/v1/docs/publish`
- Authentication: required (owner API key)
- What it does: Publish or update a markdown/HTML document in the caller's published docs storage. Creates or updates the corresponding `PublishedDoc` row, which carries the visibility tier and city scope.
- Input options:
  - `path` (required, max 200 chars; `.md` extension added if missing)
  - `content` (required, max 2 MB)
  - `visibility` (optional): one of `"private"`, `"index_owner"` (default), `"city_owner"`, `"public"`, `"members"`, or a city tier expressed as `"city:<slug>"` or an array of `"city:<slug>"` entries. `city_organizer` is accepted as an alias for `city_owner`. Omitting on a re-publish preserves the existing tier.
  - `cities` (optional): array of city slugs, used in combination with `visibility="city"`.
  - `members` (optional): array of email addresses to add as document members. Stub Clients are created for unrecognized emails.
  - `suppress_member_notifications` (optional boolean, default `false`): when true, newly created memberships do not trigger "you've been added to a document" emails.
- Output:
  - `doc` with `path`, `url`, `pub_token`, `visibility`, `cities`, `members_count`, `bytes`, `content_type`, `created_at`, `updated_at`
- Error codes: `missing_content`, `content_too_large`, `invalid_path`, `invalid_request` (unknown visibility or city slug)
- See `docs/features/doc-publish.html` for the full visibility model. Strict private: `private`-tier docs are hidden from admins and AI Tinkerers index owners; only the publishing client sees them in search, in the docs index, and in agent context (`docs_chat`/`docs_find`/`doc_get`).

### 57) DocsUnpublish

- Tool name: `docs_unpublish`
- Route: `POST /api/agents/v1/docs/unpublish`
- Authentication: required (owner API key)
- What it does: Delete a published document from the caller's storage. Cleans up empty parent directories. Destroys the `PublishedDoc` row and any `DocMembership` rows that reference the doc.
- Input options:
  - `path` (required, max 200 chars)
- Output:
  - `path`, `deleted: true`

### 58) DocsPublishedList

- Tool name: `docs_published_list`
- Route: `POST /api/agents/v1/docs/published/list`
- Authentication: required (owner API key)
- What it does: List all published documents for the authenticated API client, sorted by `last_published_at` descending. Returns tier metadata per row.
- Input options: none
- Output:
  - `docs[]` with `path`, `url`, `pub_token`, `visibility`, `cities`, `members_count`, `bytes`, `content_type`, `created_at`, `updated_at`
  - `total`, `pub_token`

### 58a-pre) DocsPublishedGet

- Tool name: `docs_published_get`
- Route: `POST /api/agents/v1/docs/published/get`
- Authentication: required (owner API key)
- What it does: Read the current visibility tier, cities array, and full members list for a single doc the caller has published. Use this to inquire about a doc's auth state without listing your full library.
- Authorization: restricted to the publishing client (the doc's `client_id == @api_client.id`). Returns 404 for any path the caller did not publish, even if they have read access to it via membership or city tier — the inquiry surface is symmetric with the setter (`docs_visibility`).
- Input options:
  - `path` (required, max 200 chars; `.md` extension added if missing)
- Output:
  - `doc` with the same shape as `docs_publish`/`docs_visibility` plus a full `members` array. Each member entry includes `email`, `client_token`, `display_name`, `invited_by_token`, `created_at`.

### 58a) DocsVisibility

- Tool name: `docs_visibility`
- Route: `POST /api/agents/v1/docs/visibility`
- Authentication: required (owner API key)
- What it does: Change the visibility tier (and optional cities) of a doc the caller has already published, without re-uploading content. Returns 404 when the caller is not the publisher.
- Input options:
  - `path` (required, max 200 chars)
  - `visibility` (required): same shape as `docs_publish`
  - `cities` (optional): array of city slugs for `visibility="city"`
- Output:
  - `doc` (same shape as `docs_publish` response)

### 58b) DocsMembersAdd

- Tool name: `docs_members_add`
- Route: `POST /api/agents/v1/docs/members/add`
- Authentication: required (owner API key)
- What it does: Add document members by email. Members get read access regardless of base visibility tier (additive). Stub Clients are created for unrecognized emails, mirroring the `add_doc_member` UI flow. `DocMailer.member_added` is dispatched for new memberships unless `suppress_member_notifications` is true.
- Input options:
  - `path` (required, max 200 chars)
  - `emails` (required): array of email addresses, max 50 entries
  - `suppress_member_notifications` (optional boolean, default `false`): when true, newly created memberships do not trigger "you've been added to a document" emails.
- Output:
  - `path`, `members[]` (each with `email`, `client_token`, `added` flag), `members_count`

### 58c) DocsMembersRemove

- Tool name: `docs_members_remove`
- Route: `POST /api/agents/v1/docs/members/remove`
- Authentication: required (owner API key)
- What it does: Remove document members by email. Returns 404 if the caller is not the publisher; silently no-ops on emails that don't have a current membership.
- Input options:
  - `path` (required, max 200 chars)
  - `emails` (required): array of email addresses, max 50 entries
- Output:
  - `path`, `removed_emails[]`, `members_count`

### 59) SocialPostGenerate

- Tool name: `social_post_generate`
- Route: `POST /api/agents/v1/social_posts/generate`
- Authentication: required (owner API key)
- What it does: Generate social media post drafts from various source types (meetup, rsvp, content page, client, or sponsor).
- Input options:
  - `source_type` (required; one of: `meetup`, `rsvp`, `content_page`, `client`, `sponsor`)
  - `source_ref` (required; token or identifier for the source)
  - optional: `platform` (default `linkedin`), `goal` (default `promote`), `tone`, `city`
- Output:
  - `source` context
  - `artifact` (generated post content)
  - `draft_only: true`, `generated_at`

### 60) EventPromoGenerate

- Tool name: `event_promo_generate`
- Route: `POST /api/agents/v1/event_promos/generate`
- Authentication: required (owner API key)
- What it does: Generate an event promotion package for a meetup.
- Input options:
  - `meetup_token` (required)
  - optional: `package_type` (default `full_campaign`), `audience` (default `general`)
- Output:
  - `meetup` data
  - `artifact` (promotional content)
  - `draft_only: true`, `generated_at`

### 61) DiscussionTopicsGenerate

- Tool name: `discussion_topics_generate`
- Route: `POST /api/agents/v1/meetups/discussion_topics/generate`
- Authentication: required (owner API key)
- What it does: Generate AI discussion topics for a meetup based on its content and demos.
- Input options:
  - `meetup_token` (required)
- Output:
  - `meetup` data
  - `discussion_topics[]`

### 68) MediaFileSearch

- Tool name: `media_file_search`
- Route: `POST /api/agents/v1/media/files/search`
- Authentication: required (owner API key)
- What it does: Search media files across accessible weblogs by filename, folder name, uploader, or notes.
- Access policy: index owners and `index_video_editor` only.
- Input options:
  - `query` (required, max 200 chars)
  - optional: `content_type` (`video`, `audio`, `image`, `document`, `text`), `has_transcript` (boolean), `folder_token`, `weblog_token`, `limit` (default 20, max 100)
- Output:
  - `query`
  - `total_results`
  - `files[]` with full serialized media upload metadata (token, filename, content_type, size, type flags, status fields, folder/uploader info, refs)

### 69) MediaTranscriptSearch

- Tool name: `media_transcript_search`
- Route: `POST /api/agents/v1/media/transcripts/search`
- Authentication: required (owner API key)
- What it does: Full-text search across transcript content in accessible weblogs.
- Access policy: index owners and `index_video_editor` only.
- Input options:
  - `query` (required, max 500 chars)
  - optional: `content_type` (`video` or `audio`), `folder_token`, `weblog_token`, `limit` (default 20, max 100)
- Output:
  - `query`
  - `total_results`
  - `files[]` with serialized media upload metadata plus `transcript_snippet` (~200 char context window around match)

### 70) MediaFileScaleDown

- Tool name: `media_file_scale_down`
- Route: `POST /api/agents/v1/media/files/scale_down`
- Authentication: required (owner API key)
- What it does: Initiate async scaling down of a video file to reduce file size/resolution.
- Access policy: index owners and `index_video_editor` only.
- Input options:
  - `file_token` (required)
- Output:
  - `file_token`, `filename`, `scale_down_status: "processing"`
- Validation: file must be a video, cannot already be processing

### 71) MediaFileScaleDownStatus

- Tool name: `media_file_scale_down_status`
- Route: `POST /api/agents/v1/media/files/scale_down_status`
- Authentication: required (owner API key)
- What it does: Poll the status of a video scale-down operation.
- Access policy: index owners and `index_video_editor` only.
- Input options:
  - `file_token` (required)
- Output:
  - `file_token`, `filename`, `scale_down_status` (`processing`/`success`/`failed`)
  - `scale_down_error` (if failed)
  - `scaled_file` (full serialized media upload, only if status is `success`)

### 74) MediaFileMove

- Tool name: `media_file_move`
- Route: `POST /api/agents/v1/media/files/move`
- Authentication: required (owner API key)
- What it does: Move a media file to a different folder the caller can access, including across accessible AI Tinkerers weblogs (or root level in the current weblog if no folder is specified).
- Access policy: index owners and `index_video_editor` only.
- Input options:
  - `file_token` (required)
  - optional: `folder_token` (destination folder; omit to move to root)
- Output:
  - `file` (full serialized media upload in new location)
- Validation: file and destination folder must both be accessible to the caller. Cross-weblog moves update the file's weblog metadata to the destination folder's weblog; moving to root keeps the file in its current weblog.

### 75) MediaFileDelete

- Tool name: `media_file_delete`
- Route: `POST /api/agents/v1/media/files/delete`
- Authentication: required (owner API key)
- What it does: Delete a media file and remove it from S3 storage.
- Access policy: index owners and `index_video_editor` only.
- Input options:
  - `file_token` (required)
- Output:
  - `file_token`, `deleted: true`

### 76) MediaFileRender

- Tool name: `media_file_render`
- Route: `POST /api/agents/v1/media/files/render`
- Authentication: required (owner API key)
- What it does: Retrieve and render the full content of a text, markdown, or JSON file.
- Access policy: index owners and `index_video_editor` only.
- Input options:
  - `file_token` (required)
- Output:
  - `file_token`, `filename`, `content_type`, `content` (full UTF-8 text)
- Validation: file must be text/markdown/JSON (not video/audio), max 5 MB

### 77) MediaTranscriptGet

- Tool name: `media_transcript_get`
- Route: `POST /api/agents/v1/media/transcripts/get`
- File-oriented alias: `POST /api/agents/v1/media/files/transcript` (`media_file_transcript_get`)
- Authentication: required (owner API key)
- What it does: Retrieve the full transcript JSON and cached plain text for a transcribed media file.
- Access policy: index owners and `index_video_editor` only.
- Input options:
  - `file_token` (required)
- Output:
  - `file_token`, `filename`, `transcribed_at`
  - `transcript` (parsed JSON object from S3)
  - `transcript_text` (cached plain text, when available)
- Validation: file must have a completed transcript

### 78) MediaTranscriptGenerate

- Tool name: `media_transcript_generate`
- Route: `POST /api/agents/v1/media/transcripts/generate`
- File-oriented alias: `POST /api/agents/v1/media/files/generate_transcript` (`media_file_transcript_generate`)
- Authentication: required (owner API key)
- What it does: Initiate async transcription of an audio or video file.
- Access policy: index owners only (not `index_video_editor`).
- Input options:
  - `file_token` (required)
- Output:
  - `file_token`, `filename`, `transcript_status: "processing"`
  - `transcript_notice` with queue/large-media context
- Validation: file must be audio or video, cannot already be processing
- Daily cap: 50 transcriptions per day

### 79) MediaTranscriptStatus

- Tool name: `media_transcript_status`
- Route: `POST /api/agents/v1/media/transcripts/status`
- File-oriented alias: `POST /api/agents/v1/media/files/transcript_status` (`media_file_transcript_status`)
- Authentication: required (owner API key)
- What it does: Poll the status of a transcription operation.
- Access policy: index owners and `index_video_editor` only.
- Input options:
  - `file_token` (required)
- Output:
  - `file_token`, `filename`, `transcript_status` (`processing`/`success`/`failed`/null)
  - `transcript_error` (if failed), `has_transcript`, `transcribed_at`, `transcript_attempts`, `processing_metadata`

### 80) MediaTranscriptDelete

- Tool name: `media_transcript_delete`
- Route: `POST /api/agents/v1/media/transcripts/delete`
- Authentication: required (owner API key)
- What it does: Delete transcript data while keeping the media file intact.
- Access policy: index owners and `index_video_editor` only.
- Input options:
  - `file_token` (required)
- Output:
  - `file_token`, `filename`, `transcript_status: null`
- Validation: transcript must exist and cannot be currently processing

## MCP Server

Build the MCP layer as a strict pass-through over `/api/agents/v1/*`, with per-user auth and dynamic tool exposure.

### Required Design

1. Authenticate each MCP session with that user’s own API key.
2. Never use a shared service key for tool calls.
3. Build a per-session capability map from the API policy.
4. Expose tool list/schemas dynamically from that capability map (index owners see more tools; city owners see fewer).
5. On every `tools/call`, forward the same user API key to `/api/agents/v1/*`.
6. Treat API `forbidden_role`, `forbidden_scope`, and `forbidden_api_group` as hard denies; do not retry through alternate paths.
7. Return API payloads as-is so email masking policy remains backend-controlled.
8. Do not cache tool results across users; cache capabilities only per user/session with short TTL.
9. Audit log: `request_id`, hashed key id, tool name, route, and outcome.

### Why This Works With This Setup

- It preserves weblog-level admin switches and endpoint-group restrictions.
- It preserves role + scope enforcement in one backend source of truth.
- It prevents privilege escalation by removing shared-key behavior.
- It ensures email visibility policy is enforced by backend settings, not MCP middleware logic.

## Legacy Route Mapping

Use this table during migration from older path/name conventions.

- New integrations should use only `/api/agents/v1/*`.
- Legacy routes/tool names may be supported as compatibility aliases during rollout.
- When a legacy alias is used, response should include a warning entry in `warnings[]` indicating the preferred modern route/tool name.

| Legacy Route | Legacy Tool Name | Preferred Route | Preferred Tool Name | Notes |
|---|---|---|---|---|
| `/api/ait_fund/agents/v1/*` | n/a | `/api/agents/v1/*` | n/a | Namespace migrated to platform-first path. |
| `/api/agents/v1/fund_content/brand_scrub/analyze` | `fund_content_brand_scrub_analyze` | `/api/agents/v1/restricted_content/brand_scrub/analyze` | `restricted_content_brand_scrub_analyze` | Renamed to remove fund-specific path coupling. |

## Media API

The Media API provides programmatic access to the media upload system (folders, files, notes). It is available to `index_owner`, `index_owner_ai_fund`, and `index_video_editor` roles. The `media` API group is automatically enabled for these roles.

### Endpoints

#### `POST /api/agents/v1/media/folders/list` — `media_folder_list`

List folders and files. Pass `folder_token` to browse a specific folder, or omit for root-level listing.

**Parameters:**
- `folder_token` (string, optional) — token of the folder to list.

**Response:** `{ current_folder, ancestors, folders[], files[] }`

#### `POST /api/agents/v1/media/folders/create` — `media_folder_create`

Create a new folder. Provide `parent_token` to create a subfolder inside an existing folder, or `weblog_token` to create a root-level folder for that weblog.

**Parameters:**
- `name` (string, required) — name for the new folder.
- `parent_token` (string, optional) — token of the parent folder. Weblog is inherited from the parent.
- `weblog_token` (string, optional) — token of the weblog for root-level folders. Required if `parent_token` is not provided.

**Response:** `{ folder: { token, name, weblog_token, parent_token, created_at, ... } }`

#### `POST /api/agents/v1/media/files/get` — `media_file_get`

Get metadata about a file: filename, content type, size, uploader, creation date, folder, note.

**Parameters:**
- `file_token` (string, required)

**Response:** `{ file: { token, filename, content_type, size_in_bytes, human_size, s3_url, created_at, weblog_name, video, audio, image, document, status, folder_token, folder_name, uploader_name, note } }`

#### `POST /api/agents/v1/media/files/download` — `media_file_download`

Get a time-limited presigned download URL for a file. URL expires in 1 hour.

**Parameters:**
- `file_token` (string, required)

**Response:** `{ file_token, filename, download_url, expires_in_seconds }`

#### `POST /api/agents/v1/media/files/upload` — `media_file_upload`

Upload a file to a folder. Content must be base64-encoded. Max 50 MB via API. Supported types: video, audio, images, text, markdown, JSON.

**Parameters:**
- `filename` (string, required) — e.g. `"data.json"`, `"clip.mp4"`
- `content_type` (string, optional) — MIME type. Auto-detected from extension if blank.
- `folder_token` (string, required) — target folder
- `body_base64` (string, required) — file content as base64
- `note` (string, optional) — sticky note (max 2000 chars)

**Response:** `{ file: { ... } }`

#### `POST /api/agents/v1/media/folders/info` — `media_folder_info`

Get detailed info about a folder including its associated meetup (event token, name, date) and weblog.

**Parameters:**
- `folder_token` (string, required)

**Response:** `{ folder: { ... }, meetup: { token, title, start_time, end_time, weblog_name, event_url } | null, weblog: { token, name, city_name } }`

#### `POST /api/agents/v1/media/notes/update` — `media_note_update`

Update or clear the sticky note on a file or folder. Provide either `file_token` or `folder_token` (not both). Pass empty string for `note` to clear.

**Parameters:**
- `file_token` (string, optional)
- `folder_token` (string, optional)
- `note` (string, required) — new note text (max 2000 chars)

**Response:** `{ file_token|folder_token, note }`

### Example: Upload a JSON file to a folder

```bash
# Base64-encode the file content
BODY=$(base64 -i my_data.json)

curl -X POST https://aitinkerers.org/api/agents/v1/media/files/upload \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d "{\"filename\": \"my_data.json\", \"folder_token\": \"FOLDER_TOKEN\", \"body_base64\": \"$BODY\", \"note\": \"Auto-generated data file\"}"
```

## Speaker Promo Kits

The speaker promo kit API mirrors the event manager tool at `/meetup/manage/:meetup_token/speaker_promo_kits`. Generation is asynchronous; list/get endpoints return current state plus hosted `image_url` values once banners are ready.

### `GET|POST /api/agents/v1/speaker_promo_banners/list` — `speaker_promo_banners_list`

List approved speakers and their promo banner state. Use `meetup_token`, `rsvp_token`, `client_token`, `email`, `query`, or `city` filters. When scoped to a `meetup_token`, the response also includes `group_banners[]`.

Response highlights:
- `speakers[]`: `rsvp_token`, `speaker_name`, `talk_title`, `event`, `kit`
- `kit.banners[]`: `card_token`, `variant`, `state`, `ready`, `image_url`, `download_api_path`, `linkedin_post`
- Missing kits are represented with `kit: null`, so callers can distinguish “approved speaker, no banners yet” from “no speaker matched.”

### `GET|POST /api/agents/v1/speaker_promo_banners/group_banners/list` — `speaker_promo_group_banners_list`

Get multi-speaker group banners for one meetup.

Required input:
- `meetup_token`

Response highlights:
- `group_context`: speaker count, missing-photo blockers, lineup fingerprint
- `group_banners[]`: `group_banner_token`, `variant`, `state`, `ready`, `stale`, `image_url`, `linkedin_post`

### `POST /api/agents/v1/speaker_promo_banners/generate` — `speaker_promo_banners_generate`

Queue standard speaker banner generation for matched approved speakers.

Inputs:
- speaker filters: `rsvp_token`, `rsvp_tokens`, `meetup_token`, `client_token`, `email`, or `query`
- `variant_scope`: `social` (default), `print`, or `all`
- `force`: regenerate ready banners when true
- `include_custom`: include custom uploaded-photo kits in the response

### `POST /api/agents/v1/speaker_promo_banners/generate_all` — `speaker_promo_banners_generate_all`

Queue missing banners for all approved speakers on one meetup, optionally including group banners.

Inputs:
- `meetup_token` (required)
- `variant_scope`: `social` (default), `print`, or `all`
- `include_group_banners`: default true
- `force`: regenerate ready/current banners when true

### `POST /api/agents/v1/speaker_promo_banners/group_banners/generate` — `speaker_promo_group_banners_generate`

Queue multi-speaker group banner generation for one meetup. The API returns blockers when the meetup has no approved speakers or one or more speakers are missing profile photos.

Inputs:
- `meetup_token` (required)
- `force`: regenerate current group banners when true

### `POST /api/agents/v1/speaker_promo_banners/regenerate` — `speaker_promo_banners_regenerate`

Regenerate one existing banner asset. Provide exactly one token:
- `card_token`
- `custom_card_token`
- `group_banner_token`

### `POST /api/agents/v1/speaker_promo_banners/custom/generate` — `speaker_promo_custom_banners_generate`

Create/update and queue a custom uploaded-photo speaker promo kit for exactly one approved speaker.

Inputs:
- speaker filters resolving to exactly one speaker
- `custom_image_url`: public image URL to import on first generation
- optional overrides: `display_name`, `display_headline`, `display_talk_title`, `display_talk_summary`
- `use_uploaded_photo_for_group_banner`: use the custom photo for group banners
- `force`: default true

### `GET|POST /api/agents/v1/speaker_promo_banners/download` — `speaker_promo_banners_download`

Download a ready standard or custom card image by `card_token`. Most API clients should use `image_url` directly when they only need a hosted URL.

## Notes

- Existing internal UI actions remain internal:
  - `AiTinkerers::DemoBrowserV2Controller#attio`
  - `AiTinkerers::DemoBrowserV2Controller#fund_screening`
