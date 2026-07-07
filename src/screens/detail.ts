// Event detail (specs/event-detail): RSVP summary, awaiting-payment,
// performance, and gallery. Renders instantly from cache, then refreshes
// per-event scoped data (performance + awaiting) in the background.
import {
  fetchEventDetail,
  fetchSurveyFollowup,
  getEventDetail,
  getEventEmail,
  getSendJobThroughput,
  getSurveyFollowup,
  refreshEmail,
} from "../api";
import type {
  AwaitingRow,
  CampaignPerformance,
  EventEmail,
  EventObj,
  GalleryPhoto,
  SendJob,
  SourceStatus,
  SurveyFollowup,
  SurveyFollowupEmail,
  SurveyFollowupSurvey,
  Throughput,
} from "../types";
import { byId, esc, fmt, num } from "../util";
import {
  emailBlockedHTML,
  fmtRate,
  sendJobRowHTML,
  statusChip,
  throughputSparkHTML,
} from "./email";
import { mountPromote } from "./promote";

// Active-send polling cadence — the gentlest value that still feels live
// (design D3 open question). Only runs while the panel is open AND a job is
// active; it stops as soon as every job is completed/failed.
const ACTIVE_POLL_MS = 60_000;

interface DetailOpts {
  onBack: () => void;
  onOpenScreening: (meetupToken: string, eventName: string) => void;
  onOpenSpeakers: (meetupToken: string, eventName: string) => void;
  onOpenMedia: (meetupToken: string, eventName: string, weblogToken?: string) => void;
}

export interface DetailController {
  open: (meetupToken: string) => Promise<void>;
  /** Re-render from cache only (no network) — used for background updates. */
  refresh: (meetupToken: string) => Promise<void>;
}

export function mountDetail(opts: DetailOpts): DetailController {
  const root = byId("scr-detail");
  let current: string | null = null;
  let email: EventEmail | null = null;
  let throughput = new Map<string, Throughput>();
  let surveyFollowup: SurveyFollowup | null = null;
  let pollTimer: ReturnType<typeof setInterval> | null = null;
  const promote = mountPromote("promoteSlot");

  function stopPolling(): void {
    if (pollTimer) {
      clearInterval(pollTimer);
      pollTimer = null;
    }
  }

  async function open(meetupToken: string): Promise<void> {
    if (meetupToken !== current) {
      stopPolling();
      email = null;
      throughput = new Map();
      surveyFollowup = null;
    }
    current = meetupToken;
    const cached = await getEventDetail(meetupToken);
    if (cached) render(cached);
    else root.innerHTML = `<div class="content"><div class="empty"><div class="spinner"></div></div></div>`;
    await promote.open(meetupToken);

    // Refresh scoped detail (performance + awaiting); degrade gracefully.
    let latest: EventObj | null = cached;
    try {
      const fresh = await fetchEventDetail(meetupToken);
      if (fresh && current === meetupToken) {
        render(fresh);
        latest = fresh;
      }
    } catch {
      /* keep cached render */
    }

    // Load cached email first (instant), then trigger a fetch + repaint.
    await loadEmail(meetupToken);
    try {
      await refreshEmail(meetupToken);
      await loadEmail(meetupToken);
    } catch {
      /* keep cached email render */
    }

    // Survey + follow-up: past events only, on open + manual refresh only —
    // never part of the poll loop (design D2, specs/survey-followup).
    if ((latest?.kind ?? "upcoming") === "past") {
      await loadSurveyFollowup(meetupToken);
      try {
        await fetchSurveyFollowup(meetupToken);
        await loadSurveyFollowup(meetupToken);
      } catch {
        /* keep cached survey render */
      }
    }
    scheduleActivePolling(meetupToken);
  }

  // Cache-only re-render for background "detail:updated" events. Must NOT call
  // fetchEventDetail — that would re-emit "detail:updated" and loop forever
  // (continuous re-render + API hammering).
  async function refresh(meetupToken: string): Promise<void> {
    if (meetupToken !== current) return;
    const cached = await getEventDetail(meetupToken);
    if (cached && meetupToken === current) render(cached);
    await loadEmail(meetupToken);
    if ((cached?.kind ?? "upcoming") === "past") {
      await loadSurveyFollowup(meetupToken);
    }
    await promote.refresh(meetupToken);
  }

  // Pull cached email + throughput for active jobs and repaint the panel.
  async function loadEmail(meetupToken: string): Promise<void> {
    try {
      const e = await getEventEmail(meetupToken);
      if (current !== meetupToken) return;
      email = e;
      for (const j of activeJobs(e)) {
        const t = await getSendJobThroughput(j.token);
        if (current !== meetupToken) return;
        if (t) throughput.set(j.token, t);
      }
    } catch {
      /* leave prior email state */
    }
    paintEmail();
  }

  // Cache-only read of the survey/follow-up row and repaint its slot.
  async function loadSurveyFollowup(meetupToken: string): Promise<void> {
    try {
      const sf = await getSurveyFollowup(meetupToken);
      if (current !== meetupToken) return;
      surveyFollowup = sf;
    } catch {
      /* leave prior survey-followup state */
    }
    paintSurveyFollowup();
  }

  // Poll active sends on a gentle cadence; stop once none are active (spec).
  function scheduleActivePolling(meetupToken: string): void {
    stopPolling();
    if (!email || !activeJobs(email).length) return;
    pollTimer = setInterval(async () => {
      if (current !== meetupToken) {
        stopPolling();
        return;
      }
      try {
        await refreshEmail(meetupToken);
        await loadEmail(meetupToken);
      } catch {
        /* transient; try again next tick */
      }
      if (!email || !activeJobs(email).length) stopPolling();
    }, ACTIVE_POLL_MS);
  }

  function render(ev: EventObj): void {
    root.innerHTML = `
      <div class="appbar">
        <img src="/logos/logo-stacked-760.png" alt="AI Tinkerers" />
        <span class="a-title">Mission Control</span>
        <span class="spacer"></span>
      </div>
      <div class="content">${bodyHTML(ev)}</div>`;
    byId<HTMLButtonElement>("backBtn").addEventListener("click", () => {
      stopPolling();
      opts.onBack();
    });
    document.getElementById("screenAttendeesBtn")?.addEventListener("click", () => {
      opts.onOpenScreening(ev.meetup_token, ev.event_name);
    });
    document.getElementById("speakerPipelineBtn")?.addEventListener("click", () => {
      opts.onOpenSpeakers(ev.meetup_token, ev.event_name);
    });
    document.getElementById("mediaBtn")?.addEventListener("click", () => {
      opts.onOpenMedia(ev.meetup_token, ev.event_name, ev.weblog_token);
    });
    paintEmail();
    paintSurveyFollowup();
    promote.paint();
  }

  // The email panel loads asynchronously from the event body, so it fills a
  // dedicated slot rather than forcing a full detail re-render.
  function paintEmail(): void {
    const slot = document.getElementById("emailSlot");
    if (!slot) return;
    slot.innerHTML = emailPanelHTML(email, throughput);
  }

  // Same pattern for the past-event survey/follow-up panel; the slot only
  // exists in the DOM for past events (bodyHTML omits it for upcoming).
  function paintSurveyFollowup(): void {
    const slot = document.getElementById("surveyFollowupSlot");
    if (!slot) return;
    slot.innerHTML = surveyFollowupPanelHTML(surveyFollowup);
  }

  return { open, refresh };
}

function activeJobs(e: EventEmail | null): SendJob[] {
  if (!e || !e.send_jobs) return [];
  return e.send_jobs.filter(
    (j) => !j.done && ["queued", "sending", "active"].includes((j.status ?? "").toLowerCase()),
  );
}

function bodyHTML(ev: EventObj): string {
  const isPast = (ev.kind ?? "upcoming") === "past";
  const r = ev.rsvps ?? {};
  const days = num(ev.days_until_event_in_event_timezone);

  // Total from the RSVP summary (attending + waitlisted + cancelled); the API's
  // `registered` field is 0 for these events, so it is not shown.
  const total =
    ev.rsvp_summary?.total_count ??
    num(r.attending) + num(r.waitlisted) + num(r.cancelled);
  const scale = Math.max(total, 1);

  const rsvpPanel = `
    <div class="panel">
      <h4>RSVP summary${isPast ? " — final" : ""}</h4>
      <div class="rsvp-rows">
        ${rsvpRow("Total", total, scale, true)}
        ${rsvpRow("Attending", num(r.attending), scale, false)}
        ${rsvpRow("Waitlisted", num(r.waitlisted), scale, true)}
        ${rsvpRow("Cancelled", num(r.cancelled), scale, true)}
      </div>
    </div>`;

  // Past events aren't awaiting payment; show performance recap instead.
  const payPanel = !isPast && ev.stripe_payment_link_active ? awaitingPanel(ev) : "";
  const perfPanel = performancePanel(ev, isPast);
  const pagePanel = eventPagePanel(ev);
  const gallery = galleryPanel(ev.gallery_preview);

  const org = ev.organizer?.name ? ` · Organizer ${esc(ev.organizer.name)}` : "";
  const url = ev.event_url
    ? ` · <a href="${esc(ev.event_url)}" target="_blank" rel="noopener">${esc(displayUrl(ev.event_url))}</a>`
    : "";

  const chip = isPast
    ? `<div class="count held"><b>${esc(heldLabel(ev))}</b><small>held</small></div>`
    : `<div class="count ${days < 0 ? "past" : ""}"><b>${days < 0 ? "—" : days === 0 ? "Today" : days + "d"}</b><small>${days > 0 ? "to go" : ""}</small></div>`;

  const foot = isPast
    ? "Recap — data frozen at last sync, event no longer polled"
    : "Rendering from local cache";

  return `
    <button class="back" id="backBtn">← All events</button>
    <div class="d-head">
      <div>
        <h2>${esc(ev.event_name)}</h2>
        <div class="d-meta">${esc(ev.city ?? "")}${org}${url}</div>
      </div>
      <div class="d-head-actions">
        <button class="btn-ghost" id="screenAttendeesBtn">Screen attendees</button>
        <button class="btn-ghost" id="speakerPipelineBtn">Speaker pipeline</button>
        <button class="btn-ghost" id="mediaBtn">Media</button>
        ${chip}
      </div>
    </div>
    <div class="d-grid">
      ${pagePanel}
      ${rsvpPanel}
      ${payPanel || perfPanel}
      ${payPanel ? perfPanel : ""}
      <div id="emailSlot" style="grid-column:1/-1"></div>
      <div id="promoteSlot" style="grid-column:1/-1"></div>
      ${gallery}
      ${isPast ? `<div id="surveyFollowupSlot" style="grid-column:1/-1"></div>` : ""}
    </div>
    <div class="lastsync-foot">${foot}</div>`;
}

// Per-event Email panel (specs/email-lifecycle): send-job status + delivery
// accounting, active-send throughput, and open/click performance. Renders only
// from cached commands; degrades (subscribers group / city-owner) without error.
function emailPanelHTML(email: EventEmail | null, throughput: Map<string, Throughput>): string {
  if (!email) return "";
  if (email.unavailable) {
    return `<div class="panel"><h4>Email</h4>${emailBlockedHTML(email.reason)}</div>`;
  }
  const jobs = email.send_jobs ?? [];
  const s = email.summary ?? null;
  const hasAny = jobs.length > 0 || (s && (num(s.sent_count) > 0 || num(s.send_jobs_count) > 0));
  if (!hasAny) {
    return `<div class="panel"><h4>Email</h4>
      <div class="not-enabled">No email sent for this event yet.</div></div>`;
  }

  // Aggregate delivery accounting from the send-job summary.
  const accounting = s
    ? `<div class="email-stats">
         ${statTile("Sent", fmt(s.sent_count))}
         ${statTile("Intended", fmt(s.intended_recipient_count))}
         ${statTile("Pending", fmt(s.pending_count))}
         ${statTile("Suppressed", fmt(s.suppressed_count))}
       </div>`
    : "";

  // Active-send throughput (one block per active job), else frozen final counts.
  const active = jobs.filter(
    (j) => !j.done && ["queued", "sending", "active"].includes((j.status ?? "").toLowerCase()),
  );
  const throughputHTML = active
    .map((j) => throughputBlock(j, throughput.get(j.token)))
    .join("");

  const jobList = jobs.length
    ? `<div class="job-list">${jobs.map((j) => sendJobRowHTML(j)).join("")}</div>`
    : "";

  const perf = campaignHTML(email.campaign ?? null);

  return `<div class="panel">
    <h4>Email ${active.length ? `<span class="b-count live">${fmt(active.length)} active</span>` : ""}</h4>
    ${accounting}
    ${throughputHTML}
    ${jobList}
    ${perf}
  </div>`;
}

function throughputBlock(j: SendJob, t?: Throughput): string {
  const buckets = t?.throughput ?? [];
  const spark = buckets.length ? throughputSparkHTML(buckets) : "";
  const observed =
    t?.progress?.observed_send_rate_per_minute ?? j.observed_rate ?? null;
  const rate =
    typeof observed === "number" ? `${fmt(Math.round(observed))}/min` : "—";
  const finish = t?.progress?.predicted_finish_at ?? j.predicted_finish ?? null;
  const eta = finish ? `ETA ${esc(shortTime(finish))}` : "predicting…";
  return `<div class="throughput">
    <div class="tp-head">
      ${statusChip(j.status, j.done)}
      <span class="tp-subj">${esc(j.subject ?? "(no subject)")}</span>
      <span class="spacer"></span>
      <span class="tp-rate"><b>${rate}</b><small>observed</small></span>
      <span class="tp-eta">${eta}</span>
    </div>
    ${spark}
  </div>`;
}

function campaignHTML(c: CampaignPerformance | null): string {
  const sum = c?.summary ?? null;
  // Performance is optional enrichment — omit the section entirely when absent
  // (spec: still show send accounting, don't error).
  if (!sum) return "";
  const hasRates =
    sum.delivery_rate != null ||
    sum.open_rate != null ||
    sum.click_rate != null;
  if (!hasRates) return "";
  return `<div class="email-perf">
    <div class="ep-title">Open / click performance</div>
    <div class="email-stats">
      ${statTile("Delivery", fmtRate(sum.delivery_rate))}
      ${statTile("Open rate", fmtRate(sum.open_rate))}
      ${statTile("Click rate", fmtRate(sum.click_rate))}
    </div>
  </div>`;
}

function statTile(label: string, value: string): string {
  return `<div class="stat-tile"><b>${esc(value)}</b><small>${esc(label)}</small></div>`;
}

// Post-event survey + follow-up panel (specs/survey-followup). Past events
// only; renders exclusively from the cached `survey_followup` row. Each
// sub-section (survey, follow-up engagement) degrades independently so one
// forbidden endpoint never blocks the other or the rest of the recap.
function surveyFollowupPanelHTML(sf: SurveyFollowup | null): string {
  if (!sf) {
    return `<div class="panel"><h4>Survey & follow-up</h4>
      <div class="empty"><div class="spinner"></div></div></div>`;
  }
  return `<div class="panel">
    <h4>Survey & follow-up</h4>
    <div class="sf-grid">
      ${surveySectionHTML(sf.survey_status, sf.survey ?? null)}
      ${followupSectionHTML(sf.email_status, sf.email ?? null)}
    </div>
  </div>`;
}

function sourceNotAvailableHTML(status: SourceStatus): string {
  if (status === "forbidden_scope") {
    return `<div class="not-enabled"><b>Needs city-owner access</b>
      Your key doesn't have city-owner scope for this chapter.</div>`;
  }
  if (status === "forbidden_role") {
    return `<div class="not-enabled"><b>Needs a different role</b>
      Your key's role can't read this data.</div>`;
  }
  if (status === "forbidden_api_group") {
    return `<div class="not-enabled"><b>Not enabled for your chapter</b>
      This API group is switched off for this weblog. Everything else still works.</div>`;
  }
  return `<div class="not-enabled">Not available right now.</div>`;
}

function surveySectionHTML(status: SourceStatus, s: SurveyFollowupSurvey | null): string {
  if (status === "forbidden_api_group" || status === "forbidden_scope" || status === "forbidden_role" || status === "unavailable") {
    return `<div class="sf-section"><div class="ep-title">Survey response rate</div>${sourceNotAvailableHTML(status)}</div>`;
  }
  const responses = s?.response_count;
  if (status === "empty" || !s || responses == null || responses === 0) {
    return `<div class="sf-section"><div class="ep-title">Survey response rate</div>
      <div class="not-enabled">No survey responses yet.</div></div>`;
  }
  // Guard the zero/unknown-denominator case: show the raw count, not a fake rate.
  const rateHTML =
    typeof s.response_rate === "number"
      ? statTile("Response rate", fmtRate(s.response_rate))
      : statTile("Responses", fmt(responses));
  const stats = `<div class="email-stats">
    ${rateHTML}
    ${typeof s.response_rate === "number" ? statTile("Responses", fmt(responses)) : ""}
    ${s.eligible_attendees != null ? statTile("Eligible", fmt(s.eligible_attendees)) : ""}
  </div>`;
  const themes = themesHTML(s.themes, s.sentiment);
  return `<div class="sf-section"><div class="ep-title">Survey response rate</div>${stats}${themes}</div>`;
}

function themesHTML(themes: unknown, sentiment: unknown): string {
  // Sentiment/themes are opportunistic — omit the block entirely rather than
  // inferring anything from counts (design D3).
  if (!themes && !sentiment) return "";
  const sentimentChip =
    sentiment != null
      ? `<span class="theme-chip sentiment">${esc(summarizeUnknown(sentiment))}</span>`
      : "";
  const themeChips = Array.isArray(themes)
    ? themes
        .slice(0, 8)
        .map((t) => `<span class="theme-chip">${esc(summarizeUnknown(t))}</span>`)
        .join("")
    : themes && typeof themes === "object"
      ? Object.keys(themes as Record<string, unknown>)
          .slice(0, 8)
          .map((k) => `<span class="theme-chip">${esc(k)}</span>`)
          .join("")
      : "";
  if (!sentimentChip && !themeChips) return "";
  return `<div class="theme-chips">${sentimentChip}${themeChips}</div>`;
}

function summarizeUnknown(v: unknown): string {
  if (typeof v === "string") return v;
  if (typeof v === "number") return String(v);
  if (v && typeof v === "object") {
    const o = v as Record<string, unknown>;
    const label = o.label ?? o.name ?? o.theme ?? o.tag;
    if (typeof label === "string") return label;
  }
  return JSON.stringify(v);
}

function followupSectionHTML(status: SourceStatus, e: SurveyFollowupEmail | null): string {
  if (status === "forbidden_api_group" || status === "forbidden_scope" || status === "forbidden_role" || status === "unavailable") {
    return `<div class="sf-section"><div class="ep-title">Follow-up engagement</div>${sourceNotAvailableHTML(status)}</div>`;
  }
  if (status === "empty" || !e || e.sends == null) {
    return `<div class="sf-section"><div class="ep-title">Follow-up engagement</div>
      <div class="not-enabled">No follow-up email sent for this event yet.</div></div>`;
  }
  return `<div class="sf-section"><div class="ep-title">Follow-up engagement</div>
    <div class="email-stats">
      ${statTile("Sent", fmt(e.sends))}
      ${statTile("Open rate", fmtRate(e.open_rate))}
      ${statTile("Click rate", fmtRate(e.click_rate))}
    </div></div>`;
}

function shortTime(iso: string): string {
  const t = Date.parse(iso);
  if (!Number.isFinite(t)) return iso;
  return new Date(t).toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" });
}

// Public event page (event-page-view): rendered inert (no scripts/active forms),
// with editorial metadata, email metrics, and a deep link to the live page.
function eventPagePanel(ev: EventObj): string {
  const cp = ev.content_page;
  // Not yet fetched (no token or background fetch pending): omit; it appears
  // once fetch_event_detail refreshes the cache.
  if (!cp) return "";
  if (cp.unavailable) {
    return `<div class="panel" style="grid-column:1/-1"><h4>Event page</h4>
      <div class="not-enabled"><b>Not enabled for your chapter</b>
        The content-pages API group is switched off (or out of scope) for this weblog.</div></div>`;
  }
  const page = cp.page;
  if (!page) {
    return `<div class="panel" style="grid-column:1/-1"><h4>Event page</h4>
      <div class="not-enabled">No public page found for this event.</div></div>`;
  }
  const title = page.title ?? page.name ?? ev.event_name;
  const bodyText =
    page.content_text ?? page.plain_text ?? page.body_text ?? page.body_markdown ?? "";
  // `author` may be a string or an object ({blogger_token, ...}); only show a
  // human-readable name, never "[object Object]".
  const author =
    typeof page.author === "string"
      ? page.author
      : typeof page.author_name === "string"
        ? page.author_name
        : undefined;
  const status = page.editorial_status ?? page.status;
  const liveUrl = page.public_url ?? page.url ?? ev.event_url;
  const m = cp.metrics;
  const metricsRow =
    m && (m.sends != null || m.opens != null || m.clicks != null)
      ? `<div class="page-metrics">
           <span><b>${fmt(m.sends)}</b> sends</span>
           <span><b>${fmt(m.opens)}</b> opens</span>
           <span><b>${fmt(m.clicks)}</b> clicks</span>
         </div>`
      : "";
  const meta = [author ? `By ${esc(author)}` : "", status ? esc(status) : ""]
    .filter(Boolean)
    .join(" · ");
  const link = liveUrl
    ? `<a class="page-link" href="${esc(liveUrl)}" target="_blank" rel="noopener">Open live page ↗</a>`
    : "";

  return `<div class="panel" style="grid-column:1/-1">
    <h4>Event page${link ? ` <span class="spacer-h"></span>${link}` : ""}</h4>
    <div class="page-title">${esc(title)}</div>
    ${meta ? `<div class="page-meta">${meta}</div>` : ""}
    ${metricsRow}
    <div class="page-body">${esc(bodyText.slice(0, 20000))}</div>
  </div>`;
}

// Short "Jun 29" held-date label from the event's start date.
function heldLabel(ev: EventObj): string {
  const iso = ev.starts_at_utc ?? ev.starts_at ?? "";
  const t = Date.parse(iso);
  if (Number.isFinite(t)) {
    return new Date(t).toLocaleDateString("en-US", { month: "short", day: "numeric" });
  }
  return ev.starts_at_local_date ?? "held";
}

function rsvpRow(label: string, val: number, total: number, alt: boolean): string {
  const pct = total ? (val / total) * 100 : 0;
  return `<div class="rsvp-row${alt ? " alt" : ""}">
    <span class="r-label">${esc(label)}</span>
    <div class="r-bar"><div class="r-fill" style="width:${Math.min(100, pct)}%"></div></div>
    <b>${fmt(val)}</b></div>`;
}

function awaitingPanel(ev: EventObj): string {
  const a = ev.awaiting_payment;
  if (a?.unavailable) {
    return `<div class="panel"><h4>Awaiting payment</h4>
      <div class="not-enabled"><b>Not available for your scope</b>
        This event's payment data is outside your chapter scope.</div></div>`;
  }
  const rows = a?.results ?? [];
  if (!rows.length) {
    return `<div class="panel"><h4>Awaiting payment <span class="b-count">0</span></h4>
      <div class="not-enabled">Everyone who registered has paid.</div></div>`;
  }
  const list = rows
    .map((row: AwaitingRow) => {
      const name = row.name ?? row.client?.name ?? "Registrant";
      const when = row.created_at ?? row.rsvp?.created_at ?? "";
      return `<div class="pay-row"><span>${esc(name)}</span><time>${esc(when)}</time></div>`;
    })
    .join("");
  return `<div class="panel">
    <h4>Awaiting payment <span class="b-count">${fmt(a?.count ?? rows.length)}</span></h4>
    <div class="pay-list">${list}</div></div>`;
}

// Real performance endpoint returns an aggregate row (page views, completed
// RSVPs, conversion) — rendered as stat tiles, not the prototype's line chart.
function performancePanel(ev: EventObj, isPast = false): string {
  const title = isPast ? "Performance — final" : "Performance";
  const p = ev.performance;
  if (p?.unavailable) {
    return `<div class="panel"><h4>${title}</h4>
      <div class="not-enabled"><b>Not enabled for your chapter</b>
        The performance API group is switched off (or out of scope) for this weblog.
        Everything else still works.</div></div>`;
  }
  const row = p?.perf;
  if (!row) {
    return `<div class="panel"><h4>${title}</h4>
      <div class="not-enabled">No performance data cached yet.</div></div>`;
  }
  const views = num(row.traffic?.page_views);
  const completed = num(row.rsvps?.completed);
  // Real door check-in count (rsvps/summary status=checked_in) — the true
  // attendance number, distinct from "completed RSVPs" (attending + waitlisted).
  const checkedIn = ev.rsvp_summary?.checked_in;
  const conv = row.conversion?.completed_rsvps_per_page_view;
  // Only show conversion when it's a sane fraction (a >100% value means the
  // traffic window is off, so suppress rather than mislead).
  const showConv = typeof conv === "number" && conv >= 0 && conv <= 1;

  const attendanceRow =
    typeof checkedIn === "number"
      ? statRow("Checked in", fmt(checkedIn))
      : statRow("Completed RSVPs", fmt(completed));

  return `<div class="panel">
    <h4>${title}</h4>
    <div class="rsvp-rows">
      ${statRow("Page views", fmt(views))}
      ${attendanceRow}
      ${showConv ? statRow("Conversion", `${(conv! * 100).toFixed(1)}%`) : ""}
    </div></div>`;
}

function statRow(label: string, value: string): string {
  return `<div class="rsvp-row"><span class="r-label">${esc(label)}</span>
    <div class="r-bar" style="visibility:hidden"></div><b>${esc(value)}</b></div>`;
}

function galleryPanel(photos?: GalleryPhoto[]): string {
  if (!photos || !photos.length) return "";
  const figs = photos
    .slice(0, 6)
    .map(
      (ph) =>
        `<figure><img src="${esc(ph.url)}" alt="${esc(ph.caption ?? "")}" />
          <figcaption>${esc(ph.caption ?? "")}</figcaption></figure>`,
    )
    .join("");
  return `<div class="panel" style="grid-column:1/-1"><h4>Gallery preview</h4>
    <div class="gallery">${figs}</div></div>`;
}

function displayUrl(u: string): string {
  return u.replace(/^https?:\/\//, "").replace(/\/$/, "");
}
