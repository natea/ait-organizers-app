## ADDED Requirements

### Requirement: List accessible message boards

The system SHALL list the message boards the authenticated caller can access, sourced
from `message_board_search` and rendered only from the local SQLite cache. The list MUST
NOT include boards the API does not return for the caller.

#### Scenario: Boards returned by the API are listed

- **WHEN** the boards list is refreshed and `message_board_search` returns one or more boards
- **THEN** the system caches those boards and renders them in the boards list, each showing its name and unread indicator

#### Scenario: No accessible boards

- **WHEN** `message_board_search` returns an empty set for the caller
- **THEN** the system renders an empty-state message and does not show any board rows

### Requirement: View a thread

The system SHALL open a message-board thread via `message_board_thread_get` and display
the root post and its visible replies in order, rendering only from the cache after sync.

#### Scenario: Open a thread from a board

- **WHEN** the user selects a post in a board and the system fetches the thread via `message_board_thread_get`
- **THEN** the system caches the thread and renders the root post followed by its replies in order

#### Scenario: Thread refreshes while focused

- **WHEN** a thread is open and the poll interval elapses
- **THEN** the system re-fetches the thread and re-renders new replies from the cache without losing the user's scroll position

### Requirement: Surface posts mentioning me or needing a response

The system SHALL fetch posts that mention the caller (`mentioned_me=true`) and posts that
need a response (`needs_response=true`) via `message_board_post_search`, cache them tagged
by reason, and surface them in an attention section above the boards list. Per-board views
SHALL support the same filters via `message_board_messages_list`.

#### Scenario: A mention is surfaced

- **WHEN** `message_board_post_search` with `mentioned_me=true` returns a post
- **THEN** the system caches the post tagged as a mention and shows it in the attention section with its board name

#### Scenario: A post needing a response is surfaced

- **WHEN** `message_board_post_search` with `needs_response=true` returns a post
- **THEN** the system caches the post tagged as needs-response and shows it in the attention section

#### Scenario: Filter a single board by mentions

- **WHEN** the user enables the mentions filter within a board
- **THEN** the system lists only that board's messages returned by `message_board_messages_list` with `mentioned_me=true`

### Requirement: Create a post or reply with explicit confirmation

The system SHALL create a post or reply via `message_board_post_create` only after the
user confirms a preview of the exact payload. The commit step MUST reject any request
lacking a valid confirmation token minted by the preview step, and MUST record the write
to the local audit log. Post content MUST NOT exceed 10000 characters; a topic-type board
post MUST include a title.

#### Scenario: Confirmed post is created and audited

- **WHEN** the user confirms a previewed post and the commit step submits `message_board_post_create` with a valid confirmation token
- **THEN** the system creates the post, writes an audit row for the write, and re-syncs the affected board so the new post renders from the cache

#### Scenario: Commit without confirmation is rejected

- **WHEN** a commit for `message_board_post_create` is attempted without a valid confirmation token
- **THEN** the system refuses the write, submits nothing to the API, and reports that confirmation is required

#### Scenario: Reply targets an existing post

- **WHEN** the user confirms a reply and the commit submits `message_board_post_create` with `reply_to_post_token` set to the target post
- **THEN** the system creates the reply under that thread and re-syncs the thread from the cache

#### Scenario: Attach images by URL to a post

- **WHEN** the user confirms a post whose preview lists up to four public image URLs
- **THEN** the system submits those URLs with the post (uploading via `message_board_attachment_upload` when the API requires a pre-uploaded token) and rejects the write if more than four URLs or a URL over 2048 characters is supplied

### Requirement: Toggle a reaction with confirmation

The system SHALL toggle an emoji reaction on a post via `message_board_reaction_toggle`
after the user confirms a single-line preview, and MUST record the write to the audit log.
The reaction type MUST be one of the API's allowed values.

#### Scenario: Reaction is toggled and audited

- **WHEN** the user confirms reacting to a post with an allowed reaction type and the commit submits `message_board_reaction_toggle`
- **THEN** the system toggles the reaction, writes an audit row, and re-syncs the post so the updated reaction renders from the cache

#### Scenario: Invalid reaction type is rejected

- **WHEN** a reaction commit specifies a reaction type not in the API's allowed set
- **THEN** the system rejects the write before calling the API

### Requirement: Direct-message a member with confirmation

The system SHALL create or reuse a direct-message conversation and post one message via
`direct_message_post_create` only after the user confirms a preview showing the resolved
recipients and message body. The message MUST always be authored as the caller
(`post_as_ashley` false), MUST NOT exceed 10000 characters, and MUST be recorded to the
audit log.

#### Scenario: Confirmed DM is sent and audited

- **WHEN** the user confirms a DM preview and the commit submits `direct_message_post_create` with recipients and content
- **THEN** the system posts the message as the caller, writes an audit row, and re-syncs the DM conversation from the cache

#### Scenario: DM commit without confirmation is rejected

- **WHEN** a commit for `direct_message_post_create` is attempted without a valid confirmation token
- **THEN** the system refuses to send and reports that confirmation is required

### Requirement: Enforce membership and visibility via graceful degradation

The system SHALL rely on the API for all membership and visibility decisions and SHALL NOT
compute board access locally. When the API returns `forbidden_role`, `forbidden_scope`, or
`forbidden_api_group`, the system MUST treat it as a hard deny, degrade only the affected
board or action, and continue rendering the rest of the screen from the cache.

#### Scenario: A board becomes forbidden on read

- **WHEN** a read for a cached board returns `forbidden_scope`
- **THEN** the system removes that board from the cache, degrades its pane with an explanatory note, and keeps rendering other boards

#### Scenario: A write action is forbidden

- **WHEN** a write commit returns `forbidden_role` or `forbidden_scope`
- **THEN** the system reports the action as unavailable, records the failure in the audit log, and does not retry through an alternate path

#### Scenario: The message boards API group is disabled

- **WHEN** a networking request returns `forbidden_api_group`
- **THEN** the system disables the networking screen's actions and shows a degraded state without erroring the rest of the app
