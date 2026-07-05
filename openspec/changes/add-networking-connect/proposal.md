# Proposal: add-networking-connect

Stack-rank #6 — Networking / Connect (high email engagement, but much lighter in-product posting).

## Why

Networking has strong email engagement but weak in-product posting — the message
boards are underused. Mission Control can surface board activity where organizers
already are, making it easy to see and respond to member posts (mentions, threads
needing a reply) and to post announcements to their community board.

## What Changes

- Add a Community/Boards view: list accessible boards, recent threads, and posts
  that mention the organizer or need a response.
- **Write actions**: create posts/replies, toggle reactions, and direct-message
  members.
- Thread view with inline replies.

## Capabilities

### New Capabilities

- `networking-connect`: Browse chapter message boards and post/reply/react and DM
  members from the app.

## Impact

- Endpoints: read — `message_board_search`, `message_board_messages_list`,
  `message_board_thread_get`, `message_board_post_search`; **write** —
  `message_board_post_create`, `message_board_reaction_toggle`,
  `message_board_attachment_upload`, `direct_message_post_create`.
- Write-capable — reuses the guardrail/confirmation work; membership/visibility is
  enforced per board. Attachments upload from URL.
- New screen; moderate polling for mentions/needs-response.
