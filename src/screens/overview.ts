// Events overview: cards rendered from the SQLite cache (specs/events-overview).
import { getEvents, refreshNow } from "../api";
import type { EventObj, EventsPayload } from "../types";
import { byId, esc, fmt, num } from "../util";

interface OverviewOpts {
  onOpenDetail: (meetupToken: string) => void;
}

export interface OverviewController {
  reload: () => Promise<void>;
}

export function mountOverview(opts: OverviewOpts): OverviewController {
  const root = byId("scr-overview");
  root.innerHTML = `
    <div class="appbar">
      <img src="/logos/logo-stacked-760.png" alt="AI Tinkerers" />
      <span class="a-title">Mission Control</span>
      <span class="spacer"></span>
      <span class="sync"><span class="s-dot" id="syncDot"></span><span id="syncLabel">—</span></span>
      <button class="refresh" id="refreshBtn">Refresh</button>
    </div>
    <div class="content">
      <div class="notice" id="truncNotice" style="display:none"></div>
      <div id="cardGrid"></div>
      <div class="lastsync-foot" id="cacheFoot"></div>
    </div>`;

  const grid = byId("cardGrid");
  const refreshBtn = byId<HTMLButtonElement>("refreshBtn");

  grid.addEventListener("click", (e) => {
    const card = (e.target as HTMLElement).closest<HTMLElement>(".ev");
    if (card?.dataset.id) opts.onOpenDetail(card.dataset.id);
  });

  refreshBtn.addEventListener("click", async () => {
    refreshBtn.disabled = true;
    refreshBtn.textContent = "Syncing…";
    try {
      await refreshNow();
      await reload();
    } finally {
      refreshBtn.disabled = false;
      refreshBtn.textContent = "Refresh";
    }
  });

  async function reload(): Promise<void> {
    let payload: EventsPayload;
    try {
      payload = await getEvents();
    } catch {
      payload = { events: [], features: {} };
    }
    render(payload);
  }

  function render(payload: EventsPayload): void {
    const { events, features } = payload;
    const upcoming = features["upcoming"];
    const lastFetch = upcoming?.last_fetch_at ?? null;

    // Sync status indicator.
    const syncLabel = byId("syncLabel");
    const syncDot = byId("syncDot");
    if (lastFetch) {
      syncLabel.textContent = `Synced ${relTime(lastFetch)}`;
      syncDot.classList.remove("stale");
    } else {
      syncLabel.textContent = "Not synced yet";
      syncDot.classList.add("stale");
    }

    // Truncation notice (specs/events-overview).
    const trunc = byId("truncNotice");
    if (upcoming?.note === "truncated") {
      trunc.textContent = `Showing top ${events.length} events — list truncated by the API`;
      trunc.style.display = "";
    } else {
      trunc.style.display = "none";
    }

    const foot = byId("cacheFoot");

    if (events.length === 0) {
      grid.className = "";
      grid.innerHTML = lastFetch
        ? `<div class="empty"><b>No upcoming events</b>
             <span>There are no scheduled events in your visible chapters right now.</span></div>`
        : `<div class="empty"><div class="spinner"></div>
             <span>Syncing events from AI Tinkerers…</span></div>`;
      foot.textContent = "";
      return;
    }

    grid.className = "cards";
    grid.innerHTML = events.map(cardHTML).join("");
    foot.textContent = "Rendering from local cache";
  }

  reload();
  return { reload };
}

function cardHTML(ev: EventObj): string {
  const r = ev.rsvps ?? {};
  const attending = num(r.attending);
  const capacity = r.capacity != null ? num(r.capacity) : null;
  const days = num(ev.days_until_event_in_event_timezone);
  const countdown = countdownLabel(days, ev.relative_day_in_event_timezone);

  const gauge =
    capacity && capacity > 0
      ? `<div class="gauge">
           <div class="g-bar"><div class="g-fill" style="width:${Math.min(100, (attending / capacity) * 100)}%"></div></div>
           <div class="g-label"><span><b>${fmt(attending)}</b> / ${fmt(capacity)} capacity</span>
             <span>${Math.round((attending / capacity) * 100)}%</span></div>
         </div>`
      : `<div class="nogauge">No capacity set — raw counts below</div>`;

  const paidBadge = ev.stripe_payment_link_active ? `<span class="badge-paid">Paid</span>` : "";

  return `<button class="ev" data-id="${esc(ev.meetup_token)}">
    <div class="e-top">
      <div>
        <div class="e-city">${esc(ev.city ?? "")}${paidBadge}</div>
        <h3>${esc(ev.event_name)}</h3>
        <div class="e-when">${esc(ev.starts_at_local ?? "")}</div>
      </div>
      <div class="count ${countdown.past ? "past" : ""}"><b>${esc(countdown.big)}</b><small>${esc(countdown.small)}</small></div>
    </div>
    ${gauge}
    <div class="funnel">
      <div><b>${fmt(r.registered)}</b><small>Registered</small></div>
      <div><b>${fmt(r.attending)}</b><small>Attending</small></div>
      <div><b>${fmt(r.waitlisted)}</b><small>Waitlisted</small></div>
      <div><b>${fmt(r.cancelled)}</b><small>Cancelled</small></div>
    </div>
  </button>`;
}

function countdownLabel(days: number, relative?: string): { big: string; small: string; past: boolean } {
  if (relative === "past" || days < 0) return { big: "—", small: "past", past: true };
  if (days === 0 || relative === "today") return { big: "Today", small: "", past: false };
  return { big: `${days}d`, small: "to go", past: false };
}

function relTime(iso: string): string {
  const t = Date.parse(iso);
  if (!Number.isFinite(t)) return "just now";
  const secs = Math.max(0, Math.round((Date.now() - t) / 1000));
  if (secs < 45) return "just now";
  const mins = Math.round(secs / 60);
  if (mins < 60) return `${mins} min ago`;
  const hrs = Math.round(mins / 60);
  if (hrs < 24) return `${hrs} hr ago`;
  return `${Math.round(hrs / 24)} d ago`;
}
