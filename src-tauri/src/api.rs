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
