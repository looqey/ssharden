# ssharden

A cross-platform desktop app that keeps your host inventory in a **Vaultwarden**
(Bitwarden-compatible) vault and opens **embedded** SSH sessions to those hosts — your
fleet, unlocked everywhere, sourced from your own vault. See [`DESIGN.md`](./DESIGN.md)
for the full design and [`PLAN.md`](./PLAN.md) for the implementation plan.

## Status: Phase 0

What works in this phase:

- **`ssharden-core`** (pure Rust, fully tested): `bw serve` vault adapter, Host/URI model
  parsing, and an SSH PTY launcher.
- **Tauri v2 shell** exposing the vault + SSH commands.
- **Frontend** (TypeScript + xterm.js): unlock screen → host list → embedded SSH terminal
  tabs, with vault lock and idle auto-lock.

Out of scope for Phase 0 (see `DESIGN.md`): embedded RDP (IronRDP), the SFTP file browser,
and org/collection sharing.

## How a host is modeled

A host is a normal Bitwarden **Login item**. The URI **scheme** selects the protocol:

- `ssh://user@host:22` — the only scheme launched in Phase 0
- `rdp://host:3389`, `sftp://host:22`, `ftp://host:21` — parsed, reserved for later phases

Protocol extras live in custom fields (e.g. `jump` → `ssh -J`). Items stay fully editable
from any Bitwarden client.

## Prerequisites

- **Rust** (stable) and **bun** — already installed in this workspace.
- **Bitwarden CLI** (`bw`): `npm install -g @bitwarden/cli`
- **Tauri v2 Linux system libraries** (the one step that needs root):

  ```bash
  sudo apt-get install -y libwebkit2gtk-4.1-dev build-essential libxdo-dev \
    libssl-dev libayatana-appindicator3-dev librsvg2-dev pkg-config
  ```

  Or just run [`./setup.sh`](./setup.sh), which installs these, the Tauri CLI, and the
  frontend deps.

## Running

```bash
# One-time: point the CLI at your server and log in (serve only *unlocks*).
bw config server https://your-vaultwarden.example.com
bw login

# Dev mode (builds the GUI shell + frontend):
cargo tauri dev
```

Then enter your master password in the app to unlock; SSH hosts appear in the sidebar.

## Testing the core

The real logic lives in `ssharden-core` and needs no webkit/GUI to verify:

```bash
cargo test -p ssharden-core
```

## Security

ssharden inherits Bitwarden's zero-knowledge model and adds a connection launcher on top.
The rules enforced in code:

- The only net-new attack surface, the local `bw serve` API, is bound to **127.0.0.1 on an
  ephemeral port** — never `0.0.0.0`. The child is killed on drop.
- The **master password** flows to the backend once; the **session token stays in Rust**
  process memory — never returned to the webview, never written to disk, never logged.
- **Idle auto-lock** (10 min) drops the session and returns to the unlock screen.
- SSH input (including any typed password) is fed over the **PTY**, never on a command line.
  Host-key checking is never disabled — prompts surface in the terminal and reuse your
  `~/.ssh/known_hosts`.
