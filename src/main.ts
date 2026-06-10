// Bootstrap: unlock screen → host list (with management) → terminal tabs.

import "@xterm/xterm/css/xterm.css";
import "./styles.css";
import {
  vaultStart,
  vaultUnlock,
  vaultLock,
  accountStatus,
  vaultListHosts,
  hostCreate,
  hostUpdate,
  hostDelete,
  hostPassword,
  type Host,
} from "./vault";
import { renderHosts } from "./hosts";
import { openHostForm } from "./form";
import { TerminalSession } from "./terminal";
import { SftpBrowser } from "./sftpui";

const AUTO_LOCK_MS = 10 * 60 * 1000;

let autoLockTimer: ReturnType<typeof setTimeout> | undefined;
let unlocked = false;

/** Anything that can live in a workspace tab (terminal or sftp browser). */
interface PaneObj {
  dispose(): void;
  refit(): void;
}
interface Tab {
  id: string;
  title: string;
  obj: PaneObj;
  pane: HTMLElement;
  tabButton: HTMLElement;
}
const tabs: Tab[] = [];

function root(): HTMLDivElement {
  const app = document.querySelector<HTMLDivElement>("#app");
  if (!app) throw new Error("#app mount point not found");
  return app;
}

function esc(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
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
        <p class="subtitle" id="unlock-account">Unlock your vault</p>
        <label>Master password
          <input type="password" id="password" autocomplete="off" autofocus />
        </label>
        <button type="submit" id="unlock-btn">Unlock</button>
        <p class="error" id="unlock-error">${message ? esc(message) : ""}</p>
      </form>
    </div>`;

  const form = app.querySelector<HTMLFormElement>("#unlock-form")!;
  form.addEventListener("submit", (ev) => {
    ev.preventDefault();
    const password = app.querySelector<HTMLInputElement>("#password")!.value;
    void doUnlock(password);
  });

  // Show which account will be unlocked (login happens once via the bw CLI).
  void accountStatus()
    .then((st) => {
      const el = document.querySelector<HTMLElement>("#unlock-account");
      if (!el) return;
      if (st.status === "unauthenticated" || !st.user_email) {
        el.innerHTML = `Not logged in — run <code>bw login</code> in a terminal first.`;
        el.classList.add("warn");
      } else {
        el.innerHTML = `Unlocking <strong>${esc(st.user_email)}</strong>`;
      }
    })
    .catch(() => {
      /* leave the default subtitle */
    });
}

async function doUnlock(password: string): Promise<void> {
  const errEl = document.querySelector<HTMLElement>("#unlock-error");
  const btn = document.querySelector<HTMLButtonElement>("#unlock-btn");
  if (btn) {
    btn.disabled = true;
    btn.textContent = "Unlocking…";
  }
  try {
    await vaultStart();
    await vaultUnlock(password);
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
        <div class="host-list-head">
          <button id="new-host" class="newbtn">+ New host</button>
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

  app.querySelector<HTMLButtonElement>("#lock-btn")!.addEventListener("click", () => void lock());
  app.querySelector<HTMLButtonElement>("#new-host")!.addEventListener("click", () => void newHost());

  await loadHosts();
}

async function loadHosts(): Promise<void> {
  const listEl = document.querySelector<HTMLElement>("#host-list");
  if (!listEl) return;
  try {
    const hosts: Host[] = await vaultListHosts();
    renderHosts(listEl, hosts, {
      onConnect: (h) => void openSession(h),
      onSftp: (h) => void openSftpBrowser(h),
      onEdit: (h) => void editHost(h),
      onDelete: (h) => void removeHost(h),
      onCopyPassword: (h) => void revealPassword(h),
    });
  } catch (e) {
    listEl.innerHTML = `<p class="hosts-empty">Failed to load hosts: ${esc(String(e))}</p>`;
  }
}

// ---------- Host management ----------

async function newHost(): Promise<void> {
  resetAutoLock();
  const res = await openHostForm();
  if (!res) return;
  try {
    await hostCreate(res.input);
    toast("Host created");
    await loadHosts();
  } catch (e) {
    toast(`Create failed: ${String(e)}`);
  }
}

async function editHost(h: Host): Promise<void> {
  resetAutoLock();
  const res = await openHostForm(h);
  if (!res || !res.id) return;
  try {
    await hostUpdate(res.id, res.input);
    toast("Host saved");
    await loadHosts();
  } catch (e) {
    toast(`Save failed: ${String(e)}`);
  }
}

async function removeHost(h: Host): Promise<void> {
  resetAutoLock();
  if (!confirm(`Delete host "${h.name}"? This removes the vault item.`)) return;
  try {
    await hostDelete(h.id);
    toast("Host deleted");
    await loadHosts();
  } catch (e) {
    toast(`Delete failed: ${String(e)}`);
  }
}

async function revealPassword(h: Host): Promise<void> {
  resetAutoLock();
  let pw: string | null;
  try {
    pw = await hostPassword(h.id);
  } catch (e) {
    toast(String(e));
    return;
  }
  if (pw == null) {
    toast("No password set on this host");
    return;
  }
  const ov = document.createElement("div");
  ov.className = "modal-overlay";
  ov.innerHTML = `
    <div class="modal-card pw-card">
      <h2>Password — ${esc(h.name)}</h2>
      <code class="pw-value">${esc(pw)}</code>
      <div class="modal-actions">
        <button class="ghost" id="pw-close">Close</button>
        <button id="pw-copy">Copy</button>
      </div>
    </div>`;
  document.body.appendChild(ov);
  const close = () => ov.remove();
  ov.addEventListener("mousedown", (e) => {
    if (e.target === ov) close();
  });
  ov.querySelector<HTMLButtonElement>("#pw-close")!.addEventListener("click", close);
  ov.querySelector<HTMLButtonElement>("#pw-copy")!.addEventListener("click", async () => {
    try {
      await navigator.clipboard.writeText(pw!);
      toast("Password copied");
    } catch {
      toast("Clipboard unavailable");
    }
    close();
  });
}

// ---------- Terminal tabs ----------

async function openSession(host: Host, kind: "ssh" | "sftp" = "ssh"): Promise<void> {
  resetAutoLock();
  const terminals = document.querySelector<HTMLElement>("#terminals")!;
  const tabstrip = document.querySelector<HTMLElement>("#tabstrip")!;
  document.querySelector("#placeholder")?.remove();

  const pane = document.createElement("div");
  pane.className = "terminal-pane";
  terminals.appendChild(pane);

  let session: TerminalSession;
  try {
    session = await TerminalSession.connect(pane, host.id, kind);
  } catch (e) {
    pane.remove();
    toast(`Could not ${kind} to ${host.name}: ${String(e)}`);
    return;
  }

  const label = kind === "sftp" ? `sftp: ${host.name}` : host.name;
  const tabButton = document.createElement("button");
  tabButton.className = "tab";
  tabButton.innerHTML = `<span>${esc(label)}</span><span class="tab-close" title="Close">×</span>`;

  const tab: Tab = { id: session.sessionId, title: label, obj: session, pane, tabButton };
  tabs.push(tab);

  tabButton.addEventListener("click", (ev) => {
    if ((ev.target as HTMLElement).classList.contains("tab-close")) closeTab(tab);
    else activateTab(tab);
  });
  tabstrip.appendChild(tabButton);
  activateTab(tab);
}

async function openSftpBrowser(host: Host): Promise<void> {
  resetAutoLock();
  const terminals = document.querySelector<HTMLElement>("#terminals")!;
  const tabstrip = document.querySelector<HTMLElement>("#tabstrip")!;
  document.querySelector("#placeholder")?.remove();

  const pane = document.createElement("div");
  pane.className = "terminal-pane";
  terminals.appendChild(pane);

  let browser: SftpBrowser;
  try {
    browser = await SftpBrowser.open(pane, host.id);
  } catch (e) {
    pane.remove();
    toast(`Could not open SFTP to ${host.name}: ${String(e)}`);
    return;
  }

  const label = `sftp: ${host.name}`;
  const tabButton = document.createElement("button");
  tabButton.className = "tab";
  tabButton.innerHTML = `<span>${esc(label)}</span><span class="tab-close" title="Close">×</span>`;

  const tab: Tab = { id: browser.connId, title: label, obj: browser, pane, tabButton };
  tabs.push(tab);
  tabButton.addEventListener("click", (ev) => {
    if ((ev.target as HTMLElement).classList.contains("tab-close")) closeTab(tab);
    else activateTab(tab);
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
  tab.obj.refit();
}

function closeTab(tab: Tab): void {
  tab.obj.dispose();
  tab.pane.remove();
  tab.tabButton.remove();
  const i = tabs.indexOf(tab);
  if (i >= 0) tabs.splice(i, 1);
  if (tabs.length) activateTab(tabs[tabs.length - 1]);
}

// ---------- Lock / auto-lock / toast ----------

async function lock(): Promise<void> {
  for (const t of tabs.splice(0)) t.obj.dispose();
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

let toastTimer: ReturnType<typeof setTimeout> | undefined;
function toast(message: string): void {
  let el = document.querySelector<HTMLDivElement>("#toast");
  if (!el) {
    el = document.createElement("div");
    el.id = "toast";
    el.className = "toast";
    document.body.appendChild(el);
  }
  el.textContent = message;
  el.classList.add("show");
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => el!.classList.remove("show"), 2500);
}

for (const evt of ["keydown", "mousedown", "mousemove"]) {
  window.addEventListener(evt, () => resetAutoLock(), { passive: true });
}

window.addEventListener("DOMContentLoaded", () => showUnlock());
