use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};

use crate::error::{AppError, AppResult};

const BASE: &str = "https://aitinkerers.org/api/agents/v1";

/// Client-side timeout for the four promotion generation endpoints. The
/// server ceiling is ~25s (docs/agents-api.md); this sits above it and is
/// distinct from the plain 6-8s implicit sync read timeout (design D5).
const GENERATION_TIMEOUT: Duration = Duration::from_secs(30);

/// Rate-limit accounting captured from every response (background-sync pacing).
#[derive(Debug, Clone, Default, Serialize)]
pub struct RateInfo {
    pub limit: Option<i64>,
    pub remaining: Option<i64>,
    pub reset: Option<i64>,
    pub retry_after: Option<i64>,
    pub tier: Option<String>,
}

/// Result of one API call: unwrapped `data` plus rate headers.
pub struct ApiOk {
    pub data: Value,
    pub rate: RateInfo,
}

pub struct ApiClient {
    http: reqwest::Client,
    key: String,
}

impl ApiClient {
    pub fn new(key: String) -> Self {
        let http = reqwest::Client::builder()
            .user_agent("AIT-MissionControl/0.1")
            .build()
            .expect("reqwest client");
        Self { http, key }
    }

    /// POST with JSON body. Reads accept POST for backward compat; the key is
    /// sent only via the Authorization header, never in query or body
    /// (docs/agents-api.md auth rules).
    async fn call(&self, path: &str, body: Value) -> AppResult<ApiOk> {
        self.call_timeout(path, body, None).await
    }

    /// POST with an explicit client-side timeout above the ~25s generation
    /// ceiling (design D5, specs/promotion-tools) — distinct from the plain
    /// `call` used by cheap sync reads.
    async fn call_gen(&self, path: &str, body: Value) -> AppResult<ApiOk> {
        self.call_timeout(path, body, Some(GENERATION_TIMEOUT)).await
    }

    async fn call_timeout(&self, path: &str, body: Value, timeout: Option<Duration>) -> AppResult<ApiOk> {
        let url = format!("{BASE}/{path}");
        let mut req = self.http.post(&url).bearer_auth(&self.key).json(&body);
        if let Some(t) = timeout {
            req = req.timeout(t);
        }
        let resp = req.send().await?;
        parse_envelope(resp).await
    }

    /// GET with query params (only `logo_search` uses this — a cheap read,
    /// not a billed generation). The key is still sent only via the
    /// Authorization header, never as a query param.
    async fn call_get(&self, path: &str, query: &[(&str, String)]) -> AppResult<ApiOk> {
        let url = format!("{BASE}/{path}");
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.key)
            .query(query)
            .send()
            .await?;
        parse_envelope(resp).await
    }

    /// GET/POST auth/validate — returns owner identity, roles, enabled groups.
    pub async fn validate(&self) -> AppResult<Value> {
        Ok(self.call("auth/validate", json!({})).await?.data)
    }

    /// Upcoming events across visible chapters (multi-city overview).
    pub async fn upcoming_events(&self, limit: u32) -> AppResult<ApiOk> {
        self.call("meetups/upcoming", json!({ "limit": limit })).await
    }

    /// The caller's past (completed) events. `meetups/search` with `status=past`
    /// returns them without needing a query; results land in `data.matches`.
    pub async fn past_events(&self, limit: u32) -> AppResult<ApiOk> {
        self.call(
            "meetups/search",
            json!({ "status": "past", "limit": limit }),
        )
        .await
    }

    /// RSVPs awaiting Stripe payment for a paid event.
    pub async fn awaiting_payment(&self, meetup_token: &str) -> AppResult<ApiOk> {
        self.call(
            "rsvps/awaiting_payment",
            json!({ "meetup_token": meetup_token }),
        )
        .await
    }

    /// RSVP summary count for an event (used for the total badge).
    pub async fn rsvp_summary(&self, meetup_token: &str) -> AppResult<ApiOk> {
        self.call("rsvps/summary", json!({ "meetup_token": meetup_token }))
            .await
    }

    /// Count of RSVPs actually checked in at the door. `rsvps/summary` with
    /// `status=checked_in` returns this as `total_count` — the true attendance
    /// number (distinct from "completed RSVPs" = attending + waitlisted).
    pub async fn rsvp_checked_in_count(&self, meetup_token: &str) -> AppResult<ApiOk> {
        self.call(
            "rsvps/summary",
            json!({ "meetup_token": meetup_token, "status": "checked_in" }),
        )
        .await
    }

    /// Aggregate performance with an explicit traffic window, so page views
    /// reflect real cumulative traffic rather than a single event-day.
    pub async fn performance_windowed(
        &self,
        weblog_token: &str,
        date_from: &str,
        date_to: &str,
        traffic_from: &str,
        traffic_to: &str,
    ) -> AppResult<ApiOk> {
        self.call(
            "meetups/performance",
            json!({
                "weblog_token": weblog_token,
                "date_from": date_from,
                "date_to": date_to,
                "traffic_from": traffic_from,
                "traffic_to": traffic_to,
            }),
        )
        .await
    }

    /// The rendered public content page for an event (body markdown/text,
    /// title, author/editorial metadata, live URL).
    pub async fn content_page_get(&self, content_page_token: &str) -> AppResult<ApiOk> {
        self.call(
            "content_pages/get",
            json!({ "content_page_token": content_page_token }),
        )
        .await
    }

    /// Email metrics (sends/opens/clicks) for a content page.
    pub async fn content_page_metrics_get(&self, content_page_token: &str) -> AppResult<ApiOk> {
        self.call(
            "content_pages/metrics/get",
            json!({ "content_page_token": content_page_token }),
        )
        .await
    }

    // ── Email lifecycle (specs/email-lifecycle) ────────────────────────────
    // All seven are gated by the `subscribers_sponsors` group + city-owner
    // scope; a denial surfaces as ForbiddenApiGroup/ForbiddenScope so the panel
    // degrades via is_capability_block(). Read-only, aggregates only, no PII.

    /// Aggregate send-job delivery for one event (sent/pending/suppressed,
    /// intended recipients, status_counts, recent send-job rows).
    pub async fn email_send_jobs_summary(
        &self,
        meetup_token: &str,
        limit: u32,
        date_from: Option<&str>,
        date_to: Option<&str>,
    ) -> AppResult<ApiOk> {
        let mut body = json!({ "meetup_token": meetup_token, "limit": limit });
        if let Some(d) = date_from {
            body["date_from"] = json!(d);
        }
        if let Some(d) = date_to {
            body["date_to"] = json!(d);
        }
        self.call("email_send_jobs/summary", body).await
    }

    /// Recent send jobs across the caller's visible weblog scope (chapter view).
    pub async fn email_send_jobs_list(
        &self,
        status: Option<&str>,
        content_page_token: Option<&str>,
        limit: u32,
        date_from: Option<&str>,
        date_to: Option<&str>,
    ) -> AppResult<ApiOk> {
        let mut body = json!({ "limit": limit });
        if let Some(s) = status {
            body["status"] = json!(s);
        }
        if let Some(t) = content_page_token {
            body["content_page_token"] = json!(t);
        }
        if let Some(d) = date_from {
            body["date_from"] = json!(d);
        }
        if let Some(d) = date_to {
            body["date_to"] = json!(d);
        }
        self.call("email_send_jobs/list", body).await
    }

    /// Detailed status for one send job (send_progress, suppression, pipeline).
    pub async fn email_send_job_get(&self, token: &str) -> AppResult<ApiOk> {
        self.call("email_send_jobs/get", json!({ "token": token })).await
    }

    /// Per-bucket throughput series for a send job (sent_count, peak/avg rates).
    pub async fn email_send_job_throughput_get(
        &self,
        token: &str,
        bucket: &str,
    ) -> AppResult<ApiOk> {
        self.call(
            "email_send_jobs/throughput",
            json!({ "token": token, "bucket": bucket }),
        )
        .await
    }

    /// Campaign open/click performance for an event (aggregate rates only). We
    /// keep the payload small: summary + trends, no per-recipient campaign rows.
    pub async fn email_campaign_performance_get(&self, meetup_token: &str) -> AppResult<ApiOk> {
        self.call(
            "analytics/email/campaign_performance",
            json!({
                "meetup_token": meetup_token,
                "campaign_type": "meetup",
                "include_campaigns": true,
                "include_trends": false,
                "include_summary": true,
                "limit": 25,
            }),
        )
        .await
    }

    // ── Survey + follow-up (specs/survey-followup) ─────────────────────────
    // Read-only. Gated by the same city-owner / API-group authorization as the
    // rest of the Agents API; a denial surfaces as ForbiddenApiGroup/Scope/Role
    // so the panel degrades per-source via is_capability_block().

    /// Per-meetup post-event survey state: scheduler eligibility gates, survey
    /// presence/open state, settings, attendee counts, and survey email counts
    /// (sent/opened). This is the primary source for response-rate figures.
    pub async fn survey_diagnostic(&self, meetup_token: &str) -> AppResult<ApiOk> {
        self.call(
            "meetups/survey_diagnostic",
            json!({ "meetup_token": meetup_token }),
        )
        .await
    }

    /// Cross-meetup survey coverage rollup for a lookback window, scoped to a
    /// weblog (the endpoint has no `meetup_token` filter). We locate our event's
    /// row by `meetup_token` in the caller and use it only as response-rate
    /// context — the diagnostic remains the source of truth for this meetup.
    pub async fn survey_report(&self, weblog_token: &str, days: u32) -> AppResult<ApiOk> {
        self.call(
            "meetups/survey_report",
            json!({ "weblog_token": weblog_token, "days": days, "include_rows": true }),
        )
        .await
    }

    /// Sender-domain deliverability health for a weblog (or the caller's default
    /// scope when `weblog_token` is None): health_score + per-domain rates.
    pub async fn email_deliverability_health_get(
        &self,
        weblog_token: Option<&str>,
        date_from: Option<&str>,
        date_to: Option<&str>,
    ) -> AppResult<ApiOk> {
        let mut body = json!({});
        if let Some(t) = weblog_token {
            body["weblog_token"] = json!(t);
        }
        if let Some(d) = date_from {
            body["date_from"] = json!(d);
        }
        if let Some(d) = date_to {
            body["date_to"] = json!(d);
        }
        self.call("analytics/email/deliverability_health", body).await
    }

    /// Fatigue-risk **tier summary** for a weblog scope. We request a small
    /// limit and store only the aggregate `summary` (no per-subscriber rows).
    pub async fn email_fatigue_risk_get(
        &self,
        weblog_token: Option<&str>,
        limit: u32,
    ) -> AppResult<ApiOk> {
        let mut body = json!({ "limit": limit });
        if let Some(t) = weblog_token {
            body["weblog_token"] = json!(t);
        }
        self.call("analytics/email/fatigue_risk", body).await
    }

    // ── Promotion tools (specs/promotion-tools) ────────────────────────────
    // Agent-backed generation writes: slow (up to ~25s), tight rate limits, and
    // never on the poll loop (design D1). Outputs are drafts only — no
    // attendee-data mutation, no publishing.

    /// Per-platform social post package (`social_post_generate`).
    pub async fn social_post_generate(
        &self,
        source_type: &str,
        source_ref: &str,
        platform: &str,
        goal: &str,
        tone: Option<&str>,
        city: Option<&str>,
    ) -> AppResult<ApiOk> {
        let mut body = json!({
            "source_type": source_type,
            "source_ref": source_ref,
            "platform": platform,
            "goal": goal,
        });
        if let Some(t) = tone {
            body["tone"] = json!(t);
        }
        if let Some(c) = city {
            body["city"] = json!(c);
        }
        self.call_gen("social_posts/generate", body).await
    }

    /// Launch-ready event promo package (`event_promo_generate`).
    pub async fn event_promo_generate(
        &self,
        meetup_token: &str,
        package_type: &str,
        audience: &str,
    ) -> AppResult<ApiOk> {
        self.call_gen(
            "event_promos/generate",
            json!({
                "meetup_token": meetup_token,
                "package_type": package_type,
                "audience": audience,
            }),
        )
        .await
    }

    /// Moderated discussion topics for a meetup (`discussion_topics_generate`).
    pub async fn discussion_topics_generate(&self, meetup_token: &str) -> AppResult<ApiOk> {
        self.call_gen(
            "meetups/discussion_topics/generate",
            json!({ "meetup_token": meetup_token }),
        )
        .await
    }

    /// Logo/brand asset search (`logo_search`) — a lightweight GET, not a
    /// billed generation, so it uses the plain (untimed) request path.
    pub async fn logo_search(
        &self,
        query: &str,
        scope: &str,
        include_co_branded: bool,
        limit: u32,
    ) -> AppResult<ApiOk> {
        let q = [
            ("query", query.to_string()),
            ("scope", scope.to_string()),
            ("include_co_branded", include_co_branded.to_string()),
            ("limit", limit.to_string()),
        ];
        self.call_get("logos/search", &q).await
    }

    // ── Sponsor tools (specs/sponsor-tools) ────────────────────────────────
    // Gated by `subscribers_sponsors` + city-owner scope. Search and contact
    // list are cheap GET reads (30/20 rpm); research/pitch are generation
    // calls sharing the same 20s hard-timeout tier as promotion tools
    // (`call_gen`), never on the poll loop, always an explicit user kickoff.

    /// Search sponsors by name/industry/city (`sponsor_search`, GET, 30 rpm).
    pub async fn sponsor_search(
        &self,
        query: &str,
        city: Option<&str>,
        industry: Option<&str>,
        active_only: bool,
        limit: u32,
    ) -> AppResult<ApiOk> {
        let mut q = vec![
            ("query", query.to_string()),
            ("limit", limit.to_string()),
            ("active_only", active_only.to_string()),
        ];
        if let Some(c) = city {
            q.push(("city", c.to_string()));
        }
        if let Some(i) = industry {
            q.push(("industry", i.to_string()));
        }
        self.call_get("sponsors/search", &q).await
    }

    /// Contacts for one sponsor (`sponsor_contact_list`, GET, 20 rpm). Email/
    /// phone masking is applied server-side — the client never unmasks.
    pub async fn sponsor_contact_list(&self, sponsor_ref: &str) -> AppResult<ApiOk> {
        self.call_get("sponsors/contacts", &[("sponsor_ref", sponsor_ref.to_string())])
            .await
    }

    /// AI research brief for a sponsor or free-text company (`sponsor_research_generate`,
    /// POST, 10 rpm, 20s hard timeout).
    pub async fn sponsor_research_generate(
        &self,
        sponsor_ref: Option<&str>,
        name: Option<&str>,
        domain: Option<&str>,
        city: Option<&str>,
        target_audience: Option<&str>,
        context: Option<&Value>,
    ) -> AppResult<ApiOk> {
        let mut body = json!({});
        if let Some(v) = sponsor_ref {
            body["sponsor_ref"] = json!(v);
        }
        if let Some(v) = name {
            body["name"] = json!(v);
        }
        if let Some(v) = domain {
            body["domain"] = json!(v);
        }
        if let Some(v) = city {
            body["city"] = json!(v);
        }
        if let Some(v) = target_audience {
            body["target_audience"] = json!(v);
        }
        if let Some(v) = context {
            body["context"] = v.clone();
        }
        self.call_gen("sponsors/research_generate", body).await
    }

    /// Tailored sponsorship pitch with event context (`sponsor_pitch_generate`,
    /// POST, 10 rpm, 20s hard timeout). The `context` payload is capped at
    /// 64 KB before send (design D2/spec) — see `cap_pitch_context_size`.
    pub async fn sponsor_pitch_generate(
        &self,
        sponsor_ref: Option<&str>,
        name: Option<&str>,
        city: Option<&str>,
        channel: Option<&str>,
        target_audience: Option<&str>,
        context: Option<&Value>,
    ) -> AppResult<ApiOk> {
        let mut body = json!({});
        if let Some(v) = sponsor_ref {
            body["sponsor_ref"] = json!(v);
        }
        if let Some(v) = name {
            body["name"] = json!(v);
        }
        if let Some(v) = city {
            body["city"] = json!(v);
        }
        if let Some(v) = channel {
            body["channel"] = json!(v);
        }
        if let Some(v) = target_audience {
            body["target_audience"] = json!(v);
        }
        if let Some(v) = context {
            body["context"] = v.clone();
        }
        body = cap_pitch_context_size(body, PITCH_CONTEXT_CAP_BYTES);
        self.call_gen("sponsors/pitch_generate", body).await
    }

    // ── RSVP screening (specs/rsvp-screening) ──────────────────────────────
    // Reads render the attendee-management screen from cache. `rsvp_state_update`
    // and `rsvp_bulk_state_update` are the ONLY mutation calls in this client
    // (design D1) — every command that reaches them first passes the
    // prepare/commit confirmation gate in `write_guard` (commands.rs).

    /// Search RSVPs for one event, optionally narrowed by `status` (raw
    /// `rsvp.state`, e.g. "denied") or free-text `query`. Capped at 25 per call
    /// (docs/agents-api.md rate table) — the attendee list is built from a
    /// handful of these calls, not one exhaustive fetch.
    pub async fn rsvp_search(
        &self,
        meetup_token: &str,
        status: Option<&str>,
        query: Option<&str>,
        limit: u32,
    ) -> AppResult<ApiOk> {
        let mut body = json!({ "meetup_token": meetup_token, "limit": limit });
        if let Some(s) = status {
            body["status"] = json!(s);
        }
        if let Some(q) = query {
            body["query"] = json!(q);
        }
        self.call("rsvps/search", body).await
    }

    /// Full RSVP detail (client + meetup context) by token — used for the
    /// priority post-write refresh (design D5) and any direct single-row lookup.
    pub async fn rsvp_get(&self, rsvp_ref: &str) -> AppResult<ApiOk> {
        self.call("rsvps/get", json!({ "rsvp_ref": rsvp_ref })).await
    }

    /// AI screening assessment for one RSVP.
    pub async fn rsvp_assessment_get(&self, rsvp_ref: &str) -> AppResult<ApiOk> {
        self.call("rsvps/assessment", json!({ "rsvp_ref": rsvp_ref })).await
    }

    /// Append-only status-change history for one RSVP, newest first.
    pub async fn rsvp_status_history_list(&self, rsvp_ref: &str, limit: u32) -> AppResult<ApiOk> {
        self.call(
            "rsvps/status_history",
            json!({ "rsvp_token": rsvp_ref, "limit": limit }),
        )
        .await
    }

    /// Subscriber engagement-score breakdown, by subscriber token.
    pub async fn subscriber_score_details_get(&self, subscriber_ref: &str) -> AppResult<ApiOk> {
        self.call(
            "subscribers/score_details",
            json!({ "subscriber_ref": subscriber_ref }),
        )
        .await
    }

    /// Change one RSVP's state. `send_email` (default true upstream) is always
    /// sent explicitly here — the confirm dialog surfaces it, never hides it
    /// (spec: "Email-send choice is explicit").
    pub async fn rsvp_state_update(
        &self,
        rsvp_ref: &str,
        state: &str,
        send_email: bool,
        note: Option<&str>,
    ) -> AppResult<ApiOk> {
        let mut body = json!({ "rsvp_ref": rsvp_ref, "state": state, "send_email": send_email });
        if let Some(n) = note {
            body["note"] = json!(n);
        }
        self.call("rsvps/state_update", body).await
    }

    /// Bulk-change a materialized, enumerated set of RSVPs. The caller
    /// (commands.rs) enforces the per-call ceiling (design D4) before this is
    /// ever reached — the permissive upstream schema never sees an unbounded body.
    pub async fn rsvp_bulk_state_update(
        &self,
        rsvp_refs: &[String],
        state: &str,
        send_email: bool,
        note: Option<&str>,
    ) -> AppResult<ApiOk> {
        let mut body = json!({ "rsvp_refs": rsvp_refs, "state": state, "send_email": send_email });
        if let Some(n) = note {
            body["note"] = json!(n);
        }
        self.call("rsvps/bulk_state_update", body).await
    }

    // ── Attendance check-in (specs/attendance-checkin) ─────────────────────
    // The app's second write path, reusing the same envelope handling as RSVP
    // screening. Called ONLY from `sync::flush_action_queue`, which itself is
    // only ever fed by rows the write_guard-gated commands enqueued.

    /// Record a door check-in (`rsvps/mark_attended`, body `{ rsvp_ref }`).
    /// Sets `confirmed_at` and appends a `rsvp_status_history_list` entry
    /// server-side; idempotent — an already-attended RSVP stays attended.
    pub async fn mark_attended(&self, rsvp_ref: &str) -> AppResult<ApiOk> {
        self.call("rsvps/mark_attended", json!({ "rsvp_ref": rsvp_ref })).await
    }

    // ── Speaker review (specs/speaker-review) ──────────────────────────────
    // The app's third write path, reusing the same write_guard prepare/commit
    // gate as rsvp-screening and attendance-checkin. Approval status is set
    // via `rsvp_speaker_proposal_upsert`'s `speaker_status` field — NOT via
    // `rsvp_state_update`, which only changes the RSVP state (registered/
    // attending/waitlisted/denied) and is the wrong tool for approval
    // (design: "Approval via speaker_proposal_upsert, not state_update").

    /// Ranked future-speaker candidate pool (`speaker_pipeline_candidates_get`,
    /// GET, 15 rpm). Read-only recommendations — a distinct panel from the
    /// review kanban, never a mutation input.
    pub async fn speaker_pipeline_candidates_get(
        &self,
        weblog_token: Option<&str>,
        limit: u32,
    ) -> AppResult<ApiOk> {
        let mut q = vec![("limit", limit.to_string())];
        if let Some(t) = weblog_token {
            q.push(("weblog_token", t.to_string()));
        }
        self.call_get("recommendations/speakers/pipeline", &q).await
    }

    /// Search speaker-tagged RSVPs (talk submissions) for one event, filtered
    /// by `speaker_status` (submitted/pending_review/approved/sidelined/etc,
    /// docs/agents-api.md). Separate from `rsvp_search` (rsvp-screening) since
    /// that method has no speaker_status parameter and the two screens' sync
    /// sweeps must not clobber each other's cached rows.
    pub async fn rsvp_search_speakers(
        &self,
        meetup_token: &str,
        speaker_status: &str,
        limit: u32,
    ) -> AppResult<ApiOk> {
        self.call(
            "rsvps/search",
            json!({ "meetup_token": meetup_token, "speaker_status": speaker_status, "limit": limit }),
        )
        .await
    }

    /// Create/update speaker proposal fields on an RSVP, optionally moving
    /// `speaker_status` (approve -> `main_stage`, decline -> `sidelined`, move
    /// to review -> `pending_review`). `send_speaker_email`/`send_rsvp_email`
    /// are always explicitly pinned `false` here — there is no UI path in this
    /// change that sets either true (spec: "Accidental email sends").
    pub async fn rsvp_speaker_proposal_upsert(
        &self,
        rsvp_ref: &str,
        speaker_title: &str,
        speaker_description: &str,
        speaker_status: Option<&str>,
        note: Option<&str>,
    ) -> AppResult<ApiOk> {
        let mut body = json!({
            "rsvp_ref": rsvp_ref,
            "speaker_title": speaker_title,
            "speaker_description": speaker_description,
            "send_speaker_email": false,
            "send_rsvp_email": false,
        });
        if let Some(s) = speaker_status {
            body["speaker_status"] = json!(s);
        }
        if let Some(n) = note {
            body["note"] = json!(n);
        }
        self.call("rsvps/speaker_proposal_upsert", body).await
    }

    // ── Networking / Connect (specs/networking-connect) ────────────────────
    // The app's fourth write path, reusing the same write_guard prepare/commit
    // gate as rsvp-screening, attendance-checkin, and speaker-review. Reads
    // are GETs (openapi/openapi.yaml is the source of truth for method, not
    // docs/agents-api.md's prose route table); writes are POSTs. Board access
    // is membership/visibility-constrained server-side — this client never
    // computes it.

    /// Search/list message boards the caller can access (`message_board_search`,
    /// GET, 20 rpm).
    pub async fn message_board_search(
        &self,
        query: Option<&str>,
        include_direct_messages: bool,
        include_unread: bool,
        limit: u32,
    ) -> AppResult<ApiOk> {
        let mut q = vec![
            ("limit", limit.to_string()),
            ("include_direct_messages", include_direct_messages.to_string()),
            ("include_unread", include_unread.to_string()),
        ];
        if let Some(v) = query {
            q.push(("query", v.to_string()));
        }
        self.call_get("message_boards/search", &q).await
    }

    /// Recent messages from one accessible board, with optional mention/
    /// needs-response filters (`message_board_messages_list`, GET, 20 rpm).
    pub async fn message_board_messages_list(
        &self,
        board_key: &str,
        mentioned_me: bool,
        needs_response: bool,
        limit: u32,
    ) -> AppResult<ApiOk> {
        let q = vec![
            ("board_key", board_key.to_string()),
            ("mentioned_me", mentioned_me.to_string()),
            ("needs_response", needs_response.to_string()),
            ("limit", limit.to_string()),
        ];
        self.call_get("message_boards/messages/list", &q).await
    }

    /// Fetch one thread by board_key + post_token, expanded root-through-replies
    /// (`message_board_thread_get`, GET, 20 rpm).
    pub async fn message_board_thread_get(
        &self,
        board_key: Option<&str>,
        post_token: &str,
        thread_limit: u32,
    ) -> AppResult<ApiOk> {
        let mut q = vec![
            ("post_token", post_token.to_string()),
            ("thread_limit", thread_limit.to_string()),
        ];
        if let Some(bk) = board_key {
            q.push(("board_key", bk.to_string()));
        }
        self.call_get("message_boards/threads/get", &q).await
    }

    /// Search posts across caller-accessible boards (or one board) with
    /// mention/needs-response filters (`message_board_post_search`, GET, 20 rpm).
    pub async fn message_board_post_search(
        &self,
        mentioned_me: bool,
        needs_response: bool,
        board_key: Option<&str>,
        limit: u32,
    ) -> AppResult<ApiOk> {
        let mut q = vec![
            ("mentioned_me", mentioned_me.to_string()),
            ("needs_response", needs_response.to_string()),
            ("limit", limit.to_string()),
        ];
        if let Some(bk) = board_key {
            q.push(("board_key", bk.to_string()));
        }
        self.call_get("message_boards/posts/search", &q).await
    }

    /// Create a post or reply, optionally attaching up to 4 public image URLs
    /// (`message_board_post_create`, POST, 10 rpm). Input caps (content <=10000
    /// chars, image_urls <=4 each <=2048 chars) are enforced here as a
    /// last-resort net — `commands.rs` validates first and rejects before a
    /// token is ever prepared.
    pub async fn message_board_post_create(
        &self,
        board_key: &str,
        content: &str,
        title: Option<&str>,
        reply_to_post_token: Option<&str>,
        image_urls: &[String],
    ) -> AppResult<ApiOk> {
        let mut body = json!({
            "board_key": board_key,
            "content": cap_chars(content, 10_000),
        });
        if let Some(t) = title {
            body["title"] = json!(cap_chars(t, 300));
        }
        if let Some(r) = reply_to_post_token {
            body["reply_to_post_token"] = json!(r);
        }
        if !image_urls.is_empty() {
            let capped: Vec<String> = image_urls.iter().take(4).map(|u| cap_chars(u, 2048)).collect();
            body["image_urls"] = json!(capped);
        }
        self.call("message_boards/posts/create", body).await
    }

    /// Toggle an emoji reaction on a post (`message_board_reaction_toggle`,
    /// POST, 20 rpm). `reaction_type` validity is checked in `commands.rs`
    /// against the API's documented allowed set before this is ever called.
    pub async fn message_board_reaction_toggle(
        &self,
        board_key: &str,
        post_token: &str,
        reaction_type: &str,
    ) -> AppResult<ApiOk> {
        self.call(
            "message_boards/reactions/toggle",
            json!({ "board_key": board_key, "post_token": post_token, "reaction_type": reaction_type }),
        )
        .await
    }

    /// Upload an image from a public URL for later attachment to a post
    /// (`message_board_attachment_upload`, POST, 10 rpm). Not on the write
    /// path most posts take — `post_create`'s `image_urls` is preferred; this
    /// exists for callers the API requires a pre-uploaded token from.
    pub async fn message_board_attachment_upload(&self, board_key: &str, image_url: &str) -> AppResult<ApiOk> {
        self.call(
            "message_boards/attachments/upload",
            json!({ "board_key": board_key, "image_url": cap_chars(image_url, 2048) }),
        )
        .await
    }

    /// Create/reuse a normal DM conversation with existing clients and post
    /// one message (`direct_message_post_create`, POST). `post_as_ashley` is
    /// always pinned `false` — there is no UI path in this change that sets it
    /// true (spec: "always authored as the caller").
    pub async fn direct_message_post_create(
        &self,
        client_refs: &[String],
        emails: &[String],
        content: &str,
    ) -> AppResult<ApiOk> {
        let mut body = json!({
            "content": cap_chars(content, 10_000),
            "post_as_ashley": false,
        });
        if !client_refs.is_empty() {
            body["client_refs"] = json!(client_refs);
        }
        if !emails.is_empty() {
            body["emails"] = json!(emails);
        }
        self.call("message_boards/direct_messages/post", body).await
    }
}

/// Char-safe truncation (never splits a multi-byte UTF-8 codepoint) for the
/// networking-connect input caps (content, titles, URLs).
fn cap_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        s.chars().take(max_chars).collect()
    }
}

/// Hard cap on the serialized `context` payload for `sponsor_pitch_generate`
/// (spec: "MUST stay within the API's 64 KB context cap"). This is the
/// last-resort safety net; `sync::build_pitch_context` already assembles a
/// small summary-only payload, so this should rarely trigger.
const PITCH_CONTEXT_CAP_BYTES: usize = 64 * 1024;

fn cap_pitch_context_size(mut body: Value, cap: usize) -> Value {
    if body.to_string().len() <= cap {
        return body;
    }
    if let Some(Value::Object(ctx)) = body.get_mut("context") {
        ctx.remove("notes");
    }
    if body.to_string().len() <= cap {
        return body;
    }
    if let Some(Value::Object(ctx)) = body.get_mut("context") {
        if let Some(Value::Object(ev)) = ctx.get_mut("event") {
            ev.retain(|k, _| k == "name" || k == "city");
        }
    }
    if body.to_string().len() <= cap {
        return body;
    }
    // Still oversized (unexpectedly large caller-supplied context) — drop it
    // entirely rather than exceed the cap.
    if let Some(obj) = body.as_object_mut() {
        obj.remove("context");
    }
    body
}

/// Shared envelope parsing for both POST and GET calls: 429 → `RateLimited`
/// (carrying `retry_after` for the sync layer), `{ok:false, error}` → typed
/// error, `{ok:true, data}` → unwrapped data + rate headers.
async fn parse_envelope(resp: reqwest::Response) -> AppResult<ApiOk> {
    let status = resp.status();
    let rate = read_rate(&resp);

    if status.as_u16() == 429 {
        let secs = rate.retry_after.unwrap_or(0);
        return Err(AppError::RateLimited(format!(
            "Rate limited by the AI Tinkerers API; retry_after={secs}"
        )));
    }

    let text = resp.text().await.unwrap_or_default();
    let parsed: Value = serde_json::from_str(&text)
        .map_err(|_| AppError::Network(format!("non-JSON response ({status})")))?;

    if parsed.get("ok").and_then(Value::as_bool) == Some(true) {
        let data = parsed.get("data").cloned().unwrap_or(Value::Null);
        return Ok(ApiOk { data, rate });
    }

    // Error envelope: { ok:false, error:{ code, message } }
    let err = parsed.get("error");
    let code = err
        .and_then(|e| e.get("code"))
        .and_then(Value::as_str)
        .unwrap_or("other");
    let message = err
        .and_then(|e| e.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("Request failed")
        .to_string();
    Err(AppError::from_api_code(code, message))
}

fn read_rate(resp: &reqwest::Response) -> RateInfo {
    let h = resp.headers();
    let num = |name: &str| {
        h.get(name)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.trim().parse::<i64>().ok())
    };
    RateInfo {
        limit: num("x-ratelimit-limit"),
        remaining: num("x-ratelimit-remaining"),
        reset: num("x-ratelimit-reset"),
        retry_after: num("retry-after"),
        tier: h
            .get("x-ratelimit-tier")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string),
    }
}
