use serde::Serialize;
use serde_json::{json, Value};

use crate::error::{AppError, AppResult};

const BASE: &str = "https://aitinkerers.org/api/agents/v1";

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
        let url = format!("{BASE}/{path}");
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.key)
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        let rate = read_rate(&resp);

        if status.as_u16() == 429 {
            // Encode Retry-After so the sync layer can honor it (specs/background-sync).
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
