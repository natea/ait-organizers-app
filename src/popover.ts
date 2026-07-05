// Tray popover window (specs/tray-notifications): next event's funnel + a
// link into the full detail view. Data comes from the same cache via commands.
import "./styles.css";
import { getNextEvent, onPopoverData, openMain } from "./api";
import type { NextEvent } from "./types";
import { esc, fmt, num } from "./util";

const root = document.getElementById("pv")!;

function render(ev: NextEvent | null): void {
  if (!ev || !ev.name) {
    root.innerHTML = `
      <div class="pv-kicker">Mission Control</div>
      <h3>No upcoming events</h3>
      <div class="pv-idle">Nothing scheduled in your visible chapters yet.</div>
      <button class="pv-open" id="pvOpen">Open Mission Control</button>`;
    wire();
    return;
  }
  const cap = ev.capacity != null ? ` of ${fmt(ev.capacity)}` : "";
  root.innerHTML = `
    <div class="pv-kicker">Next event${ev.city ? " · " + esc(ev.city) : ""}</div>
    <h3>${esc(ev.name)}</h3>
    <div class="pv-when">${esc(ev.when ?? "")}${daysText(ev.days)}</div>
    <div class="pv-big"><b>${fmt(ev.attending)}</b><span>attending${cap}</span></div>
    <div class="pv-funnel">
      <div><b>${fmt(ev.registered)}</b><small>Reg</small></div>
      <div><b>${fmt(ev.attending)}</b><small>Att</small></div>
      <div><b>${fmt(ev.waitlisted)}</b><small>Wait</small></div>
      <div><b>${fmt(ev.cancelled)}</b><small>Canc</small></div>
    </div>
    <button class="pv-open" id="pvOpen">Open Mission Control</button>`;
  wire();
}

function daysText(days?: number): string {
  const d = num(days, -1);
  if (d < 0) return "";
  if (d === 0) return " · today";
  return ` · in ${d} day${d === 1 ? "" : "s"}`;
}

function wire(): void {
  document.getElementById("pvOpen")?.addEventListener("click", () => openMain());
}

async function boot(): Promise<void> {
  await onPopoverData(render);
  try {
    render(await getNextEvent());
  } catch {
    render(null);
  }
}

boot();
