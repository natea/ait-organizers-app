// Sponsor tools (specs/sponsor-tools): search sponsors, view a sponsor's
// contacts (masking rendered exactly as the API returns it — never unmasked),
// and generate an AI research brief or a tailored pitch. Generation is
// user-initiated only (never the poll loop), runs as a tracked background job
// with progress reported via `sponsor_draft_progress` events, and drafts are
// cached per subject/kind so reopening one never re-spends a slow (~20s),
// rate-limited (10 rpm) call.
import {
  getEvents,
  getSponsorContacts,
  getSponsorDrafts,
  onSponsorDraftProgress,
  sponsorContactsGet,
  sponsorGenerate,
  sponsorGenerationCancel,
  sponsorSearch,
} from "../api";
import type {
  EventObj,
  SponsorContact,
  SponsorContactsResult,
  SponsorDraft,
  SponsorDraftKind,
  SponsorDraftProgressEvent,
  SponsorJobStatus,
  SponsorMatch,
  SponsorSearchResult,
} from "../types";
import { byId, esc } from "../util";
import { relTime } from "./email";

interface SponsorsOpts {
  onBack: () => void;
}

export interface SponsorsController {
  open: () => Promise<void>;
}

interface JobState {
  jobId?: string;
  status?: SponsorJobStatus;
  errorCode?: string | null;
}

const CHANNELS = ["email", "linkedin", "call", "in_person"];

export function mountSponsors(opts: SponsorsOpts): SponsorsController {
  const root = byId("scr-sponsors");

  // Search state.
  let query = "";
  let cityFilter = "";
  let industryFilter = "";
  let activeOnly = false;
  let searching = false;
  let searchResult: SponsorSearchResult | null = null;

  // Selection: either an existing sponsor, or a free-text company name.
  let selectedSponsor: SponsorMatch | null = null;
  let freeformName = "";
  let contacts: SponsorContactsResult | null = null;
  let contactsLoading = false;

  // Generation inputs.
  let events: EventObj[] = [];
  let meetupToken = "";
  let channel = "email";
  let targetAudience = "";
  let notes = "";

  const jobs: Record<SponsorDraftKind, JobState> = { research: {}, pitch: {} };
  const drafts: Record<SponsorDraftKind, SponsorDraft[]> = { research: [], pitch: [] };
  let unlisten: (() => void) | null = null;

  function subject(): { sponsorRef?: string; name?: string } {
    return selectedSponsor
      ? { sponsorRef: selectedSponsor.sponsor_token }
      : { name: freeformName.trim() };
  }

  function hasSubject(): boolean {
    return !!selectedSponsor || freeformName.trim().length > 0;
  }

  async function open(): Promise<void> {
    if (!unlisten) {
      unlisten = await onSponsorDraftProgress(onProgress);
    }
    if (!events.length) {
      try {
        const payload = await getEvents();
        events = payload.events;
      } catch {
        events = [];
      }
    }
    paint();
  }

  function onProgress(e: SponsorDraftProgressEvent): void {
    if (!hasSubject()) return;
    const subj = subject();
    const expected = subj.sponsorRef ? `token:${subj.sponsorRef}` : `name:${(subj.name ?? "").toLowerCase()}`;
    if (e.subject !== expected) return;
    if (e.status === "cancelled") {
      jobs[e.kind] = {};
    } else {
      jobs[e.kind] = { jobId: e.job_id, status: e.status, errorCode: e.error_code };
    }
    if (e.status === "ready") {
      void loadDrafts(e.kind);
    } else {
      paint();
    }
  }

  async function runSearch(): Promise<void> {
    const q = query.trim();
    if (!q) return;
    searching = true;
    paint();
    try {
      searchResult = await sponsorSearch(q, cityFilter.trim() || undefined, industryFilter.trim() || undefined, activeOnly);
    } catch {
      searchResult = { results: [], truncated: false, unavailable: true, reason: "unavailable" };
    } finally {
      searching = false;
      paint();
    }
  }

  async function selectSponsor(m: SponsorMatch): Promise<void> {
    selectedSponsor = m;
    freeformName = "";
    contacts = null;
    drafts.research = [];
    drafts.pitch = [];
    jobs.research = {};
    jobs.pitch = {};
    paint();
    await loadContacts(m.sponsor_token);
    await loadDrafts("research");
    await loadDrafts("pitch");
  }

  function selectFreeform(): void {
    selectedSponsor = null;
    contacts = null;
    drafts.research = [];
    drafts.pitch = [];
    jobs.research = {};
    jobs.pitch = {};
    paint();
  }

  async function loadContacts(sponsorToken: string): Promise<void> {
    contactsLoading = true;
    paint();
    try {
      contacts = await sponsorContactsGet(sponsorToken);
    } catch {
      try {
        contacts = await getSponsorContacts(sponsorToken);
      } catch {
        contacts = null;
      }
    } finally {
      contactsLoading = false;
      paint();
    }
  }

  async function loadDrafts(kind: SponsorDraftKind): Promise<void> {
    if (!hasSubject()) return;
    const subj = subject();
    try {
      const d = await getSponsorDrafts(subj, kind);
      // Guard against a stale response landing after the selection changed.
      if (subject().sponsorRef === subj.sponsorRef && subject().name === subj.name) {
        drafts[kind] = d;
      }
    } catch {
      /* keep prior cached drafts */
    }
    paint();
  }

  async function generate(kind: SponsorDraftKind): Promise<void> {
    if (!hasSubject()) return;
    const subj = subject();
    jobs[kind] = { status: "pending" };
    paint();
    try {
      const jobId = await sponsorGenerate(kind, {
        sponsorRef: subj.sponsorRef,
        name: subj.name,
        city: selectedSponsor?.city ?? (cityFilter.trim() || undefined),
        channel: kind === "pitch" ? channel : undefined,
        targetAudience: targetAudience.trim() || undefined,
        meetupToken: kind === "pitch" ? meetupToken || undefined : undefined,
        notes: notes.trim() || undefined,
      });
      jobs[kind] = { ...jobs[kind], jobId };
    } catch {
      jobs[kind] = { status: "error", errorCode: "other" };
      paint();
    }
  }

  function wire(el: HTMLElement): void {
    byId<HTMLButtonElement>("sponsorsBackBtn").addEventListener("click", opts.onBack);

    const qInput = el.querySelector<HTMLInputElement>('input[data-field="query"]');
    const cityInput = el.querySelector<HTMLInputElement>('input[data-field="city"]');
    const industryInput = el.querySelector<HTMLInputElement>('input[data-field="industry"]');
    const activeInput = el.querySelector<HTMLInputElement>('input[data-field="active_only"]');
    qInput?.addEventListener("input", () => (query = qInput.value));
    qInput?.addEventListener("keydown", (ev) => {
      if (ev.key === "Enter") void runSearch();
    });
    cityInput?.addEventListener("input", () => (cityFilter = cityInput.value));
    industryInput?.addEventListener("input", () => (industryFilter = industryInput.value));
    activeInput?.addEventListener("change", () => (activeOnly = activeInput.checked));
    el.querySelector<HTMLButtonElement>(".sp-search-btn")?.addEventListener("click", () => void runSearch());

    el.querySelectorAll<HTMLElement>(".sp-card").forEach((card) => {
      card.addEventListener("click", () => {
        const token = card.dataset.token;
        const match = searchResult?.results.find((m) => m.sponsor_token === token);
        if (match) void selectSponsor(match);
      });
    });

    const freeformInput = el.querySelector<HTMLInputElement>('input[data-field="freeform_name"]');
    freeformInput?.addEventListener("input", () => {
      freeformName = freeformInput.value;
      selectedSponsor = null;
    });
    freeformInput?.addEventListener("focus", () => selectFreeform());

    const meetupSelect = el.querySelector<HTMLSelectElement>('select[data-field="meetup_token"]');
    meetupSelect?.addEventListener("change", () => (meetupToken = meetupSelect.value));
    const channelSelect = el.querySelector<HTMLSelectElement>('select[data-field="channel"]');
    channelSelect?.addEventListener("change", () => (channel = channelSelect.value));
    const audienceInput = el.querySelector<HTMLInputElement>('input[data-field="target_audience"]');
    audienceInput?.addEventListener("input", () => (targetAudience = audienceInput.value));
    const notesInput = el.querySelector<HTMLTextAreaElement>('textarea[data-field="notes"]');
    notesInput?.addEventListener("input", () => (notes = notesInput.value));

    el.querySelectorAll<HTMLButtonElement>(".sp-generate, .sp-retry").forEach((btn) => {
      btn.addEventListener("click", () => {
        const kind = btn.closest<HTMLElement>(".sp-section")?.dataset.kind as SponsorDraftKind | undefined;
        if (kind) void generate(kind);
      });
    });
    el.querySelectorAll<HTMLButtonElement>(".sp-cancel").forEach((btn) => {
      btn.addEventListener("click", () => {
        const jobId = btn.dataset.jobId;
        if (jobId) void sponsorGenerationCancel(jobId);
      });
    });
    el.querySelectorAll<HTMLButtonElement>(".sp-copy").forEach((btn) => {
      btn.addEventListener("click", async () => {
        const text = btn.dataset.copyText ?? "";
        try {
          await navigator.clipboard.writeText(text);
          const orig = btn.textContent;
          btn.textContent = "Copied!";
          setTimeout(() => (btn.textContent = orig), 1500);
        } catch {
          /* clipboard unavailable in this webview — no-op */
        }
      });
    });
  }

  function paint(): void {
    root.innerHTML = `
      <div class="appbar">
        <img src="/logos/logo-stacked-760.png" alt="AI Tinkerers" />
        <span class="a-title">Mission Control</span>
        <span class="spacer"></span>
      </div>
      <div class="content">
        <button class="back" id="sponsorsBackBtn">&larr; All events</button>
        <div class="d-head">
          <div>
            <h2>Sponsors</h2>
            <div class="d-meta">Find sponsors, view contacts, and draft AI research briefs and pitches</div>
          </div>
        </div>
        ${searchPanelHTML()}
        ${hasSubject() ? detailHTML() : ""}
      </div>`;
    wire(root);
  }

  function searchPanelHTML(): string {
    return `<div class="panel" style="grid-column:1/-1">
      <h4>Search sponsors</h4>
      <div class="promo-controls">
        <input class="promo-input" type="text" placeholder="Company, industry, or city" value="${esc(query)}" data-field="query" />
        <input class="promo-input" type="text" placeholder="City filter (optional)" value="${esc(cityFilter)}" data-field="city" />
        <input class="promo-input" type="text" placeholder="Industry filter (optional)" value="${esc(industryFilter)}" data-field="industry" />
        <label class="promo-checkbox"><input type="checkbox" data-field="active_only" ${activeOnly ? "checked" : ""} /> Active only</label>
        <button class="btn sp-search-btn" ${searching || !query.trim() ? "disabled" : ""}>${searching ? "Searching…" : "Search"}</button>
      </div>
      ${searchResultsHTML()}
      <div class="promo-controls" style="margin-top:12px">
        <input class="promo-input" type="text" placeholder="Or type a new company name to generate for" value="${esc(freeformName)}" data-field="freeform_name" />
      </div>
    </div>`;
  }

  function searchResultsHTML(): string {
    if (!searchResult) return "";
    if (searchResult.unavailable) return degradeHTML(searchResult.reason);
    if (!searchResult.results.length) {
      return `<div class="not-enabled">No sponsors found for that search.</div>`;
    }
    const trunc = searchResult.truncated
      ? `<div class="notice">Showing the top matches — not the complete list.</div>`
      : "";
    return `${trunc}<div class="sp-grid">${searchResult.results.map(sponsorCardHTML).join("")}</div>`;
  }

  function sponsorCardHTML(m: SponsorMatch): string {
    const selected = selectedSponsor?.sponsor_token === m.sponsor_token;
    return `<div class="sp-card ${selected ? "selected" : ""}" data-token="${esc(m.sponsor_token)}">
      <div class="ep-title">${esc(m.name ?? "Unnamed sponsor")}</div>
      <div class="sp-meta">
        ${m.domain ? `<span>${esc(m.domain)}</span>` : ""}
        ${m.city ? `<span>${esc(m.city)}</span>` : ""}
      </div>
      ${m.short_profile ? `<p class="sp-profile">${esc(m.short_profile)}</p>` : ""}
    </div>`;
  }

  function degradeHTML(reason?: string | null): string {
    if (reason === "forbidden_api_group") {
      return `<div class="not-enabled"><b>Sponsor tools aren't enabled for this chapter</b>
        The sponsors API group is switched off for this weblog.</div>`;
    }
    if (reason === "forbidden_scope" || reason === "forbidden_role") {
      return `<div class="not-enabled"><b>Your role can't use sponsor tools for this chapter</b>
        Sponsor tools are available to city owners only.</div>`;
    }
    if (reason === "rate_limited") {
      return `<div class="not-enabled"><b>Rate limited</b>
        The API's request budget is temporarily used up. Try again shortly.</div>`;
    }
    return `<div class="not-enabled"><b>Sponsor data isn't available right now</b></div>`;
  }

  function detailHTML(): string {
    const name = selectedSponsor?.name ?? freeformName.trim();
    return `<div class="panel" style="grid-column:1/-1">
      <h4>${esc(name || "New company")}</h4>
      ${selectedSponsor ? contactsHTML() : `<p class="promo-note">Generating for a company not yet in the sponsor list — no contacts to show.</p>`}
      <div class="promo-grid">
        ${researchSectionHTML()}
        ${pitchSectionHTML()}
      </div>
    </div>`;
  }

  function contactsHTML(): string {
    if (contactsLoading && !contacts) {
      return `<div class="empty"><div class="spinner"></div><span>Loading contacts…</span></div>`;
    }
    if (!contacts) return `<div class="not-enabled">No contacts loaded yet.</div>`;
    if (contacts.unavailable) return degradeHTML(contacts.reason);
    if (!contacts.contacts.length) return `<div class="not-enabled">No contacts found for this sponsor.</div>`;
    const trunc = contacts.truncated
      ? `<div class="notice">Showing the first ${contacts.contacts.length} contacts — the list is capped.</div>`
      : "";
    return `${trunc}<div class="job-list">${contacts.contacts.map(contactRowHTML).join("")}</div>`;
  }

  function contactRowHTML(c: SponsorContact): string {
    return `<div class="job-row">
      <div class="job-main">
        <span class="job-subj">${esc(c.role ?? c.title ?? "Contact")}</span>
        ${c.title && c.role ? `<small>${esc(c.title)}</small>` : ""}
      </div>
      <div class="job-counts">
        ${maskedFieldHTML(c.email, c.email_masked, "email")}
        ${maskedFieldHTML(c.phone, c.phone_masked, "phone")}
        ${c.linkedin ? `<span>${esc(c.linkedin)}</span>` : ""}
        ${typeof c.confidence === "number" ? `<span>confidence ${(c.confidence * (c.confidence <= 1 ? 100 : 1)).toFixed(0)}%</span>` : ""}
      </div>
    </div>`;
  }

  function maskedFieldHTML(value: string | null | undefined, masked: boolean, label: string): string {
    if (masked) {
      return `<span class="chip" title="Enable email visibility for this chapter to see this ${label}">masked ${label}</span>`;
    }
    if (!value) return "";
    return `<span>${esc(value)}</span>`;
  }

  function researchSectionHTML(): string {
    const job = jobs.research;
    const list = drafts.research;
    const busy = job.status === "pending" || job.status === "running";
    return `<div class="promo-section sp-section" data-kind="research">
      <div class="ep-title">Research brief</div>
      <div class="promo-controls">
        <input class="promo-input" type="text" placeholder="Target audience (optional)" value="${esc(targetAudience)}" data-field="target_audience" />
        <button class="btn sp-generate" ${busy || isForbidden(job) ? "disabled" : ""}>${list.length ? "Regenerate" : "Generate"}</button>
      </div>
      ${statusBannerHTML(job)}
      ${draftListHTML("research", list)}
    </div>`;
  }

  function pitchSectionHTML(): string {
    const job = jobs.pitch;
    const list = drafts.pitch;
    const busy = job.status === "pending" || job.status === "running";
    return `<div class="promo-section sp-section" data-kind="pitch">
      <div class="ep-title">Tailored pitch</div>
      <div class="promo-controls">
        <select class="promo-select" data-field="meetup_token">
          <option value="">No event context</option>
          ${events.map((e) => `<option value="${esc(e.meetup_token)}" ${meetupToken === e.meetup_token ? "selected" : ""}>${esc(e.event_name)}</option>`).join("")}
        </select>
        <select class="promo-select" data-field="channel">
          ${CHANNELS.map((c) => `<option value="${c}" ${channel === c ? "selected" : ""}>${labelize(c)}</option>`).join("")}
        </select>
      </div>
      <div class="promo-controls">
        <textarea class="promo-input" rows="2" placeholder="Extra notes for the pitch (optional)" data-field="notes">${esc(notes)}</textarea>
        <button class="btn sp-generate" ${busy || isForbidden(job) ? "disabled" : ""}>${list.length ? "Regenerate" : "Generate"}</button>
      </div>
      ${statusBannerHTML(job)}
      ${draftListHTML("pitch", list)}
    </div>`;
  }

  function isForbidden(job: JobState): boolean {
    return (
      job.status === "error" &&
      (job.errorCode === "forbidden_api_group" || job.errorCode === "forbidden_scope" || job.errorCode === "forbidden_role")
    );
  }

  function statusBannerHTML(job: JobState): string {
    if (!job.status) return "";
    if (job.status === "pending" || job.status === "running") {
      return `<div class="promo-progress">
        <div class="spinner"></div>
        <span>${job.status === "pending" ? "Queued…" : "Generating… (can take up to ~20s)"}</span>
        <button class="linklike sp-cancel" data-job-id="${esc(job.jobId ?? "")}">Cancel</button>
      </div>`;
    }
    if (job.status === "timeout") {
      return `<div class="not-enabled promo-issue"><b>Timed out</b>Generation took too long.
        <button class="linklike sp-retry">Retry</button></div>`;
    }
    if (job.status === "error") {
      if (job.errorCode === "forbidden_api_group") {
        return `<div class="not-enabled promo-issue"><b>Not enabled for your chapter</b>
          The sponsors API group is switched off. Everything else still works.</div>`;
      }
      if (job.errorCode === "forbidden_scope" || job.errorCode === "forbidden_role") {
        return `<div class="not-enabled promo-issue"><b>Needs a different access level</b>
          Sponsor tools are available to city owners only.</div>`;
      }
      if (job.errorCode === "rate_limited") {
        return `<div class="not-enabled promo-issue"><b>Rate limited</b>
          Try again shortly. <button class="linklike sp-retry">Retry</button></div>`;
      }
      return `<div class="not-enabled promo-issue"><b>Generation failed</b>
        <button class="linklike sp-retry">Retry</button></div>`;
    }
    return "";
  }

  function draftListHTML(kind: SponsorDraftKind, list: SponsorDraft[]): string {
    if (!list.length) {
      return `<div class="not-enabled">No draft yet — click Generate.</div>`;
    }
    return `<div class="promo-draft-list">${list.map((d) => draftHTML(kind, d)).join("")}</div>`;
  }

  function draftHTML(kind: SponsorDraftKind, d: SponsorDraft): string {
    const text = kind === "research" ? (d.result?.research_summary ?? "") : (d.result?.pitch_text ?? "");
    const variants = kind === "pitch" ? (d.result?.variants ?? []) : [];
    return `<div class="promo-draft">
      <div class="promo-draft-meta">
        <span>Generated ${esc(relTime(d.created_at))}</span>
        <button class="linklike sp-copy" data-copy-text="${esc(text)}">Copy</button>
      </div>
      <div class="promo-draft-body"><div class="promo-text">${esc(text || "No content returned.")}</div></div>
      ${variants.length ? `<div class="promo-block"><div class="promo-block-label">Variants</div>
        <ul class="promo-topics">${variants.map((v) => `<li>${esc(v)}</li>`).join("")}</ul></div>` : ""}
      <p class="promo-note">AI-generated draft for copy/export only — nothing here is sent automatically.</p>
    </div>`;
  }

  return { open };
}

function labelize(k: string): string {
  return k.replace(/_/g, " ").replace(/\b\w/g, (c) => c.toUpperCase());
}
