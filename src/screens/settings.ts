// Settings: view the connected key's identity, change the API key, toggle
// notifications, and sign out (specs/api-auth). Renders from get_identity +
// notification state; the key itself is never shown or returned to the UI.
import {
  getIdentity,
  getNotificationsEnabled,
  setNotificationsEnabled,
  signOut,
} from "../api";
import type { Identity } from "../types";
import { byId, esc } from "../util";

const GROUP_LABELS: Record<string, string> = {
  people_events: "events",
  subscribers_sponsors: "subscribers",
  docs_rag: "docs",
  hackathons: "hackathons",
  media: "media",
};

interface SettingsOpts {
  onBack: () => void;
  /** Route to onboarding to enter a new key (validate_and_store overwrites it). */
  onChangeKey: () => void;
  /** Route to onboarding after the key + cache are cleared. */
  onSignedOut: () => void;
}

export interface SettingsController {
  open: () => Promise<void>;
}

export function mountSettings(opts: SettingsOpts): SettingsController {
  const root = byId("scr-settings");

  async function open(): Promise<void> {
    root.innerHTML = `<div class="content"><div class="empty"><div class="spinner"></div></div></div>`;
    let identity: Identity | null = null;
    try {
      identity = await getIdentity();
    } catch {
      identity = null; // offline / rate-limited — show cached-ish fallback
    }
    let notif = true;
    try {
      notif = await getNotificationsEnabled();
    } catch {
      /* default on */
    }
    render(identity, notif);
  }

  function render(identity: Identity | null, notif: boolean): void {
    const owner = identity?.owner ?? {};
    const auth = identity?.authorization ?? {};
    const roles = (auth.caller_roles ?? []).join(", ") || "member";
    const groups = auth.enabled_api_groups ?? [];
    const chips =
      groups.map((g) => `<span class="chip on">${esc(GROUP_LABELS[g] ?? g)}</span>`).join("") ||
      `<span class="groups-note">API groups unavailable (offline or rate-limited).</span>`;

    root.innerHTML = `
      <div class="appbar">
        <img src="/logos/logo-stacked-760.png" alt="AI Tinkerers" />
        <span class="a-title">Mission Control</span>
        <span class="spacer"></span>
      </div>
      <div class="content">
        <button class="back" id="settingsBackBtn">← All events</button>
        <div class="d-head"><div>
          <h2>Settings</h2>
          <div class="d-meta">Your connected key, notifications, and account</div>
        </div></div>
        <div class="d-grid">
          <div class="panel">
            <h4>Connected key</h4>
            <div class="ok" style="color:var(--accent-2);font:600 12px/1 var(--sans);letter-spacing:.06em;text-transform:uppercase">Signed in</div>
            <h2 style="font:700 20px/1.2 var(--sans);margin:8px 0 2px">${esc(owner.name ?? "Organizer")}</h2>
            <div class="role" style="color:var(--muted);font-size:14px">${esc(roles)}</div>
            <div class="groups">${chips}</div>
            <div class="settings-actions">
              <button class="btn" id="changeKeyBtn">Change API key</button>
              <button class="btn btn-ghost" id="signOutBtn">Sign out</button>
            </div>
            <p class="groups-note">Changing the key validates and replaces the one in your keychain. Sign out clears it and all cached data.</p>
          </div>
          <div class="panel">
            <h4>Notifications</h4>
            <label class="settings-row">
              <span>OS notifications when RSVP counts change</span>
              <input type="checkbox" id="notifToggle" ${notif ? "checked" : ""} />
            </label>
          </div>
        </div>
        <div class="lastsync-foot">Your API key lives only in the OS keychain — never in config, logs, or this window.</div>
      </div>`;

    byId<HTMLButtonElement>("settingsBackBtn").addEventListener("click", opts.onBack);
    byId<HTMLButtonElement>("changeKeyBtn").addEventListener("click", () => opts.onChangeKey());
    byId<HTMLButtonElement>("signOutBtn").addEventListener("click", async () => {
      try {
        await signOut();
      } catch {
        /* ignore — clearing is best-effort */
      }
      opts.onSignedOut();
    });
    byId<HTMLInputElement>("notifToggle").addEventListener("change", (e) => {
      void setNotificationsEnabled((e.target as HTMLInputElement).checked);
    });
  }

  return { open };
}
