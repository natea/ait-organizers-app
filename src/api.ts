// Thin wrappers over Tauri commands. The frontend never talks to the network
// directly — all API access and caching live in Rust (design D2).
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  Board,
  BoardMessage,
  ChapterDeliverability,
  CheckinAttendeesResult,
  CheckinCommitResult,
  CheckinConfirm,
  CheckinCount,
  CheckinDenial,
  CheckinQueueUpdatedEvent,
  ConfirmSummary,
  EventEmail,
  EventObj,
  EventsPayload,
  FlaggedPost,
  FlaggedReason,
  Identity,
  LogoSearchResult,
  NetworkingWriteSettledEvent,
  NextEvent,
  PromotionDraft,
  PromotionDraftMap,
  PromotionJobEvent,
  RsvpDetail,
  RsvpListResult,
  RsvpRow,
  RsvpWriteSettledEvent,
  SpeakerCandidatesResult,
  SpeakerConfirm,
  SpeakerPipeline,
  SpeakerProposal,
  SpeakerWriteSettledEvent,
  SponsorContactsResult,
  SponsorDraft,
  SponsorDraftProgressEvent,
  SponsorSearchResult,
  SurveyFollowup,
  Thread,
  Throughput,
  WriteAuditEntry,
  WritePreview,
} from "./types";

export function validateAndStore(key: string): Promise<Identity> {
  return invoke("validate_and_store", { key });
}

export function hasKey(): Promise<boolean> {
  return invoke("has_key");
}

export function getIdentity(): Promise<Identity> {
  return invoke("get_identity");
}

export function signOut(): Promise<void> {
  return invoke("sign_out");
}

export function getEvents(): Promise<EventsPayload> {
  return invoke("get_events");
}

export function getEventDetail(meetupToken: string): Promise<EventObj | null> {
  return invoke("get_event_detail", { meetupToken });
}

export function fetchEventDetail(meetupToken: string): Promise<EventObj | null> {
  return invoke("fetch_event_detail", { meetupToken });
}

export function refreshNow(): Promise<void> {
  return invoke("refresh_now");
}

export function getNextEvent(): Promise<NextEvent | null> {
  return invoke("get_next_event");
}

// ── Email lifecycle (specs/email-lifecycle) ────────────────────────────────

export function getEventEmail(meetupToken: string): Promise<EventEmail> {
  return invoke("get_event_email", { meetupToken });
}

export function getSendJobThroughput(token: string): Promise<Throughput | null> {
  return invoke("get_send_job_throughput", { token });
}

export function getChapterDeliverability(): Promise<ChapterDeliverability> {
  return invoke("get_chapter_deliverability");
}

/** Trigger a manual email fetch: an event's send data, or (no token) the chapter. */
export function refreshEmail(meetupToken?: string): Promise<void> {
  return invoke("refresh_email", { meetupToken: meetupToken ?? null });
}

// ── Survey + follow-up (specs/survey-followup) ─────────────────────────────

/** Cached-only read (no network) — used for background re-render. */
export function getSurveyFollowup(meetupToken: string): Promise<SurveyFollowup | null> {
  return invoke("get_survey_followup", { meetupToken });
}

/** Fetch on detail open / manual refresh; only meaningful for past events. */
export function fetchSurveyFollowup(meetupToken: string): Promise<SurveyFollowup | null> {
  return invoke("fetch_survey_followup", { meetupToken });
}

export function setNotificationsEnabled(enabled: boolean): Promise<void> {
  return invoke("set_notifications_enabled", { enabled });
}

export function getNotificationsEnabled(): Promise<boolean> {
  return invoke("get_notifications_enabled");
}

export function openMain(): Promise<void> {
  return invoke("open_main");
}

export function hidePopover(): Promise<void> {
  return invoke("hide_popover");
}

// Event subscriptions emitted by the sync engine.
export function onSyncUpdated(cb: () => void): Promise<UnlistenFn> {
  return listen("sync:updated", cb);
}

export function onDetailUpdated(cb: (meetupToken: string) => void): Promise<UnlistenFn> {
  return listen<{ meetup_token: string }>("detail:updated", (e) =>
    cb(e.payload.meetup_token),
  );
}

export function onPopoverData(cb: (data: NextEvent | null) => void): Promise<UnlistenFn> {
  return listen<NextEvent | null>("popover:data", (e) => cb(e.payload));
}

// Emitted when an event's email data finishes syncing.
export function onEmailEvent(cb: (meetupToken: string) => void): Promise<UnlistenFn> {
  return listen<{ meetup_token: string }>("email:event", (e) =>
    cb(e.payload.meetup_token),
  );
}

// Emitted when the chapter deliverability surface finishes syncing.
export function onEmailChapter(cb: () => void): Promise<UnlistenFn> {
  return listen("email:chapter", cb);
}

// ── Promotion tools (specs/promotion-tools) ────────────────────────────────

/** Kick off (or, if one is already in flight, return the id of) a generation job. */
export function promotionGenerate(
  kind: string,
  meetupToken: string,
  platform: string | undefined,
  params: Record<string, unknown>,
): Promise<string> {
  return invoke("promotion_generate", { kind, meetupToken, platform: platform ?? null, params });
}

/** Cancel an in-flight generation job; the action falls back to its cached draft. */
export function promotionCancel(jobId: string): Promise<void> {
  return invoke("promotion_cancel", { jobId });
}

/** All cached promotion drafts for one event (fast path; no network). */
export function getPromotionDrafts(meetupToken: string): Promise<PromotionDraftMap> {
  return invoke("get_promotion_drafts", { meetupToken });
}

/** The cached draft for one (event, kind, platform), if any. */
export function getPromotionDraft(
  meetupToken: string,
  kind: string,
  platform?: string,
): Promise<PromotionDraft | null> {
  return invoke("get_promotion_draft", { meetupToken, kind, platform: platform ?? null });
}

/** Logo/brand asset search — cheap GET, cached with a short freshness window. */
export function logoSearch(
  query: string,
  scope?: string,
  includeCoBranded?: boolean,
  limit?: number,
): Promise<LogoSearchResult> {
  return invoke("logo_search", {
    query,
    scope: scope ?? null,
    includeCoBranded: includeCoBranded ?? null,
    limit: limit ?? null,
  });
}

/** Progress events for tracked generation jobs (design D2). */
export function onPromotionJob(cb: (e: PromotionJobEvent) => void): Promise<UnlistenFn> {
  return listen<PromotionJobEvent>("promotion:job", (e) => cb(e.payload));
}

// ── Sponsor tools (specs/sponsor-tools) ────────────────────────────────────

/** Search sponsors (fetch + cache) and return the cached result page. */
export function sponsorSearch(
  query: string,
  city?: string,
  industry?: string,
  activeOnly?: boolean,
): Promise<SponsorSearchResult> {
  return invoke("sponsor_search", {
    query,
    city: city || null,
    industry: industry || null,
    activeOnly: activeOnly ?? null,
  });
}

/** Cache-only read of one sponsor's contacts (no network). */
export function getSponsorContacts(sponsorRef: string): Promise<SponsorContactsResult> {
  return invoke("get_sponsor_contacts", { sponsorRef });
}

/** Fetch + cache contacts for one sponsor (explicit action on selection). */
export function sponsorContactsGet(sponsorRef: string): Promise<SponsorContactsResult> {
  return invoke("sponsor_contacts_get", { sponsorRef });
}

export interface SponsorGenerateParams {
  sponsorRef?: string;
  name?: string;
  domain?: string;
  city?: string;
  channel?: string;
  targetAudience?: string;
  meetupToken?: string;
  notes?: string;
}

/** Kick off (or, if one is already in flight, return the id of) a research or
 *  pitch generation job. `kind` is "research" or "pitch". */
export function sponsorGenerate(kind: "research" | "pitch", params: SponsorGenerateParams): Promise<string> {
  return invoke("sponsor_generate", {
    kind,
    sponsorRef: params.sponsorRef ?? null,
    name: params.name ?? null,
    domain: params.domain ?? null,
    city: params.city ?? null,
    channel: params.channel ?? null,
    targetAudience: params.targetAudience ?? null,
    meetupToken: params.meetupToken ?? null,
    notes: params.notes ?? null,
  });
}

/** Cancel an in-flight sponsor generation job; falls back to cached drafts. */
export function sponsorGenerationCancel(jobId: string): Promise<void> {
  return invoke("sponsor_generation_cancel", { jobId });
}

/** All cached drafts for one subject (sponsor_token or free-text name),
 *  newest first; `kind` narrows to "research" or "pitch". */
export function getSponsorDrafts(
  subject: { sponsorRef?: string; name?: string },
  kind?: "research" | "pitch",
): Promise<SponsorDraft[]> {
  return invoke("get_sponsor_drafts", {
    sponsorRef: subject.sponsorRef ?? null,
    name: subject.name ?? null,
    kind: kind ?? null,
  });
}

/** One cached draft by id, if any. */
export function getSponsorDraft(draftId: string): Promise<SponsorDraft | null> {
  return invoke("get_sponsor_draft", { draftId });
}

/** Current state of one sponsor generation job (in case an event was missed). */
export function getSponsorJob(jobId: string): Promise<Record<string, unknown> | null> {
  return invoke("get_sponsor_job", { jobId });
}

/** Progress events for tracked sponsor research/pitch generation jobs. */
export function onSponsorDraftProgress(cb: (e: SponsorDraftProgressEvent) => void): Promise<UnlistenFn> {
  return listen<SponsorDraftProgressEvent>("sponsor_draft_progress", (e) => cb(e.payload));
}

// ── RSVP screening (specs/rsvp-screening) — first write feature ────────────
// Reads render the attendee list from cache; every mutation is a two-step
// prepare/commit pair. `_prepare` makes no network call and returns a
// `ConfirmSummary` for the confirm dialog; `_commit` must echo back the exact
// same arguments plus the token, or the write guardrail rejects it server-side.

/** Cached attendee list for one event (fast path; no network). */
export function getRsvpList(meetupToken: string): Promise<RsvpListResult> {
  return invoke("get_rsvp_list", { meetupToken });
}

/** Fetch + cache the attendee list for one event, then return it. */
export function fetchRsvpList(meetupToken: string): Promise<RsvpListResult> {
  return invoke("fetch_rsvp_list", { meetupToken });
}

/** Cached per-registrant detail (assessment, status history, score). */
export function getRsvpDetail(rsvpRef: string): Promise<RsvpDetail | null> {
  return invoke("get_rsvp_detail", { rsvpRef });
}

/** Fetch + cache one registrant's assessment/history/score, then return it. */
export function fetchRsvpDetail(rsvpRef: string): Promise<RsvpDetail | null> {
  return invoke("fetch_rsvp_detail", { rsvpRef });
}

/** Recent write-audit entries for one event (cache-only). */
export function getWriteAudit(meetupToken: string, limit?: number): Promise<WriteAuditEntry[]> {
  return invoke("get_write_audit", { meetupToken, limit: limit ?? null });
}

/** Step 1: bind a confirmation token to an exact single-RSVP mutation. */
export function rsvpStateUpdatePrepare(
  rsvpRef: string,
  newState: string,
  sendEmail: boolean,
  note?: string,
): Promise<ConfirmSummary> {
  return invoke("rsvp_state_update_prepare", { rsvpRef, newState, sendEmail, note: note ?? null });
}

/** Step 2: commit the confirmed single-RSVP mutation. Arguments must exactly
 *  match what was passed to `rsvpStateUpdatePrepare`, or the token is rejected. */
export function rsvpStateUpdateCommit(
  token: string,
  meetupToken: string,
  rsvpRef: string,
  newState: string,
  sendEmail: boolean,
  note?: string,
): Promise<RsvpRow> {
  return invoke("rsvp_state_update_commit", {
    token,
    meetupToken,
    rsvpRef,
    newState,
    sendEmail,
    note: note ?? null,
  });
}

/** Step 1 for a bulk triage over a materialized selection (ceiling-enforced). */
export function rsvpBulkStateUpdatePrepare(
  rsvpRefs: string[],
  newState: string,
  sendEmail: boolean,
  note?: string,
): Promise<ConfirmSummary> {
  return invoke("rsvp_bulk_state_update_prepare", { rsvpRefs, newState, sendEmail, note: note ?? null });
}

/** Step 2: commit the confirmed bulk mutation. */
export function rsvpBulkStateUpdateCommit(
  token: string,
  meetupToken: string,
  rsvpRefs: string[],
  newState: string,
  sendEmail: boolean,
  note?: string,
): Promise<{ updated: number }> {
  return invoke("rsvp_bulk_state_update_commit", {
    token,
    meetupToken,
    rsvpRefs,
    newState,
    sendEmail,
    note: note ?? null,
  });
}

/** Emitted after the priority post-write refresh settles the cache. */
export function onRsvpWriteSettled(cb: (e: RsvpWriteSettledEvent) => void): Promise<UnlistenFn> {
  return listen<RsvpWriteSettledEvent>("rsvp_write:settled", (e) => cb(e.payload));
}

/** Emitted after the attendee list finishes a background fetch. */
export function onRsvpListUpdated(cb: (meetupToken: string) => void): Promise<UnlistenFn> {
  return listen<{ meetup_token: string }>("rsvp_list:updated", (e) => cb(e.payload.meetup_token));
}

// ── Attendance check-in (specs/attendance-checkin) ──────────────────────────
// The door screen for the live/next event. A tap still goes through the same
// prepare/commit write guardrail as RSVP screening — just without a blocking
// dialog in between (design D2): the tap itself is the confirmation. The
// commit only enqueues to a durable offline queue; the actual network write
// happens on the next sync flush, so a tap never blocks on connectivity.

/** Cached attendee list for the live/next event (fast path; no network).
 *  Pass an explicit `meetupToken` to target a specific event. */
export function getCheckinAttendees(meetupToken?: string): Promise<CheckinAttendeesResult> {
  return invoke("get_checkin_attendees", { meetupToken: meetupToken ?? null });
}

/** Fetch + cache the attendee list, opportunistically flushing the offline
 *  check-in queue, then return the merged view. */
export function fetchCheckinAttendees(meetupToken?: string): Promise<CheckinAttendeesResult> {
  return invoke("fetch_checkin_attendees", { meetupToken: meetupToken ?? null });
}

/** Live checked-in-vs-attending progress for the resolved event. */
export function getCheckinCount(meetupToken?: string): Promise<CheckinCount> {
  return invoke("get_checkin_count", { meetupToken: meetupToken ?? null });
}

/** Step 1: bind a confirmation token to an exact door check-in. */
export function checkinPrepare(rsvpRef: string, meetupToken: string): Promise<CheckinConfirm> {
  return invoke("checkin_prepare", { rsvpRef, meetupToken });
}

/** Step 2: commit the confirmed check-in. Arguments must exactly match what
 *  was passed to `checkinPrepare`, or the token is rejected. Returns
 *  immediately with the optimistic row — the network write is deferred. */
export function checkinCommit(token: string, rsvpRef: string, meetupToken: string): Promise<CheckinCommitResult> {
  return invoke("checkin_commit", { token, rsvpRef, meetupToken });
}

/** Emitted after a sync cycle flushes the offline check-in queue for an event. */
export function onCheckinQueueUpdated(cb: (e: CheckinQueueUpdatedEvent) => void): Promise<UnlistenFn> {
  return listen<CheckinQueueUpdatedEvent>("checkin:queue_updated", (e) => cb(e.payload));
}

/** Terminally-denied check-ins for the resolved event (design D7) — used to
 *  disable the check-in controls with an explanatory notice. */
export function getCheckinDenials(meetupToken?: string): Promise<CheckinDenial[]> {
  return invoke("get_checkin_denials", { meetupToken: meetupToken ?? null });
}

// ── Speaker review (specs/speaker-review) ───────────────────────────────────
// The kanban pipeline reads render from cache; approve/decline and the
// create/edit-proposal form both go through the same two-step prepare/commit
// gate as rsvp-screening and attendance-checkin.

/** Cached talk-proposal pipeline for one event (fast path; no network). */
export function getSpeakerProposals(meetupToken: string): Promise<SpeakerPipeline> {
  return invoke("get_speaker_proposals", { meetupToken });
}

/** Fetch + cache the talk-proposal pipeline for one event, then return it. */
export function fetchSpeakerProposals(meetupToken: string): Promise<SpeakerPipeline> {
  return invoke("fetch_speaker_proposals", { meetupToken });
}

/** Cached ranked candidate pool for the resolved (or explicit) scope. */
export function getSpeakerCandidates(weblogToken?: string): Promise<SpeakerCandidatesResult> {
  return invoke("get_speaker_candidates", { weblogToken: weblogToken ?? null });
}

/** Fetch + cache the ranked candidate pool; degrades in place on 429/forbidden
 *  rather than throwing (task 3.2) — the returned result carries `meta`. */
export function fetchSpeakerCandidates(weblogToken?: string): Promise<SpeakerCandidatesResult> {
  return invoke("fetch_speaker_candidates", { weblogToken: weblogToken ?? null });
}

/** Step 1: bind a confirmation token to an approve/decline/move-to-review action. */
export function speakerApprovalPrepare(
  rsvpRef: string,
  newStatus: string,
  note?: string,
): Promise<SpeakerConfirm> {
  return invoke("speaker_approval_prepare", { rsvpRef, newStatus, note: note ?? null });
}

/** Step 2: commit the confirmed approval/decline. Arguments must exactly
 *  match what was passed to `speakerApprovalPrepare`, or the token is rejected. */
export function speakerApprovalCommit(
  token: string,
  meetupToken: string,
  rsvpRef: string,
  newStatus: string,
  note?: string,
): Promise<SpeakerProposal> {
  return invoke("speaker_approval_commit", { token, meetupToken, rsvpRef, newStatus, note: note ?? null });
}

/** Step 1: bind a confirmation token to a create/edit-proposal mutation. */
export function speakerProposalPrepare(
  rsvpRef: string,
  speakerTitle: string,
  speakerDescription: string,
  speakerStatus?: string,
  note?: string,
): Promise<SpeakerConfirm> {
  return invoke("speaker_proposal_prepare", {
    rsvpRef,
    speakerTitle,
    speakerDescription,
    speakerStatus: speakerStatus ?? null,
    note: note ?? null,
  });
}

/** Step 2: commit the confirmed create/edit-proposal mutation. */
export function speakerProposalCommit(
  token: string,
  meetupToken: string,
  rsvpRef: string,
  speakerTitle: string,
  speakerDescription: string,
  speakerStatus?: string,
  note?: string,
): Promise<SpeakerProposal> {
  return invoke("speaker_proposal_commit", {
    token,
    meetupToken,
    rsvpRef,
    speakerTitle,
    speakerDescription,
    speakerStatus: speakerStatus ?? null,
    note: note ?? null,
  });
}

/** Emitted after the priority post-write refresh settles the cache. */
export function onSpeakerWriteSettled(cb: (e: SpeakerWriteSettledEvent) => void): Promise<UnlistenFn> {
  return listen<SpeakerWriteSettledEvent>("speaker_write:settled", (e) => cb(e.payload));
}

/** Emitted after the talk-proposal pipeline finishes a background fetch. */
export function onSpeakerPipelineUpdated(cb: (meetupToken: string) => void): Promise<UnlistenFn> {
  return listen<{ meetup_token: string }>("speaker_pipeline:updated", (e) => cb(e.payload.meetup_token));
}

// ── Networking / Connect (specs/networking-connect) ─────────────────────────
// Boards, board messages, threads, and the cross-board Attention inbox render
// only from the SQLite cache. Every mutation (post/reply, reaction toggle,
// attachment upload, DM) is a two-step prepare/commit pair, same gate as
// rsvp-screening/attendance-checkin/speaker-review.

/** Cached accessible boards (fast path; no network). */
export function getNetworkingBoards(): Promise<Board[]> {
  return invoke("get_networking_boards");
}

/** Fetch + cache boards and the Attention inbox, then return the boards list. */
export function refreshNetworking(): Promise<Board[]> {
  return invoke("refresh_networking");
}

/** Cached messages for one board (fast path; no network). */
export function getBoardMessages(boardKey: string): Promise<BoardMessage[]> {
  return invoke("get_board_messages", { boardKey });
}

/** Fetch + cache one board's messages, optionally filtered. */
export function fetchBoardMessages(
  boardKey: string,
  mentionedMe?: boolean,
  needsResponse?: boolean,
): Promise<BoardMessage[]> {
  return invoke("fetch_board_messages", {
    boardKey,
    mentionedMe: mentionedMe ?? null,
    needsResponse: needsResponse ?? null,
  });
}

/** Cached thread (fast path; no network). */
export function getThread(boardKey: string, rootPostToken: string): Promise<Thread | null> {
  return invoke("get_thread", { boardKey, rootPostToken });
}

/** Fetch + cache one thread (open, or the focus-based/interval refresh of an
 *  already-open thread), then return it from cache. */
export function fetchThread(postToken: string, boardKey?: string): Promise<Thread | null> {
  return invoke("fetch_thread", { boardKey: boardKey ?? null, postToken });
}

/** Cached cross-board Attention inbox (fast path; no network). */
export function getFlaggedPosts(reason?: FlaggedReason): Promise<FlaggedPost[]> {
  return invoke("get_flagged_posts", { reason: reason ?? null });
}

/** Fetch + cache the Attention inbox, then return it from cache. */
export function refreshFlaggedPosts(reason?: FlaggedReason): Promise<FlaggedPost[]> {
  return invoke("refresh_flagged_posts", { reason: reason ?? null });
}

/** Step 1: bind a confirmation token to an exact post/reply, with optional
 *  title (topic-type boards) and up to 4 image URLs. */
export function postCreatePrepare(
  boardKey: string,
  content: string,
  opts?: { title?: string; replyToPostToken?: string; imageUrls?: string[] },
): Promise<WritePreview> {
  return invoke("post_create_prepare", {
    boardKey,
    content,
    title: opts?.title ?? null,
    replyToPostToken: opts?.replyToPostToken ?? null,
    imageUrls: opts?.imageUrls ?? null,
  });
}

/** Step 2: commit the confirmed post/reply. Arguments must exactly match
 *  what was passed to `postCreatePrepare`, or the token is rejected. */
export function postCreateCommit(
  token: string,
  boardKey: string,
  content: string,
  opts?: { title?: string; replyToPostToken?: string; imageUrls?: string[] },
): Promise<Record<string, unknown>> {
  return invoke("post_create_commit", {
    token,
    boardKey,
    content,
    title: opts?.title ?? null,
    replyToPostToken: opts?.replyToPostToken ?? null,
    imageUrls: opts?.imageUrls ?? null,
  });
}

/** Step 1: bind a confirmation token to a single reaction toggle. */
export function reactionTogglePrepare(boardKey: string, postToken: string, reactionType: string): Promise<WritePreview> {
  return invoke("reaction_toggle_prepare", { boardKey, postToken, reactionType });
}

/** Step 2: commit the confirmed reaction toggle. */
export function reactionToggleCommit(
  token: string,
  boardKey: string,
  postToken: string,
  reactionType: string,
): Promise<Record<string, unknown>> {
  return invoke("reaction_toggle_commit", { token, boardKey, postToken, reactionType });
}

/** Step 1: bind a confirmation token to uploading an image from a public URL. */
export function attachmentUploadPrepare(boardKey: string, imageUrl: string): Promise<WritePreview> {
  return invoke("attachment_upload_prepare", { boardKey, imageUrl });
}

/** Step 2: commit the confirmed attachment upload. Returns
 *  `{ attachment_token, image_url, board_key }` for use in a subsequent post. */
export function attachmentUploadCommit(token: string, boardKey: string, imageUrl: string): Promise<Record<string, unknown>> {
  return invoke("attachment_upload_commit", { token, boardKey, imageUrl });
}

/** Step 1: bind a confirmation token to a DM. At least one of `clientRefs`/
 *  `emails` is required — the preview shows the resolved recipients. */
export function directMessagePrepare(
  content: string,
  recipients: { clientRefs?: string[]; emails?: string[] },
): Promise<WritePreview> {
  return invoke("direct_message_prepare", {
    clientRefs: recipients.clientRefs ?? null,
    emails: recipients.emails ?? null,
    content,
  });
}

/** Step 2: commit the confirmed DM. `post_as_ashley` is always false. */
export function directMessageCommit(
  token: string,
  content: string,
  recipients: { clientRefs?: string[]; emails?: string[] },
): Promise<Record<string, unknown>> {
  return invoke("direct_message_commit", {
    token,
    clientRefs: recipients.clientRefs ?? null,
    emails: recipients.emails ?? null,
    content,
  });
}

/** Emitted after boards + the Attention inbox finish a background fetch. */
export function onNetworkingBoardsUpdated(cb: () => void): Promise<UnlistenFn> {
  return listen("networking:boards_updated", cb);
}

export function onNetworkingFlaggedUpdated(cb: () => void): Promise<UnlistenFn> {
  return listen("networking:flagged_updated", cb);
}

/** Emitted after one board's messages finish a background fetch. */
export function onNetworkingBoardUpdated(cb: (boardKey: string) => void): Promise<UnlistenFn> {
  return listen<{ board_key: string }>("networking:board_updated", (e) => cb(e.payload.board_key));
}

/** Emitted when a cached board becomes forbidden (`forbidden_scope`) on read
 *  and is dropped from the cache. */
export function onNetworkingBoardForbidden(cb: (boardKey: string) => void): Promise<UnlistenFn> {
  return listen<{ board_key: string }>("networking:board_forbidden", (e) => cb(e.payload.board_key));
}

/** Emitted after a thread finishes a background fetch. */
export function onNetworkingThreadUpdated(cb: (boardKey: string, rootPostToken: string) => void): Promise<UnlistenFn> {
  return listen<{ board_key: string; root_post_token: string }>("networking:thread_updated", (e) =>
    cb(e.payload.board_key, e.payload.root_post_token),
  );
}

/** Emitted after the targeted post-write re-sync settles the cache. */
export function onNetworkingWriteSettled(cb: (e: NetworkingWriteSettledEvent) => void): Promise<UnlistenFn> {
  return listen<NetworkingWriteSettledEvent>("networking_write:settled", (e) => cb(e.payload));
}
