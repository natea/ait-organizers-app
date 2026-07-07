// Promote panel (specs/promotion-tools): per-event generation of social post
// drafts, an event promo package, and discussion topics, plus a logo/brand
// search. Generation is user-initiated only (never the poll loop), runs as a
// tracked background job with progress reported via `promotion:job` events,
// and the latest draft per (event, kind, platform) renders from the SQLite
// cache — including offline — so revisiting the panel never re-spends a slow,
// rate-limited call.
import {
  getPromotionDrafts,
  logoSearch,
  onPromotionJob,
  promotionCancel,
  promotionGenerate,
} from "../api";
import type {
  LogoMatch,
  LogoSearchResult,
  PromotionDraft,
  PromotionDraftMap,
  PromotionJobEvent,
  PromotionJobStatus,
} from "../types";
import { esc } from "../util";
import { relTime } from "./email";

interface ActionState {
  jobId?: string;
  status?: PromotionJobStatus;
  errorCode?: string | null;
}

export interface PromoteController {
  /** Full (re)load for a newly opened event: reset state, load cached drafts. */
  open: (meetupToken: string) => Promise<void>;
  /** Cache-only reload for background re-renders (no state reset). */
  refresh: (meetupToken: string) => Promise<void>;
  /** Synchronous repaint into the slot from whatever is already in memory —
   *  call this right after the slot element is (re)inserted into the DOM. */
  paint: () => void;
}

const SOCIAL_GOALS = ["promote", "recap", "spotlight", "announce", "sponsor_thanks"];
const PACKAGE_TYPES = ["launch", "reminder", "final_push", "recap", "full_campaign"];
const AUDIENCES = ["general", "builders", "founders", "sponsors", "students"];

export function mountPromote(slotId: string): PromoteController {
  let meetupToken: string | null = null;
  let drafts: PromotionDraftMap = {};
  const actions: Record<string, ActionState> = {};
  let unlisten: (() => void) | null = null;

  let socialPlatform: "linkedin" | "x" = "linkedin";
  let socialGoal = "promote";
  let packageType = "full_campaign";
  let audience = "general";

  let logoQuery = "";
  let logoCoBranded = false;
  let logoLoading = false;
  let logoError: string | null = null;
  let logoResult: LogoSearchResult | null = null;

  function slot(): HTMLElement | null {
    return document.getElementById(slotId);
  }

  function actionKey(kind: string, platform: string): string {
    return platform ? `${kind}:${platform}` : kind;
  }

  async function open(token: string): Promise<void> {
    const changed = token !== meetupToken;
    meetupToken = token;
    if (changed) {
      for (const k of Object.keys(actions)) delete actions[k];
      drafts = {};
      logoResult = null;
      logoError = null;
      logoQuery = "";
    }
    if (!unlisten) {
      unlisten = await onPromotionJob(onJobEvent);
    }
    await loadDrafts();
  }

  async function refresh(token: string): Promise<void> {
    if (token !== meetupToken) return;
    await loadDrafts();
  }

  async function loadDrafts(): Promise<void> {
    if (!meetupToken) return;
    const token = meetupToken;
    try {
      const d = await getPromotionDrafts(token);
      if (token === meetupToken) drafts = d;
    } catch {
      /* keep prior cached drafts */
    }
    paint();
  }

  function onJobEvent(e: PromotionJobEvent): void {
    if (e.meetup_token !== meetupToken) return;
    const key = actionKey(e.kind, e.platform);
    if (e.status === "cancelled") {
      delete actions[key];
    } else {
      actions[key] = { jobId: e.job_id, status: e.status, errorCode: e.error_code };
    }
    if (e.status === "ready") {
      void loadDrafts(); // pull the freshly-cached draft, then repaint
    } else {
      paint();
    }
  }

  async function generate(kind: string): Promise<void> {
    if (!meetupToken) return;
    const token = meetupToken;
    let platform = "";
    let params: Record<string, unknown>;
    if (kind === "social_post") {
      platform = socialPlatform;
      params = { source_type: "meetup", source_ref: token, platform: socialPlatform, goal: socialGoal };
    } else if (kind === "event_promo") {
      params = { meetup_token: token, package_type: packageType, audience };
    } else {
      params = { meetup_token: token };
    }
    const key = actionKey(kind, platform);
    // Optimistic pending state — also disables the button immediately so a
    // fast double-click can't fire a second request before the job event
    // round-trips (design D7 duplicate suppression is belt-and-suspenders).
    actions[key] = { status: "pending" };
    paint();
    try {
      const jobId = await promotionGenerate(kind, token, platform || undefined, params);
      if (token === meetupToken) actions[key] = { ...actions[key], jobId };
    } catch {
      if (token === meetupToken) actions[key] = { status: "error", errorCode: "other" };
      paint();
    }
  }

  async function runLogoSearch(): Promise<void> {
    const q = logoQuery.trim();
    if (!q) return;
    logoLoading = true;
    logoError = null;
    paint();
    try {
      logoResult = await logoSearch(q, "smart_match", logoCoBranded, 20);
    } catch {
      logoError = "Could not search logos right now.";
    } finally {
      logoLoading = false;
      paint();
    }
  }

  function isForbidden(a: ActionState | undefined): boolean {
    return (
      a?.status === "error" &&
      (a.errorCode === "forbidden_api_group" || a.errorCode === "forbidden_scope" || a.errorCode === "forbidden_role")
    );
  }

  function wire(el: HTMLElement): void {
    el.querySelectorAll<HTMLSelectElement>("select[data-field]").forEach((input) => {
      input.addEventListener("change", () => {
        switch (input.dataset.field) {
          case "platform":
            socialPlatform = input.value as "linkedin" | "x";
            break;
          case "goal":
            socialGoal = input.value;
            break;
          case "package_type":
            packageType = input.value;
            break;
          case "audience":
            audience = input.value;
            break;
        }
        paint();
      });
    });

    const logoInput = el.querySelector<HTMLInputElement>('input[data-field="logo_query"]');
    const searchBtn = el.querySelector<HTMLButtonElement>(".promo-search-btn");
    logoInput?.addEventListener("input", () => {
      logoQuery = logoInput.value;
      // Toggle directly rather than a full paint() — a repaint on every
      // keystroke would steal focus back from the input.
      if (searchBtn && !logoLoading) searchBtn.disabled = !logoQuery.trim();
    });
    logoInput?.addEventListener("keydown", (ev) => {
      if (ev.key === "Enter") void runLogoSearch();
    });
    const coBrandedInput = el.querySelector<HTMLInputElement>('input[data-field="logo_co_branded"]');
    coBrandedInput?.addEventListener("change", () => {
      logoCoBranded = coBrandedInput.checked;
    });

    el.querySelectorAll<HTMLButtonElement>(".promo-generate, .promo-retry").forEach((btn) => {
      btn.addEventListener("click", () => {
        const kind = btn.closest<HTMLElement>(".promo-section")?.dataset.kind;
        if (kind) void generate(kind);
      });
    });

    el.querySelectorAll<HTMLButtonElement>(".promo-cancel").forEach((btn) => {
      btn.addEventListener("click", () => {
        const jobId = btn.dataset.jobId;
        if (jobId) void promotionCancel(jobId);
      });
    });

    searchBtn?.addEventListener("click", () => void runLogoSearch());

    el.querySelectorAll<HTMLButtonElement>(".promo-copy").forEach((btn) => {
      btn.addEventListener("click", async () => {
        const text = btn.dataset.copyText ?? "";
        try {
          await navigator.clipboard.writeText(text);
          const orig = btn.textContent;
          btn.textContent = "Copied!";
          setTimeout(() => {
            btn.textContent = orig;
          }, 1500);
        } catch {
          /* clipboard unavailable in this webview — no-op */
        }
      });
    });
  }

  function paint(): void {
    const el = slot();
    if (!el || !meetupToken) return;
    el.innerHTML = panelHTML();
    wire(el);
  }

  function panelHTML(): string {
    return `<div class="panel" style="grid-column:1/-1">
      <h4>Promote</h4>
      <p class="promo-note">AI-generated drafts for copy/export only — nothing here is posted, sent, or published automatically.</p>
      <div class="promo-grid">
        ${socialPostSectionHTML()}
        ${eventPromoSectionHTML()}
        ${discussionTopicsSectionHTML()}
        ${logoSectionHTML()}
      </div>
    </div>`;
  }

  function socialPostSectionHTML(): string {
    const key = actionKey("social_post", socialPlatform);
    const action = actions[key];
    const draft = drafts[key];
    const busy = action?.status === "pending" || action?.status === "running";
    return `<div class="promo-section" data-kind="social_post">
      <div class="ep-title">Social post</div>
      <div class="promo-controls">
        <select class="promo-select" data-field="platform">
          <option value="linkedin" ${socialPlatform === "linkedin" ? "selected" : ""}>LinkedIn</option>
          <option value="x" ${socialPlatform === "x" ? "selected" : ""}>X</option>
        </select>
        <select class="promo-select" data-field="goal">
          ${SOCIAL_GOALS.map((g) => `<option value="${g}" ${socialGoal === g ? "selected" : ""}>${labelize(g)}</option>`).join("")}
        </select>
        <button class="btn promo-generate" ${busy || isForbidden(action) ? "disabled" : ""}>${draft ? "Regenerate" : "Generate"}</button>
      </div>
      ${statusBannerHTML(action)}
      ${draftBodyHTML("social_post", draft)}
    </div>`;
  }

  function eventPromoSectionHTML(): string {
    const key = actionKey("event_promo", "");
    const action = actions[key];
    const draft = drafts[key];
    const busy = action?.status === "pending" || action?.status === "running";
    return `<div class="promo-section" data-kind="event_promo">
      <div class="ep-title">Event promo package</div>
      <div class="promo-controls">
        <select class="promo-select" data-field="package_type">
          ${PACKAGE_TYPES.map((p) => `<option value="${p}" ${packageType === p ? "selected" : ""}>${labelize(p)}</option>`).join("")}
        </select>
        <select class="promo-select" data-field="audience">
          ${AUDIENCES.map((a) => `<option value="${a}" ${audience === a ? "selected" : ""}>${labelize(a)}</option>`).join("")}
        </select>
        <button class="btn promo-generate" ${busy || isForbidden(action) ? "disabled" : ""}>${draft ? "Regenerate" : "Generate"}</button>
      </div>
      ${statusBannerHTML(action)}
      ${draftBodyHTML("event_promo", draft)}
    </div>`;
  }

  function discussionTopicsSectionHTML(): string {
    const key = actionKey("discussion_topics", "");
    const action = actions[key];
    const draft = drafts[key];
    const busy = action?.status === "pending" || action?.status === "running";
    return `<div class="promo-section" data-kind="discussion_topics">
      <div class="ep-title">Discussion topics</div>
      <div class="promo-controls">
        <button class="btn promo-generate" ${busy || isForbidden(action) ? "disabled" : ""}>${draft ? "Regenerate" : "Generate"}</button>
      </div>
      ${statusBannerHTML(action)}
      ${draftBodyHTML("discussion_topics", draft)}
    </div>`;
  }

  function logoSectionHTML(): string {
    const matches = logoResult?.result?.matches ?? [];
    return `<div class="promo-section" data-kind="logo_search">
      <div class="ep-title">Logo &amp; brand search</div>
      <div class="promo-controls">
        <input class="promo-input" type="text" placeholder="e.g. city name, sponsor" value="${esc(logoQuery)}" data-field="logo_query" />
        <label class="promo-checkbox"><input type="checkbox" data-field="logo_co_branded" ${logoCoBranded ? "checked" : ""} /> Co-branded</label>
        <button class="btn promo-search-btn" ${logoLoading || !logoQuery.trim() ? "disabled" : ""}>${logoLoading ? "Searching…" : "Search"}</button>
      </div>
      ${logoError ? `<div class="not-enabled promo-issue"><b>Search failed</b>${esc(logoError)}</div>` : ""}
      ${matches.length ? logoResultsHTML(matches) : logoResult ? `<div class="not-enabled">No logos found for that search.</div>` : ""}
    </div>`;
  }

  return { open, refresh, paint };
}

function statusBannerHTML(action: ActionState | undefined): string {
  if (!action) return "";
  if (action.status === "pending" || action.status === "running") {
    return `<div class="promo-progress">
      <div class="spinner"></div>
      <span>${action.status === "pending" ? "Queued…" : "Generating… (can take up to ~25s)"}</span>
      <button class="linklike promo-cancel" data-job-id="${esc(action.jobId ?? "")}">Cancel</button>
    </div>`;
  }
  if (action.status === "timeout") {
    return `<div class="not-enabled promo-issue">
      <b>Timed out</b>Generation took too long. Any previous draft below is still shown.
      <button class="linklike promo-retry">Retry</button></div>`;
  }
  if (action.status === "error") {
    if (action.errorCode === "forbidden_api_group") {
      return `<div class="not-enabled promo-issue"><b>Not enabled for your chapter</b>
        This API group is switched off for this weblog. Everything else still works.</div>`;
    }
    if (action.errorCode === "forbidden_scope" || action.errorCode === "forbidden_role") {
      return `<div class="not-enabled promo-issue"><b>Needs a different access level</b>
        Your key's role/scope can't generate this. Everything else still works.</div>`;
    }
    if (action.errorCode === "rate_limited") {
      return `<div class="not-enabled promo-issue"><b>Rate limited</b>
        The API's generation budget is temporarily used up.
        <button class="linklike promo-retry">Retry</button></div>`;
    }
    return `<div class="not-enabled promo-issue"><b>Generation failed</b>
      Something went wrong. <button class="linklike promo-retry">Retry</button></div>`;
  }
  return "";
}

function draftBodyHTML(kind: string, draft: PromotionDraft | undefined): string {
  if (!draft || !draft.result) {
    return `<div class="not-enabled">No draft yet — click Generate.</div>`;
  }
  const text = extractDraftText(kind, draft.result);
  return `<div class="promo-draft">
    <div class="promo-draft-meta">
      <span>Generated ${esc(relTime(draft.generated_at))}</span>
      <button class="linklike promo-copy" data-copy-text="${esc(text)}">Copy</button>
    </div>
    <div class="promo-draft-body">${renderArtifactHTML(kind, draft.result)}</div>
  </div>`;
}

function logoResultsHTML(matches: LogoMatch[]): string {
  return `<div class="logo-grid">${matches.slice(0, 12).map(logoCardHTML).join("")}</div>`;
}

function logoCardHTML(m: LogoMatch): string {
  const img = m.thumbnail_light_url ?? m.padded_imgix_url ?? m.imgix_url ?? "";
  const name = m.metadata?.brand_name ?? m.caption ?? m.text_content ?? "Logo";
  const coBranded = m.metadata?.is_co_branded ? `<span class="chip on">co-branded</span>` : "";
  return `<figure class="logo-card">
    ${img ? `<img src="${esc(img)}" alt="${esc(name)}" loading="lazy" />` : ""}
    <figcaption>${esc(name)} ${coBranded}</figcaption>
    ${m.imgix_url ? `<button class="linklike promo-copy" data-copy-text="${esc(m.imgix_url)}">Copy URL</button>` : ""}
  </figure>`;
}

// ── Defensive artifact rendering (design D3: unknown fields pass through) ──

type ArtifactResult = NonNullable<PromotionDraft["result"]>;

function renderArtifactHTML(kind: string, result: ArtifactResult): string {
  if (kind === "discussion_topics") {
    const topics = Array.isArray(result.discussion_topics) ? result.discussion_topics : [];
    if (!topics.length) return `<div class="not-enabled">No topics returned.</div>`;
    return `<ul class="promo-topics">${topics.map((t) => `<li>${esc(String(t))}</li>`).join("")}</ul>`;
  }
  const artifact = (result as Record<string, unknown>).artifact ?? result;
  return renderUnknown(artifact);
}

function renderUnknown(v: unknown): string {
  if (v == null) return `<div class="not-enabled">No content returned.</div>`;
  if (typeof v === "string") return `<div class="promo-text">${esc(v)}</div>`;
  if (Array.isArray(v)) {
    return `<ul class="promo-topics">${v.map((x) => `<li>${esc(summarize(x))}</li>`).join("")}</ul>`;
  }
  if (typeof v === "object") {
    const obj = v as Record<string, unknown>;
    const blocks = Object.entries(obj)
      .map(([key, val]) => {
        if (typeof val === "string" && val.trim()) {
          return `<div class="promo-block"><div class="promo-block-label">${esc(labelize(key))}</div>
            <div class="promo-text">${esc(val)}</div></div>`;
        }
        if (Array.isArray(val) && val.length) {
          return `<div class="promo-block"><div class="promo-block-label">${esc(labelize(key))}</div>
            <ul class="promo-topics">${val.map((x) => `<li>${esc(summarize(x))}</li>`).join("")}</ul></div>`;
        }
        return "";
      })
      .filter(Boolean)
      .join("");
    return blocks || `<pre class="promo-raw">${esc(JSON.stringify(obj, null, 2))}</pre>`;
  }
  return `<div class="promo-text">${esc(String(v))}</div>`;
}

function extractDraftText(kind: string, result: ArtifactResult): string {
  if (kind === "discussion_topics") {
    const topics = Array.isArray(result.discussion_topics) ? result.discussion_topics : [];
    return topics.map((t, i) => `${i + 1}. ${t}`).join("\n");
  }
  const artifact = (result as Record<string, unknown>).artifact ?? result;
  return flattenText(artifact);
}

function flattenText(v: unknown): string {
  if (v == null) return "";
  if (typeof v === "string") return v;
  if (Array.isArray(v)) return v.map((x) => flattenText(x)).join("\n");
  if (typeof v === "object") {
    return Object.entries(v as Record<string, unknown>)
      .map(([k, val]) => `${labelize(k)}:\n${flattenText(val)}`)
      .join("\n\n");
  }
  return String(v);
}

function summarize(v: unknown): string {
  if (typeof v === "string") return v;
  if (v && typeof v === "object") return JSON.stringify(v);
  return String(v);
}

function labelize(k: string): string {
  return k.replace(/_/g, " ").replace(/\b\w/g, (c) => c.toUpperCase());
}
