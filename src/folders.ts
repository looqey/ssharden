// Folder (group) manager: create / rename / delete vault folders.

import {
  vaultFolders,
  folderCreate,
  folderRename,
  folderDelete,
  type Folder,
} from "./vault";

function esc(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}

/**
 * Open the folder manager modal. Resolves when closed (the caller should reload the
 * host list afterward, since group names may have changed). `notify` shows toasts.
 */
export function openFolderManager(notify: (msg: string) => void): Promise<void> {
  return new Promise((resolve) => {
    const overlay = document.createElement("div");
    overlay.className = "modal-overlay";
    overlay.innerHTML = `
      <div class="modal-card">
        <h2>Folders</h2>
        <div class="newfolder">
          <input id="nf-name" placeholder="New folder name" autocomplete="off" />
          <button id="nf-add">Add</button>
        </div>
        <div class="folder-list" id="folder-list"></div>
        <div class="modal-actions">
          <button class="ghost" id="nf-close">Close</button>
        </div>
      </div>`;
    document.body.appendChild(overlay);
    const q = <T extends HTMLElement>(s: string) => overlay.querySelector<T>(s)!;

    const close = () => {
      overlay.remove();
      document.removeEventListener("keydown", onKey);
      resolve();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") close();
    };
    document.addEventListener("keydown", onKey);
    overlay.addEventListener("mousedown", (e) => {
      if (e.target === overlay) close();
    });
    q("#nf-close").addEventListener("click", close);

    const render = async (): Promise<void> => {
      const list = q("#folder-list");
      list.innerHTML = `<div class="fm-empty">Loading…</div>`;
      let folders: Folder[];
      try {
        folders = await vaultFolders();
      } catch (err) {
        list.innerHTML = `<div class="fm-empty">${esc(String(err))}</div>`;
        return;
      }
      if (folders.length === 0) {
        list.innerHTML = `<div class="fm-empty">No folders yet.</div>`;
        return;
      }
      list.innerHTML = "";
      for (const f of folders) {
        const row = document.createElement("div");
        row.className = "fm-row";

        const name = document.createElement("span");
        name.className = "fm-name";
        name.textContent = f.name;

        const ren = document.createElement("button");
        ren.className = "icon-btn";
        ren.textContent = "✎";
        ren.title = "Rename";
        ren.addEventListener("click", async () => {
          const nn = prompt("Rename folder:", f.name);
          if (!nn || nn === f.name) return;
          try {
            await folderRename(f.id, nn);
            notify("Folder renamed");
            await render();
          } catch (err) {
            notify(`Rename failed: ${String(err)}`);
          }
        });

        const del = document.createElement("button");
        del.className = "icon-btn";
        del.textContent = "🗑";
        del.title = "Delete";
        del.addEventListener("click", async () => {
          if (!confirm(`Delete folder "${f.name}"? Its hosts become unfiled.`)) return;
          try {
            await folderDelete(f.id);
            notify("Folder deleted");
            await render();
          } catch (err) {
            notify(`Delete failed: ${String(err)}`);
          }
        });

        row.append(name, ren, del);
        list.appendChild(row);
      }
    };

    const add = async (): Promise<void> => {
      const input = q<HTMLInputElement>("#nf-name");
      const nm = input.value.trim();
      if (!nm) return;
      try {
        await folderCreate(nm);
        input.value = "";
        notify("Folder created");
        await render();
      } catch (err) {
        notify(`Create failed: ${String(err)}`);
      }
    };
    q("#nf-add").addEventListener("click", () => void add());
    q<HTMLInputElement>("#nf-name").addEventListener("keydown", (e) => {
      if (e.key === "Enter") {
        e.preventDefault();
        void add();
      }
    });

    void render();
  });
}
