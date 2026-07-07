// Speaker review (specs/speaker-review) — the app's third write-capable
// screen. Reads (talk-proposal pipeline, ranked candidate pool) render only
// from the SQLite cache. Every approve/decline/upsert mutation goes through
// the same two-step prepare/commit confirmation gate as rsvp-screening and
// attendance-checkin: `_prepare` makes no network call and returns a summary
// for the confirm dialog; `_commit` must echo back the identical arguments
// plus the bound token, or the write guardrail rejects it.
import {
  fetchSpeakerCandidates,
  fetchSpeakerProposals,
  getEvents,
  getSpeakerCandidates,
  getSpeakerProposals,
  onSpeakerPipelineUpdated,
  onSpeakerWriteSettled,
  speakerApprovalCommit,
  speakerApprovalPrepare,
  speakerProposalCommit,
  speakerProposalPrepare,
} from "../api";
import type {
  AppErr,
  FeatureState,
  SpeakerCandidate,
  SpeakerCandidatesResult,
  SpeakerConfirm,
  SpeakerPipeline,
  SpeakerProposal,
} from "../types";
import { byId, esc, fmt } from "../util";

interface SpeakersOpts {
  onBack: () => void;
}

export interface SpeakersController {
  open: (meetupToken: string, eventName?: string) => Promise<void>;
}

type DialogKind = "approval" | "upsert";

interface DialogState {
  kind: DialogKind;
  summary: SpeakerConfirm;
  rsvpRef: string;
  toStatus?: string;
  title: string;
  description: string;
  note: string;
  busy: boolean;
  error?: string;
}

const LANE_TITLES: Record<string, string> = {
  proposed: "Proposed",
  under_review: "Under review",
  approved: "Approved",
};

export function mountSpeakers(opts: SpeakersOpts): SpeakersController {
  const root = byId("scr-speakers");

  let meetupToken: string | null = null;
  let eventName = "";
  let pipeline: SpeakerPipeline | null = null;
  let candidates: SpeakerCandidatesResult | null = null;
  let loading = false;
  let pipelineUnavailable: FeatureState | null = null;
  let showDeclined = false;
  let editingRef: string | null = null;
  let newProposalOpen = false;
  let newRsvpRef = "";

  let dialog: DialogState | null = null;
  let toast: string | null = null;

  let unlistenSettled: (() => void) | null = null;
  let unlistenUpdated: (() => void) | null = null;

  async function open(token: string, name?: string): Promise<void> {
    meetupToken = token;
    eventName = name ?? eventName;
    dialog = null;
    toast = null;
    editingRef = null;
    newProposalOpen = false;

    if (!unlistenSettled) {
      unlistenSettled = await onSpeakerWriteSettled((e) => {
        if (e.meetup_token !== meetupToken) return;
        void loadCachedPipeline();
      });
    }
    if (!unlistenUpdated) {
      unlistenUpdated = await onSpeakerPipelineUpdated((mt) => {
        if (mt !== meetupToken) return;
        void loadCachedPipeline();
      });
    }

    await loadCachedPipeline();
    await loadCachedCandidates();
    loading = true;
    paint();
    try {
      await fetchSpeakerProposals(token);
    } catch {
      /* keep cached render; degrade state read below */
    }
    await loadFeatureState();
    await loadCachedPipeline();
    loading = false;
    paint();

    try {
      candidates = await fetchSpeakerCandidates();
    } catch {
      /* keep cached candidates */
    }
    paint();
  }

  async function loadCachedPipeline(): Promise<void> {
    if (!meetupToken) return;
    try {
      const p = await getSpeakerProposals(meetupToken);
      if (meetupToken === p.meetup_token) pipeline = p;
    } catch {
      /* keep prior pipeline */
    }
    paint();
  }

  async function loadCachedCandidates(): Promise<void> {
    try {
      candidates = await getSpeakerCandidates();
    } catch {
      candidates = null;
    }
    paint();
  }

  async function loadFeatureState(): Promise<void> {
    try {
      const payload = await getEvents();
      pipelineUnavailable = payload.features["speaker_pipeline"] ?? null;
    } catch {
      pipelineUnavailable = null;
    }
  }

  // ── approve / decline / move-to-review ──────────────────────────────────

  async function startApproval(row: SpeakerProposal, newStatus: string): Promise<void> {
    try {
      const summary = await speakerApprovalPrepare(row.rsvp_ref, newStatus);
      dialog = {
        kind: "approval",
        summary,
        rsvpRef: row.rsvp_ref,
        toStatus: newStatus,
        title: summary.speaker_title,
        description: summary.speaker_description,
        note: "",
        busy: false,
      };
      paint();
    } catch (e) {
      toast = describeError(e);
      paint();
    }
  }

  // ── create / edit proposal ──────────────────────────────────────────────

  function startEdit(row: SpeakerProposal): void {
    editingRef = row.rsvp_ref;
    paint();
  }

  async function startUpsert(rsvpRef: string, title: string, description: string): Promise<void> {
    try {
      const summary = await speakerProposalPrepare(rsvpRef, title, description);
      dialog = {
        kind: "upsert",
        summary,
        rsvpRef,
        title,
        description,
        note: "",
        busy: false,
      };
      paint();
    } catch (e) {
      toast = describeError(e);
      paint();
    }
  }

  // ── confirm dialog ─────────────────────────────────────────────────────

  async function onDialogCancel(): Promise<void> {
    dialog = null;
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
      if (active.kind === "approval") {
        await speakerApprovalCommit(active.summary.token, mt, active.rsvpRef, active.toStatus!, active.note || undefined);
      } else {
        await speakerProposalCommit(
          active.summary.token,
          mt,
          active.rsvpRef,
          active.title,
          active.description,
          undefined,
          active.note || undefined,
        );
        editingRef = null;
        newProposalOpen = false;
      }
      dialog = null;
      await loadCachedPipeline();
      paint();
    } catch (e) {
      // Aborted (forbidden_*) or rate-limited: no cache change beyond the
      // audit row — surface why, without auto-retrying.
      active.busy = false;
      active.error = describeError(e);
      paint();
    }
  }

  // ── render ───────────────────────────────────────────────────────────────

  function paint(): void {
    root.innerHTML = `
      <div class="appbar">
        <img src="/logos/logo-stacked-760.png" alt="AI Tinkerers" />
        <span class="a-title">Mission Control</span>
        <span class="spacer"></span>
        <button class="refresh" id="spkRefreshBtn">${loading ? "Syncing…" : "Refresh"}</button>
      </div>
      <div class="content">
        <button class="back" id="spkBackBtn">← ${esc(eventName || "Event")}</button>
        <div class="d-head">
          <div>
            <h2>Speaker pipeline</h2>
            <div class="d-meta">${esc(eventName)} · rendering from local cache</div>
          </div>
        </div>
        ${degradeBannerHTML(pipelineUnavailable)}
        ${toast ? `<div class="notice notice-err">${esc(toast)}</div>` : ""}
        ${newProposalHTML()}
        ${kanbanHTML()}
        ${declinedHTML()}
        ${candidatePoolHTML()}
      </div>
      ${dialog ? dialogHTML(dialog) : ""}
    `;
    wire();
  }

  function wire(): void {
    byId<HTMLButtonElement>("spkBackBtn").addEventListener("click", opts.onBack);
    byId<HTMLButtonElement>("spkRefreshBtn").addEventListener("click", async () => {
      if (!meetupToken) return;
      loading = true;
      paint();
      try {
        await fetchSpeakerProposals(meetupToken);
      } catch {
        /* keep cache */
      }
      await loadFeatureState();
      await loadCachedPipeline();
      loading = false;
      paint();
    });

    document.getElementById("spkToggleDeclined")?.addEventListener("click", () => {
      showDeclined = !showDeclined;
      paint();
    });

    document.getElementById("spkNewProposalBtn")?.addEventListener("click", () => {
      newProposalOpen = !newProposalOpen;
      paint();
    });
    document.getElementById("spkNewRsvpRef")?.addEventListener("input", (e) => {
      newRsvpRef = (e.target as HTMLInputElement).value;
    });
    document.getElementById("spkNewProposalForm")?.addEventListener("submit", (e) => {
      e.preventDefault();
      const title = (document.getElementById("spkNewTitle") as HTMLInputElement | null)?.value ?? "";
      const desc = (document.getElementById("spkNewDesc") as HTMLTextAreaElement | null)?.value ?? "";
      if (!newRsvpRef.trim() || !title.trim() || !desc.trim()) return;
      void startUpsert(newRsvpRef.trim(), title.trim(), desc.trim());
    });

    for (const el of document.querySelectorAll<HTMLButtonElement>("[data-approve]")) {
      el.addEventListener("click", () => {
        const ref = el.dataset.ref!;
        const row = findRow(ref);
        if (row) void startApproval(row, el.dataset.approve!);
      });
    }
    for (const el of document.querySelectorAll<HTMLButtonElement>("[data-edit-ref]")) {
      el.addEventListener("click", () => {
        const row = findRow(el.dataset.editRef!);
        if (row) startEdit(row);
      });
    }
    for (const el of document.querySelectorAll<HTMLFormElement>("[data-edit-form]")) {
      el.addEventListener("submit", (e) => {
        e.preventDefault();
        const ref = el.dataset.editForm!;
        const title = (el.querySelector(".spk-edit-title") as HTMLInputElement).value;
        const desc = (el.querySelector(".spk-edit-desc") as HTMLTextAreaElement).value;
        if (!title.trim() || !desc.trim()) return;
        void startUpsert(ref, title.trim(), desc.trim());
      });
    }
    document.getElementById("spkEditCancel")?.addEventListener("click", () => {
      editingRef = null;
      paint();
    });

    document.getElementById("spkCandidatesRefresh")?.addEventListener("click", async () => {
      try {
        candidates = await fetchSpeakerCandidates();
      } catch {
        /* keep cached candidates */
      }
      paint();
    });

    const noteEl = document.getElementById("dlgNote") as HTMLTextAreaElement | null;
    noteEl?.addEventListener("input", () => {
      if (dialog) dialog.note = noteEl.value;
    });
    document.getElementById("dlgCancel")?.addEventListener("click", () => void onDialogCancel());
    document.getElementById("dlgConfirm")?.addEventListener("click", () => void onDialogConfirm());
  }

  function findRow(rsvpRef: string): SpeakerProposal | undefined {
    return pipeline?.rows.find((r) => r.rsvp_ref === rsvpRef);
  }

  function newProposalHTML(): string {
    return `<div class="panel">
      <div class="d-head">
        <h4>New proposal</h4>
        <button class="btn-ghost" id="spkNewProposalBtn">${newProposalOpen ? "Cancel" : "Add proposal on an RSVP"}</button>
      </div>
      ${
        newProposalOpen
          ? `<form id="spkNewProposalForm" class="spk-form">
              <input type="text" id="spkNewRsvpRef" placeholder="RSVP token" value="${esc(newRsvpRef)}" required />
              <input type="text" id="spkNewTitle" placeholder="Talk title" required />
              <textarea id="spkNewDesc" placeholder="Talk description" rows="2" required></textarea>
              <button class="btn" type="submit">Prepare confirmation</button>
            </form>`
          : ""
      }
    </div>`;
  }

  function kanbanHTML(): string {
    if (!pipeline) {
      return `<div class="panel"><div class="empty"><div class="spinner"></div><span>Loading pipeline…</span></div></div>`;
    }
    const lanes = pipeline.lanes;
    const total = lanes.proposed.length + lanes.under_review.length + lanes.approved.length;
    if (!total) {
      return `<div class="panel"><div class="not-enabled">No submitted talk proposals cached for this event yet.</div></div>`;
    }
    return `<div class="kanban-board">
      ${laneColumnHTML("proposed", lanes.proposed)}
      ${laneColumnHTML("under_review", lanes.under_review)}
      ${laneColumnHTML("approved", lanes.approved)}
    </div>`;
  }

  function laneColumnHTML(lane: string, rows: SpeakerProposal[]): string {
    return `<div class="kanban-col panel">
      <div class="kanban-col-head">
        <span>${LANE_TITLES[lane]}</span>
        <span class="b-count">${fmt(rows.length)}</span>
      </div>
      <div class="kanban-col-body">
        ${rows.length ? rows.map((r) => cardHTML(r, lane)).join("") : `<div class="not-enabled">No proposals in this lane.</div>`}
      </div>
    </div>`;
  }

  function cardHTML(r: SpeakerProposal, lane: string): string {
    const isEditing = editingRef === r.rsvp_ref;
    const actions: string[] = [];
    if (lane === "proposed") {
      actions.push(`<button class="btn-ghost" data-approve="pending_review" data-ref="${esc(r.rsvp_ref)}">Move to review</button>`);
      actions.push(`<button class="btn" data-approve="main_stage" data-ref="${esc(r.rsvp_ref)}">Approve</button>`);
      actions.push(`<button class="btn-ghost" data-approve="sidelined" data-ref="${esc(r.rsvp_ref)}">Decline</button>`);
    } else if (lane === "under_review") {
      actions.push(`<button class="btn" data-approve="main_stage" data-ref="${esc(r.rsvp_ref)}">Approve</button>`);
      actions.push(`<button class="btn-ghost" data-approve="sidelined" data-ref="${esc(r.rsvp_ref)}">Decline</button>`);
    } else if (lane === "approved") {
      actions.push(`<button class="btn-ghost" data-approve="sidelined" data-ref="${esc(r.rsvp_ref)}">Decline</button>`);
    }
    actions.push(`<button class="btn-ghost" data-edit-ref="${esc(r.rsvp_ref)}">Edit</button>`);

    return `<div class="kanban-card">
      <div class="rsvp-who">
        <b>${esc(r.name ?? "(no name)")}</b>
        <small>${esc(r.email ?? "")}</small>
        ${r.phone_number ? `<small>${esc(r.phone_number)}</small>` : ""}
      </div>
      <div class="kanban-card-title">${esc(r.speaker_title ?? "(untitled talk)")}</div>
      <p class="page-body">${esc((r.speaker_description ?? "").slice(0, 220))}</p>
      ${
        isEditing
          ? `<form data-edit-form="${esc(r.rsvp_ref)}" class="spk-form">
              <input type="text" class="spk-edit-title" value="${esc(r.speaker_title ?? "")}" required />
              <textarea class="spk-edit-desc" rows="2" required>${esc(r.speaker_description ?? "")}</textarea>
              <div class="confirm-actions">
                <button class="btn-ghost" type="button" id="spkEditCancel">Cancel</button>
                <button class="btn" type="submit">Prepare confirmation</button>
              </div>
            </form>`
          : `<div class="rsvp-actions">${actions.join("")}</div>`
      }
    </div>`;
  }

  function declinedHTML(): string {
    const declined = pipeline?.lanes.declined ?? [];
    if (!declined.length) return "";
    return `<div class="panel">
      <div class="d-head">
        <h4>Declined <span class="b-count">${fmt(declined.length)}</span></h4>
        <button class="btn-ghost" id="spkToggleDeclined">${showDeclined ? "Hide" : "Show"}</button>
      </div>
      ${showDeclined ? `<div class="kanban-declined">${declined.map((r) => declinedRowHTML(r)).join("")}</div>` : ""}
    </div>`;
  }

  function declinedRowHTML(r: SpeakerProposal): string {
    return `<div class="job-row dimmed">
      <div class="job-main">
        <span class="job-subj">${esc(r.name ?? r.rsvp_ref)} — ${esc(r.speaker_title ?? "(untitled talk)")}</span>
      </div>
      <div class="job-counts">
        <button class="btn-ghost" data-approve="pending_review" data-ref="${esc(r.rsvp_ref)}">Move to review</button>
      </div>
    </div>`;
  }

  function candidatePoolHTML(): string {
    const meta = candidates?.meta;
    const list = candidates?.candidates ?? [];
    return `<div class="panel">
      <div class="d-head">
        <h4>Candidate pool <span class="b-count">${fmt(list.length)}</span></h4>
        <button class="btn-ghost" id="spkCandidatesRefresh">Refresh</button>
      </div>
      <p class="groups-note">Ranked future-speaker recommendations — a separate recruiting list, not part of the review funnel above.</p>
      ${meta?.unavailable ? `<div class="notice">Refresh unavailable right now (${esc(meta.reason ?? "unknown")}) — showing last cached candidates.</div>` : ""}
      ${
        list.length
          ? `<div class="job-list">${list.map(candidateRowHTML).join("")}</div>`
          : `<div class="not-enabled">No candidates cached yet.</div>`
      }
    </div>`;
  }

  function candidateRowHTML(c: SpeakerCandidate): string {
    const score = typeof c.speaker_fit_score === "number" ? c.speaker_fit_score.toFixed(0) : "—";
    const angles = (c.recommended_topic_angles ?? []).slice(0, 3).join(", ");
    const whyNow = Array.isArray(c.why_now) ? c.why_now.slice(0, 2).join("; ") : "";
    return `<div class="job-row">
      <div class="job-main">
        <span class="job-chip done">${esc(score)}</span>
        <span class="job-subj">${esc(c.name ?? "(no name)")} · ${esc(c.home_city ?? "")}</span>
      </div>
      <div class="job-counts">
        <span>${esc(c.talk_history_summary ?? "")}</span>
      </div>
      ${angles ? `<div class="job-counts"><span>Angles: ${esc(angles)}</span></div>` : ""}
      ${whyNow ? `<div class="job-counts"><span>Why now: ${esc(whyNow)}</span></div>` : ""}
    </div>`;
  }

  function dialogHTML(d: DialogState): string {
    const title = d.kind === "approval" ? actionVerb(d.toStatus!) : "Save proposal";
    return `<div class="confirm-overlay">
      <div class="confirm-dialog">
        <h3>${title}</h3>
        <p class="confirm-body">
          ${d.kind === "approval" ? `<b>${esc(d.title)}</b> → <b>${esc(d.toStatus ?? "")}</b>` : `<b>${esc(d.title)}</b>`}
        </p>
        <p class="page-body">${esc(d.description.slice(0, 300))}</p>
        <p class="groups-note">No speaker or RSVP notification email will be sent for this change.</p>
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

function actionVerb(status: string): string {
  if (status === "main_stage") return "Approve speaker";
  if (status === "science_fair") return "Approve for science fair";
  if (status === "sidelined") return "Decline proposal";
  if (status === "pending_review") return "Move to review";
  return `Set status to ${status}`;
}

function degradeBannerHTML(fs: FeatureState | null): string {
  if (!fs || !fs.unavailable) return "";
  const note = fs.note ?? "";
  if (note === "forbidden_scope") {
    return `<div class="panel"><div class="not-enabled"><b>Needs city-owner access</b>
      Your key doesn't have city-owner scope for this chapter, so the speaker pipeline can't sync.</div></div>`;
  }
  if (note === "forbidden_role") {
    return `<div class="panel"><div class="not-enabled"><b>Needs a different role</b>
      Your key's role can't read speaker-review data.</div></div>`;
  }
  if (note === "forbidden_api_group") {
    return `<div class="panel"><div class="not-enabled"><b>Not enabled for your chapter</b>
      The RSVPs API group is switched off for this weblog. Cached data (if any) still renders below.</div></div>`;
  }
  return `<div class="panel"><div class="not-enabled">Speaker pipeline unavailable right now — showing cached data.</div></div>`;
}

function describeError(e: unknown): string {
  const err = e as AppErr | { message?: string } | undefined;
  const code = (err as AppErr)?.code;
  const message = (err as { message?: string })?.message;
  if (code === "confirmation_required") {
    return message || "That confirmation is no longer valid — please try again.";
  }
  if (code === "rate_limited") {
    return message || "Rate limited by the AI Tinkerers API — please wait and re-confirm.";
  }
  if (code === "forbidden_scope" || code === "forbidden_role" || code === "forbidden_api_group") {
    return message || "This action was refused by the API for your key's access level.";
  }
  if (code === "not_found") {
    return message || "That proposal isn't cached yet — refresh the pipeline first.";
  }
  return message || "Something went wrong — please try again.";
}
