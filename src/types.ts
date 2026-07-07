// Shapes mirror the AI Tinkerers Agents API responses we consume. Optional
// everywhere because the API is best-effort and fields vary by scope.

export interface Rsvps {
  registered?: number;
  attending?: number;
  waitlisted?: number;
  cancelled?: number;
  awaiting_payment?: number | null;
  capacity?: number | null;
}

export interface GalleryPhoto {
  photo_token?: string;
  url: string;
  caption?: string;
}

export interface Organizer {
  name?: string;
  title?: string;
  company?: string;
}

export interface PerformanceRow {
  rsvps?: Rsvps & { completed?: number };
  traffic?: { page_views?: number };
  conversion?: { completed_rsvps_per_page_view?: number };
}

export type EventKind = "upcoming" | "past";

export interface EventObj {
  meetup_token: string;
  weblog_token?: string;
  content_page_token?: string;
  event_name: string;
  event_type?: string;
  starts_at?: string;
  starts_at_utc?: string;
  starts_at_local?: string;
  starts_at_local_date?: string;
  days_until_event_in_event_timezone?: number;
  relative_day_in_event_timezone?: string;
  city?: string;
  region?: string;
  country?: string;
  event_url?: string;
  status?: string;
  stripe_payment_link_active?: boolean;
  // Injected by the Rust cache: which tab this event belongs to.
  kind?: EventKind;
  rsvps?: Rsvps;
  organizer?: Organizer;
  gallery_preview?: GalleryPhoto[];
  // Merged in by get_event_detail:
  performance?: {
    perf?: PerformanceRow | null;
    unavailable?: boolean;
    reason?: string | null;
  } | null;
  awaiting_payment?: {
    count?: number;
    results?: AwaitingRow[] | null;
    unavailable?: boolean;
  } | null;
  rsvp_summary?: {
    total_count?: number;
    // Real door check-in count (rsvps/summary status=checked_in); null when
    // not yet fetched or out of scope.
    checked_in?: number | null;
    groups?: unknown;
  } | null;
  // Public content page + email metrics (event-page-view).
  content_page?: {
    page?: ContentPage | null;
    metrics?: ContentPageMetrics | null;
    unavailable?: boolean;
    reason?: string | null;
  } | null;
}

// Rendered public event page. Fields parsed defensively (the API returns a
// single article payload with body markdown/text + editorial metadata).
export interface ContentPage {
  title?: string;
  name?: string;
  body_markdown?: string;
  body_text?: string;
  content_text?: string;
  plain_text?: string;
  author?: string | Record<string, unknown>;
  author_name?: string;
  editorial_status?: string;
  status?: string;
  public_url?: string;
  url?: string;
  published_at?: string;
  updated_at?: string;
  [k: string]: unknown;
}

export interface ContentPageMetrics {
  sends?: number;
  opens?: number;
  clicks?: number;
  [k: string]: unknown;
}

export interface AwaitingRow {
  name?: string;
  client?: { name?: string };
  rsvp?: { created_at?: string };
  created_at?: string;
  [k: string]: unknown;
}

export interface FeatureState {
  unavailable: boolean;
  note: string | null;
  last_fetch_at: string | null;
  backoff_until?: string | null;
}

export interface EventsPayload {
  events: EventObj[];
  features: Record<string, FeatureState>;
}

export interface Identity {
  valid?: boolean;
  owner?: { name?: string; email?: string; admin?: boolean };
  authorization?: {
    caller_roles?: string[];
    enabled_api_groups?: string[];
    scope_full?: boolean;
    email_fields_allowed?: boolean;
  };
}

export interface NextEvent {
  meetup_token?: string;
  name?: string;
  city?: string;
  when?: string;
  days?: number;
  attending?: number;
  capacity?: number | null;
  registered?: number;
  waitlisted?: number;
  cancelled?: number;
}

// ── Email lifecycle (specs/email-lifecycle) ────────────────────────────────
// Aggregates only — no recipient lists or email addresses. Fields optional
// because live shapes were unverifiable; the Rust cache parses defensively.

export interface SendJob {
  token: string;
  meetup_token?: string | null;
  subject?: string | null;
  status?: string | null;
  distribution_option?: string | null;
  sent_count?: number | null;
  pending_count?: number | null;
  suppressed_count?: number | null;
  intended_count?: number | null;
  delivered_percent?: number | null;
  observed_rate?: number | null;
  predicted_finish?: string | null;
  done?: boolean;
  fetched_at?: string;
}

// Aggregate send accounting for an event (email_send_jobs_summary).
export interface EmailSummary {
  send_jobs_count?: number;
  sent_count?: number;
  intended_recipient_count?: number;
  pending_count?: number;
  suppressed_count?: number;
  pre_send_excluded_count?: number;
  status_counts?: Record<string, number>;
  first_sent_at?: string;
  last_sent_at?: string;
  [k: string]: unknown;
}

// Campaign open/click performance (email_campaign_performance_get → summary).
export interface CampaignPerformance {
  summary?: {
    sends?: number;
    delivered?: number;
    delivery_rate?: number;
    opens?: number;
    open_rate?: number;
    clicks?: number;
    click_rate?: number;
    bounces?: number;
    bounce_rate?: number;
    unsubscribes?: number;
    unsubscribe_rate?: number;
    [k: string]: unknown;
  } | null;
  [k: string]: unknown;
}

export interface EventEmail {
  meetup_token?: string;
  summary?: EmailSummary | null;
  campaign?: CampaignPerformance | null;
  send_jobs: SendJob[];
  unavailable?: boolean;
  reason?: string | null;
  updated_at?: string | null;
}

export interface ThroughputBucket {
  bucket_start?: string;
  sent_count?: number;
}

export interface SendProgress {
  observed_send_rate_per_minute?: number;
  predicted_finish_at?: string;
  [k: string]: unknown;
}

export interface Throughput {
  token?: string;
  throughput?: ThroughputBucket[] | null;
  progress?: SendProgress | null;
  peak_rate?: number | null;
  average_rate?: number | null;
  total_sent?: number | null;
  done?: boolean;
  updated_at?: string | null;
}

export interface SenderDomain {
  domain?: string;
  sent?: number;
  delivered?: number;
  bounce_rate?: number;
  complaint_rate?: number;
  unsubscribe_rate?: number;
  status?: string;
}

export interface DeliverabilityHealth {
  health_score?: number;
  sender_domains?: SenderDomain[];
  alerts?: { severity?: string; code?: string; message?: string }[];
  [k: string]: unknown;
}

// Fatigue tier summary only (no per-subscriber rows).
export interface FatigueTierSummary {
  summary?: {
    counts_by_tier?: Record<string, number>;
    by_tier?: Record<string, number>;
    average_fatigue_score?: number;
    evaluated?: number;
    [k: string]: unknown;
  } | null;
  truncated?: boolean;
}

export interface ChapterDeliverability {
  health?: DeliverabilityHealth | null;
  fatigue?: FatigueTierSummary | null;
  recent_jobs: SendJob[];
  truncated?: boolean;
  unavailable?: boolean;
  reason?: string | null;
  updated_at?: string | null;
}

// ── Survey + follow-up (specs/survey-followup) ─────────────────────────────
// Post-event survey coverage + follow-up email engagement for a past event.
// Fields optional/loose because live shapes were unverifiable; the Rust cache
// parses defensively and derives the response rate itself.

export type SourceStatus =
  | "ok"
  | "empty"
  | "forbidden_api_group"
  | "forbidden_scope"
  | "forbidden_role"
  | "unavailable";

export interface SurveyFollowupSurvey {
  eligible_attendees?: number | null;
  response_count?: number | null;
  // Derived server-side; null when the denominator is zero/unknown (never a
  // fabricated 0%).
  response_rate?: number | null;
  survey_email_sent?: number | null;
  survey_email_opened?: number | null;
  // Only present when the API payload actually carries aggregate tallies —
  // never synthesized from counts.
  sentiment?: unknown;
  themes?: unknown;
  report_row_found?: boolean;
  [k: string]: unknown;
}

export interface SurveyFollowupEmail {
  sends?: number | null;
  delivered?: number | null;
  opens?: number | null;
  clicks?: number | null;
  open_rate?: number | null;
  click_rate?: number | null;
  campaign_count?: number;
  [k: string]: unknown;
}

export interface SurveyFollowup {
  meetup_token: string;
  survey?: SurveyFollowupSurvey | null;
  survey_status: SourceStatus;
  email?: SurveyFollowupEmail | null;
  email_status: SourceStatus;
  updated_at?: string | null;
}

// Typed error surfaced from Rust (error envelope code + message).
export interface AppErr {
  code: string;
  message?: string;
}

// ── Promotion tools (specs/promotion-tools) ────────────────────────────────
// Agent-backed generation drafts (social posts, event promo package,
// discussion topics) plus logo/brand search. Generation is user-initiated,
// runs as a tracked background job, and is cached per event/kind/platform so
// navigating away and back renders instantly without re-spending a slow,
// rate-limited call.

export type PromotionKind = "social_post" | "event_promo" | "discussion_topics";

export type PromotionJobStatus =
  | "pending"
  | "running"
  | "ready"
  | "error"
  | "timeout"
  | "cancelled";

export interface PromotionJobEvent {
  job_id: string;
  meetup_token: string;
  kind: PromotionKind;
  platform: string;
  status: PromotionJobStatus;
  error_code?: string | null;
}

// `result` is the raw envelope `data` persisted verbatim — unknown/added
// fields pass through rather than being dropped (design D3).
export interface PromotionDraft {
  result?: {
    artifact?: unknown;
    source?: unknown;
    meetup?: unknown;
    discussion_topics?: string[];
    draft_only?: boolean;
    generated_at?: string;
    [k: string]: unknown;
  } | null;
  generated_at: string;
}

// Map keyed `"kind"` or `"kind:platform"` (get_promotion_drafts).
export type PromotionDraftMap = Record<string, PromotionDraft>;

export interface LogoMatch {
  id?: string;
  token?: string;
  text_content?: string;
  caption?: string;
  imgix_url?: string;
  padded_imgix_url?: string;
  thumbnail_light_url?: string;
  thumbnail_dark_url?: string;
  metadata?: {
    brand_name?: string;
    is_on_dark_background?: boolean;
    is_co_branded?: boolean;
    [k: string]: unknown;
  };
  [k: string]: unknown;
}

export interface LogoSearchResult {
  result?: {
    matches?: LogoMatch[];
    needs_disambiguation?: boolean;
    [k: string]: unknown;
  } | null;
  fetched_at: string;
}

// ── Sponsor tools (specs/sponsor-tools) ────────────────────────────────────
// Sponsor search + contacts are cached reads; research/pitch are agent-backed
// generation drafts, tracked as background jobs and cached per subject/kind so
// reopening a draft never re-spends a slow (~20s), rate-limited (10 rpm) call.

export interface SponsorMatch {
  sponsor_token: string;
  name?: string;
  domain?: string;
  city?: string;
  short_profile?: string;
  [k: string]: unknown;
}

export interface SponsorSearchResult {
  results: SponsorMatch[];
  truncated: boolean;
  unavailable: boolean;
  reason?: string | null;
  fetched_at?: string | null;
}

export interface SponsorContact {
  contact_id: string;
  role?: string | null;
  title?: string | null;
  email?: string | null;
  email_masked: boolean;
  phone?: string | null;
  phone_masked: boolean;
  linkedin?: string | null;
  confidence?: number | null;
}

export interface SponsorContactsResult {
  sponsor_token: string;
  contacts: SponsorContact[];
  truncated: boolean;
  unavailable: boolean;
  reason?: string | null;
  fetched_at?: string | null;
}

export type SponsorDraftKind = "research" | "pitch";

export type SponsorJobStatus =
  | "pending"
  | "running"
  | "ready"
  | "error"
  | "timeout"
  | "cancelled";

export interface SponsorDraftProgressEvent {
  job_id: string;
  subject: string;
  kind: SponsorDraftKind;
  status: SponsorJobStatus;
  error_code?: string | null;
  draft_id?: string | null;
}

// `result` is the raw envelope `data` persisted verbatim — unknown/added
// fields pass through rather than being dropped.
export interface SponsorDraft {
  draft_id: string;
  sponsor_token?: string | null;
  company_name?: string | null;
  kind: SponsorDraftKind;
  params?: Record<string, unknown> | null;
  result?: {
    research_summary?: string;
    sponsor?: unknown;
    pitch_text?: string;
    variants?: string[];
    rationale?: unknown;
    [k: string]: unknown;
  } | null;
  status: string;
  created_at: string;
  updated_at: string;
}

// ── RSVP screening (specs/rsvp-screening) ───────────────────────────────────
// The app's first write feature. Reads render an attendee-management screen
// from cache; writes go through a two-step prepare/commit confirmation gate
// enforced in Rust (not just the confirm dialog), with an append-only audit
// trail of every attempted mutation.

export type RsvpState = "registered" | "attending" | "waitlisted" | "denied";

// Raw `state` drives mutation decisions; `registrant_status*` is what the
// registrant sees (internal `denied` reads as "waitlisted" externally — the
// API's own semantics, not something this app invents).
export interface RsvpRow {
  rsvp_ref: string;
  meetup_token: string;
  name?: string | null;
  email?: string | null;
  state: string;
  registrant_status?: string | null;
  registrant_status_label?: string | null;
  registrant_status_text?: string | null;
  checked_in: boolean;
  score?: number | null;
  updated_at: string;
}

export interface RsvpListResult {
  meetup_token: string;
  rows: RsvpRow[];
}

export interface RsvpDetail {
  rsvp_ref: string;
  assessment?: Record<string, unknown> | null;
  assessment_status: SourceStatus;
  history?: { events?: RsvpHistoryEvent[]; [k: string]: unknown } | null;
  history_status: SourceStatus;
  score?: Record<string, unknown> | null;
  score_status: SourceStatus;
  updated_at: string;
}

export interface RsvpHistoryEvent {
  event_id?: string | number;
  changed_at?: string;
  from_status?: string;
  to_status?: string;
  actor_type?: string;
  actor_name?: string;
  source?: string;
  reason?: string;
  [k: string]: unknown;
}

// Returned by `_prepare` — the summary the confirm dialog renders. `token`
// must be echoed back unchanged to `_commit`, along with the identical
// mutation arguments (a mismatch is rejected server-side as tampered).
export interface ConfirmSummary {
  token: string;
  action: "rsvp_state_update" | "rsvp_bulk_state_update";
  rsvp_ref?: string;
  rsvp_refs?: string[];
  from_state?: string | null;
  to_state: string;
  registrant_status_label?: string | null;
  send_email: boolean;
  count: number;
}

export interface WriteAuditEntry {
  id: string;
  created_at: string;
  action: string;
  targets: string[];
  from_state?: string | null;
  to_state?: string | null;
  send_email: boolean;
  confirmed: boolean;
  outcome: string;
  error_code?: string | null;
  updated_at: string;
}

export interface RsvpWriteSettledEvent {
  meetup_token: string;
  rsvp_refs: string[];
}
