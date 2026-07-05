# Proposal: add-promotion-tools

Stack-rank #8 — Promotion tools: banners, social, speaker promo (useful but
smaller volume: 3,231 content-page image uploads, 2,000 promo banners, 985
speaker-badge emails network-wide).

## Why

Organizers need promotional assets — social posts, event promo packages,
discussion topics, banners/logos — and the Agents API can generate them. Putting
one-click promo generation next to each event removes a manual, tool-hopping step
in the pre-event push.

## What Changes

- Add a Promote action on event detail that generates a promo package: social
  post drafts (per platform), an event promo package, and AI discussion topics.
- Logo/asset lookup for co-branded promo.
- **Generation endpoints** (server-side AI writes, but no attendee-data
  mutations); outputs are drafts the organizer copies/exports.

## Capabilities

### New Capabilities

- `promotion-tools`: Generate social/promo/discussion assets for an event and
  search brand logos.

## Impact

- Endpoints (generation): `social_post_generate`, `event_promo_generate`,
  `discussion_topics_generate`, `logo_search`.
- Generation calls are rate-limited and slower (up to ~25s) — treat as async
  kickoffs with clear progress; cache the latest generated drafts per event.
- Lower risk than attendee-write features: no RSVP/state mutations. Available to
  city/series owners.
