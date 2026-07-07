## Context

Mission Control is a read-only Tauri desktop app today: screens render exclusively
from the local SQLite cache (`src-tauri/src/db.rs`), which `sync.rs` fills by calling
the Agents API through `api.rs`. `add-networking-connect` is the app's first WRITE
feature alongside `add-rsvp-screening`, so it must introduce and share a single write
guardrail rather than scatter mutation logic per screen.

The Networking / Connect surface targets the gap identified in the stack-rank: strong
email engagement but weak in-product posting. Organizers already live in Mission
Control for events; bringing chapter
message boards into the same app lets them see mentions and posts needing a response,
then post, reply, react, and DM members without a context switch.

Constraints:

- Board access is membership- and visibility-constrained server-side. The app never
  computes access itself; it renders what the API returns and degrades on `forbidden_*`.
- Screens read ONLY from the SQLite cache. Writes go straight to the API, then trigger
  a targeted re-sync of the affected board/thread so the cache reflects the new state.
- All requests carry the Bearer key and expect the `{ok, data, error{code}}` envelope
  with the documented rate limits.
- Attachments are uploaded from a public URL (`message_board_attachment_upload`); there
  is no local file picker in scope.

Relevant endpoints (verified against `openapi/openapi.yaml`):

- Read: `message_board_search`, `message_board_messages_list`,
  `message_board_thread_get`, `message_board_post_search`.
- Write: `message_board_post_create`, `message_board_reaction_toggle`,
  `message_board_attachment_upload`, `direct_message_post_create`.

## Goals / Non-Goals

**Goals:**

- List the boards the caller can access, plus recent threads per board.
- Surface posts that mention the organizer or need a response, network-wide and per board.
- Open a thread and read its replies in order.
- Create a post or reply, toggle a reaction, and DM a member — each write gated by an
  explicit confirmation step and recorded to a local audit log.
- Attach images to a post by uploading from a public URL before posting.
- Reuse the shared write guardrail (confirmation + audit) with `add-rsvp-screening`.
- Enforce membership/visibility by trusting the API and degrading gracefully.

**Non-Goals:**

- Moderation actions (delete/edit others' posts), post editing, and self-delete
  (`message_board_post_update`, `message_board_post_delete`) — out of scope this change.
- Reach analytics (`message_board_posts_reach`) and adding members
  (`message_board_members_add`).
- Local file uploads or drag-and-drop attachments; only URL-based upload is supported.
- Real-time push; refresh is poll-based only.
- Posting as Ashley (`post_as_ashley`) — the app always posts as the authenticated caller.

## Decisions

### Boards list + thread view as one new screen

Add a single `networking` screen (`src/screens/networking.ts`) with a two-pane shape:
a left list of accessible boards (from `message_board_search`, cached) and a right pane
that shows either a board's recent threads (`message_board_messages_list`) or an opened
thread's replies (`message_board_thread_get`). This mirrors the existing
overview/detail split (`overview.ts` / `detail.ts`) and keeps rendering cache-only.

*Alternative considered:* separate top-level screens for boards vs. threads. Rejected —
the thread is always entered from a board or a mention, so a master/detail pane keeps
navigation state simple and matches the existing app pattern.

### Mentions / needs-response as a saved cross-board inbox

Fetch `message_board_post_search` with `mentioned_me=true` and, separately,
`needs_response=true`, and cache both into a `networking_flagged_posts` table tagged by
reason. Render them as an "Attention" section at the top of the boards list so the
organizer sees actionable items first. Per-board views additionally pass
`mentioned_me` / `needs_response` to `message_board_messages_list` for filtering.

*Alternative considered:* computing mentions client-side from message bodies. Rejected —
mention resolution and visibility are server concerns; the API already exposes the flags.

### Single shared write guardrail (confirmation + audit)

Introduce `src-tauri/src/write_guard.rs`, shared with `add-rsvp-screening`. Every write
command (`post_create`, `reaction_toggle`, `attachment_upload`, `direct_message_post_create`)
takes a two-phase shape: the frontend first calls a `prepare_*` command that returns a
normalized preview of exactly what will be sent (board name, target thread, body text,
attachment URLs, recipients); the user confirms; the frontend then calls the matching
`commit_*` command with the same payload plus a confirmation token minted by `prepare_*`.
`commit_*` refuses any payload without a valid, unexpired token. Each committed write
appends a row to a `write_audit` table (timestamp, capability, endpoint, target keys,
outcome, error code) before returning.

*Alternative considered:* a frontend-only confirm dialog. Rejected — confirmation that
lives only in the UI can be bypassed by a direct command invocation, and it leaves no
audit trail. Gating in the Rust command layer makes confirmation and audit unskippable.

### Reaction toggle is confirmed but lightweight

Reactions are low-risk and reversible, but to keep one guardrail path they still flow
through prepare/commit. The reaction preview is a single line ("React :fire: to Diego's
post") and the confirmation token is honored the same way; the audit row is still written.

### Attachment upload then post as a two-request sequence

`message_board_attachment_upload` returns an attachment token from a public image URL.
Because `message_board_post_create` accepts `image_urls` directly (max 4), the app passes
the validated public URLs straight to `post_create` and only calls the dedicated upload
endpoint when the API requires a pre-uploaded token. The preview shows each URL so the
organizer confirms exactly which images post. URLs are capped at 2048 chars and 4 images.

### DM members via direct_message_post_create

DMing a member reuses the same guardrail. The prepare step resolves recipients from
`client_refs`/`emails` and shows them by display name in the confirmation; `post_as_ashley`
is always false. Content is capped at 10000 chars, salutation/signature-free per the API.

### Writes trigger a targeted re-sync, not a full cycle

After a successful `commit_*`, the command re-fetches only the affected board's messages
(or the thread) and upserts into the cache, then signals the frontend to re-render from
cache. This preserves the cache-only rendering rule without waiting for the next poll.

### Polling cadence

Mentions/needs-response and board lists refresh on a moderate interval (default 180s,
consistent with the existing `sync.rs` cadence and rate limits), backing off on `429`
via the existing `get_backoff` mechanism. An open thread refreshes on view and on the
same interval while focused. Writes always re-sync their target immediately regardless
of the poll clock.

### Membership / visibility enforcement + degradation

The app never gates boards locally. `forbidden_role`, `forbidden_scope`, and
`forbidden_api_group` map through the existing `error.rs` variants to a non-blocking
degraded state: the affected board or action is hidden or disabled with an explanatory
note, and the rest of the screen keeps rendering from cache. These are hard denies — no
retry through an alternate path.

## Risks / Trade-offs

- [A write bypasses confirmation via direct command call] → `commit_*` requires a
  server-of-record confirmation token minted by `prepare_*`; audit row is written before
  return so every mutation is traceable.
- [Cache goes stale between poll and a write elsewhere] → targeted re-sync after each
  write refreshes the affected board/thread immediately; screens still render cache-only.
- [Attachment URL is not publicly reachable, so upload/post fails] → validate URL length
  and scheme in `prepare_*`; surface the API error in the preview and block commit.
- [Visibility changes server-side and a cached board becomes forbidden] → treat
  `forbidden_*` on read as a signal to drop the board from cache and degrade that pane,
  not to error the whole screen.
- [Reaction confirmation adds friction to a trivial action] → keep the reaction preview
  to one line and allow the confirmation to be a single keystroke, preserving the guardrail
  without a heavy dialog.
- [Rate limits on mentions/needs-response polling] → moderate 180s cadence with existing
  backoff on `429`; writes re-sync only their own target, not the whole network.
