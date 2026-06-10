# ssharden

**Your host fleet, unlocked everywhere — sourced from your own Vaultwarden vault.**

ssharden is a cross-platform desktop app that uses a **Vaultwarden** (Bitwarden-compatible)
vault as the single source of truth for your servers, then opens **embedded** SSH and SFTP
sessions to them — with passwords pulled straight from the vault. Think *Termius / Royal TSO*,
but the inventory lives in a vault you host, editable from any Bitwarden client, with no
separate proprietary cloud. It's built on [Tauri v2](https://tauri.app) (Rust backend + web
frontend) and [xterm.js](https://xtermjs.org).

> See [`DESIGN.md`](./DESIGN.md) for the full design and [`PLAN.md`](./PLAN.md) for the
> implementation plan.

## Features

- **Unlock by account.** Log in once with the Bitwarden CLI; the app shows which account is
  active and unlocks the vault with your master password — it never handles login itself.
- **Host management.** Add, edit, and delete hosts inline. Copy or reveal a host's password
  on demand (user-initiated secret egress only).
- **Embedded SSH terminal** with **auto password-fill**: ssharden connects over a PTY and,
  when `ssh` prompts for a password, injects the one stored in the vault — exactly once, so a
  later in-session prompt (e.g. `sudo`) is never auto-filled. Supports jump hosts (`ssh -J`).
- **One-click SFTP terminal** to any SSH host, reusing the same vault-backed credentials.
- **Graphical dual-pane SFTP browser** (local ↔ remote): browse directories, and create
  folders, rename, and delete on either side. Transfers run through a **queue with live
  progress**, streamed in fixed-size chunks so memory stays flat for files of any size.

## How a host is modeled

A host is a normal Bitwarden **Login item** — fully readable and editable from any Bitwarden
client. The URI **scheme** selects the protocol:

```jsonc
{
  "name": "prod-db-01",
  "login": {
    "username": "admin",
    "password": "…",                          // used for SSH/SFTP password auth and copy/reveal
    "uris": [{ "uri": "ssh://admin@10.0.0.5:22" }]
  },
  "fields": [
    { "name": "jump", "value": "bastion.corp" } // custom field → ssh -J
  ]
}
```

- **Scheme** (`ssh://`, `sftp://`) → which launcher runs; **host / port / user** come from the URI.
- Protocol extras live in **named custom fields** — e.g. `jump` for an `ssh -J` bastion.

## Prerequisites

- **Rust** (stable) and **[bun](https://bun.sh)**.
- **[Bitwarden CLI](https://bitwarden.com/help/cli/)** (`bw`): `npm install -g @bitwarden/cli`
- **Tauri v2 Linux system libraries** (webkit2gtk etc. — the one step that needs root).

Run [`./setup.sh`](./setup.sh) to install the system libraries, the Tauri CLI, and the frontend
dependencies in one shot. (Or install the Tauri prerequisites manually per
[the Tauri docs](https://tauri.app/start/prerequisites/).)

### Log in once

`bw serve` only **unlocks** an account — it never logs in. Point the CLI at your server and
authenticate a single time:

```bash
bw config server https://your-vaultwarden.example.com   # self-hosted; skip for bitwarden.com
bw login
```

After that, the app handles unlock/lock from its own window.

## Run

```bash
# Install frontend deps (once)
bun install

# Dev mode (builds the Rust shell + frontend, hot-reloads the UI)
cargo tauri dev
```

Enter your master password in the app to unlock; hosts appear in the sidebar.

A standalone, distributable build is:

```bash
cargo tauri build
```

### Test the core

The real logic lives in the pure-Rust `ssharden-core` crate and needs no webkit/GUI to verify:

```bash
cargo test -p ssharden-core
```

## Status & security

ssharden is **early and experimental.** It inherits Bitwarden's zero-knowledge model and adds a
connection launcher on top; the rules below are enforced in code:

- **The master password** flows to the backend once. The **session token stays in Rust process
  memory** — never returned to the webview, never written to disk, never logged.
- **Secrets never touch argv or logs.** The SSH/SFTP password is injected over the **PTY**, never
  as a command-line argument.
- **Idle auto-lock** drops the session and returns to the unlock screen.
- **Host-key verification:** the interactive `ssh`/`sftp` CLI paths do full `~/.ssh/known_hosts`
  checking — never disabled, prompts surface in the terminal. The graphical SFTP browser (a
  pure-Rust russh client) currently accepts host keys **trust-on-first-use (TOFU)**; wiring it
  into `known_hosts` is a tracked follow-up.
- **The `bw serve` loopback API is unauthenticated by design.** It exposes the decrypted vault
  over localhost with no auth, so ssharden binds it to **`127.0.0.1` on an ephemeral random port**
  (never `0.0.0.0`), kills the child process on drop, and pairs it with aggressive auto-lock.

A tool that decrypts your whole host inventory is a juicy local target. ssharden is not more
exposed than Bitwarden desktop itself, and the `bw serve` port gets the loopback + ephemeral +
aggressive-lock treatment to keep it that way — but treat it accordingly.

## License

MIT — see [`LICENSE`](./LICENSE).
