# Tasks: add-event-page-rsvp-view

## 1. Backend — API client

- [ ] 1.1 Add `content_page_get(content_page_token?, slug?)` to `api.rs`, unwrapping the envelope to a typed content page (body, plain text, author/editorial metadata, live URL)
- [ ] 1.2 Add `content_page_metrics_get(content_page_token?, slug?)` to `api.rs` returning sends/opens/clicks
- [ ] 1.3 Add a `rsvp_summary` call variant for `status=checked_in` (checked-in count) if not already exposed, alongside the existing `group_by=status` summary
- [ ] 1.4 Map `forbidden_api_group` / `forbidden_scope` / unavailable responses to typed errors per section so callers can degrade independently

## 2. Backend — cache

- [ ] 2.1 Add a `content_pages` cache table (or detail columns) in `db.rs` keyed by `meetup_token`: body, plain text, author/editorial metadata, live URL, fetched-at timestamp
- [ ] 2.2 Store `content_page_metrics_get` results (sends/opens/clicks) in the cache keyed to the event
- [ ] 2.3 Store the `status=checked_in` count with the event's cached RSVP summary
- [ ] 2.4 Add upsert + read helpers for the content page + metrics rows

## 3. Backend — sync & command

- [ ] 3.1 In `sync.rs`, resolve the event's content page token/slug; when present fetch `content_page_get` and `content_page_metrics_get` on detail open/refresh (existing slow cadence, not the upcoming poll loop)
- [ ] 3.2 In `sync.rs`, reuse the already-cached `meetup_performance_get` row for traffic; fetch the `status=checked_in` count when refreshing the RSVP summary
- [ ] 3.3 Record per-section degradation state (page/metrics/rsvp forbidden or unavailable) in the cache so the frontend can render "not enabled"
- [ ] 3.4 Add a `get_event_page` Tauri command in `commands.rs` returning the cached page, metrics, RSVP flow, and per-section availability for an event

## 4. Frontend — types & command binding

- [ ] 4.1 Add event-page types to `types.ts`: content page (title, body, author/editorial, live URL), email metrics, RSVP flow (attending/waitlisted/cancelled/total/checked-in), and per-section availability flags
- [ ] 4.2 Bind the `get_event_page` command in the frontend API layer

## 5. Frontend — detail panel

- [ ] 5.1 Add an event-page panel to `screens/detail.ts` rendering the content page title, inert body, author/editorial metadata, and a deep link to the live URL
- [ ] 5.2 Render the RSVP flow (attending → waitlisted → cancelled → derived Total → checked-in); headline attending + checked-in, never a bare "Registered"
- [ ] 5.3 Suppress conversion when it exceeds 100%; use the `status=checked_in` count for checked-in (never performance "completed RSVPs")
- [ ] 5.4 Render traffic figures from the cached performance row and email metrics (sends/opens/clicks) when present
- [ ] 5.5 Render per-section "not enabled" / "not available" states and a "no public page" empty state
- [ ] 5.6 Add event-page panel styles to `styles.css` per `design/DESIGN.md`

## 6. Verification

- [ ] 6.1 `bunx tsc --noEmit` clean
- [ ] 6.2 `cargo build` and `cargo test` clean
- [ ] 6.3 Drive the detail flow in-browser with mocked Tauri IPC using real content-page + RSVP shapes: page body renders inert, RSVP funnel (attending/waitlisted/cancelled/total/checked-in) correct, deep link present
- [ ] 6.4 Confirm each section degrades independently: force `forbidden_api_group` on content pages (body → "not enabled", RSVP flow still renders) and `forbidden_scope` on rsvps (RSVP flow → "not enabled", body still renders)
- [ ] 6.5 Confirm checked-in comes from `status=checked_in` and implausible (>100%) conversion is hidden
