// Onboarding: key entry → validate → identity summary (specs/api-auth).
import { validateAndStore } from "../api";
import type { AppErr, Identity } from "../types";
import { byId, esc } from "../util";

// API-group flags returned by auth/validate → friendly chip labels.
const GROUP_LABELS: Record<string, string> = {
  people_events: "events",
  subscribers_sponsors: "subscribers",
  docs_rag: "docs",
  hackathons: "hackathons",
  media: "media",
};
// Groups this app's features actually depend on (shown even when disabled).
const RELEVANT_GROUPS = ["people_events", "subscribers_sponsors", "docs_rag", "hackathons"];

export function renderOnboarding(onDone: () => void): void {
  const root = byId("scr-onboarding");
  root.innerHTML = `
    <div class="onb">
      <div class="onb-card" id="onbStep1">
        <img src="/logos/logo-stacked-760.png" alt="AI Tinkerers" />
        <h1>Connect your organizer key</h1>
        <p class="sub">Paste your Agents API key. It's validated once, then stored
          in the OS keychain — never in config or logs.</p>
        <div class="field">
          <label for="keyInput">Agent API key</label>
          <input id="keyInput" type="password" spellcheck="false" autocomplete="off"
            placeholder="sk_…" />
        </div>
        <button class="btn btn-block" id="validateBtn">Validate key</button>
        <div class="onb-err" id="onbErr"></div>
        <p class="onb-hint">Read-only access. The app never calls write endpoints.</p>
      </div>
      <div class="onb-card" id="onbStep2" style="display:none"></div>
    </div>`;

  const input = byId<HTMLInputElement>("keyInput");
  const btn = byId<HTMLButtonElement>("validateBtn");
  const err = byId("onbErr");

  const submit = async () => {
    const key = input.value.trim();
    err.classList.remove("show");
    if (key.length < 8) {
      showError(err, "That doesn't look like a key. Paste the full value from your profile.");
      return;
    }
    btn.disabled = true;
    btn.textContent = "Validating…";
    try {
      const identity = await validateAndStore(key);
      renderIdentity(identity, onDone);
    } catch (e) {
      const code = (e as AppErr)?.code ?? "unknown";
      showError(
        err,
        code === "rate_limited"
          ? "Rate limited — wait a moment and try again."
          : "That key didn't validate. Check for a trailing space, or generate a new key.",
      );
    } finally {
      btn.disabled = false;
      btn.textContent = "Validate key";
    }
  };

  btn.addEventListener("click", submit);
  input.addEventListener("keydown", (e) => {
    if (e.key === "Enter") submit();
  });
  input.focus();
}

function showError(el: HTMLElement, msg: string): void {
  el.innerHTML = `${esc(msg)} Generate a new key at
    <a href="https://aitinkerers.org/profile" target="_blank" rel="noopener">aitinkerers.org/profile</a>.`;
  el.classList.add("show");
}

function renderIdentity(identity: Identity, onDone: () => void): void {
  byId("onbStep1").style.display = "none";
  const step2 = byId("onbStep2");
  step2.style.display = "block";

  const owner = identity.owner ?? {};
  const auth = identity.authorization ?? {};
  const roles = auth.caller_roles ?? [];
  const enabled = new Set(auth.enabled_api_groups ?? []);

  const chips = RELEVANT_GROUPS.map((g) => {
    const on = enabled.has(g);
    const label = GROUP_LABELS[g] ?? g;
    return `<span class="chip ${on ? "on" : "off"}">${esc(label)}</span>`;
  }).join("");

  step2.innerHTML = `
    <img src="/logos/logo-stacked-760.png" alt="AI Tinkerers" />
    <div class="idcard">
      <div class="ok">Key validated</div>
      <h2>${esc(owner.name ?? "Organizer")}</h2>
      <div class="role">${esc(roles.join(", ") || "member")}</div>
      <div class="groups">${chips}</div>
      <p class="groups-note">Struck-through groups aren't enabled for your chapter —
        those sections show a "not enabled" state instead of erroring.</p>
    </div>
    <button class="btn btn-block" id="continueBtn">Continue to events</button>`;

  byId<HTMLButtonElement>("continueBtn").addEventListener("click", onDone);
}
