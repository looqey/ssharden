// Graphical dual-pane SFTP browser: local filesystem (left) ↔ remote host (right).

import { invoke } from "@tauri-apps/api/core";

interface FsEntry {
  name: string;
  is_dir: boolean;
  size: number;
}

interface SftpOpened {
  conn_id: string;
  home: string;
}

function esc(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}

/** POSIX-style join (both local on Linux and remote use `/`). */
function join(base: string, name: string): string {
  if (base.endsWith("/")) return base + name;
  return base + "/" + name;
}

/** Parent directory of a POSIX path. */
function parent(p: string): string {
  if (p === "/" || p === "") return "/";
  const trimmed = p.replace(/\/+$/, "");
  const idx = trimmed.lastIndexOf("/");
  return idx <= 0 ? "/" : trimmed.slice(0, idx);
}

function humanSize(n: number): string {
  if (n < 1024) return `${n} B`;
  const u = ["KB", "MB", "GB", "TB"];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < u.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(1)} ${u[i]}`;
}

/** A live dual-pane SFTP browser bound to one host connection. */
export class SftpBrowser {
  readonly connId: string;
  private localPath: string;
  private remotePath: string;
  private localList!: HTMLElement;
  private remoteList!: HTMLElement;
  private localPathEl!: HTMLInputElement;
  private remotePathEl!: HTMLInputElement;
  private statusEl!: HTMLElement;
  private disposed = false;

  private constructor(connId: string, localPath: string, remotePath: string) {
    this.connId = connId;
    this.localPath = localPath;
    this.remotePath = remotePath;
  }

  /** Open an SFTP connection to `hostId` and render the browser into `mount`. */
  static async open(mount: HTMLElement, hostId: string): Promise<SftpBrowser> {
    const opened = await invoke<SftpOpened>("sftp_open", { hostId });
    const localHome = await invoke<string>("local_home");
    const b = new SftpBrowser(opened.conn_id, localHome, opened.home);
    b.render(mount);
    await b.refreshBoth();
    return b;
  }

  private render(mount: HTMLElement): void {
    mount.innerHTML = `
      <div class="fb-root">
        <div class="fb">
          <div class="fb-pane">
            <div class="fb-head">
              <span class="fb-title">Local</span>
              <button class="fb-up" data-side="local" title="Up">↑</button>
              <input class="fb-path" data-side="local" spellcheck="false" />
              <button class="fb-refresh" data-side="local" title="Refresh">⟳</button>
            </div>
            <div class="fb-list" data-side="local"></div>
          </div>
          <div class="fb-pane">
            <div class="fb-head">
              <span class="fb-title">Remote</span>
              <button class="fb-up" data-side="remote" title="Up">↑</button>
              <input class="fb-path" data-side="remote" spellcheck="false" />
              <button class="fb-refresh" data-side="remote" title="Refresh">⟳</button>
            </div>
            <div class="fb-list" data-side="remote"></div>
          </div>
        </div>
        <div class="fb-status"></div>
      </div>`;

    const q = <T extends HTMLElement>(sel: string) => mount.querySelector<T>(sel)!;
    this.localList = q('.fb-list[data-side="local"]');
    this.remoteList = q('.fb-list[data-side="remote"]');
    this.localPathEl = q<HTMLInputElement>('.fb-path[data-side="local"]');
    this.remotePathEl = q<HTMLInputElement>('.fb-path[data-side="remote"]');
    this.statusEl = q(".fb-status");

    mount.querySelectorAll<HTMLButtonElement>(".fb-up").forEach((b) =>
      b.addEventListener("click", () => {
        if (b.dataset.side === "local") this.go("local", parent(this.localPath));
        else this.go("remote", parent(this.remotePath));
      }),
    );
    mount.querySelectorAll<HTMLButtonElement>(".fb-refresh").forEach((b) =>
      b.addEventListener("click", () => {
        if (b.dataset.side === "local") void this.refreshLocal();
        else void this.refreshRemote();
      }),
    );
    const wirePath = (el: HTMLInputElement, side: "local" | "remote") =>
      el.addEventListener("keydown", (e) => {
        if (e.key === "Enter") this.go(side, el.value.trim() || "/");
      });
    wirePath(this.localPathEl, "local");
    wirePath(this.remotePathEl, "remote");
  }

  private go(side: "local" | "remote", path: string): void {
    if (side === "local") {
      this.localPath = path;
      void this.refreshLocal();
    } else {
      this.remotePath = path;
      void this.refreshRemote();
    }
  }

  private async refreshBoth(): Promise<void> {
    await Promise.all([this.refreshLocal(), this.refreshRemote()]);
  }

  private async refreshLocal(): Promise<void> {
    this.localPathEl.value = this.localPath;
    try {
      const entries = await invoke<FsEntry[]>("local_ls", { path: this.localPath });
      this.renderList(this.localList, entries, "local");
    } catch (e) {
      this.localList.innerHTML = `<div class="fb-empty">${esc(String(e))}</div>`;
    }
  }

  private async refreshRemote(): Promise<void> {
    this.remotePathEl.value = this.remotePath;
    try {
      const entries = await invoke<FsEntry[]>("sftp_ls", { connId: this.connId, path: this.remotePath });
      this.renderList(this.remoteList, entries, "remote");
    } catch (e) {
      this.remoteList.innerHTML = `<div class="fb-empty">${esc(String(e))}</div>`;
    }
  }

  private renderList(container: HTMLElement, entries: FsEntry[], side: "local" | "remote"): void {
    container.innerHTML = "";
    for (const ent of entries) {
      const row = document.createElement("div");
      row.className = "fb-row" + (ent.is_dir ? " dir" : "");

      const icon = ent.is_dir ? "📁" : "📄";
      const meta = ent.is_dir ? "" : `<span class="fb-size">${humanSize(ent.size)}</span>`;
      row.innerHTML =
        `<span class="fb-icon">${icon}</span>` +
        `<span class="fb-name">${esc(ent.name)}</span>${meta}`;

      if (ent.is_dir) {
        row.addEventListener("click", () => {
          const base = side === "local" ? this.localPath : this.remotePath;
          this.go(side, join(base, ent.name));
        });
      } else {
        const btn = document.createElement("button");
        btn.className = "fb-xfer";
        if (side === "remote") {
          btn.textContent = "← get";
          btn.title = "Download to local";
          btn.addEventListener("click", (e) => {
            e.stopPropagation();
            void this.download(ent.name);
          });
        } else {
          btn.textContent = "put →";
          btn.title = "Upload to remote";
          btn.addEventListener("click", (e) => {
            e.stopPropagation();
            void this.upload(ent.name);
          });
        }
        row.appendChild(btn);
      }
      container.appendChild(row);
    }
    if (entries.length === 0) {
      container.innerHTML = `<div class="fb-empty">empty</div>`;
    }
  }

  private status(msg: string): void {
    this.statusEl.textContent = msg;
  }

  private async download(name: string): Promise<void> {
    const remote = join(this.remotePath, name);
    const local = join(this.localPath, name);
    this.status(`Downloading ${name}…`);
    try {
      await invoke("sftp_get", { connId: this.connId, remote, local });
      this.status(`Downloaded ${name} → ${this.localPath}`);
      await this.refreshLocal();
    } catch (e) {
      this.status(`Download failed: ${String(e)}`);
    }
  }

  private async upload(name: string): Promise<void> {
    const local = join(this.localPath, name);
    const remote = join(this.remotePath, name);
    this.status(`Uploading ${name}…`);
    try {
      await invoke("sftp_put", { connId: this.connId, local, remote });
      this.status(`Uploaded ${name} → ${this.remotePath}`);
      await this.refreshRemote();
    } catch (e) {
      this.status(`Upload failed: ${String(e)}`);
    }
  }

  /** No-op: the browser reflows with CSS (kept for the tab interface). */
  refit(): void {}

  /** Close the SFTP connection. */
  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    void invoke("sftp_close", { connId: this.connId }).catch(() => {});
  }
}
