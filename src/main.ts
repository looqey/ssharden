// Bootstrap: unlock screen → host list → terminal tabs.

import "@xterm/xterm/css/xterm.css";
import "./styles.css";
import {
  vaultStart,
  vaultUnlock,
  vaultLock,
  vaultListHosts,
  type Host,
} from "./vault";
import { renderHosts } from "./hosts";
import { TerminalSession } from "./terminal";

const AUTO_LOCK_MS = 10 * 60 * 1000;

let autoLockTimer: ReturnType<typeof setTimeout> | undefined;
let unlocked = false;

interface Tab {
  id: string;
  title: string;
  session: TerminalSession;
  pane: HTMLElement;
  tabButton: HTMLElement;
}
const tabs: Tab[] = [];

function root(): HTMLDivElement {
  const app = document.querySelector<HTMLDivElement>("#app");
  if (!app) throw new Error("#app mount point not found");
  return app;
}

// ---------- Unlock screen ----------

function showUnlock(message?: string): void {
  unlocked = false;
  clearTimeout(autoLockTimer);
  const app = root();
  app.innerHTML = `
    <div class="unlock">
      <form class="unlock-card" id="unlock-form">
        <h1>ssharden</h1>
        <p class="subtitle">Unlock your Vaultwarden host inventory</p>
        <label>Server URL <span class="optional">(optional)</span>
          <input type="url" id="server" placeholder="https://vault.example.com" autocomplete="off" />
        </label>
        <label>Master password
          <input type="password" id="password" autocomplete="off" autofocus />
        </label>
        <button type="submit" id="unlock-btn">Unlock</button>
        <p class="error" id="unlock-error">${message ? message : ""}</p>
      </form>
    </div>`;

  const form = app.querySelector<HTMLFormElement>("#unlock-form")!;
  form.addEventListener("submit", (ev) => {
    ev.preventDefault();
    const server = app.querySelector<HTMLInputElement>("#server")!.value.trim();
    const password = app.querySelector<HTMLInputElement>("#password")!.value;
    void doUnlock(server, password);
  });
}

async function doUnlock(serverUrl: string, password: string): Promise<void> {
  const errEl = document.querySelector<HTMLElement>("#unlock-error");
  const btn = document.querySelector<HTMLButtonElement>("#unlock-btn");
  if (btn) {
    btn.disabled = true;
    btn.textContent = "Unlocking…";
  }
  try {
    await vaultStart();
    await vaultUnlock(serverUrl, password);
    unlocked = true;
    await showApp();
    resetAutoLock();
  } catch (e) {
    if (errEl) errEl.textContent = String(e);
    if (btn) {
      btn.disabled = false;
      btn.textContent = "Unlock";
    }
  }
}

// ---------- Main app (sidebar + terminal tabs) ----------

async function showApp(): Promise<void> {
  const app = root();
  app.innerHTML = `
    <div class="layout">
      <aside class="sidebar">
        <div class="sidebar-head">
          <span class="brand">ssharden</span>
          <button id="lock-btn" class="ghost" title="Lock vault">Lock</button>
        </div>
        <div class="host-list" id="host-list"><p class="hosts-empty">Loading…</p></div>
      </aside>
      <section class="workspace">
        <div class="tabstrip" id="tabstrip"></div>
        <div class="terminals" id="terminals">
          <div class="placeholder" id="placeholder">Pick a host to open an SSH session.</div>
        </div>
      </section>
    </div>`;

  app.querySelector<HTMLButtonElement>("#lock-btn")!.addEventListener("click", () => {
    void lock();
  });

  await loadHosts();
}

async function loadHosts(): Promise<void> {
  const listEl = document.querySelector<HTMLElement>("#host-list");
  if (!listEl) return;
  try {
    const hosts: Host[] = await vaultListHosts();
    renderHosts(listEl, hosts, (host) => {
      void openSession(host);
    });
  } catch (e) {
    listEl.innerHTML = `<p class="hosts-empty">Failed to load hosts: ${String(e)}</p>`;
  }
}

async function openSession(host: Host): Promise<void> {
  resetAutoLock();
  const terminals = document.querySelector<HTMLElement>("#terminals")!;
  const tabstrip = document.querySelector<HTMLElement>("#tabstrip")!;
  document.querySelector("#placeholder")?.remove();

  const pane = document.createElement("div");
  pane.className = "terminal-pane";
  terminals.appendChild(pane);

  let session: TerminalSession;
  try {
    session = await TerminalSession.connect(pane, host.id);
  } catch (e) {
    pane.remove();
    alert(`Could not connect to ${host.name}: ${String(e)}`);
    return;
  }

  const tabButton = document.createElement("button");
  tabButton.className = "tab";
  tabButton.innerHTML = `<span>${host.name}</span><span class="tab-close" title="Close">×</span>`;

  const tab: Tab = { id: session.sessionId, title: host.name, session, pane, tabButton };
  tabs.push(tab);

  tabButton.addEventListener("click", (ev) => {
    if ((ev.target as HTMLElement).classList.contains("tab-close")) {
      closeTab(tab);
    } else {
      activateTab(tab);
    }
  });
  tabstrip.appendChild(tabButton);
  activateTab(tab);
}

function activateTab(tab: Tab): void {
  for (const t of tabs) {
    const active = t === tab;
    t.pane.classList.toggle("active", active);
    t.tabButton.classList.toggle("active", active);
  }
  tab.session.refit();
}

function closeTab(tab: Tab): void {
  tab.session.dispose();
  tab.pane.remove();
  tab.tabButton.remove();
  const i = tabs.indexOf(tab);
  if (i >= 0) tabs.splice(i, 1);
  if (tabs.length) activateTab(tabs[tabs.length - 1]);
}

// ---------- Lock / auto-lock ----------

async function lock(): Promise<void> {
  for (const t of tabs.splice(0)) t.session.dispose();
  try {
    await vaultLock();
  } catch {
    /* lock best-effort */
  }
  showUnlock("Vault locked.");
}

function resetAutoLock(): void {
  if (!unlocked) return;
  clearTimeout(autoLockTimer);
  autoLockTimer = setTimeout(() => void lock(), AUTO_LOCK_MS);
}

// Reset the idle timer on user activity while unlocked.
for (const evt of ["keydown", "mousedown", "mousemove"]) {
  window.addEventListener(evt, () => resetAutoLock(), { passive: true });
}

window.addEventListener("DOMContentLoaded", () => showUnlock());
