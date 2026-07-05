# Tasks: add-past-events-tab

## 1. Backend — data source & cache

- [ ] 1.1 Add `past_events(limit)` to `api.rs` calling `meetups/search` with `{ status: "past", limit }`
- [ ] 1.2 Add a `kind` column (`'upcoming'`/`'past'`) to the `events` table in `db.rs`; default existing rows to `'upcoming'`
- [ ] 1.3 Scope `get_events` and `retain_events` by `kind` so upcoming refreshes never evict past events (and vice-versa)
- [ ] 1.4 Include per-event `kind` in the `get_events` command payload

## 2. Backend — sync behavior

- [ ] 2.1 Fetch past events on app launch and on manual refresh only (not in the 2-minute upcoming loop); upsert with `kind='past'`
- [ ] 2.2 Exclude `kind='past'` from poll-diff notification comparison
- [ ] 2.3 Exclude past events from the tray "next event" computation
- [ ] 2.4 Dedupe by `meetup_token` if a token appears in both kinds (upcoming wins until it flips to past)
- [ ] 2.5 (Optional) enrich past detail with `meetups/performance` `rsvps.completed` as the checked-in count; fall back to final attending on scope/disabled

## 3. Frontend — overview

- [ ] 3.1 Port `.seg`, `.seg button`, `.seg button.on`, `.count.held`, `.count.held b` from `design/mission-control.html` into `styles.css`
- [ ] 3.2 Add the Upcoming/Past segmented control and `listTab` state to `overview.ts`; filter cards by tab
- [ ] 3.3 Past card variant: held-date chip, checked-in gauge ("checked in"), Attended funnel cell; per-tab notice + footer copy
- [ ] 3.4 Past-tab empty/loading state and recap-window notice

## 4. Frontend — detail

- [ ] 4.1 Past detail variant in `detail.ts`: held chip, Attended row, "final" labels, "recap — data frozen" footer
- [ ] 4.2 Add `kind`/`attended`/`status` fields to `types.ts`

## 5. Verification

- [ ] 5.1 Onboard → overview shows Upcoming/Past tabs; Past lists real completed events (known-good: `mu_sQIVmUBn120`, Boston, final 91 attending / 23 waitlisted)
- [ ] 5.2 Confirm an upcoming poll cycle does NOT evict cached past events
- [ ] 5.3 Confirm past events never fire notifications and never claim the tray "next event"
- [ ] 5.4 Drive the flow in-browser with mocked Tauri IPC using real past-event shapes (upcoming + past tabs, past card + detail variants)
- [ ] 5.5 `bunx tsc --noEmit` clean; `cargo build` clean
