# Changelog

All notable changes to ssharden are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - Unreleased

### Added

- Vaultwarden-backed connection manager: hosts and their secrets live in your
  vault, unlocked once per session via the `bw` CLI and a loopback `bw serve`
  vault adapter on an ephemeral port.
- Phase 0 embedded SSH: launch an interactive SSH session in an in-app
  xterm.js terminal driven by a real PTY.
- Auto-fill SSH passwords from the vault, typed over the PTY so credentials
  never need to be entered by hand.
- In-app host management: create, edit, and delete hosts, plus copy or reveal
  a host's stored password.
- Unlock screen that surfaces the currently logged-in account and drops the
  vestigial server field for a cleaner unlock flow.
- One-click SFTP: open an SFTP session against any SSH host without
  re-entering connection details.
- Graphical dual-pane SFTP file browser for navigating local and remote trees
  side by side.
- File operations in the SFTP browser: create folder, rename, and delete.
- SFTP transfer queue with live per-transfer progress bars.
- One-click external RDP via FreeRDP (`xfreerdp`), password fed over stdin.
- SSH key auth from a vault SSH Key item (host `sshkey` custom field), used by
  both the terminal and the graphical SFTP browser.
- Hosts grouped by their real Bitwarden folder names; folder picker in the
  host form plus a folder manager (create/rename/delete).
- Host search/filter in the sidebar (`/` or Ctrl+F to focus).
- CI and cross-platform release workflows (GitHub Actions).

### Security

- SSH password auto-fill matches only the exact OpenSSH prompt for the
  connected target — crafted banners and jump-host prompts can no longer
  elicit the stored password.
- The graphical SFTP browser verifies server host keys against
  `~/.ssh/known_hosts` (refuses changed keys, `accept-new` on first contact).
- RDP server certificates are pinned trust-on-first-use (`/cert:tofu`)
  instead of ignored.
- Strict Content-Security-Policy on the webview.

### Fixed

- `bw serve` is now tied to the app's lifetime via `PR_SET_PDEATHSIG`, so the
  vault server no longer survives as an orphan after the app exits.
- SFTP transfers stream in 128 KB chunks instead of loading whole files into
  memory.
- Share a single PTY writer to work around `portable-pty`'s
  take-writer-once limitation.
- Added a Tauri capability granting `core:default` so the frontend can listen
  for and emit events.
- Valid app icon and a clear "bw not logged in" error message on startup.
- SFTP upload progress reflects server-confirmed bytes (flush every 1 MB)
  instead of locally-queued writes.
- `bw` resolved to an absolute path and its directory put on the child PATH,
  fixing launches from the installed `.deb` desktop entry.
