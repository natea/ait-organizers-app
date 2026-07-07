## 1. Backend — shared write guardrail

- [x] 1.1 Add `src-tauri/src/write_guard.rs` with a prepare/commit token type: `prepare_*` mints a short-lived confirmation token bound to a normalized payload hash; `commit_*` validates and consumes it.
- [x] 1.2 Add a `write_audit` table in `db.rs` (timestamp, capability, endpoint, target keys, outcome, error_code) and a helper to append a row.
- [x] 1.3 Register `write_guard` in `lib.rs` and thread it through shared state (`state.rs`) so both networking and rsvp-screening writes reuse it.

## 2. Backend — API client (read + write)

- [x] 2.1 Add read methods to `api.rs`: `message_board_search`, `message_board_messages_list` (board_key, filters, pagination), `message_board_thread_get` (url or board_key+post_token), `message_board_post_search` (mentioned_me, needs_response).
- [x] 2.2 Add write methods to `api.rs`: `message_board_post_create` (board_key, title, content, reply_to_post_token, image_urls), `message_board_reaction_toggle`, `message_board_attachment_upload`, `direct_message_post_create` (post_as_ashley always false).
- [x] 2.3 Enforce API input caps in the client: content <=10000 chars, image_urls <=4 and each <=2048 chars, reaction_type within the allowed set.
- [x] 2.4 Confirm `forbidden_role` / `forbidden_scope` / `forbidden_api_group` map through existing `error.rs` variants for both read and write paths.

## 3. Backend — cache (db.rs)

- [x] 3.1 Add cache tables: `networking_boards`, `networking_messages`, `networking_threads`, `networking_flagged_posts` (tagged mention | needs_response).
- [x] 3.2 Add upsert + get helpers for boards, messages, threads, and flagged posts, plus a retain/prune helper keyed by board.
- [x] 3.3 Add a helper to drop a board from cache when a read returns `forbidden_scope`.

## 4. Backend — sync (sync.rs)

- [x] 4.1 Add a networking poll (default 180s cadence, existing backoff on 429) that refreshes boards, mentions, and needs-response into the cache.
- [x] 4.2 Add targeted re-sync functions: refresh one board's messages and refresh one thread, callable after a write commit and on thread focus.

## 5. Backend — commands (commands.rs)

- [x] 5.1 Read commands: `get_networking_boards`, `get_board_messages` (with filters), `get_thread`, `get_flagged_posts` — all returning cache data only.
- [x] 5.2 Write prepare commands returning a preview + confirmation token: `prepare_post_create`, `prepare_reaction_toggle`, `prepare_attachment_upload`, `prepare_direct_message`.
- [x] 5.3 Write commit commands requiring a valid token, writing an audit row, then triggering a targeted re-sync: `commit_post_create`, `commit_reaction_toggle`, `commit_attachment_upload`, `commit_direct_message`.
- [x] 5.4 Register all new commands in the Tauri handler in `lib.rs`.

## 6. Frontend — types and API bridge

- [x] 6.1 Add `types.ts` interfaces: Board, BoardMessage, Thread, FlaggedPost, WritePreview, ConfirmationToken.
- [x] 6.2 Add `src/api.ts` wrappers for each read command and each prepare/commit write command.

## 7. Frontend — networking screen

- [x] 7.1 Add `src/screens/networking.ts` with a two-pane layout: boards list (with an attention section for mentions/needs-response) and a right pane for board threads or an opened thread.
- [x] 7.2 Render the boards list and attention section from cache; wire filters (mentions / needs-response) per board.
- [x] 7.3 Render thread view (root post + ordered replies) with focus-based and interval refresh.
- [x] 7.4 Wire the post/reply composer, reaction control, and DM composer through prepare -> confirmation preview -> commit, blocking commit until the user confirms.
- [x] 7.5 Render degraded/disabled states for `forbidden_*` per board and per action without breaking the rest of the screen.
- [x] 7.6 Register the screen in `main.ts` navigation and add styles to `styles.css` per `design/DESIGN.md`.

## 8. Verification

- [x] 8.1 `tsc --noEmit` passes for the frontend.
- [x] 8.2 `cargo build` and `cargo test` pass for `src-tauri`.
- [x] 8.3 Add unit tests for the write guardrail: commit without a token is rejected; commit with a valid token writes an audit row.
- [x] 8.4 Mock-drive the flows against the documented envelope: list boards, open a thread, surface a mention, confirm-and-post, toggle a reaction, DM a member, and a `forbidden_scope` degradation.
