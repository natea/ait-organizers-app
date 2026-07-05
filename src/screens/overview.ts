// Events overview: cards rendered from the SQLite cache, split into Upcoming
// and Past tabs (specs/events-overview, specs/past-events).
import { getEvents, refreshNow } from "../api";
import type { EventKind, EventObj, EventsPayload, FeatureState } from "../types";
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
      <div class="seg" role="tablist">
        <button class="on" id="tabUpcoming" role="tab">Upcoming</button>
        <button id="tabPast" role="tab">Past</button>
      </div>
      <div class="notice" id="truncNotice" style="display:none"></div>
      <div id="cardGrid"></div>
      <div class="lastsync-foot" id="cacheFoot"></div>
    </div>`;

  const grid = byId("cardGrid");
  const refreshBtn = byId<HTMLButtonElement>("refreshBtn");
  let payload: EventsPayload = { events: [], features: {} };
  let listTab: EventKind = "upcoming";

  grid.addEventListener("click", (e) => {
    const card = (e.target as HTMLElement).closest<HTMLElement>(".ev");
    if (card?.dataset.id) opts.onOpenDetail(card.dataset.id);
  });

  byId("tabUpcoming").addEventListener("click", () => setTab("upcoming"));
  byId("tabPast").addEventListener("click", () => setTab("past"));

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

  function setTab(tab: EventKind): void {
    listTab = tab;
    render();
  }

  async function reload(): Promise<void> {
    try {
      payload = await getEvents();
    } catch {
      payload = { events: [], features: {} };
    }
    render();
  }

  function render(): void {
    const { events, features } = payload;
    const past = listTab === "past";
    const feat: FeatureState | undefined = features[past ? "past" : "upcoming"];
    const lastFetch = feat?.last_fetch_at ?? null;
    const tabEvents = events.filter((e) => (e.kind ?? "upcoming") === listTab);

    byId("tabUpcoming").classList.toggle("on", !past);
    byId("tabPast").classList.toggle("on", past);

    // Sync indicator tracks the live (upcoming) feed regardless of active tab.
    const upcomingFetch = features["upcoming"]?.last_fetch_at ?? null;
    const syncLabel = byId("syncLabel");
    const syncDot = byId("syncDot");
    if (upcomingFetch) {
      syncLabel.textContent = `Synced ${relTime(upcomingFetch)}`;
      syncDot.classList.remove("stale");
    } else {
      syncLabel.textContent = "Not synced yet";
      syncDot.classList.add("stale");
    }

    // Notice line differs per tab.
    const notice = byId("truncNotice");
    if (past) {
      notice.textContent =
        feat?.note === "truncated"
          ? `Recent past events — top ${tabEvents.length} shown, recap data frozen at last sync`
          : "Recent past events — recap data frozen at last sync";
      notice.style.display = tabEvents.length ? "" : "none";
    } else if (feat?.note === "truncated") {
      notice.textContent = `Showing top ${tabEvents.length} events — list truncated by the API`;
      notice.style.display = "";
    } else {
      notice.style.display = "none";
    }

    const foot = byId("cacheFoot");

    if (tabEvents.length === 0) {
      grid.className = "";
      grid.innerHTML = emptyState(past, lastFetch);
      foot.textContent = "";
      return;
    }

    grid.className = "cards";
    grid.innerHTML = tabEvents.map(cardHTML).join("");
    foot.textContent = past
      ? "Rendering from local cache · concluded events are not polled"
      : "Rendering from local cache";
  }

  reload();
  return { reload };
}

function emptyState(past: boolean, lastFetch: string | null): string {
  if (!lastFetch) {
    return `<div class="empty"><div class="spinner"></div>
      <span>${past ? "Loading past events…" : "Syncing events from AI Tinkerers…"}</span></div>`;
  }
  return past
    ? `<div class="empty"><b>No past events</b>
        <span>No completed events in the recap window for your visible chapters.</span></div>`
    : `<div class="empty"><b>No upcoming events</b>
        <span>There are no scheduled events in your visible chapters right now.</span></div>`;
}

function cardHTML(ev: EventObj): string {
  return (ev.kind ?? "upcoming") === "past" ? pastCardHTML(ev) : upcomingCardHTML(ev);
}

function upcomingCardHTML(ev: EventObj): string {
  const r = ev.rsvps ?? {};
  const attending = num(r.attending);
  const capacity = r.capacity != null ? num(r.capacity) : null;
  const days = num(ev.days_until_event_in_event_timezone);
  const countdown = countdownLabel(days, ev.relative_day_in_event_timezone);

  const gauge =
    capacity && capacity > 0
      ? gaugeHTML(attending, capacity, "capacity")
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
    ${funnelHTML(r)}
  </button>`;
}

// The API's `registered` field is 0 for these events, so the funnel shows a
// computed Total (attending + waitlisted + cancelled) instead.
function funnelHTML(r: EventObj["rsvps"] = {}): string {
  const total = num(r.attending) + num(r.waitlisted) + num(r.cancelled);
  return `<div class="funnel">
    <div><b>${fmt(total)}</b><small>Total</small></div>
    <div><b>${fmt(r.attending)}</b><small>Attending</small></div>
    <div><b>${fmt(r.waitlisted)}</b><small>Waitlisted</small></div>
    <div><b>${fmt(r.cancelled)}</b><small>Cancelled</small></div>
  </div>`;
}

// Past card: "held" chip instead of a countdown, and an Attended cell in place
// of Waitlisted. The precise check-in count is unavailable on the list payload,
// so this falls back to the final attending count (the true check-in figure is
// surfaced on the detail view via performance).
function pastCardHTML(ev: EventObj): string {
  const r = ev.rsvps ?? {};
  const attending = num(r.attending);
  const capacity = r.capacity != null ? num(r.capacity) : null;
  const paidBadge = ev.stripe_payment_link_active ? `<span class="badge-paid">Paid</span>` : "";

  const gauge =
    capacity && capacity > 0
      ? gaugeHTML(attending, capacity, "attending")
      : `<div class="nogauge">No capacity set — final counts below</div>`;

  return `<button class="ev" data-id="${esc(ev.meetup_token)}">
    <div class="e-top">
      <div>
        <div class="e-city">${esc(ev.city ?? "")}${paidBadge}</div>
        <h3>${esc(ev.event_name)}</h3>
        <div class="e-when">${esc(ev.starts_at_local ?? "")}</div>
      </div>
      <div class="count held"><b>${esc(heldLabel(ev))}</b><small>held</small></div>
    </div>
    ${gauge}
    ${funnelHTML(r)}
  </button>`;
}

function gaugeHTML(value: number, capacity: number, label: string): string {
  const pct = Math.min(100, (value / capacity) * 100);
  return `<div class="gauge">
    <div class="g-bar"><div class="g-fill" style="width:${pct}%"></div></div>
    <div class="g-label"><span><b>${fmt(value)}</b> / ${fmt(capacity)} ${esc(label)}</span>
      <span>${Math.round((value / capacity) * 100)}%</span></div>
  </div>`;
}

function countdownLabel(days: number, relative?: string): { big: string; small: string; past: boolean } {
  if (relative === "past" || days < 0) return { big: "—", small: "past", past: true };
  if (days === 0 || relative === "today") return { big: "Today", small: "", past: false };
  return { big: `${days}d`, small: "to go", past: false };
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
