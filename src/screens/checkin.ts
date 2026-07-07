// Attendance check-in (specs/attendance-checkin) — the at-the-door screen for
// the live/next event. Speed and offline tolerance are the two priorities
// (design): a tap enqueues to a durable local queue and reflects checked-in
// immediately, whether or not the network is up; the actual `mark_attended`
// POST happens later on a sync flush. The write still goes through the same
// write_guard prepare/commit handshake as RSVP screening — just without a
// blocking dialog, since a per-tap modal would defeat door speed (design D2).
import {
  checkinCommit,
  checkinPrepare,
  fetchCheckinAttendees,
  getCheckinAttendees,
  getCheckinCount,
  getCheckinDenials,
  getEvents,
  onCheckinQueueUpdated,
  onRsvpListUpdated,
} from "../api";
import type { CheckinCount, CheckinDenial, FeatureState, RsvpRow } from "../types";
import { byId, esc, fmt } from "../util";

interface CheckinOpts {
  onBack: () => void;
}

export interface CheckinController {
  /** Opens the live/next event's check-in screen, or an explicit event when given. */
  open: (meetupToken?: string, eventName?: string) => Promise<void>;
}

export function mountCheckin(opts: CheckinOpts): CheckinController {
  const root = byId("scr-checkin");

  let meetupToken: string | null = null;
  let eventName = "";
  let rows: RsvpRow[] = [];
  let pendingRefs = new Set<string>();
  let denials = new Map<string, string | null | undefined>();
  let count: CheckinCount | null = null;
  let listUnavailable: FeatureState | null = null;
  let query = "";
  let loading = false;
  let inFlight = new Set<string>();
  let toast: string | null = null;

  let unlistenQueue: (() => void) | null = null;
  let unlistenListUpdated: (() => void) | null = null;

  async function open(token?: string, name?: string): Promise<void> {
    eventName = name ?? eventName;
    toast = null;

    if (!unlistenQueue) {
      unlistenQueue = await onCheckinQueueUpdated((e) => {
        if (e.meetup_token !== meetupToken) return;
        void loadCached();
      });
    }
    if (!unlistenListUpdated) {
      unlistenListUpdated = await onRsvpListUpdated((mt) => {
        if (mt !== meetupToken) return;
        void loadCached();
      });
    }

    loading = true;
    paint();
    try {
      const fresh = await fetchCheckinAttendees(token);
      meetupToken = fresh.meetup_token;
    } catch {
      /* keep cached render below */
    }
    await loadFeatureState();
    await loadCached();
    loading = false;
    paint();
  }

  async function loadCached(): Promise<void> {
    try {
      const list = await getCheckinAttendees(meetupToken ?? undefined);
      meetupToken = list.meetup_token ?? meetupToken;
      rows = list.rows;
      pendingRefs = new Set(list.pending_refs);
    } catch {
      /* keep prior rows */
    }
    try {
      count = await getCheckinCount(meetupToken ?? undefined);
    } catch {
      count = null;
    }
    try {
      const d = await getCheckinDenials(meetupToken ?? undefined);
      denials = new Map(d.map((x: CheckinDenial) => [x.rsvp_ref, x.error_code]));
    } catch {
      denials = new Map();
    }
    paint();
  }

  async function loadFeatureState(): Promise<void> {
    try {
      const payload = await getEvents();
      listUnavailable = payload.features["rsvp_screening"] ?? null;
    } catch {
      listUnavailable = null;
    }
  }

  function filteredRows(): RsvpRow[] {
    const q = query.trim().toLowerCase();
    if (!q) return rows;
    return rows.filter(
      (r) => (r.name ?? "").toLowerCase().includes(q) || (r.email ?? "").toLowerCase().includes(q),
    );
  }

  // Any denial anywhere in this event is a scope/role/group-level hard deny
  // (design D7) — disable check-in for the whole screen, not just that row.
  function eventBlockedCode(): string | null {
    for (const code of denials.values()) {
      if (code) return code;
    }
    return null;
  }

  async function checkIn(row: RsvpRow): Promise<void> {
    if (!meetupToken || row.checked_in || inFlight.has(row.rsvp_ref) || eventBlockedCode()) return;
    inFlight.add(row.rsvp_ref);
    // Optimistic — reflect immediately, before any network round trip
    // (design D3: speed + offline tolerance are the whole point of this screen).
    row.checked_in = true;
    pendingRefs.add(row.rsvp_ref);
    paint();
    try {
      const confirm = await checkinPrepare(row.rsvp_ref, meetupToken);
      await checkinCommit(confirm.token, row.rsvp_ref, meetupToken);
      count = await getCheckinCount(meetupToken).catch(() => count);
    } catch (e) {
      // The prepare/commit round trip itself only fails on a guardrail
      // rejection (stale token) or an already-checked-in/queued no-op — a
      // real forbidden_* deny surfaces later via the queue (loadCached()).
      toast = describeError(e);
    } finally {
      inFlight.delete(row.rsvp_ref);
      paint();
    }
  }

  function paint(): void {
    const blocked = eventBlockedCode();
    root.innerHTML = `
      <div class="appbar">
        <img src="/logos/logo-stacked-760.png" alt="AI Tinkerers" />
        <span class="a-title">Mission Control</span>
        <span class="spacer"></span>
        <button class="refresh" id="chkRefreshBtn">${loading ? "Syncing…" : "Refresh"}</button>
      </div>
      <div class="content">
        <button class="back" id="chkBackBtn">← ${esc(eventName || "Event")}</button>
        <div class="d-head">
          <div>
            <h2>Check in</h2>
            <div class="d-meta">${esc(eventName)} · rendering from local cache</div>
          </div>
        </div>
        ${degradeBannerHTML(listUnavailable)}
        ${blocked ? forbiddenBannerHTML(blocked) : ""}
        ${toast ? `<div class="notice notice-err">${esc(toast)}</div>` : ""}
        ${progressHTML(count)}
        ${toolbarHTML()}
        ${listHTML(blocked)}
      </div>
    `;
    wire();
  }

  function wire(): void {
    byId<HTMLButtonElement>("chkBackBtn").addEventListener("click", opts.onBack);
    byId<HTMLButtonElement>("chkRefreshBtn").addEventListener("click", async () => {
      loading = true;
      paint();
      try {
        const fresh = await fetchCheckinAttendees(meetupToken ?? undefined);
        meetupToken = fresh.meetup_token ?? meetupToken;
      } catch {
        /* keep cache */
      }
      await loadCached();
      loading = false;
      paint();
    });

    const searchEl = document.getElementById("chkSearch") as HTMLInputElement | null;
    searchEl?.addEventListener("input", () => {
      query = searchEl.value;
      paint();
    });

    for (const el of document.querySelectorAll<HTMLButtonElement>("[data-checkin-ref]")) {
      el.addEventListener("click", () => {
        const ref = el.dataset.checkinRef!;
        const row = rows.find((r) => r.rsvp_ref === ref);
        if (row) void checkIn(row);
      });
    }
  }

  function toolbarHTML(): string {
    return `<div class="panel scr-toolbar">
      <div class="scr-filters">
        <input type="text" id="chkSearch" placeholder="Search name or email…" value="${esc(query)}" />
      </div>
    </div>`;
  }

  function listHTML(blocked: string | null): string {
    const visible = filteredRows();
    if (!rows.length && loading) {
      return `<div class="panel"><div class="empty"><div class="spinner"></div><span>Loading attendees…</span></div></div>`;
    }
    if (!meetupToken) {
      return `<div class="panel"><div class="not-enabled">No upcoming event to check attendees in for.</div></div>`;
    }
    if (!visible.length) {
      return `<div class="panel"><div class="not-enabled">No registrants match this search.</div></div>`;
    }
    return `<div class="panel scr-list">
      <div class="rsvp-row-head checkin-row-head">
        <span>Registrant</span><span>Status</span><span></span>
      </div>
      ${visible.map((r) => rowHTML(r, blocked)).join("")}
    </div>`;
  }

  function rowHTML(r: RsvpRow, blocked: string | null): string {
    const label = r.registrant_status_label ?? r.registrant_status ?? r.state;
    const busy = inFlight.has(r.rsvp_ref);
    const isPendingSync = pendingRefs.has(r.rsvp_ref) && r.checked_in;
    const denialCode = denials.get(r.rsvp_ref);

    let stateChip: string;
    let action: string;
    if (r.checked_in) {
      stateChip = isPendingSync
        ? `<span class="chip pending-chip">checked in · syncing…</span>`
        : `<span class="chip on">checked in${r.checked_in_at ? ` · ${esc(shortTime(r.checked_in_at))}` : ""}</span>`;
      action = `<button class="btn-ghost" disabled>Checked in</button>`;
    } else if (denialCode) {
      stateChip = `<span class="job-chip failed">not permitted</span>`;
      action = `<button class="btn-ghost" disabled>Blocked</button>`;
    } else {
      stateChip = "";
      action = `<button class="btn checkin-tap" data-checkin-ref="${esc(r.rsvp_ref)}" ${busy || blocked ? "disabled" : ""}>${busy ? "Checking in…" : "Check in"}</button>`;
    }

    return `<div class="rsvp-row-item">
      <div class="rsvp-row-main checkin-row-main">
        <div class="rsvp-who">
          <b>${esc(r.name ?? "(no name)")}</b>
          <small>${esc(r.email ?? "")}</small>
        </div>
        <div>
          <span class="job-chip idle">${esc(label)}</span>
          ${stateChip}
        </div>
        <div class="rsvp-actions">${action}</div>
      </div>
    </div>`;
  }

  function progressHTML(c: CheckinCount | null): string {
    if (!c || !meetupToken) return "";
    const total = Math.max(c.attending, c.checked_in, 1);
    const pct = Math.min(100, (c.checked_in / total) * 100);
    const pendingNote = c.pending > 0 ? ` <span class="pending-chip chip">${fmt(c.pending)} syncing</span>` : "";
    return `<div class="panel checkin-progress">
      <h4>Checked in <span class="b-count live">${fmt(c.checked_in)} / ${fmt(c.attending)}</span>${pendingNote}</h4>
      <div class="gauge">
        <div class="g-bar"><div class="g-fill" style="width:${pct}%"></div></div>
        <div class="g-label"><span><b>${fmt(c.checked_in)}</b> / ${fmt(c.attending)} attending</span>
          <span>${Math.round(pct)}%</span></div>
      </div>
    </div>`;
  }

  return { open };
}

function forbiddenBannerHTML(code: string): string {
  if (code === "forbidden_scope") {
    return `<div class="panel"><div class="not-enabled"><b>Can't check in — not permitted for your scope</b>
      Your key doesn't have city-owner scope for this chapter. Check-in is disabled for this event.</div></div>`;
  }
  if (code === "forbidden_role") {
    return `<div class="panel"><div class="not-enabled"><b>Can't check in — not permitted for your role</b>
      Your key's role can't record attendance. Check-in is disabled for this event.</div></div>`;
  }
  if (code === "forbidden_api_group") {
    return `<div class="panel"><div class="not-enabled"><b>Can't check in — not enabled for your chapter</b>
      The RSVPs API group is switched off for this weblog. Check-in is disabled for this event.</div></div>`;
  }
  return `<div class="panel"><div class="not-enabled"><b>Can't check in</b>
    Check-in was refused by the API for your key's access level and won't be retried automatically.</div></div>`;
}

function degradeBannerHTML(fs: FeatureState | null): string {
  if (!fs || !fs.unavailable) return "";
  const note = fs.note ?? "";
  if (note === "forbidden_scope") {
    return `<div class="panel"><div class="not-enabled"><b>Needs city-owner access</b>
      Your key doesn't have city-owner scope for this chapter, so the attendee list can't sync.</div></div>`;
  }
  if (note === "forbidden_role") {
    return `<div class="panel"><div class="not-enabled"><b>Needs a different role</b>
      Your key's role can't read RSVP data.</div></div>`;
  }
  if (note === "forbidden_api_group") {
    return `<div class="panel"><div class="not-enabled"><b>Not enabled for your chapter</b>
      The RSVPs API group is switched off for this weblog. Cached data (if any) still renders below.</div></div>`;
  }
  return `<div class="panel"><div class="not-enabled">Attendee list unavailable right now — showing cached data.</div></div>`;
}

function shortTime(iso: string): string {
  const t = Date.parse(iso);
  if (!Number.isFinite(t)) return iso;
  return new Date(t).toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" });
}

function describeError(e: unknown): string {
  const err = e as { code?: string; message?: string } | undefined;
  const code = err?.code;
  const message = err?.message;
  if (code === "confirmation_required") {
    return message || "That confirmation is no longer valid — please tap check in again.";
  }
  if (code === "rate_limited") {
    return message || "Rate limited by the AI Tinkerers API — the check-in is queued and will sync once it clears.";
  }
  if (code === "forbidden_scope" || code === "forbidden_role" || code === "forbidden_api_group") {
    return message || "This action was refused by the API for your key's access level.";
  }
  return message || "Something went wrong — please try again.";
}
