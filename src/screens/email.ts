// Chapter email deliverability view (specs/email-lifecycle): sender-domain
// health, fatigue-risk tier summary, and recent send jobs. Renders only from
// the SQLite cache; fetched on launch and manual refresh (never the poll loop).
// This module also exports the shared render helpers used by the per-event
// Email panel in detail.ts (status chips, rate formatting, throughput spark,
// send-job rows, degraded copy) so both surfaces stay visually consistent.
import { getChapterDeliverability, onEmailChapter, refreshEmail } from "../api";
import type {
  ChapterDeliverability,
  SendJob,
  SenderDomain,
  ThroughputBucket,
} from "../types";
import { byId, esc, fmt, num } from "../util";

interface EmailOpts {
  onBack: () => void;
}

export interface EmailController {
  /** Show the screen: render cache, then trigger a manual chapter refresh. */
  open: () => Promise<void>;
  /** Cache-only re-render (background email:chapter updates). */
  refresh: () => Promise<void>;
}

export function mountEmail(opts: EmailOpts): EmailController {
  const root = byId("scr-email");
  let data: ChapterDeliverability | null = null;
  let loaded = false;

  onEmailChapter(() => {
    void refresh();
  });

  function paint(): void {
    root.innerHTML = `
      <div class="appbar">
        <img src="/logos/logo-stacked-760.png" alt="AI Tinkerers" />
        <span class="a-title">Mission Control</span>
        <span class="spacer"></span>
        <button class="refresh" id="emailRefreshBtn">Refresh</button>
      </div>
      <div class="content">
        <button class="back" id="emailBackBtn">← All events</button>
        <div class="d-head">
          <div>
            <h2>Email deliverability</h2>
            <div class="d-meta">Chapter-wide sender health, fatigue risk, and recent sends</div>
          </div>
        </div>
        ${bodyHTML(data, loaded)}
        <div class="lastsync-foot">${footNote(data)}</div>
      </div>`;
    byId<HTMLButtonElement>("emailBackBtn").addEventListener("click", opts.onBack);
    const rb = byId<HTMLButtonElement>("emailRefreshBtn");
    rb.addEventListener("click", async () => {
      rb.disabled = true;
      rb.textContent = "Syncing…";
      try {
        await refreshEmail();
        await refresh();
      } finally {
        rb.disabled = false;
        rb.textContent = "Refresh";
      }
    });
  }

  async function load(): Promise<void> {
    try {
      data = await getChapterDeliverability();
    } catch {
      data = null;
    }
    loaded = true;
    paint();
  }

  async function open(): Promise<void> {
    await load();
    // Manual/opened refresh (chapter data is not on the poll loop).
    try {
      await refreshEmail();
      await load();
    } catch {
      /* keep cached render */
    }
  }

  async function refresh(): Promise<void> {
    await load();
  }

  return { open, refresh };
}

function footNote(d: ChapterDeliverability | null): string {
  const when = d?.updated_at ? ` · synced ${relTime(d.updated_at)}` : "";
  return `Rendering from local cache · chapter data refreshes on launch and manual refresh${when}`;
}

function bodyHTML(d: ChapterDeliverability | null, loaded: boolean): string {
  const isEmpty =
    !d || (!d.health && !d.fatigue && (!d.recent_jobs || !d.recent_jobs.length) && !d.unavailable);
  if (isEmpty) {
    if (!loaded) {
      return `<div class="panel"><h4>Email deliverability</h4>
        <div class="empty"><div class="spinner"></div><span>Loading deliverability data…</span></div></div>`;
    }
    // Loaded but nothing came back — don't spin forever (that was the bug).
    return `<div class="panel"><h4>Email deliverability</h4>
      <div class="empty"><b>No deliverability data</b>
        <span>Nothing to show yet. This happens when your chapter has no recent
        sends, the subscribers group is off for your key, the API's daily request
        budget is used up, or the app hasn't resolved your chapter (open an event
        first, then hit Refresh).</span></div></div>`;
  }
  if (d.unavailable) {
    return `<div class="panel"><h4>Email deliverability</h4>${emailBlockedHTML(d.reason)}</div>`;
  }
  const trunc = d.truncated
    ? `<div class="notice">Showing a recent window — not the complete history.</div>`
    : "";
  return `
    ${trunc}
    <div class="d-grid">
      ${healthPanel(d.health ?? null)}
      ${fatiguePanel(d.fatigue ?? null)}
      ${recentJobsPanel(d.recent_jobs ?? [])}
    </div>`;
}

function healthPanel(h: ChapterDeliverability["health"]): string {
  if (!h) {
    return `<div class="panel"><h4>Sender health</h4>
      <div class="not-enabled">No deliverability health cached yet.</div></div>`;
  }
  const score = typeof h.health_score === "number" ? Math.round(h.health_score) : null;
  const scoreClass = score == null ? "" : score >= 80 ? "good" : score >= 60 ? "warn" : "bad";
  const gauge =
    score == null
      ? ""
      : `<div class="health-score ${scoreClass}">
           <b>${score}</b><small>/ 100 health</small>
           <div class="g-bar"><div class="g-fill" style="width:${score}%"></div></div>
         </div>`;
  const domains = h.sender_domains ?? [];
  const rows = domains.length
    ? domains.map(domainRow).join("")
    : `<div class="not-enabled">No sender-domain rows in this window.</div>`;
  return `<div class="panel" style="grid-column:1/-1">
    <h4>Sender health</h4>
    ${gauge}
    <div class="domain-list">${rows}</div>
  </div>`;
}

function domainRow(dm: SenderDomain): string {
  const status = (dm.status ?? "ok").toLowerCase();
  const cls = status.includes("critical") || status.includes("bad")
    ? "bad"
    : status.includes("warn") || status.includes("risk")
      ? "warn"
      : "good";
  return `<div class="domain-row">
    <div class="dm-name"><span class="dot ${cls}"></span>${esc(dm.domain ?? "—")}</div>
    <div class="dm-stat"><b>${fmt(dm.sent)}</b><small>sent</small></div>
    <div class="dm-stat"><b>${fmtRate(dm.bounce_rate)}</b><small>bounce</small></div>
    <div class="dm-stat"><b>${fmtRate(dm.complaint_rate)}</b><small>complaint</small></div>
    <div class="dm-stat"><b>${fmtRate(dm.unsubscribe_rate)}</b><small>unsub</small></div>
  </div>`;
}

function fatiguePanel(f: ChapterDeliverability["fatigue"]): string {
  const summary = f?.summary ?? null;
  const tiers = (summary?.counts_by_tier ?? summary?.by_tier ?? null) as
    | Record<string, number>
    | null;
  if (!summary || !tiers) {
    return `<div class="panel"><h4>Fatigue risk</h4>
      <div class="not-enabled">No fatigue-risk summary cached yet.</div></div>`;
  }
  const order = ["low", "medium", "high", "critical"];
  const keys = Object.keys(tiers).sort(
    (a, b) => order.indexOf(a.toLowerCase()) - order.indexOf(b.toLowerCase()),
  );
  const chips = keys
    .map(
      (k) =>
        `<div class="tier tier-${esc(k.toLowerCase())}"><b>${fmt(tiers[k])}</b><small>${esc(k)}</small></div>`,
    )
    .join("");
  const avg =
    typeof summary.average_fatigue_score === "number"
      ? `<div class="fatigue-avg">Avg score <b>${summary.average_fatigue_score.toFixed(1)}</b>${
          summary.evaluated ? ` · ${fmt(summary.evaluated)} evaluated` : ""
        }</div>`
      : "";
  return `<div class="panel"><h4>Fatigue risk</h4>
    <div class="tier-grid">${chips}</div>
    ${avg}
    <p class="groups-note">Tier summary only — individual subscribers are never shown.</p>
  </div>`;
}

function recentJobsPanel(jobs: SendJob[]): string {
  if (!jobs.length) {
    return `<div class="panel"><h4>Recent send jobs</h4>
      <div class="not-enabled">No recent send jobs cached.</div></div>`;
  }
  return `<div class="panel" style="grid-column:1/-1">
    <h4>Recent send jobs <span class="b-count">${fmt(jobs.length)}</span></h4>
    <div class="job-list">${jobs.map((j) => sendJobRowHTML(j)).join("")}</div>
  </div>`;
}

// ── Shared helpers (imported by detail.ts email panel) ──────────────────────

/** Non-alarming degraded copy branched on the block reason (design D6). */
export function emailBlockedHTML(reason?: string | null): string {
  if (reason === "forbidden_scope") {
    return `<div class="not-enabled"><b>Needs city-owner access</b>
      Email monitoring is available to city owners. Your key doesn't have
      city-owner scope for this chapter, so email data isn't shown.</div>`;
  }
  if (reason === "forbidden_api_group") {
    return `<div class="not-enabled"><b>Subscribers group not enabled</b>
      Email monitoring needs the subscribers group switched on for your key.
      Everything else still works.</div>`;
  }
  return `<div class="not-enabled"><b>Email monitoring not available</b>
    This needs the subscribers group and city-owner access on your key.</div>`;
}

/** Status chip for a send job. `done` freezes the visual to a neutral tone. */
export function statusChip(status?: string | null, done?: boolean): string {
  const s = (status ?? "unknown").toLowerCase();
  const active = s === "sending" || s === "queued" || s === "active";
  const cls = s === "failed" ? "failed" : s === "completed" ? "done" : active ? "sending" : "idle";
  const live = active && !done ? `<span class="live-dot"></span>` : "";
  return `<span class="job-chip ${cls}">${live}${esc(status ?? "unknown")}</span>`;
}

/** Format a rate that may arrive as a 0–1 fraction or a 0–100 percent. */
export function fmtRate(v: unknown): string {
  if (v == null || v === "") return "—";
  const n = typeof v === "number" ? v : Number(v);
  if (!Number.isFinite(n)) return "—";
  const pct = n >= 0 && n <= 1 ? n * 100 : n;
  return `${pct.toFixed(1)}%`;
}

/** A single send-job summary row (status, delivery counts, delivered percent). */
export function sendJobRowHTML(j: SendJob): string {
  const delivered = typeof j.delivered_percent === "number" ? j.delivered_percent : null;
  const bar =
    delivered == null
      ? ""
      : `<div class="r-bar"><div class="r-fill" style="width:${Math.min(100, Math.max(0, delivered))}%"></div></div>`;
  const rate =
    !j.done && typeof j.observed_rate === "number"
      ? `<span class="job-rate">${fmt(Math.round(j.observed_rate))}/min</span>`
      : "";
  return `<div class="job-row">
    <div class="job-main">
      ${statusChip(j.status, j.done)}
      <span class="job-subj">${esc(j.subject ?? "(no subject)")}</span>
      ${rate}
    </div>
    <div class="job-counts">
      <span><b>${fmt(j.sent_count)}</b> sent</span>
      <span><b>${fmt(j.pending_count)}</b> pending</span>
      <span><b>${fmt(j.suppressed_count)}</b> suppressed</span>
      ${delivered != null ? `<span><b>${delivered.toFixed(0)}%</b> delivered</span>` : ""}
    </div>
    ${bar}
  </div>`;
}

/** Inline SVG sparkline of sent-per-bucket, for active-send throughput. */
export function throughputSparkHTML(buckets: ThroughputBucket[]): string {
  const vals = buckets.map((b) => num(b.sent_count));
  if (!vals.length) return "";
  const max = Math.max(1, ...vals);
  const w = 220;
  const h = 40;
  const bw = w / vals.length;
  const bars = vals
    .map((v, i) => {
      const bh = Math.max(1, (v / max) * (h - 4));
      const x = (i * bw).toFixed(1);
      const y = (h - bh).toFixed(1);
      return `<rect x="${x}" y="${y}" width="${Math.max(1, bw - 1).toFixed(1)}" height="${bh.toFixed(1)}" rx="1"></rect>`;
    })
    .join("");
  return `<svg class="spark" viewBox="0 0 ${w} ${h}" preserveAspectRatio="none" role="img" aria-label="Send throughput">${bars}</svg>`;
}

export function relTime(iso: string): string {
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
