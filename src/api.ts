// Thin wrappers over Tauri commands. The frontend never talks to the network
// directly — all API access and caching live in Rust (design D2).
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  ChapterDeliverability,
  EventEmail,
  EventObj,
  EventsPayload,
  Identity,
  NextEvent,
  SurveyFollowup,
  Throughput,
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
