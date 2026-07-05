// Event detail (specs/event-detail): RSVP summary, awaiting-payment,
// performance, and gallery. Renders instantly from cache, then refreshes
// per-event scoped data (performance + awaiting) in the background.
import { fetchEventDetail, getEventDetail } from "../api";
import type { AwaitingRow, EventObj, GalleryPhoto } from "../types";
import { byId, esc, fmt, num } from "../util";

interface DetailOpts {
  onBack: () => void;
}

export interface DetailController {
  open: (meetupToken: string) => Promise<void>;
}

export function mountDetail(opts: DetailOpts): DetailController {
  const root = byId("scr-detail");
  let current: string | null = null;

  async function open(meetupToken: string): Promise<void> {
    current = meetupToken;
    const cached = await getEventDetail(meetupToken);
    if (cached) render(cached);
    else root.innerHTML = `<div class="content"><div class="empty"><div class="spinner"></div></div></div>`;

    // Refresh scoped detail (performance + awaiting); degrade gracefully.
    try {
      const fresh = await fetchEventDetail(meetupToken);
      if (fresh && current === meetupToken) render(fresh);
    } catch {
      /* keep cached render */
    }
  }

  function render(ev: EventObj): void {
    root.innerHTML = `
      <div class="appbar">
        <img src="/logos/logo-stacked-760.png" alt="AI Tinkerers" />
        <span class="a-title">Mission Control</span>
        <span class="spacer"></span>
      </div>
      <div class="content">${bodyHTML(ev)}</div>`;
    byId<HTMLButtonElement>("backBtn").addEventListener("click", opts.onBack);
  }

  return { open };
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
      ${chip}
    </div>
    <div class="d-grid">
      ${pagePanel}
      ${rsvpPanel}
      ${payPanel || perfPanel}
      ${payPanel ? perfPanel : ""}
      ${gallery}
    </div>
    <div class="lastsync-foot">${foot}</div>`;
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
  const author = page.author ?? page.author_name;
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
