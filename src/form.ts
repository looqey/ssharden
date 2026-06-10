// Host create/edit form — a modal overlay that resolves to a HostInput (or null).

import type { Folder, Host, HostInput } from "./vault";

export interface HostFormResult {
  /** Present when editing an existing host. */
  id?: string;
  input: HostInput;
}

const PROTOCOLS = ["ssh", "sftp", "rdp", "ftp"] as const;
const DEFAULT_PORTS: Record<string, number> = { ssh: 22, sftp: 22, rdp: 3389, ftp: 21 };

/**
 * Open the host form. Resolves with the entered data on save, or `null` on cancel.
 * When `existing` is given, the form is prefilled for editing (password left blank
 * to mean "keep current").
 */
export function openHostForm(
  existing?: Host,
  folders: Folder[] = [],
): Promise<HostFormResult | null> {
  return new Promise((resolve) => {
    const primary = existing?.uris[0];
    const proto0 = primary?.scheme ?? "ssh";
    const host0 = primary?.host ?? "";
    const port0 = primary?.port ?? DEFAULT_PORTS[proto0] ?? 22;
    const user0 = primary?.user ?? existing?.username ?? "";
    const jump0 = existing?.fields?.jump ?? "";

    const overlay = document.createElement("div");
    overlay.className = "modal-overlay";
    overlay.innerHTML = `
      <form class="modal-card" id="host-form">
        <h2>${existing ? "Edit host" : "New host"}</h2>
        <label>Name <input id="f-name" value="${esc(existing?.name ?? "")}" autofocus required></label>
        <div class="row2">
          <label>Protocol
            <select id="f-proto">
              ${PROTOCOLS.map((p) => `<option value="${p}"${p === proto0 ? " selected" : ""}>${p}</option>`).join("")}
            </select>
          </label>
          <label>Port <input id="f-port" type="number" min="1" max="65535" value="${port0}"></label>
        </div>
        <label>Host <input id="f-host" value="${esc(host0)}" placeholder="10.0.0.5 or host.lan" required></label>
        <label>Username <input id="f-user" value="${esc(user0)}" autocomplete="off"></label>
        <label>Password
          <input id="f-pass" type="password" autocomplete="off"
                 placeholder="${existing ? "(leave blank to keep current)" : ""}">
        </label>
        <label>Jump host <span class="optional">(ssh -J, optional)</span>
          <input id="f-jump" value="${esc(jump0)}" autocomplete="off">
        </label>
        <label>Folder
          <select id="f-folder">
            <option value="">No folder</option>
            ${folders
              .map(
                (f) =>
                  `<option value="${esc(f.id)}"${existing?.folder_id === f.id ? " selected" : ""}>${esc(f.name)}</option>`,
              )
              .join("")}
          </select>
        </label>
        <p class="error" id="f-error"></p>
        <div class="modal-actions">
          <button type="button" class="ghost" id="f-cancel">Cancel</button>
          <button type="submit" id="f-save">${existing ? "Save" : "Create"}</button>
        </div>
      </form>`;
    document.body.appendChild(overlay);

    const $ = <T extends HTMLElement>(s: string) => overlay.querySelector<T>(s)!;
    const proto = $<HTMLSelectElement>("#f-proto");
    const port = $<HTMLInputElement>("#f-port");
    // When protocol changes and the port is still a known default, follow it.
    proto.addEventListener("change", () => {
      const cur = Number(port.value);
      if (Object.values(DEFAULT_PORTS).includes(cur) || !port.value) {
        port.value = String(DEFAULT_PORTS[proto.value] ?? cur);
      }
    });

    const close = (result: HostFormResult | null) => {
      overlay.remove();
      document.removeEventListener("keydown", onKey);
      resolve(result);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") close(null);
    };
    document.addEventListener("keydown", onKey);
    $("#f-cancel").addEventListener("click", () => close(null));
    overlay.addEventListener("mousedown", (e) => {
      if (e.target === overlay) close(null);
    });

    $<HTMLFormElement>("#host-form").addEventListener("submit", (e) => {
      e.preventDefault();
      const name = $<HTMLInputElement>("#f-name").value.trim();
      const host = $<HTMLInputElement>("#f-host").value.trim();
      const p = proto.value;
      const portNum = port.value.trim();
      const user = $<HTMLInputElement>("#f-user").value.trim();
      const pass = $<HTMLInputElement>("#f-pass").value;
      const jump = $<HTMLInputElement>("#f-jump").value.trim();
      if (!name || !host) {
        $("#f-error").textContent = "Name and host are required.";
        return;
      }
      const uri = `${p}://${host}${portNum ? ":" + portNum : ""}`;
      const fields: Record<string, string> = { ...(existing?.fields ?? {}) };
      if (jump) fields.jump = jump;
      else delete fields.jump;

      const input: HostInput = {
        name,
        // Always send the picker's choice: a folder id, or "" for "No folder".
        folder_id: $<HTMLSelectElement>("#f-folder").value,
        username: user || null,
        password: pass ? pass : null, // blank = keep current (edit) / none (create)
        uris: [uri],
        fields,
      };
      close({ id: existing?.id, input });
    });
  });
}

function esc(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}
