// Host-list rendering: grouped by folder, filtered to ssh:// URIs for Phase 0.

import type { Host } from "./vault";

/** Callback fired when the user picks a host to connect to. */
export type OnConnect = (host: Host, uriIndex: number) => void;

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
 * `ssh` scheme for Phase 0. Wires each row to `onConnect`.
 */
export function renderHosts(
  container: HTMLElement,
  hosts: Host[],
  onConnect: OnConnect,
): void {
  container.innerHTML = "";

  const sshHosts = hosts.filter((h) => h.uris.some((u) => u.scheme === "ssh"));
  if (sshHosts.length === 0) {
    const empty = document.createElement("p");
    empty.className = "hosts-empty";
    empty.textContent =
      "No SSH hosts found. Add a Bitwarden Login item with an ssh:// URI.";
    container.appendChild(empty);
    return;
  }

  // Group by folder id (Phase 0 has only the id, not the folder name).
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
      const idx = h.uris.findIndex((u) => u.scheme === "ssh");
      const uri = h.uris[idx];
      const who = uri.user ?? h.username ?? "";
      const detail = `${who ? who + "@" : ""}${uri.host}:${uri.port ?? 22}`;

      const row = document.createElement("button");
      row.className = "host-row";
      row.type = "button";
      row.innerHTML =
        `<span class="host-name">${escapeHtml(h.name)}</span>` +
        `<span class="host-detail">${escapeHtml(detail)}</span>`;
      row.addEventListener("click", () => onConnect(h, idx));
      section.appendChild(row);
    }

    container.appendChild(section);
  }
}
