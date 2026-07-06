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

// Typed error surfaced from Rust (error envelope code + message).
export interface AppErr {
  code: string;
  message?: string;
}
