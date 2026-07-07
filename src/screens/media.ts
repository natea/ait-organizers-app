// Media video kit (specs/media-video-kit) — the app's fifth write-capable
// screen and its FIRST screen where the common case for our primary audience
// (city owners) is a prominent "not available for your role" panel: the
// Media API group is authorized ONLY for index owners and (mostly)
// `index_video_editor` — city owners get forbidden_role/forbidden_scope/
// forbidden_api_group on every media call. Browsing (folder tree, files,
// notes, transcripts) renders only from the SQLite cache. Every mutation
// (upload, folder create, note update, transcription/scale-down kickoff)
// goes through the same two-step prepare/commit confirmation gate as
// rsvp-screening, attendance-checkin, speaker-review, and networking-connect.
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import {
  fetchMediaFolder,
  fetchMediaJobStatus,
  fetchMediaView,
  getMediaFolder,
  getMediaTranscript,
  getMediaView,
  mediaFileDownload,
  mediaFolderCreateCommit,
  mediaFolderCreatePrepare,
  mediaNoteUpdateCommit,
  mediaNoteUpdatePrepare,
  mediaScaleDownCommit,
  mediaScaleDownPrepare,
  mediaTranscriptGenerateCommit,
  mediaTranscriptGeneratePrepare,
  mediaUploadCommit,
  mediaUploadPrepare,
} from "../api";
import type {
  MediaFile,
  MediaFolder,
  MediaFolderView,
  MediaJob,
  MediaJobType,
  MediaTranscript,
  MediaView,
  MediaWritePreview,
} from "../types";
import { byId, esc, fmt } from "../util";

interface MediaOpts {
  onBack: () => void;
}

export interface MediaController {
  open: (meetupToken: string, eventName?: string, weblogToken?: string) => Promise<void>;
}

type DialogKind = "upload" | "create_folder" | "note" | "transcript" | "scale_down";

interface DialogState {
  kind: DialogKind;
  preview: MediaWritePreview;
  busy: boolean;
  error?: string;
  // upload-specific
  filePath?: string;
  filename?: string;
  note?: string;
  // note-specific
  noteTarget?: { fileToken?: string; folderToken?: string };
  // transcript/scale_down-specific
  fileToken?: string;
  // create_folder-specific
  folderName?: string;
}

const JOB_POLL_MS = 4000;

export function mountMedia(opts: MediaOpts): MediaController {
  const root = byId("scr-media");

  let meetupToken: string | null = null;
  let eventName = "";
  let weblogToken: string | undefined;
  let view: MediaView | null = null;
  // Stack of folder_tokens browsed into below the event's root folder, each
  // with its own cached contents (design decision 1's subtree browsing).
  let crumbs: { folder_token: string; name: string }[] = [];
  let subView: MediaFolderView | null = null;
  let loading = false;
  let toast: string | null = null;
  let newFolderOpen = false;

  let dialog: DialogState | null = null;
  const jobs = new Map<string, MediaJob>();
  const transcripts = new Map<string, MediaTranscript>();
  const jobTimers = new Map<string, ReturnType<typeof setInterval>>();
  const jobNotices = new Map<string, string>();

  function jobKey(fileToken: string, jobType: MediaJobType): string {
    return `${fileToken}:${jobType}`;
  }

  function stopAllPolling(): void {
    for (const t of jobTimers.values()) clearInterval(t);
    jobTimers.clear();
  }

  async function open(token: string, name?: string, weblog?: string): Promise<void> {
    if (token !== meetupToken) {
      stopAllPolling();
      jobs.clear();
      transcripts.clear();
      jobNotices.clear();
      crumbs = [];
      subView = null;
    }
    meetupToken = token;
    eventName = name ?? eventName;
    weblogToken = weblog ?? weblogToken;
    toast = null;
    newFolderOpen = false;

    const cached = await getMediaView(token).catch(() => null);
    if (cached) view = cached;
    loading = true;
    paint();

    try {
      view = await fetchMediaView(token);
    } catch {
      /* keep cached render */
    }
    loading = false;
    paint();
  }

  function currentFolderToken(): string | null {
    if (crumbs.length) return crumbs[crumbs.length - 1].folder_token;
    return view?.folder?.folder_token ?? null;
  }

  function currentSubfolders(): MediaFolder[] {
    if (crumbs.length) return subView?.subfolders ?? [];
    return view?.subfolders ?? [];
  }

  function currentFiles(): MediaFile[] {
    if (crumbs.length) return subView?.files ?? [];
    return view?.files ?? [];
  }

  async function enterFolder(folder: MediaFolder): Promise<void> {
    crumbs.push({ folder_token: folder.folder_token, name: folder.name ?? "(untitled)" });
    loading = true;
    paint();
    try {
      subView = await getMediaFolder(folder.folder_token);
      paint();
      subView = await fetchMediaFolder(folder.folder_token);
    } catch {
      /* keep cached render */
    }
    loading = false;
    paint();
  }

  async function backToCrumb(index: number): Promise<void> {
    crumbs = crumbs.slice(0, index);
    if (!crumbs.length) {
      subView = null;
      paint();
      return;
    }
    const target = crumbs[crumbs.length - 1].folder_token;
    subView = await getMediaFolder(target).catch(() => subView);
    paint();
  }

  // ── upload ───────────────────────────────────────────────────────────────

  async function pickFileAndPrepareUpload(): Promise<void> {
    const folderToken = currentFolderToken();
    if (!folderToken) return;
    const selected = await openFileDialog({ multiple: false, directory: false }).catch(() => null);
    const filePath = Array.isArray(selected) ? selected[0] : selected;
    if (!filePath) return;
    const filename = filePath.split(/[\\/]/).pop() ?? filePath;
    try {
      const preview = await mediaUploadPrepare(folderToken, filePath, filename);
      dialog = { kind: "upload", preview, busy: false, filePath, filename, note: "" };
      paint();
    } catch (e) {
      toast = describeError(e);
      paint();
    }
  }

  // ── create folder ────────────────────────────────────────────────────────

  async function startCreateFolder(name: string): Promise<void> {
    const parentToken = currentFolderToken() ?? undefined;
    if (!parentToken && !weblogToken) {
      toast = "Can't create a root folder without a known weblog for this event.";
      paint();
      return;
    }
    try {
      const preview = await mediaFolderCreatePrepare(name, parentToken, parentToken ? undefined : weblogToken);
      dialog = { kind: "create_folder", preview, busy: false, folderName: name };
      paint();
    } catch (e) {
      toast = describeError(e);
      paint();
    }
  }

  // ── note ─────────────────────────────────────────────────────────────────

  async function startNote(target: { fileToken?: string; folderToken?: string }, current: string): Promise<void> {
    try {
      const preview = await mediaNoteUpdatePrepare(current, target);
      dialog = { kind: "note", preview, busy: false, note: current, noteTarget: target };
      paint();
    } catch (e) {
      toast = describeError(e);
      paint();
    }
  }

  // ── transcript / scale-down ──────────────────────────────────────────────

  async function startTranscript(fileToken: string): Promise<void> {
    try {
      const preview = await mediaTranscriptGeneratePrepare(fileToken);
      dialog = { kind: "transcript", preview, busy: false, fileToken };
      paint();
    } catch (e) {
      toast = describeError(e);
      paint();
    }
  }

  async function startScaleDown(fileToken: string): Promise<void> {
    try {
      const preview = await mediaScaleDownPrepare(fileToken);
      dialog = { kind: "scale_down", preview, busy: false, fileToken };
      paint();
    } catch (e) {
      toast = describeError(e);
      paint();
    }
  }

  function pollJob(fileToken: string, jobType: MediaJobType): void {
    const key = jobKey(fileToken, jobType);
    if (jobTimers.has(key)) return;
    const tick = async () => {
      try {
        const job = await fetchMediaJobStatus(fileToken, jobType);
        jobNotices.delete(key);
        if (job) jobs.set(key, job);
        if (job && jobType === "transcript" && job.status === "success" && !transcripts.has(fileToken)) {
          const tr = await getMediaTranscript(fileToken).catch(() => null);
          if (tr) transcripts.set(fileToken, tr);
        }
        if (job && jobType === "scale_down" && job.status === "success") {
          // The scaled file's metadata is already cached server-side by the
          // Rust layer — refresh the current folder listing so it appears.
          await refreshCurrentFolder();
        }
        if (!job || job.status === "success" || job.status === "failed") {
          const t = jobTimers.get(key);
          if (t) clearInterval(t);
          jobTimers.delete(key);
        }
        paint();
      } catch (e) {
        const err = e as { code?: string } | undefined;
        if (err?.code === "rate_limited") {
          jobNotices.set(key, "Rate limited — retrying shortly, keeping last known status.");
        }
        paint();
      }
    };
    void tick();
    jobTimers.set(key, setInterval(() => void tick(), JOB_POLL_MS));
  }

  async function refreshCurrentFolder(): Promise<void> {
    const folderToken = currentFolderToken();
    if (!folderToken) return;
    if (crumbs.length) {
      subView = await getMediaFolder(folderToken).catch(() => subView);
    } else if (meetupToken) {
      view = await getMediaView(meetupToken).catch(() => view);
    }
  }

  // ── confirm dialog ─────────────────────────────────────────────────────

  function onDialogCancel(): void {
    dialog = null;
    paint();
  }

  async function onDialogConfirm(): Promise<void> {
    if (!dialog) return;
    const active = dialog;
    active.busy = true;
    active.error = undefined;
    paint();
    try {
      if (active.kind === "upload") {
        await mediaUploadCommit(active.preview.token, active.preview.folder_token as string, active.filePath!, active.filename!, undefined, active.note || undefined);
        await refreshCurrentFolder();
      } else if (active.kind === "create_folder") {
        await mediaFolderCreateCommit(
          active.preview.token,
          active.preview.name as string,
          active.preview.parent_token as string | undefined,
          active.preview.weblog_token as string | undefined,
        );
        await refreshCurrentFolder();
        newFolderOpen = false;
      } else if (active.kind === "note") {
        await mediaNoteUpdateCommit(active.preview.token, active.note ?? "", active.noteTarget ?? {});
        await refreshCurrentFolder();
      } else if (active.kind === "transcript") {
        const job = await mediaTranscriptGenerateCommit(active.preview.token, active.fileToken!);
        jobs.set(jobKey(active.fileToken!, "transcript"), job);
        pollJob(active.fileToken!, "transcript");
      } else if (active.kind === "scale_down") {
        const job = await mediaScaleDownCommit(active.preview.token, active.fileToken!);
        jobs.set(jobKey(active.fileToken!, "scale_down"), job);
        pollJob(active.fileToken!, "scale_down");
      }
      dialog = null;
      paint();
    } catch (e) {
      active.busy = false;
      active.error = describeError(e);
      paint();
    }
  }

  async function onDownload(fileToken: string): Promise<void> {
    try {
      const d = await mediaFileDownload(fileToken);
      if (d.download_url) window.open(d.download_url, "_blank", "noopener");
    } catch (e) {
      toast = describeError(e);
      paint();
    }
  }

  // ── render ───────────────────────────────────────────────────────────────

  function paint(): void {
    const availability = view?.availability;
    root.innerHTML = `
      <div class="appbar">
        <img src="/logos/logo-stacked-760.png" alt="AI Tinkerers" />
        <span class="a-title">Mission Control</span>
        <span class="spacer"></span>
        <button class="refresh" id="mediaRefreshBtn">${loading ? "Syncing…" : "Refresh"}</button>
      </div>
      <div class="content">
        <button class="back" id="mediaBackBtn">← ${esc(eventName || "Event")}</button>
        <div class="d-head">
          <div>
            <h2>Media</h2>
            <div class="d-meta">${esc(eventName)} · rendering from local cache</div>
          </div>
        </div>
        ${toast ? `<div class="notice notice-err">${esc(toast)}</div>` : ""}
        ${!view ? `<div class="panel"><div class="empty"><div class="spinner"></div><span>Loading…</span></div></div>` : bodyHTML(availability)}
      </div>
      ${dialog ? dialogHTML(dialog) : ""}
    `;
    wire();
  }

  function bodyHTML(availability: MediaView["availability"] | undefined): string {
    if (availability?.unavailable) {
      return roleGatedPanelHTML(availability.reason ?? null);
    }
    if (!view?.folder && !crumbs.length) {
      return `<div class="panel">
        <div class="not-enabled"><b>No media folder linked to this event yet</b>
          Once a media folder is created for this event, its files and notes will appear here.</div>
        ${createFolderControlsHTML()}
      </div>`;
    }
    return `
      ${breadcrumbHTML()}
      <div class="panel">
        <div class="d-head">
          <h4>Files &amp; folders</h4>
          <button class="btn-ghost" id="mediaUploadBtn">Upload file</button>
        </div>
        ${createFolderControlsHTML()}
        ${folderContentsHTML()}
      </div>`;
  }

  function roleGatedPanelHTML(reason: string | null): string {
    const detail =
      reason === "forbidden_role"
        ? "Your key's role can't access the Media API group."
        : reason === "forbidden_scope"
          ? "Your key doesn't have the required scope for this chapter's media."
          : reason === "forbidden_api_group"
            ? "The Media API group is switched off (or out of scope) for this weblog."
            : "The Media API group refused this request for your key's access level.";
    return `<div class="panel">
      <div class="not-enabled">
        <b>Media isn't available for your role</b>
        The Media library is limited to index owners and video editors — city owners are not
        authorized to browse, upload, or transcribe media files. ${esc(detail)}
        Everything else on this event still works normally.
      </div>
    </div>`;
  }

  function breadcrumbHTML(): string {
    if (!crumbs.length) return "";
    const root = `<button class="linklike" data-crumb="0">${esc(view?.folder?.name ?? "Event folder")}</button>`;
    const rest = crumbs
      .map((c, i) => ` / <button class="linklike" data-crumb="${i + 1}">${esc(c.name)}</button>`)
      .join("");
    return `<div class="groups-note">${root}${rest}</div>`;
  }

  function createFolderControlsHTML(): string {
    return `<div class="d-head" style="margin-top:8px">
      <button class="btn-ghost" id="mediaNewFolderBtn">${newFolderOpen ? "Cancel" : "New subfolder"}</button>
    </div>
    ${
      newFolderOpen
        ? `<form id="mediaNewFolderForm" class="spk-form">
            <input type="text" id="mediaNewFolderName" placeholder="Folder name" required />
            <button class="btn" type="submit">Prepare confirmation</button>
          </form>`
        : ""
    }`;
  }

  function folderContentsHTML(): string {
    const subfolders = currentSubfolders();
    const files = currentFiles();
    if (!subfolders.length && !files.length) {
      return `<div class="not-enabled">This folder is empty — upload a file or create a subfolder.</div>`;
    }
    const folderRows = subfolders.map(folderRowHTML).join("");
    const fileRows = files.map(fileRowHTML).join("");
    return `<div class="job-list">${folderRows}${fileRows}</div>`;
  }

  function folderRowHTML(f: MediaFolder): string {
    return `<div class="job-row">
      <div class="job-main">
        <span class="job-chip idle">folder</span>
        <button class="linklike job-subj" data-open-folder="${esc(f.folder_token)}">${esc(f.name ?? "(untitled)")}</button>
      </div>
      <div class="job-counts">
        ${f.note ? `<span>${esc(f.note)}</span>` : ""}
        <button class="linklike" data-note-folder="${esc(f.folder_token)}" data-note-current="${esc(f.note ?? "")}">Edit note</button>
      </div>
    </div>`;
  }

  function fileRowHTML(f: MediaFile): string {
    const size = f.size_in_bytes != null ? humanSize(f.size_in_bytes) : "";
    const kind = (f.content_type ?? "").split("/")[0];
    const isMedia = kind === "video" || kind === "audio";
    const isVideo = kind === "video";
    const transcriptJob = jobs.get(jobKey(f.file_token, "transcript"));
    const scaleJob = jobs.get(jobKey(f.file_token, "scale_down"));
    const transcript = transcripts.get(f.file_token);

    return `<div class="job-row">
      <div class="job-main">
        <span class="job-chip idle">${esc(kind || "file")}</span>
        <span class="job-subj">${esc(f.filename ?? f.file_token)}</span>
        <span class="spacer"></span>
        <span>${esc(size)}</span>
      </div>
      <div class="job-counts">
        <span>${esc(f.uploader_name ?? "")}</span>
        <span>${esc(f.created_at ?? "")}</span>
        <button class="linklike" data-download="${esc(f.file_token)}">Download</button>
        <button class="linklike" data-note-file="${esc(f.file_token)}" data-note-current="${esc(f.note ?? "")}">Edit note</button>
      </div>
      ${f.note ? `<div class="job-counts"><span>Note: ${esc(f.note)}</span></div>` : ""}
      ${isMedia ? transcriptSectionHTML(f.file_token, f.has_transcript, transcriptJob, transcript) : ""}
      ${isVideo ? scaleDownSectionHTML(f.file_token, scaleJob) : ""}
    </div>`;
  }

  function transcriptSectionHTML(fileToken: string, hasTranscript: boolean, job: MediaJob | undefined, transcript: MediaTranscript | undefined): string {
    const notice = jobNotices.get(jobKey(fileToken, "transcript"));
    if (job && (job.status === "processing")) {
      return `<div class="job-counts"><span class="job-chip idle">Transcribing…</span>
        ${notice ? `<span>${esc(notice)}</span>` : ""}</div>`;
    }
    if (job && job.status === "failed") {
      return `<div class="job-counts">
        <span class="job-chip failed">Transcription failed${job.attempts ? ` (attempt ${job.attempts})` : ""}</span>
        <span>${esc(job.error_detail ?? "")}</span>
        <button class="linklike" data-transcript="${esc(fileToken)}">Retry</button>
      </div>`;
    }
    if (hasTranscript || (job && job.status === "success")) {
      const text = transcript?.transcript_text;
      return `<div class="job-counts"><span class="job-chip done">Transcript ready</span></div>
        ${text ? `<p class="page-body">${esc(text.slice(0, 400))}</p>` : ""}`;
    }
    return `<div class="job-counts"><button class="linklike" data-transcript="${esc(fileToken)}">Start transcription</button></div>`;
  }

  function scaleDownSectionHTML(fileToken: string, job: MediaJob | undefined): string {
    const notice = jobNotices.get(jobKey(fileToken, "scale_down"));
    if (job && job.status === "processing") {
      return `<div class="job-counts"><span class="job-chip idle">Scaling down…</span>
        ${notice ? `<span>${esc(notice)}</span>` : ""}</div>`;
    }
    if (job && job.status === "failed") {
      return `<div class="job-counts">
        <span class="job-chip failed">Scale-down failed</span>
        <span>${esc(job.error_detail ?? "")}</span>
        <button class="linklike" data-scaledown="${esc(fileToken)}">Retry</button>
      </div>`;
    }
    if (job && job.status === "success") {
      return `<div class="job-counts"><span class="job-chip done">Scaled file ready</span></div>`;
    }
    return `<div class="job-counts"><button class="linklike" data-scaledown="${esc(fileToken)}">Scale down</button></div>`;
  }

  function dialogHTML(d: DialogState): string {
    const title =
      d.kind === "upload" ? "Confirm upload" :
      d.kind === "create_folder" ? "Confirm new folder" :
      d.kind === "note" ? "Confirm note" :
      d.kind === "transcript" ? "Confirm start transcription" :
      "Confirm start scale-down";
    return `<div class="confirm-overlay">
      <div class="confirm-dialog">
        <h3>${title}</h3>
        ${dialogBodyHTML(d)}
        ${d.error ? `<div class="notice notice-err">${esc(d.error)}</div>` : ""}
        <div class="confirm-actions">
          <button class="btn-ghost" id="dlgCancel" ${d.busy ? "disabled" : ""}>Cancel</button>
          <button class="btn" id="dlgConfirm" ${d.busy ? "disabled" : ""}>${d.busy ? "Confirming…" : "Confirm"}</button>
        </div>
      </div>
    </div>`;
  }

  function dialogBodyHTML(d: DialogState): string {
    if (d.kind === "upload") {
      const size = typeof d.preview.size_in_bytes === "number" ? humanSize(d.preview.size_in_bytes as number) : "";
      return `<p class="confirm-body"><b>${esc(d.filename ?? "")}</b> (${esc(size)})</p>
        <textarea id="dlgNote" placeholder="Optional note…" rows="2">${esc(d.note ?? "")}</textarea>`;
    }
    if (d.kind === "create_folder") {
      return `<p class="confirm-body">Create folder <b>${esc(d.folderName ?? "")}</b>${d.preview.parent_token ? " inside the current folder" : " at the weblog root"}.</p>`;
    }
    if (d.kind === "note") {
      return `<textarea id="dlgNote" placeholder="Note (leave blank to clear)…" rows="3">${esc(d.note ?? "")}</textarea>`;
    }
    if (d.kind === "transcript") {
      return `<p class="confirm-body">Start AI transcription for this file. This is billed against a shared daily cap and cannot be undone once started.</p>`;
    }
    return `<p class="confirm-body">Start a video scale-down (a smaller, re-encoded copy) for this file.</p>`;
  }

  function wire(): void {
    byId<HTMLButtonElement>("mediaBackBtn").addEventListener("click", () => {
      stopAllPolling();
      opts.onBack();
    });
    byId<HTMLButtonElement>("mediaRefreshBtn").addEventListener("click", async () => {
      if (!meetupToken) return;
      loading = true;
      paint();
      try {
        view = await fetchMediaView(meetupToken);
        await refreshCurrentFolder();
      } catch {
        /* keep cache */
      }
      loading = false;
      paint();
    });

    document.getElementById("mediaUploadBtn")?.addEventListener("click", () => void pickFileAndPrepareUpload());
    document.getElementById("mediaNewFolderBtn")?.addEventListener("click", () => {
      newFolderOpen = !newFolderOpen;
      paint();
    });
    document.getElementById("mediaNewFolderForm")?.addEventListener("submit", (e) => {
      e.preventDefault();
      const name = (document.getElementById("mediaNewFolderName") as HTMLInputElement | null)?.value ?? "";
      if (!name.trim()) return;
      void startCreateFolder(name.trim());
    });

    for (const el of document.querySelectorAll<HTMLButtonElement>("[data-open-folder]")) {
      el.addEventListener("click", () => {
        const token = el.dataset.openFolder!;
        const list = crumbs.length ? subView?.subfolders : view?.subfolders;
        const folder = list?.find((f) => f.folder_token === token);
        if (folder) void enterFolder(folder);
      });
    }
    for (const el of document.querySelectorAll<HTMLButtonElement>("[data-crumb]")) {
      el.addEventListener("click", () => void backToCrumb(Number(el.dataset.crumb)));
    }
    for (const el of document.querySelectorAll<HTMLButtonElement>("[data-download]")) {
      el.addEventListener("click", () => void onDownload(el.dataset.download!));
    }
    for (const el of document.querySelectorAll<HTMLButtonElement>("[data-note-file]")) {
      el.addEventListener("click", () => void startNote({ fileToken: el.dataset.noteFile }, el.dataset.noteCurrent ?? ""));
    }
    for (const el of document.querySelectorAll<HTMLButtonElement>("[data-note-folder]")) {
      el.addEventListener("click", () => void startNote({ folderToken: el.dataset.noteFolder }, el.dataset.noteCurrent ?? ""));
    }
    for (const el of document.querySelectorAll<HTMLButtonElement>("[data-transcript]")) {
      el.addEventListener("click", () => void startTranscript(el.dataset.transcript!));
    }
    for (const el of document.querySelectorAll<HTMLButtonElement>("[data-scaledown]")) {
      el.addEventListener("click", () => void startScaleDown(el.dataset.scaledown!));
    }

    const noteEl = document.getElementById("dlgNote") as HTMLTextAreaElement | null;
    noteEl?.addEventListener("input", () => {
      if (dialog) dialog.note = noteEl.value;
    });
    document.getElementById("dlgCancel")?.addEventListener("click", onDialogCancel);
    document.getElementById("dlgConfirm")?.addEventListener("click", () => void onDialogConfirm());
  }

  return { open };
}

function humanSize(bytes: number): string {
  if (bytes < 1024) return `${fmt(bytes)} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

function describeError(e: unknown): string {
  const err = e as { code?: string; message?: string } | undefined;
  const code = err?.code;
  const message = err?.message;
  if (code === "confirmation_required") {
    return message || "That confirmation is no longer valid — please try again.";
  }
  if (code === "rate_limited") {
    return message || "Rate limited by the AI Tinkerers API — please wait and re-confirm.";
  }
  if (code === "forbidden_scope" || code === "forbidden_role" || code === "forbidden_api_group") {
    return message || "This action was refused by the API for your key's access level.";
  }
  return message || "Something went wrong — please try again.";
}
