// RSVP screening (specs/rsvp-screening) — the app's first write-capable
// screen. Reads (attendee list, assessment, history, score) render only from
// the SQLite cache. Every mutation goes through a two-step prepare/commit
// confirmation gate enforced in Rust: `_prepare` makes no network call and
// returns a summary for the confirm dialog; `_commit` must echo back the
// identical arguments plus the bound token, or the write guardrail rejects it.
import {
  fetchRsvpDetail,
  fetchRsvpList,
  getEvents,
  getRsvpDetail,
  getRsvpList,
  getWriteAudit,
  onRsvpListUpdated,
  onRsvpWriteSettled,
  rsvpBulkStateUpdateCommit,
  rsvpBulkStateUpdatePrepare,
  rsvpStateUpdateCommit,
  rsvpStateUpdatePrepare,
} from "../api";
import type {
  AppErr,
  ConfirmSummary,
  FeatureState,
  RsvpDetail,
  RsvpHistoryEvent,
  RsvpRow,
  RsvpState,
  WriteAuditEntry,
} from "../types";
import { byId, esc, fmt } from "../util";

interface ScreeningOpts {
  onBack: () => void;
}

export interface ScreeningController {
  open: (meetupToken: string, eventName?: string) => Promise<void>;
}

/// Mirrors write_guard::BULK_CEILING (src-tauri/src/write_guard.rs) — a
/// selection above this is chunked into separately confirmed batches.
const BULK_CEILING = 100;

type StatusFilter = "all" | "registered" | "attending" | "waitlisted" | "denied";

type DialogKind = "single" | "bulk";

interface DialogState {
  kind: DialogKind;
  summary: ConfirmSummary;
  sendEmail: boolean;
  note: string;
  rsvpRef?: string;
  rsvpRefs?: string[];
  busy: boolean;
  error?: string;
  chunkInfo?: { index: number; total: number };
  names: string[];
}

export function mountScreening(opts: ScreeningOpts): ScreeningController {
  const root = byId("scr-screening");

  let meetupToken: string | null = null;
  let eventName = "";
  let rows: RsvpRow[] = [];
  let loading = false;
  let listUnavailable: FeatureState | null = null;

  let query = "";
  let statusFilter: StatusFilter = "all";
  let selected = new Set<string>();
  let pending = new Set<string>();
  let expanded: string | null = null;
  let details = new Map<string, RsvpDetail>();
  let auditEntries: WriteAuditEntry[] = [];

  let dialog: DialogState | null = null;
  let chunkResolve: ((ok: boolean) => void) | null = null;
  let toast: string | null = null;

  let unlistenSettled: (() => void) | null = null;
  let unlistenListUpdated: (() => void) | null = null;

  async function open(token: string, name?: string): Promise<void> {
    meetupToken = token;
    eventName = name ?? eventName;
    selected = new Set();
    pending = new Set();
    expanded = null;
    details = new Map();
    dialog = null;
    toast = null;

    if (!unlistenSettled) {
      unlistenSettled = await onRsvpWriteSettled((e) => {
        if (e.meetup_token !== meetupToken) return;
        for (const r of e.rsvp_refs) pending.delete(r);
        void loadCached();
      });
    }
    if (!unlistenListUpdated) {
      unlistenListUpdated = await onRsvpListUpdated((mt) => {
        if (mt !== meetupToken) return;
        void loadCached();
      });
    }

    await loadCached();
    await loadAudit();
    loading = true;
    paint();
    try {
      await fetchRsvpList(token);
    } catch {
      /* keep cached render; degrade state read below */
    }
    await loadFeatureState();
    await loadCached();
    loading = false;
    paint();
  }

  async function loadCached(): Promise<void> {
    if (!meetupToken) return;
    try {
      const list = await getRsvpList(meetupToken);
      if (meetupToken === list.meetup_token) rows = list.rows;
    } catch {
      /* keep prior rows */
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

  async function loadAudit(): Promise<void> {
    if (!meetupToken) return;
    try {
      auditEntries = await getWriteAudit(meetupToken, 25);
    } catch {
      auditEntries = [];
    }
  }

  function filteredRows(): RsvpRow[] {
    const q = query.trim().toLowerCase();
    return rows.filter((r) => {
      if (statusFilter !== "all" && r.state !== statusFilter) return false;
      if (!q) return true;
      return (
        (r.name ?? "").toLowerCase().includes(q) || (r.email ?? "").toLowerCase().includes(q)
      );
    });
  }

  // ── single-row actions ─────────────────────────────────────────────────

  async function startSingleAction(row: RsvpRow, newState: RsvpState): Promise<void> {
    try {
      const summary = await rsvpStateUpdatePrepare(row.rsvp_ref, newState, true);
      dialog = {
        kind: "single",
        summary,
        sendEmail: true,
        note: "",
        rsvpRef: row.rsvp_ref,
        busy: false,
        names: [row.name ?? row.rsvp_ref],
      };
      paint();
    } catch (e) {
      toast = describeError(e);
      paint();
    }
  }

  // ── bulk actions ───────────────────────────────────────────────────────

  async function startBulkAction(newState: RsvpState): Promise<void> {
    const refs = Array.from(selected);
    if (!refs.length) return;
    const chunks: string[][] = [];
    for (let i = 0; i < refs.length; i += BULK_CEILING) chunks.push(refs.slice(i, i + BULK_CEILING));
    for (let i = 0; i < chunks.length; i++) {
      const ok = await runOneBulkChunk(chunks[i], newState, i + 1, chunks.length);
      if (!ok) break; // cancelled or failed — do not auto-continue to the next chunk
    }
    selected = new Set();
    paint();
  }

  function runOneBulkChunk(refs: string[], newState: RsvpState, index: number, total: number): Promise<boolean> {
    return new Promise((resolve) => {
      chunkResolve = resolve;
      rsvpBulkStateUpdatePrepare(refs, newState, true)
        .then((summary) => {
          const names = refs.map((r) => rows.find((row) => row.rsvp_ref === r)?.name ?? r);
          dialog = {
            kind: "bulk",
            summary,
            sendEmail: true,
            note: "",
            rsvpRefs: refs,
            busy: false,
            chunkInfo: total > 1 ? { index, total } : undefined,
            names,
          };
          paint();
        })
        .catch((e) => {
          toast = describeError(e);
          paint();
          resolve(false);
        });
    });
  }

  // ── confirm dialog ─────────────────────────────────────────────────────

  async function onDialogSendEmailToggle(v: boolean): Promise<void> {
    if (!dialog) return;
    dialog.sendEmail = v;
    // Re-prepare so the bound token matches the (now different) payload —
    // the guardrail rejects a commit whose payload doesn't match its token.
    try {
      dialog.summary =
        dialog.kind === "single"
          ? await rsvpStateUpdatePrepare(dialog.rsvpRef!, dialog.summary.to_state, v, dialog.note || undefined)
          : await rsvpBulkStateUpdatePrepare(dialog.rsvpRefs!, dialog.summary.to_state, v, dialog.note || undefined);
    } catch (e) {
      dialog.error = describeError(e);
    }
    paint();
  }

  async function onDialogCancel(): Promise<void> {
    dialog = null;
    chunkResolve?.(false);
    chunkResolve = null;
    paint();
  }

  async function onDialogConfirm(): Promise<void> {
    if (!dialog || !meetupToken) return;
    const active = dialog;
    const mt = meetupToken;
    active.busy = true;
    active.error = undefined;
    paint();
    try {
      if (active.kind === "single") {
        pending.add(active.rsvpRef!);
        paint();
        await rsvpStateUpdateCommit(
          active.summary.token,
          mt,
          active.rsvpRef!,
          active.summary.to_state,
          active.sendEmail,
          active.note || undefined,
        );
      } else {
        for (const r of active.rsvpRefs!) pending.add(r);
        paint();
        await rsvpBulkStateUpdateCommit(
          active.summary.token,
          mt,
          active.rsvpRefs!,
          active.summary.to_state,
          active.sendEmail,
          active.note || undefined,
        );
      }
      dialog = null;
      await loadAudit();
      chunkResolve?.(true);
      chunkResolve = null;
      paint();
    } catch (e) {
      // Aborted (forbidden_*) or rate-limited: no cache change beyond the
      // audit row (design D6/D7) — clear the optimistic pending flag and
      // surface why, without auto-retrying.
      if (active.kind === "single") pending.delete(active.rsvpRef!);
      else for (const r of active.rsvpRefs ?? []) pending.delete(r);
      active.busy = false;
      active.error = describeError(e);
      await loadAudit();
      paint();
      chunkResolve?.(false);
      chunkResolve = null;
    }
  }

  // ── row detail (assessment / history / score) ──────────────────────────

  async function toggleDetail(rsvpRef: string): Promise<void> {
    expanded = expanded === rsvpRef ? null : rsvpRef;
    paint();
    if (expanded !== rsvpRef) return;
    try {
      const cached = await getRsvpDetail(rsvpRef);
      if (cached) details.set(rsvpRef, cached);
      paint();
      const fresh = await fetchRsvpDetail(rsvpRef);
      if (fresh && expanded === rsvpRef) {
        details.set(rsvpRef, fresh);
        paint();
      }
    } catch {
      /* keep cached/absent detail */
    }
  }

  // ── render ───────────────────────────────────────────────────────────────

  function paint(): void {
    root.innerHTML = `
      <div class="appbar">
        <img src="/logos/logo-stacked-760.png" alt="AI Tinkerers" />
        <span class="a-title">Mission Control</span>
        <span class="spacer"></span>
        <button class="refresh" id="scrRefreshBtn">${loading ? "Syncing…" : "Refresh"}</button>
      </div>
      <div class="content">
        <button class="back" id="scrBackBtn">← ${esc(eventName || "Event")}</button>
        <div class="d-head">
          <div>
            <h2>Attendee screening</h2>
            <div class="d-meta">${esc(eventName)} · rendering from local cache</div>
          </div>
        </div>
        ${degradeBannerHTML(listUnavailable)}
        ${toast ? `<div class="notice notice-err">${esc(toast)}</div>` : ""}
        ${toolbarHTML()}
        ${listHTML()}
        ${auditPanelHTML(auditEntries)}
      </div>
      ${dialog ? dialogHTML(dialog) : ""}
    `;
    wire();
  }

  function wire(): void {
    byId<HTMLButtonElement>("scrBackBtn").addEventListener("click", opts.onBack);
    byId<HTMLButtonElement>("scrRefreshBtn").addEventListener("click", async () => {
      if (!meetupToken) return;
      loading = true;
      paint();
      try {
        await fetchRsvpList(meetupToken);
      } catch {
        /* keep cache */
      }
      await loadFeatureState();
      await loadCached();
      loading = false;
      paint();
    });

    const searchEl = document.getElementById("scrSearch") as HTMLInputElement | null;
    searchEl?.addEventListener("input", () => {
      query = searchEl.value;
      paint();
    });
    const statusEl = document.getElementById("scrStatusFilter") as HTMLSelectElement | null;
    statusEl?.addEventListener("change", () => {
      statusFilter = statusEl.value as StatusFilter;
      paint();
    });

    const selectAllEl = document.getElementById("scrSelectAll") as HTMLInputElement | null;
    selectAllEl?.addEventListener("change", () => {
      const visible = filteredRows();
      if (selectAllEl.checked) {
        for (const r of visible) selected.add(r.rsvp_ref);
      } else {
        for (const r of visible) selected.delete(r.rsvp_ref);
      }
      paint();
    });

    for (const el of document.querySelectorAll<HTMLInputElement>(".row-select")) {
      el.addEventListener("change", () => {
        const ref = el.dataset.ref!;
        if (el.checked) selected.add(ref);
        else selected.delete(ref);
        paint();
      });
    }

    for (const el of document.querySelectorAll<HTMLElement>(".rsvp-row-main")) {
      el.addEventListener("click", (ev) => {
        if ((ev.target as HTMLElement).closest("button, input")) return;
        void toggleDetail(el.dataset.ref!);
      });
    }

    for (const el of document.querySelectorAll<HTMLButtonElement>("[data-single-action]")) {
      el.addEventListener("click", () => {
        const ref = el.dataset.ref!;
        const row = rows.find((r) => r.rsvp_ref === ref);
        if (row) void startSingleAction(row, el.dataset.singleAction as RsvpState);
      });
    }
    for (const el of document.querySelectorAll<HTMLButtonElement>("[data-bulk-action]")) {
      el.addEventListener("click", () => void startBulkAction(el.dataset.bulkAction as RsvpState));
    }

    const emailToggle = document.getElementById("dlgSendEmail") as HTMLInputElement | null;
    emailToggle?.addEventListener("change", () => void onDialogSendEmailToggle(emailToggle.checked));
    const noteEl = document.getElementById("dlgNote") as HTMLTextAreaElement | null;
    noteEl?.addEventListener("input", () => {
      if (dialog) dialog.note = noteEl.value;
    });
    document.getElementById("dlgCancel")?.addEventListener("click", () => void onDialogCancel());
    document.getElementById("dlgConfirm")?.addEventListener("click", () => void onDialogConfirm());
  }

  function toolbarHTML(): string {
    const selCount = selected.size;
    return `<div class="panel scr-toolbar">
      <div class="scr-filters">
        <input type="text" id="scrSearch" placeholder="Search name or email…" value="${esc(query)}" />
        <select id="scrStatusFilter">
          <option value="all" ${statusFilter === "all" ? "selected" : ""}>All statuses</option>
          <option value="registered" ${statusFilter === "registered" ? "selected" : ""}>Registered</option>
          <option value="attending" ${statusFilter === "attending" ? "selected" : ""}>Attending</option>
          <option value="waitlisted" ${statusFilter === "waitlisted" ? "selected" : ""}>Waitlisted</option>
          <option value="denied" ${statusFilter === "denied" ? "selected" : ""}>Declined</option>
        </select>
      </div>
      <div class="scr-bulkbar">
        <span class="scr-selcount">${selCount ? `${fmt(selCount)} selected` : "No selection"}</span>
        <button class="btn-ghost" data-bulk-action="attending" ${selCount ? "" : "disabled"}>Promote</button>
        <button class="btn-ghost" data-bulk-action="waitlisted" ${selCount ? "" : "disabled"}>Waitlist</button>
        <button class="btn-ghost" data-bulk-action="denied" ${selCount ? "" : "disabled"}>Decline</button>
      </div>
    </div>`;
  }

  function listHTML(): string {
    const visible = filteredRows();
    if (!rows.length && loading) {
      return `<div class="panel"><div class="empty"><div class="spinner"></div><span>Loading attendees…</span></div></div>`;
    }
    if (!visible.length) {
      return `<div class="panel"><div class="not-enabled">No registrants match this search/filter.</div></div>`;
    }
    const allSelected = visible.length > 0 && visible.every((r) => selected.has(r.rsvp_ref));
    return `<div class="panel scr-list">
      <div class="rsvp-row-head">
        <input type="checkbox" id="scrSelectAll" ${allSelected ? "checked" : ""} />
        <span>Registrant</span><span>Status</span><span>Score</span><span></span>
      </div>
      ${visible.map(rowHTML).join("")}
    </div>`;
  }

  function rowHTML(r: RsvpRow): string {
    const isPending = pending.has(r.rsvp_ref);
    const label = r.registrant_status_label ?? r.registrant_status ?? r.state;
    const chipClass = statusChipClass(r.state);
    const scoreText = typeof r.score === "number" ? r.score.toFixed(0) : "—";
    const isExpanded = expanded === r.rsvp_ref;

    const actions = `
      <button class="btn-ghost" data-single-action="attending" data-ref="${esc(r.rsvp_ref)}" ${r.state === "attending" || isPending ? "disabled" : ""}>Promote</button>
      <button class="btn-ghost" data-single-action="waitlisted" data-ref="${esc(r.rsvp_ref)}" ${r.state === "waitlisted" || isPending ? "disabled" : ""}>Waitlist</button>
      <button class="btn-ghost" data-single-action="denied" data-ref="${esc(r.rsvp_ref)}" ${r.state === "denied" || isPending ? "disabled" : ""}>Decline</button>`;

    return `<div class="rsvp-row-item">
      <div class="rsvp-row-main" data-ref="${esc(r.rsvp_ref)}">
        <input type="checkbox" class="row-select" data-ref="${esc(r.rsvp_ref)}" ${selected.has(r.rsvp_ref) ? "checked" : ""} />
        <div class="rsvp-who">
          <b>${esc(r.name ?? "(no name)")}</b>
          <small>${esc(r.email ?? "")}</small>
        </div>
        <div>
          <span class="job-chip ${chipClass}">${esc(label)}</span>
          ${isPending ? `<span class="chip pending-chip">pending…</span>` : ""}
          ${r.checked_in ? `<span class="chip on">checked in</span>` : ""}
        </div>
        <div class="rsvp-score">${esc(scoreText)}</div>
        <div class="rsvp-actions">${actions}</div>
      </div>
      ${isExpanded ? detailHTML(r.rsvp_ref) : ""}
    </div>`;
  }

  function detailHTML(rsvpRef: string): string {
    const d = details.get(rsvpRef);
    if (!d) {
      return `<div class="rsvp-detail"><div class="empty"><div class="spinner"></div></div></div>`;
    }
    return `<div class="rsvp-detail">
      <div class="sf-grid">
        ${detailSectionHTML("AI assessment", d.assessment_status, assessmentBodyHTML(d.assessment))}
        ${detailSectionHTML("Status history", d.history_status, historyBodyHTML(d.history?.events))}
        ${detailSectionHTML("Engagement score", d.score_status, scoreBodyHTML(d.score))}
      </div>
    </div>`;
  }

  function detailSectionHTML(title: string, status: string, body: string): string {
    if (status !== "ok") {
      return `<div class="sf-section"><div class="ep-title">${esc(title)}</div>${notAvailableHTML(status)}</div>`;
    }
    return `<div class="sf-section"><div class="ep-title">${esc(title)}</div>${body}</div>`;
  }

  function assessmentBodyHTML(a?: Record<string, unknown> | null): string {
    if (!a) return `<div class="not-enabled">No assessment cached yet.</div>`;
    const summary = (a.summary ?? a.assessment_summary ?? a.notes) as string | undefined;
    const risk = (a.risk_level ?? a.risk) as string | undefined;
    return `${risk ? `<span class="chip">${esc(String(risk))}</span>` : ""}
      <p class="page-body">${esc(typeof summary === "string" ? summary : JSON.stringify(a).slice(0, 500))}</p>`;
  }

  function historyBodyHTML(events?: RsvpHistoryEvent[]): string {
    if (!events || !events.length) return `<div class="not-enabled">No status history yet.</div>`;
    return `<div class="job-list">${events
      .slice(0, 10)
      .map(
        (e) => `<div class="job-row">
          <div class="job-main">
            <span class="job-subj">${esc(e.from_status ?? "—")} → ${esc(e.to_status ?? "—")}</span>
          </div>
          <div class="job-counts">
            <span>${esc(e.changed_at ?? "")}</span>
            <span>${esc(e.actor_name ?? e.actor_type ?? "system")}</span>
          </div>
        </div>`,
      )
      .join("")}</div>`;
  }

  function scoreBodyHTML(s?: Record<string, unknown> | null): string {
    if (!s) return `<div class="not-enabled">No score breakdown cached yet.</div>`;
    const total = s.total_score ?? s.score;
    return `${typeof total === "number" ? `<div class="stat-tile"><b>${esc(String(total))}</b><small>total score</small></div>` : ""}
      <p class="page-body">${esc(JSON.stringify(s).slice(0, 500))}</p>`;
  }

  function auditPanelHTML(entries: WriteAuditEntry[]): string {
    if (!entries.length) return "";
    return `<div class="panel">
      <h4>Recent write attempts <span class="b-count">${fmt(entries.length)}</span></h4>
      <div class="job-list">${entries.map(auditRowHTML).join("")}</div>
      <p class="groups-note">Client-side record of every attempted mutation — the server's own status history is the authoritative record of effect.</p>
    </div>`;
  }

  function auditRowHTML(e: WriteAuditEntry): string {
    const cls = e.outcome === "ok" ? "done" : e.outcome === "attempted" ? "sending" : "failed";
    return `<div class="job-row">
      <div class="job-main">
        <span class="job-chip ${cls}">${esc(e.outcome)}</span>
        <span class="job-subj">${esc(e.action)} · ${fmt(e.targets.length)} target(s)${e.to_state ? ` → ${esc(e.to_state)}` : ""}</span>
      </div>
      <div class="job-counts">
        <span>${esc(e.created_at)}</span>
        ${e.send_email ? "<span>email sent</span>" : "<span>no email</span>"}
      </div>
    </div>`;
  }

  function dialogHTML(d: DialogState): string {
    const count = d.summary.count;
    const chunkNote = d.chunkInfo ? `<div class="notice">Chunk ${d.chunkInfo.index} of ${d.chunkInfo.total}</div>` : "";
    const namesPreview =
      d.names.length > 1
        ? `<div class="confirm-names">${d.names
            .slice(0, 8)
            .map((n) => esc(n))
            .join(", ")}${d.names.length > 8 ? ` and ${d.names.length - 8} more` : ""}</div>`
        : "";
    return `<div class="confirm-overlay">
      <div class="confirm-dialog">
        <h3>${d.kind === "bulk" ? `Bulk ${actionVerb(d.summary.to_state)}` : actionVerb(d.summary.to_state)}</h3>
        ${chunkNote}
        <p class="confirm-body">
          ${count === 1 ? "1 registrant" : `${fmt(count)} registrants`}
          ${d.summary.from_state ? ` from <b>${esc(d.summary.from_state)}</b>` : ""}
          → <b>${esc(d.summary.to_state)}</b>
          ${d.summary.registrant_status_label ? ` (shown to them as “${esc(d.summary.registrant_status_label)}”)` : ""}
        </p>
        ${namesPreview}
        <label class="confirm-toggle">
          <input type="checkbox" id="dlgSendEmail" ${d.sendEmail ? "checked" : ""} />
          Send the standard status-change email
        </label>
        <textarea id="dlgNote" placeholder="Optional internal note…" rows="2">${esc(d.note)}</textarea>
        ${d.error ? `<div class="notice notice-err">${esc(d.error)}</div>` : ""}
        <div class="confirm-actions">
          <button class="btn-ghost" id="dlgCancel" ${d.busy ? "disabled" : ""}>Cancel</button>
          <button class="btn" id="dlgConfirm" ${d.busy ? "disabled" : ""}>${d.busy ? "Confirming…" : "Confirm"}</button>
        </div>
      </div>
    </div>`;
  }

  return { open };
}

function actionVerb(state: string): string {
  if (state === "attending") return "Promote to attending";
  if (state === "waitlisted") return "Move to waitlist";
  if (state === "denied") return "Decline";
  return `Set state to ${state}`;
}

function statusChipClass(state: string): string {
  if (state === "attending") return "done";
  if (state === "waitlisted") return "sending";
  if (state === "denied") return "failed";
  return "idle";
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
      Your key's role can't read RSVP screening data.</div></div>`;
  }
  if (note === "forbidden_api_group") {
    return `<div class="panel"><div class="not-enabled"><b>Not enabled for your chapter</b>
      The RSVPs API group is switched off for this weblog. Cached data (if any) still renders below.</div></div>`;
  }
  return `<div class="panel"><div class="not-enabled">Attendee list unavailable right now — showing cached data.</div></div>`;
}

function notAvailableHTML(status: string): string {
  if (status === "forbidden_scope") return `<div class="not-enabled">Needs city-owner access.</div>`;
  if (status === "forbidden_role") return `<div class="not-enabled">Needs a different role.</div>`;
  if (status === "forbidden_api_group") return `<div class="not-enabled">Not enabled for your chapter.</div>`;
  return `<div class="not-enabled">Not available yet.</div>`;
}

function describeError(e: unknown): string {
  const err = e as AppErr | { message?: string } | undefined;
  const code = (err as AppErr)?.code;
  const message = (err as { message?: string })?.message;
  if (code === "confirmation_required") {
    return message || "That confirmation is no longer valid — please try again.";
  }
  if (code === "ceiling_exceeded") {
    return message || "Selection too large for one call — it will be split into smaller confirmed batches.";
  }
  if (code === "rate_limited") {
    return message || "Rate limited by the AI Tinkerers API — please wait and re-confirm.";
  }
  if (code === "forbidden_scope" || code === "forbidden_role" || code === "forbidden_api_group") {
    return message || "This action was refused by the API for your key's access level.";
  }
  return message || "Something went wrong — please try again.";
}
