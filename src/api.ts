// Thin wrappers over Tauri commands. The frontend never talks to the network
// directly — all API access and caching live in Rust (design D2).
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  ChapterDeliverability,
  ConfirmSummary,
  EventEmail,
  EventObj,
  EventsPayload,
  Identity,
  LogoSearchResult,
  NextEvent,
  PromotionDraft,
  PromotionDraftMap,
  PromotionJobEvent,
  RsvpDetail,
  RsvpListResult,
  RsvpRow,
  RsvpWriteSettledEvent,
  SponsorContactsResult,
  SponsorDraft,
  SponsorDraftProgressEvent,
  SponsorSearchResult,
  SurveyFollowup,
  Throughput,
  WriteAuditEntry,
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
