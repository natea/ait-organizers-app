// App shell + router. Screens render from the SQLite cache via Tauri commands;
// the sync engine pushes "sync:updated" / "detail:updated" events to re-render.
import "./styles.css";
import { hasKey, onDetailUpdated, onSyncUpdated } from "./api";
import { renderOnboarding } from "./screens/onboarding";
import { mountOverview, type OverviewController } from "./screens/overview";
import { mountDetail, type DetailController } from "./screens/detail";
import { mountEmail, type EmailController } from "./screens/email";
import { mountSponsors, type SponsorsController } from "./screens/sponsors";
import { mountScreening, type ScreeningController } from "./screens/screening";
import { mountSettings, type SettingsController } from "./screens/settings";

type ScreenId =
  | "scr-onboarding"
  | "scr-overview"
  | "scr-detail"
  | "scr-email"
  | "scr-sponsors"
  | "scr-screening"
  | "scr-settings";

let overview: OverviewController | null = null;
let detail: DetailController | null = null;
let email: EmailController | null = null;
let sponsors: SponsorsController | null = null;
let screening: ScreeningController | null = null;
let settings: SettingsController | null = null;
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
    onOpenSponsors: () => {
      show("scr-sponsors");
      sponsors?.open();
    },
    onOpenSettings: () => {
      show("scr-settings");
      settings?.open();
    },
  });
  detail = mountDetail({
    onBack: () => {
      currentDetailToken = null;
      show("scr-overview");
      overview?.reload();
    },
    onOpenScreening: (token, name) => {
      show("scr-screening");
      void screening?.open(token, name);
    },
  });
  screening = mountScreening({
    onBack: () => {
      if (currentDetailToken) {
        show("scr-detail");
        void detail?.refresh(currentDetailToken);
      } else {
        show("scr-overview");
        overview?.reload();
      }
    },
  });
  email = mountEmail({
    onBack: () => {
      show("scr-overview");
      overview?.reload();
    },
  });
  sponsors = mountSponsors({
    onBack: () => {
      show("scr-overview");
      overview?.reload();
    },
  });
  settings = mountSettings({
    onBack: () => {
      show("scr-overview");
      overview?.reload();
    },
    onChangeKey: goOnboarding,
    onSignedOut: goOnboarding,
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
    goOnboarding();
  }
}

// Show onboarding to (re)enter a key; on success, resume the app.
function goOnboarding(): void {
  renderOnboarding(() => {
    // Mount app screens after successful onboarding.
    if (!overview) enterApp();
    else show("scr-overview");
  });
  show("scr-onboarding");
}

boot();
