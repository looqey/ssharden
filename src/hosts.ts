// Host-list rendering: grouped by folder, filtered to ssh:// URIs for Phase 0,
// with per-row actions (connect / edit / copy password / delete).

import type { Host } from "./vault";

/** Actions wired to each host row. */
export interface HostActions {
  onConnect: (host: Host) => void;
  onSftp: (host: Host) => void;
  onEdit: (host: Host) => void;
  onDelete: (host: Host) => void;
  onCopyPassword: (host: Host) => void;
}

/** Minimal HTML-escape for untrusted vault strings rendered into the DOM. */
function escapeHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

/**
 * Render the host list into `container`, grouped by folder and filtered to the
 * `ssh` scheme for Phase 0. Wires each row to the supplied actions.
 */
export function renderHosts(
  container: HTMLElement,
  hosts: Host[],
  actions: HostActions,
): void {
  container.innerHTML = "";

  const sshHosts = hosts.filter((h) => h.uris.some((u) => u.scheme === "ssh"));
  if (sshHosts.length === 0) {
    const empty = document.createElement("p");
    empty.className = "hosts-empty";
    empty.textContent =
      "No SSH hosts yet. Use + New host to add one (a Login item with an ssh:// URI).";
    container.appendChild(empty);
    return;
  }

  const groups = new Map<string, Host[]>();
  for (const h of sshHosts) {
    const key = h.folder_id ?? "";
    const bucket = groups.get(key);
    if (bucket) bucket.push(h);
    else groups.set(key, [h]);
  }

  for (const [folder, hs] of groups) {
    const section = document.createElement("div");
    section.className = "host-group";

    const title = document.createElement("div");
    title.className = "host-group-title";
    title.textContent = folder ? `Folder ${folder.slice(0, 8)}` : "Ungrouped";
    section.appendChild(title);

    for (const h of hs) {
      const uri = h.uris.find((u) => u.scheme === "ssh")!;
      const who = uri.user ?? h.username ?? "";
      const detail = `${who ? who + "@" : ""}${uri.host}:${uri.port ?? 22}`;

      const row = document.createElement("div");
      row.className = "host-row";

      const main = document.createElement("button");
      main.className = "host-main";
      main.type = "button";
      main.title = "Connect (SSH)";
      main.innerHTML =
        `<span class="host-name">${escapeHtml(h.name)}</span>` +
        `<span class="host-detail">${escapeHtml(detail)}</span>`;
      main.addEventListener("click", () => actions.onConnect(h));

      const acts = document.createElement("div");
      acts.className = "host-actions";
      acts.appendChild(iconBtn("⇅", "Open SFTP", () => actions.onSftp(h)));
      acts.appendChild(iconBtn("⧉", "Copy password", () => actions.onCopyPassword(h)));
      acts.appendChild(iconBtn("✎", "Edit", () => actions.onEdit(h)));
      acts.appendChild(iconBtn("🗑", "Delete", () => actions.onDelete(h)));

      row.appendChild(main);
      row.appendChild(acts);
      section.appendChild(row);
    }

    container.appendChild(section);
  }
}

function iconBtn(glyph: string, title: string, onClick: () => void): HTMLButtonElement {
  const b = document.createElement("button");
  b.className = "icon-btn";
  b.type = "button";
  b.title = title;
  b.textContent = glyph;
  b.addEventListener("click", (e) => {
    e.stopPropagation();
    onClick();
  });
  return b;
}
