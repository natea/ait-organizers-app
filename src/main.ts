// App shell + router. Screens render from the SQLite cache via Tauri commands;
// the sync engine pushes "sync:updated" / "detail:updated" events to re-render.
import "./styles.css";
import { hasKey, onDetailUpdated, onSyncUpdated } from "./api";
import { renderOnboarding } from "./screens/onboarding";
import { mountOverview, type OverviewController } from "./screens/overview";
import { mountDetail, type DetailController } from "./screens/detail";
import { mountEmail, type EmailController } from "./screens/email";
import { byId } from "./util";

type ScreenId = "scr-onboarding" | "scr-overview" | "scr-detail" | "scr-email";

let overview: OverviewController | null = null;
let detail: DetailController | null = null;
let email: EmailController | null = null;
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
    onOpenEmail: () => {
      show("scr-email");
      email?.open();
    },
  });
  detail = mountDetail({
    onBack: () => {
      currentDetailToken = null;
      show("scr-overview");
      overview?.reload();
    },
  });
  email = mountEmail({
    onBack: () => {
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
    // Re-render from cache only. Calling open() here would re-fetch, which
    // re-emits "detail:updated" → infinite loop (logo flicker + API hammering).
    if (token === currentDetailToken) detail?.refresh(token);
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
