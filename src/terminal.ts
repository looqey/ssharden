// xterm.js wiring: PTY byte stream over Tauri events.
// Connects via ssh_connect, listens for `ssh://{id}` events for output, and
// sends typed input back with ssh_write / dimensions with ssh_resize.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";

/** A live embedded SSH terminal bound to a backend session id. */
export class TerminalSession {
  private term: Terminal;
  private fit: FitAddon;
  private unlisten: UnlistenFn | null = null;
  private resizeObserver: ResizeObserver | null = null;
  readonly sessionId: string;

  private constructor(sessionId: string) {
    this.sessionId = sessionId;
    this.term = new Terminal({
      convertEol: false,
      cursorBlink: true,
      fontFamily: 'ui-monospace, "SFMono-Regular", Menlo, Consolas, monospace',
      fontSize: 13,
      theme: { background: "#0e1014", foreground: "#e6e6e6" },
    });
    this.fit = new FitAddon();
    this.term.loadAddon(this.fit);
  }

  /** Connect to `hostId` (ssh or sftp), mount an xterm into `mount`, stream the PTY. */
  static async connect(
    mount: HTMLElement,
    hostId: string,
    kind: "ssh" | "sftp" = "ssh",
  ): Promise<TerminalSession> {
    const cmd = kind === "sftp" ? "sftp_connect" : "ssh_connect";
    const sessionId = await invoke<string>(cmd, { hostId });
    const s = new TerminalSession(sessionId);

    s.term.open(mount);
    s.fit.fit();

    // PTY output arrives as a byte array on the `ssh://{id}` event.
    s.unlisten = await listen<number[]>(`ssh://${sessionId}`, (ev) => {
      s.term.write(new Uint8Array(ev.payload));
    });

    // Typed input → PTY (this is how a password reaches ssh; never via argv).
    s.term.onData((data) => {
      void s.write(new TextEncoder().encode(data));
    });

    // Keep the remote pty sized to the widget.
    s.term.onResize(({ cols, rows }) => {
      void s.resize(cols, rows);
    });
    s.resizeObserver = new ResizeObserver(() => {
      try {
        s.fit.fit();
      } catch {
        /* element not visible yet */
      }
    });
    s.resizeObserver.observe(mount);

    void s.resize(s.term.cols, s.term.rows);
    s.term.focus();
    return s;
  }

  /** Refit to the container (call when the tab becomes visible). */
  refit(): void {
    try {
      this.fit.fit();
      this.term.focus();
    } catch {
      /* not visible */
    }
  }

  /** Send typed bytes to the PTY. */
  async write(data: Uint8Array): Promise<void> {
    return invoke("ssh_write", {
      sessionId: this.sessionId,
      data: Array.from(data),
    });
  }

  /** Notify the backend of new terminal dimensions. */
  async resize(cols: number, rows: number): Promise<void> {
    return invoke("ssh_resize", { sessionId: this.sessionId, cols, rows });
  }

  /** Tear down the event listener and dispose the terminal. */
  dispose(): void {
    if (this.unlisten) {
      this.unlisten();
      this.unlisten = null;
    }
    if (this.resizeObserver) {
      this.resizeObserver.disconnect();
      this.resizeObserver = null;
    }
    this.term.dispose();
  }
}
