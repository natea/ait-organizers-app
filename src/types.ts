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
  rsvp_summary?: { total_count?: number; groups?: unknown } | null;
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

// Typed error surfaced from Rust (error envelope code + message).
export interface AppErr {
  code: string;
  message?: string;
}
