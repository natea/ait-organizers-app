# Proposal: add-sponsor-tools

Stack-rank #10 — Sponsor tools (niche but high-value when used).

## Why

Sponsorship funds chapters, and while usage is niche it's high-value per use.
Mission Control can give organizers a sponsor workbench: find sponsors, generate
AI research on a company, and draft a tailored pitch — turning an occasional,
manual outreach task into a fast in-app flow.

## What Changes

- Add a Sponsors view: search sponsors, list sponsor contacts, and generate an AI
  research brief and a tailored pitch for a sponsor/company + event context.
- **Generation endpoints** (AI writes; no attendee-data mutations); outputs are
  drafts to review/export.

## Capabilities

### New Capabilities

- `sponsor-tools`: Search sponsors/contacts and generate sponsor research briefs
  and pitches.

## Impact

- Endpoints: read — `sponsor_search`, `sponsor_contact_list`; generation —
  `sponsor_research_generate`, `sponsor_pitch_generate`.
- Gated by the `subscribers_sponsors` API group; **city owners only** (series
  owners not authorized) — degrade gracefully otherwise.
- Contact-field visibility (email/phone masking) rules apply. Generation is
  slow (~20s) — async kickoff + progress. Lower risk (no RSVP mutations).
