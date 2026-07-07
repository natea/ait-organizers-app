// Networking / Connect (specs/networking-connect) — the app's fourth
// write-capable screen. Reads (boards, board messages, threads, the
// cross-board Attention inbox) render only from the SQLite cache. Every
// mutation — post/reply, reaction toggle, attachment upload, DM — goes
// through the same two-step prepare/commit confirmation gate as
// rsvp-screening, attendance-checkin, and speaker-review: `_prepare` makes no
// network call and returns a preview for the confirm dialog; `_commit` must
// echo back the identical arguments plus the bound token, or the write
// guardrail rejects it.
import {
  directMessageCommit,
  directMessagePrepare,
  fetchBoardMessages,
  fetchThread,
  getBoardMessages,
  getFlaggedPosts,
  getNetworkingBoards,
  getThread,
  onNetworkingBoardForbidden,
  onNetworkingBoardUpdated,
  onNetworkingBoardsUpdated,
  onNetworkingFlaggedUpdated,
  onNetworkingThreadUpdated,
  onNetworkingWriteSettled,
  postCreateCommit,
  postCreatePrepare,
  reactionToggleCommit,
  reactionTogglePrepare,
  refreshFlaggedPosts,
  refreshNetworking,
} from "../api";
import type {
  AppErr,
  Board,
  BoardMessage,
  FlaggedPost,
  Thread,
  WritePreview,
} from "../types";
import { byId, esc, fmt } from "../util";

interface NetworkingOpts {
  onBack: () => void;
}

export interface NetworkingController {
  open: () => Promise<void>;
}

type DialogKind = "post" | "reply" | "reaction" | "attachment" | "dm";

interface DialogState {
  kind: DialogKind;
  preview: WritePreview;
  boardKey?: string;
  postToken?: string;
  content?: string;
  title?: string;
  imageUrls?: string[];
  reactionType?: string;
  clientRefs?: string[];
  emails?: string[];
  busy: boolean;
  error?: string;
}

const REACTIONS = ["thumbs_up", "fire", "love", "haha", "100", "trophy", "rocket"] as const;

export function mountNetworking(opts: NetworkingOpts): NetworkingController {
  const root = byId("scr-boards");

  let boards: Board[] = [];
  let mentions: FlaggedPost[] = [];
  let needsResponse: FlaggedPost[] = [];
  let selectedBoard: Board | null = null;
  let messages: BoardMessage[] = [];
  let openThread: Thread | null = null;
  let openRootToken: string | null = null;
  let filterMentions = false;
  let filterNeedsResponse = false;
  let loading = false;
  let boardsUnavailable: { unavailable: boolean; reason?: string | null } | null = null;

  let composerOpen = false;
  let composerContent = "";
  let composerTitle = "";
  let composerImageUrls = "";
  let replyContent = "";

  let dmOpen = false;
  let dmRecipients = "";
  let dmContent = "";

  let dialog: DialogState | null = null;
  let toast: string | null = null;

  let unlistenBoards: (() => void) | null = null;
  let unlistenFlagged: (() => void) | null = null;
  let unlistenBoardUpdated: (() => void) | null = null;
  let unlistenBoardForbidden: (() => void) | null = null;
  let unlistenThreadUpdated: (() => void) | null = null;
  let unlistenSettled: (() => void) | null = null;

  async function open(): Promise<void> {
    dialog = null;
    toast = null;

    if (!unlistenBoards) unlistenBoards = await onNetworkingBoardsUpdated(() => void loadCachedBoards());
    if (!unlistenFlagged) unlistenFlagged = await onNetworkingFlaggedUpdated(() => void loadCachedFlagged());
    if (!unlistenBoardUpdated) {
      unlistenBoardUpdated = await onNetworkingBoardUpdated((bk) => {
        if (selectedBoard?.board_key === bk) void loadCachedMessages();
      });
    }
    if (!unlistenBoardForbidden) {
      unlistenBoardForbidden = await onNetworkingBoardForbidden((bk) => {
        if (selectedBoard?.board_key === bk) {
          selectedBoard = null;
          messages = [];
          openThread = null;
          toast = "That board is no longer accessible with your key — it's been removed.";
        }
        void loadCachedBoards();
      });
    }
    if (!unlistenThreadUpdated) {
      unlistenThreadUpdated = await onNetworkingThreadUpdated((bk, root) => {
        if (selectedBoard?.board_key === bk && openRootToken === root) void loadCachedThread();
      });
    }
    if (!unlistenSettled) {
      unlistenSettled = await onNetworkingWriteSettled(() => {
        void loadCachedMessages();
        if (openRootToken) void loadCachedThread();
      });
    }

    await loadCachedBoards();
    await loadCachedFlagged();
    loading = true;
    paint();
    try {
      await refreshNetworking();
    } catch {
      /* keep cached render; degrade state read below */
    }
    await loadFeatureState();
    await loadCachedBoards();
    await loadCachedFlagged();
    loading = false;
    paint();
  }

  async function loadCachedBoards(): Promise<void> {
    try {
      boards = await getNetworkingBoards();
    } catch {
      /* keep prior boards */
    }
    paint();
  }

  async function loadCachedFlagged(): Promise<void> {
    try {
      mentions = await getFlaggedPosts("mentioned_me");
      needsResponse = await getFlaggedPosts("needs_response");
    } catch {
      /* keep prior */
    }
    paint();
  }

  async function loadFeatureState(): Promise<void> {
    // Cache-only signal: an empty boards list plus a still-loading state
    // reads as "no access yet", not an error — the not-enabled copy branches
    // off explicit degrade events instead of guessing from an empty list.
    boardsUnavailable = null;
  }

  async function loadCachedMessages(): Promise<void> {
    if (!selectedBoard) return;
    try {
      messages = await getBoardMessages(selectedBoard.board_key);
    } catch {
      /* keep prior */
    }
    paint();
  }

  async function loadCachedThread(): Promise<void> {
    if (!selectedBoard || !openRootToken) return;
    try {
      openThread = await getThread(selectedBoard.board_key, openRootToken);
    } catch {
      openThread = null;
    }
    paint();
  }

  async function selectBoard(b: Board): Promise<void> {
    selectedBoard = b;
    openThread = null;
    openRootToken = null;
    filterMentions = false;
    filterNeedsResponse = false;
    composerOpen = false;
    await loadCachedMessages();
    paint();
    try {
      messages = await fetchBoardMessages(b.board_key, false, false);
    } catch {
      /* keep cache */
    }
    paint();
  }

  async function applyFilter(): Promise<void> {
    if (!selectedBoard) return;
    loading = true;
    paint();
    try {
      messages = await fetchBoardMessages(selectedBoard.board_key, filterMentions, filterNeedsResponse);
    } catch {
      /* keep cache */
    }
    loading = false;
    paint();
  }

  async function openThreadFor(postToken: string, boardKey?: string): Promise<void> {
    const bk = boardKey ?? selectedBoard?.board_key;
    if (bk && (!selectedBoard || selectedBoard.board_key !== bk)) {
      selectedBoard = boards.find((b) => b.board_key === bk) ?? { board_key: bk, is_dm: false, updated_at: "" };
    }
    openRootToken = postToken;
    await loadCachedThread();
    try {
      openThread = await fetchThread(postToken, bk);
      openRootToken = openThread?.root_post_token ?? postToken;
    } catch {
      /* keep cache */
    }
    paint();
  }

  function closeThread(): void {
    openThread = null;
    openRootToken = null;
    paint();
  }

  // ── composer / reply / reaction / DM ────────────────────────────────────

  async function startPost(): Promise<void> {
    if (!selectedBoard || !composerContent.trim()) return;
    const urls = parseImageUrls(composerImageUrls);
    try {
      const preview = await postCreatePrepare(selectedBoard.board_key, composerContent.trim(), {
        title: composerTitle.trim() || undefined,
        imageUrls: urls.length ? urls : undefined,
      });
      dialog = {
        kind: "post",
        preview,
        boardKey: selectedBoard.board_key,
        content: composerContent.trim(),
        title: composerTitle.trim() || undefined,
        imageUrls: urls,
        busy: false,
      };
      paint();
    } catch (e) {
      toast = describeError(e);
      paint();
    }
  }

  async function startReply(postToken: string): Promise<void> {
    if (!selectedBoard || !replyContent.trim()) return;
    try {
      const preview = await postCreatePrepare(selectedBoard.board_key, replyContent.trim(), {
        replyToPostToken: postToken,
      });
      dialog = {
        kind: "reply",
        preview,
        boardKey: selectedBoard.board_key,
        postToken,
        content: replyContent.trim(),
        busy: false,
      };
      paint();
    } catch (e) {
      toast = describeError(e);
      paint();
    }
  }

  async function startReaction(postToken: string, reactionType: string): Promise<void> {
    if (!selectedBoard) return;
    try {
      const preview = await reactionTogglePrepare(selectedBoard.board_key, postToken, reactionType);
      dialog = {
        kind: "reaction",
        preview,
        boardKey: selectedBoard.board_key,
        postToken,
        reactionType,
        busy: false,
      };
      paint();
    } catch (e) {
      toast = describeError(e);
      paint();
    }
  }

  async function startDm(): Promise<void> {
    if (!dmContent.trim()) return;
    const { clientRefs, emails } = parseRecipients(dmRecipients);
    if (!clientRefs.length && !emails.length) {
      toast = "Enter at least one recipient (client token or email).";
      paint();
      return;
    }
    try {
      const preview = await directMessagePrepare(dmContent.trim(), { clientRefs, emails });
      dialog = {
        kind: "dm",
        preview,
        content: dmContent.trim(),
        clientRefs,
        emails,
        busy: false,
      };
      paint();
    } catch (e) {
      toast = describeError(e);
      paint();
    }
  }

  async function onDialogCancel(): Promise<void> {
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
      if (active.kind === "post") {
        await postCreateCommit(active.preview.token, active.boardKey!, active.content!, {
          title: active.title,
          imageUrls: active.imageUrls?.length ? active.imageUrls : undefined,
        });
        composerOpen = false;
        composerContent = "";
        composerTitle = "";
        composerImageUrls = "";
      } else if (active.kind === "reply") {
        await postCreateCommit(active.preview.token, active.boardKey!, active.content!, {
          replyToPostToken: active.postToken,
        });
            replyContent = "";
      } else if (active.kind === "reaction") {
        await reactionToggleCommit(active.preview.token, active.boardKey!, active.postToken!, active.reactionType!);
      } else if (active.kind === "dm") {
        await directMessageCommit(active.preview.token, active.content!, {
          clientRefs: active.clientRefs,
          emails: active.emails,
        });
        dmOpen = false;
        dmContent = "";
        dmRecipients = "";
      }
      dialog = null;
      await loadCachedMessages();
      if (openRootToken) await loadCachedThread();
      paint();
    } catch (e) {
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
        <button class="refresh" id="netRefreshBtn">${loading ? "Syncing…" : "Refresh"}</button>
      </div>
      <div class="content">
        <button class="back" id="netBackBtn">← All events</button>
        <div class="d-head">
          <div>
            <h2>Boards</h2>
            <div class="d-meta">Message boards and direct messages · rendering from local cache</div>
          </div>
        </div>
        ${toast ? `<div class="notice notice-err">${esc(toast)}</div>` : ""}
        ${degradeBannerHTML(boardsUnavailable)}
        <div class="net-layout">
          ${leftPaneHTML()}
          ${rightPaneHTML()}
        </div>
      </div>
      ${dialog ? dialogHTML(dialog) : ""}
    `;
    wire();
  }

  function leftPaneHTML(): string {
    return `<div class="net-left panel">
      ${attentionHTML()}
      <div class="d-head">
        <h4>Boards <span class="b-count">${fmt(boards.length)}</span></h4>
        <button class="btn-ghost" id="netDmBtn">${dmOpen ? "Cancel" : "New DM"}</button>
      </div>
      ${dmOpen ? dmFormHTML() : ""}
      ${
        boards.length
          ? `<div class="net-board-list">${boards.map(boardRowHTML).join("")}</div>`
          : `<div class="not-enabled">No boards cached yet for your key.</div>`
      }
    </div>`;
  }

  function attentionHTML(): string {
    const total = mentions.length + needsResponse.length;
    if (!total) return "";
    return `<div class="net-attention">
      <div class="d-head"><h4>Attention <span class="b-count">${fmt(total)}</span></h4></div>
      ${mentions.length ? attentionGroupHTML("Mentions you", mentions) : ""}
      ${needsResponse.length ? attentionGroupHTML("Needs a response", needsResponse) : ""}
    </div>`;
  }

  function attentionGroupHTML(label: string, rows: FlaggedPost[]): string {
    return `<div class="net-attn-group">
      <div class="groups-note">${esc(label)}</div>
      ${rows
        .slice(0, 8)
        .map(
          (r) => `<div class="job-row" data-open-flagged="${esc(r.post_token)}" data-flagged-board="${esc(r.board_key ?? "")}">
            <div class="job-main">
              <span class="job-subj">${esc(boardTitle(r.board_key) ?? r.board_key ?? "(board)")}</span>
            </div>
          </div>`,
        )
        .join("")}
    </div>`;
  }

  function boardTitle(boardKey?: string | null): string | undefined {
    return boards.find((b) => b.board_key === boardKey)?.title ?? undefined;
  }

  function boardRowHTML(b: Board): string {
    const active = selectedBoard?.board_key === b.board_key ? " on" : "";
    return `<button class="net-board-row${active}" data-board="${esc(b.board_key)}">
      <span class="net-board-title">${esc(b.title ?? b.board_key)}${b.is_dm ? ` <small>DM</small>` : ""}</span>
      ${b.unread_count ? `<span class="b-count live">${fmt(b.unread_count)}</span>` : ""}
    </button>`;
  }

  function dmFormHTML(): string {
    return `<form id="netDmForm" class="spk-form">
      <input type="text" id="netDmRecipients" placeholder="Recipient client tokens / emails (comma-separated)" value="${esc(dmRecipients)}" required />
      <textarea id="netDmContent" placeholder="Message" rows="3" required>${esc(dmContent)}</textarea>
      <button class="btn" type="submit">Prepare confirmation</button>
    </form>`;
  }

  function rightPaneHTML(): string {
    if (openThread || openRootToken) return threadPaneHTML();
    if (selectedBoard) return boardPaneHTML();
    return `<div class="net-right panel"><div class="not-enabled">Select a board to view its recent messages.</div></div>`;
  }

  function boardPaneHTML(): string {
    const board = selectedBoard!;
    return `<div class="net-right panel">
      <div class="d-head">
        <h4>${esc(board.title ?? board.board_key)}</h4>
        <button class="btn-ghost" id="netComposerToggle">${composerOpen ? "Cancel" : "New post"}</button>
      </div>
      <div class="net-filters">
        <label><input type="checkbox" id="netFilterMentions" ${filterMentions ? "checked" : ""} /> Mentions me</label>
        <label><input type="checkbox" id="netFilterNeeds" ${filterNeedsResponse ? "checked" : ""} /> Needs response</label>
      </div>
      ${composerOpen ? composerHTML() : ""}
      ${
        messages.length
          ? `<div class="net-message-list">${messages.map(messageRowHTML).join("")}</div>`
          : `<div class="not-enabled">No messages cached for this board yet.</div>`
      }
    </div>`;
  }

  function composerHTML(): string {
    return `<form id="netComposerForm" class="spk-form">
      <input type="text" id="netComposerTitle" placeholder="Title (topic boards only)" value="${esc(composerTitle)}" />
      <textarea id="netComposerContent" placeholder="Post content" rows="3" required>${esc(composerContent)}</textarea>
      <input type="text" id="netComposerImages" placeholder="Image URLs (comma-separated, up to 4)" value="${esc(composerImageUrls)}" />
      <button class="btn" type="submit">Prepare confirmation</button>
    </form>`;
  }

  function messageRowHTML(m: BoardMessage): string {
    const flags = [
      m.mentioned_me ? `<span class="chip">mention</span>` : "",
      m.needs_response ? `<span class="chip">needs response</span>` : "",
    ].join("");
    return `<div class="net-message">
      <div class="rsvp-who">
        <b>${esc(m.author ?? "(unknown)")}</b>
        ${m.posted_at ? `<small>${esc(m.posted_at)}</small>` : ""}
        ${flags}
      </div>
      ${m.title ? `<div class="kanban-card-title">${esc(m.title)}</div>` : ""}
      <p class="page-body">${esc((m.content_text ?? "").slice(0, 400))}</p>
      <div class="rsvp-actions">
        <button class="btn-ghost" data-open-thread="${esc(m.post_token)}">Open thread</button>
        ${REACTIONS.slice(0, 3)
          .map((r) => `<button class="btn-ghost" data-react="${r}" data-react-post="${esc(m.post_token)}">${reactionEmoji(r)}</button>`)
          .join("")}
      </div>
    </div>`;
  }

  function threadPaneHTML(): string {
    const posts = (openThread?.posts ?? []) as BoardMessage[];
    return `<div class="net-right panel">
      <div class="d-head">
        <h4>Thread</h4>
        <button class="btn-ghost" id="netThreadClose">← Back to board</button>
      </div>
      ${
        !openThread
          ? `<div class="empty"><div class="spinner"></div><span>Loading thread…</span></div>`
          : `<div class="net-thread">${posts.map(threadPostHTML).join("")}</div>`
      }
      ${openThread?.truncated ? `<div class="notice">Showing a recent window of this thread — not the full history.</div>` : ""}
      ${replyFormHTML()}
    </div>`;
  }

  function threadPostHTML(p: BoardMessage): string {
    return `<div class="net-message">
      <div class="rsvp-who">
        <b>${esc(p.author ?? "(unknown)")}</b>
        ${p.posted_at ? `<small>${esc(p.posted_at)}</small>` : ""}
      </div>
      <p class="page-body">${esc((p.content_text ?? "").slice(0, 800))}</p>
      <div class="rsvp-actions">
        ${REACTIONS.slice(0, 3)
          .map((r) => `<button class="btn-ghost" data-react="${r}" data-react-post="${esc(p.post_token)}">${reactionEmoji(r)}</button>`)
          .join("")}
      </div>
    </div>`;
  }

  function replyFormHTML(): string {
    return `<form id="netReplyForm" class="spk-form">
      <textarea id="netReplyContent" placeholder="Reply to this thread…" rows="2" required>${esc(replyContent)}</textarea>
      <button class="btn" type="submit">Prepare confirmation</button>
    </form>`;
  }

  function dialogHTML(d: DialogState): string {
    const title = dialogTitle(d);
    return `<div class="confirm-overlay">
      <div class="confirm-dialog">
        <h3>${esc(title)}</h3>
        ${dialogBodyHTML(d)}
        ${d.error ? `<div class="notice notice-err">${esc(d.error)}</div>` : ""}
        <div class="confirm-actions">
          <button class="btn-ghost" id="dlgCancel" ${d.busy ? "disabled" : ""}>Cancel</button>
          <button class="btn" id="dlgConfirm" ${d.busy ? "disabled" : ""}>${d.busy ? "Confirming…" : "Confirm"}</button>
        </div>
      </div>
    </div>`;
  }

  function dialogTitle(d: DialogState): string {
    if (d.kind === "post") return "Create post";
    if (d.kind === "reply") return "Post reply";
    if (d.kind === "reaction") return "Toggle reaction";
    if (d.kind === "dm") return "Send direct message";
    return "Confirm";
  }

  function dialogBodyHTML(d: DialogState): string {
    if (d.kind === "reaction") {
      return `<p class="confirm-body">React ${reactionEmoji(d.reactionType!)} <b>${esc(d.reactionType!)}</b> to this post.</p>`;
    }
    if (d.kind === "dm") {
      const to = [...(d.clientRefs ?? []), ...(d.emails ?? [])].join(", ");
      return `<p class="confirm-body">To: <b>${esc(to)}</b></p><p class="page-body">${esc((d.content ?? "").slice(0, 400))}</p>`;
    }
    const boardLabel = boardTitle(d.boardKey) ?? d.boardKey;
    return `<p class="confirm-body">Board: <b>${esc(boardLabel ?? "")}</b></p>
      ${d.title ? `<p class="confirm-body"><b>${esc(d.title)}</b></p>` : ""}
      <p class="page-body">${esc((d.content ?? "").slice(0, 400))}</p>
      ${d.imageUrls?.length ? `<p class="groups-note">${d.imageUrls.length} image URL(s) attached.</p>` : ""}`;
  }

  function wire(): void {
    byId<HTMLButtonElement>("netBackBtn").addEventListener("click", opts.onBack);
    byId<HTMLButtonElement>("netRefreshBtn").addEventListener("click", async () => {
      loading = true;
      paint();
      try {
        await refreshNetworking();
        await refreshFlaggedPosts();
      } catch {
        /* keep cache */
      }
      await loadCachedBoards();
      await loadCachedFlagged();
      if (selectedBoard) await loadCachedMessages();
      loading = false;
      paint();
    });

    for (const el of document.querySelectorAll<HTMLButtonElement>("[data-board]")) {
      el.addEventListener("click", () => {
        const b = boards.find((x) => x.board_key === el.dataset.board);
        if (b) void selectBoard(b);
      });
    }
    for (const el of document.querySelectorAll<HTMLElement>("[data-open-flagged]")) {
      el.addEventListener("click", () => {
        const token = el.dataset.openFlagged!;
        const bk = el.dataset.flaggedBoard || undefined;
        void openThreadFor(token, bk);
      });
    }
    for (const el of document.querySelectorAll<HTMLButtonElement>("[data-open-thread]")) {
      el.addEventListener("click", () => void openThreadFor(el.dataset.openThread!));
    }
    for (const el of document.querySelectorAll<HTMLButtonElement>("[data-react]")) {
      el.addEventListener("click", () => void startReaction(el.dataset.reactPost!, el.dataset.react!));
    }

    document.getElementById("netComposerToggle")?.addEventListener("click", () => {
      composerOpen = !composerOpen;
      paint();
    });
    document.getElementById("netComposerForm")?.addEventListener("submit", (e) => {
      e.preventDefault();
      composerTitle = (document.getElementById("netComposerTitle") as HTMLInputElement).value;
      composerContent = (document.getElementById("netComposerContent") as HTMLTextAreaElement).value;
      composerImageUrls = (document.getElementById("netComposerImages") as HTMLInputElement).value;
      void startPost();
    });

    document.getElementById("netFilterMentions")?.addEventListener("change", (e) => {
      filterMentions = (e.target as HTMLInputElement).checked;
      void applyFilter();
    });
    document.getElementById("netFilterNeeds")?.addEventListener("change", (e) => {
      filterNeedsResponse = (e.target as HTMLInputElement).checked;
      void applyFilter();
    });

    document.getElementById("netThreadClose")?.addEventListener("click", closeThread);
    document.getElementById("netReplyForm")?.addEventListener("submit", (e) => {
      e.preventDefault();
      replyContent = (document.getElementById("netReplyContent") as HTMLTextAreaElement).value;
      if (openRootToken) void startReply(openRootToken);
    });

    document.getElementById("netDmBtn")?.addEventListener("click", () => {
      dmOpen = !dmOpen;
      paint();
    });
    document.getElementById("netDmForm")?.addEventListener("submit", (e) => {
      e.preventDefault();
      dmRecipients = (document.getElementById("netDmRecipients") as HTMLInputElement).value;
      dmContent = (document.getElementById("netDmContent") as HTMLTextAreaElement).value;
      void startDm();
    });

    document.getElementById("dlgCancel")?.addEventListener("click", () => void onDialogCancel());
    document.getElementById("dlgConfirm")?.addEventListener("click", () => void onDialogConfirm());
  }

  return { open };
}

function parseImageUrls(raw: string): string[] {
  return raw
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean)
    .slice(0, 4);
}

function parseRecipients(raw: string): { clientRefs: string[]; emails: string[] } {
  const parts = raw
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean);
  const emails = parts.filter((p) => p.includes("@"));
  const clientRefs = parts.filter((p) => !p.includes("@"));
  return { clientRefs, emails };
}

function reactionEmoji(t: string): string {
  const map: Record<string, string> = {
    thumbs_up: "👍",
    thumbs_down: "👎",
    love: "❤️",
    haha: "😂",
    fire: "🔥",
    "100": "💯",
    trophy: "🏆",
    pizza: "🍕",
    rocket: "🚀",
    robot: "🤖",
    rainbow: "🌈",
    salute: "🫡",
    gen_ai: "✨",
  };
  return map[t] ?? t;
}

function degradeBannerHTML(fs: { unavailable: boolean; reason?: string | null } | null): string {
  if (!fs || !fs.unavailable) return "";
  if (fs.reason === "forbidden_scope") {
    return `<div class="panel"><div class="not-enabled"><b>Needs city-owner access</b>
      Your key doesn't have city-owner scope for this chapter, so boards can't sync.</div></div>`;
  }
  if (fs.reason === "forbidden_api_group") {
    return `<div class="panel"><div class="not-enabled"><b>Not enabled for your chapter</b>
      The message boards API group is switched off. Cached data (if any) still renders below.</div></div>`;
  }
  return `<div class="panel"><div class="not-enabled">Boards unavailable right now — showing cached data.</div></div>`;
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
    return message || "That board or post isn't cached yet — refresh first.";
  }
  return message || "Something went wrong — please try again.";
}
