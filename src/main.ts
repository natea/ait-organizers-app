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
import { mountSpeakers, type SpeakersController } from "./screens/speakers";
import { mountCheckin, type CheckinController } from "./screens/checkin";
import { mountNetworking, type NetworkingController } from "./screens/networking";
import { mountMedia, type MediaController } from "./screens/media";
import { mountSettings, type SettingsController } from "./screens/settings";

type ScreenId =
  | "scr-onboarding"
  | "scr-overview"
  | "scr-detail"
  | "scr-email"
  | "scr-sponsors"
  | "scr-screening"
  | "scr-speakers"
  | "scr-checkin"
  | "scr-boards"
  | "scr-media"
  | "scr-settings";

let overview: OverviewController | null = null;
let detail: DetailController | null = null;
let email: EmailController | null = null;
let sponsors: SponsorsController | null = null;
let screening: ScreeningController | null = null;
let speakers: SpeakersController | null = null;
let checkin: CheckinController | null = null;
let boards: NetworkingController | null = null;
let media: MediaController | null = null;
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
    onOpenCheckin: () => {
      show("scr-checkin");
      void checkin?.open();
    },
    onOpenBoards: () => {
      show("scr-boards");
      void boards?.open();
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
    onOpenSpeakers: (token, name) => {
      show("scr-speakers");
      void speakers?.open(token, name);
    },
    onOpenMedia: (token, name, weblogToken) => {
      show("scr-media");
      void media?.open(token, name, weblogToken);
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
  speakers = mountSpeakers({
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
  checkin = mountCheckin({
    onBack: () => {
      show("scr-overview");
      overview?.reload();
    },
  });
  boards = mountNetworking({
    onBack: () => {
      show("scr-overview");
      overview?.reload();
    },
  });
  media = mountMedia({
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
