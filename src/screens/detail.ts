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
  const r = ev.rsvps ?? {};
  const days = num(ev.days_until_event_in_event_timezone);
  const total = Math.max(num(r.registered), num(r.attending) + num(r.waitlisted) + num(r.cancelled), 1);

  const rsvpPanel = `
    <div class="panel">
      <h4>RSVP summary</h4>
      <div class="rsvp-rows">
        ${rsvpRow("Registered", num(r.registered), total, true)}
        ${rsvpRow("Attending", num(r.attending), total, false)}
        ${rsvpRow("Waitlisted", num(r.waitlisted), total, true)}
        ${rsvpRow("Cancelled", num(r.cancelled), total, true)}
      </div>
    </div>`;

  const payPanel = ev.stripe_payment_link_active ? awaitingPanel(ev) : "";
  const perfPanel = performancePanel(ev);
  const gallery = galleryPanel(ev.gallery_preview);

  const org = ev.organizer?.name ? ` · Organizer ${esc(ev.organizer.name)}` : "";
  const url = ev.event_url
    ? ` · <a href="${esc(ev.event_url)}" target="_blank" rel="noopener">${esc(displayUrl(ev.event_url))}</a>`
    : "";

  return `
    <button class="back" id="backBtn">← All events</button>
    <div class="d-head">
      <div>
        <h2>${esc(ev.event_name)}</h2>
        <div class="d-meta">${esc(ev.city ?? "")}${org}${url}</div>
      </div>
      <div class="count ${days < 0 ? "past" : ""}"><b>${days < 0 ? "—" : days === 0 ? "Today" : days + "d"}</b><small>${days > 0 ? "to go" : ""}</small></div>
    </div>
    <div class="d-grid">
      ${rsvpPanel}
      ${payPanel || perfPanel}
      ${payPanel ? perfPanel : ""}
      ${gallery}
    </div>
    <div class="lastsync-foot">Rendering from local cache</div>`;
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
function performancePanel(ev: EventObj): string {
  const p = ev.performance;
  if (p?.unavailable) {
    return `<div class="panel"><h4>Performance</h4>
      <div class="not-enabled"><b>Not enabled for your chapter</b>
        The performance API group is switched off (or out of scope) for this weblog.
        Everything else still works.</div></div>`;
  }
  const row = p?.perf;
  if (!row) {
    return `<div class="panel"><h4>Performance</h4>
      <div class="not-enabled">No performance data cached yet.</div></div>`;
  }
  const views = num(row.traffic?.page_views);
  const completed = num(row.rsvps?.completed);
  const conv = row.conversion?.completed_rsvps_per_page_view;
  const convPct = typeof conv === "number" ? `${(conv * 100).toFixed(1)}%` : "—";
  return `<div class="panel">
    <h4>Performance</h4>
    <div class="rsvp-rows">
      ${statRow("Page views", fmt(views))}
      ${statRow("Completed RSVPs", fmt(completed))}
      ${statRow("Conversion", convPct)}
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
