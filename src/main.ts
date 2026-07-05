// App shell + router. Screens render from the SQLite cache via Tauri commands;
// the sync engine pushes "sync:updated" / "detail:updated" events to re-render.
import "./styles.css";
import { hasKey, onDetailUpdated, onSyncUpdated } from "./api";
import { renderOnboarding } from "./screens/onboarding";
import { mountOverview, type OverviewController } from "./screens/overview";
import { mountDetail, type DetailController } from "./screens/detail";
import { byId } from "./util";

type ScreenId = "scr-onboarding" | "scr-overview" | "scr-detail";

let overview: OverviewController | null = null;
let detail: DetailController | null = null;
let currentDetailToken: string | null = null;

function show(id: ScreenId): void {
  for (const el of document.querySelectorAll<HTMLElement>(".screen")) {
    el.classList.toggle("active", el.id === id);
  }
}

function enterApp(): void {
  overview = mountOverview({
    onOpenDetail: (token) => {
      currentDetailToken = token;
      detail?.open(token);
      show("scr-detail");
    },
  });
  detail = mountDetail({
    onBack: () => {
      currentDetailToken = null;
      show("scr-overview");
      overview?.reload();
    },
  });
  show("scr-overview");
}

async function boot(): Promise<void> {
  // Live updates from the sync engine.
  await onSyncUpdated(() => {
    overview?.reload();
  });
  await onDetailUpdated((token) => {
    if (token === currentDetailToken) detail?.open(token);
  });

  let onboarded = false;
  try {
    onboarded = await hasKey();
  } catch {
    onboarded = false;
  }

  if (onboarded) {
    enterApp();
  } else {
    renderOnboarding(() => {
      // Mount app screens after successful onboarding.
      if (!overview) enterApp();
      else show("scr-overview");
    });
    show("scr-onboarding");
  }

  // Titlebar label is set in index.html; nothing else needed here.
  void byId;
}

boot();
