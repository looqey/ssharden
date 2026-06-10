# ssharden

Your servers live in your Vaultwarden vault. ssharden connects to them.

It's a desktop app that treats a self-hosted **Vaultwarden** (or Bitwarden) vault as
the source of truth for your hosts, then opens SSH and SFTP sessions to them right
inside the window — pulling the password from the vault so you don't retype it. The
inventory stays in a vault you control and that any Bitwarden client can edit. No
extra cloud, no separate password file.

Built with Tauri (Rust) and xterm.js. Linux for now.

## What it does

- **Unlock with your account.** You log in once with the `bw` CLI; the app shows whose
  vault it is and unlocks it with your master password. It never handles login itself.
- **Manage hosts in-app.** Add, edit, delete. Copy or reveal a host's password when you
  need it.
- **SSH in a real terminal**, with the vault password typed in for you at the prompt
  (once — a later `sudo` prompt won't get auto-filled). Jump hosts work (`ssh -J`).
- **SFTP two ways:** a quick terminal session, or a **dual-pane file browser** — local on
  the left, remote on the right, with make-folder / rename / delete and a transfer queue
  that shows progress. Big files stream in chunks, so they won't eat your RAM.

## How a host is stored

A host is just a Bitwarden **Login item**. The URI scheme picks the protocol:

```jsonc
{
  "name": "prod-db-01",
  "login": {
    "username": "admin",
    "password": "…",
    "uris": [{ "uri": "ssh://admin@10.0.0.5:22" }]
  },
  "fields": [{ "name": "jump", "value": "bastion.corp" }]
}
```

`ssh://` or `sftp://` decides the launcher; host, port and user come from the URI.
Extras like a bastion go in a named custom field (`jump` → `ssh -J`). Edit it from the
ssharden host form or from any Bitwarden app — it's the same item.

## Getting started

You need **Rust**, **[bun](https://bun.sh)**, the **[Bitwarden CLI](https://bitwarden.com/help/cli/)**
(`npm install -g @bitwarden/cli`), and the Tauri/webkit system libraries. The included
`./setup.sh` installs the system libs, the Tauri CLI and the frontend deps in one go.

Point the CLI at your server and log in once (the app only ever *unlocks*, it can't log
in for you):

```bash
bw config server https://vault.example.com   # skip for bitwarden.com
bw login
```

Then run it:

```bash
bun install
cargo tauri dev          # development, with hot reload
# or a real build:
cargo tauri build        # produces a standalone binary + .deb
```

The Rust core has no GUI dependency, so you can run its tests anywhere:

```bash
cargo test -p ssharden-core
```

## A word on security

ssharden is young — treat it as such. It keeps Bitwarden's zero-knowledge model and adds
a launcher on top:

- Your master password reaches the backend once. The unlocked session token stays in
  Rust memory — never sent to the UI, never written to disk, never logged.
- Passwords go into the SSH/SFTP session over the PTY, never on a command line.
- Idle auto-lock drops the session.
- Host keys are checked against `~/.ssh/known_hosts` (the file your normal `ssh` uses).
  A changed key is refused; a new host is recorded on first use.
- The `bw serve` API that does the decryption is unauthenticated by design, so ssharden
  binds it to `127.0.0.1` on a random port, kills it when the app exits, and leans on the
  auto-lock. It's no more exposed than Bitwarden's own desktop app — but it does decrypt
  your whole host list, so run it on a machine you trust.

Found a hole? See [SECURITY.md](./SECURITY.md).

## License

MIT. See [LICENSE](./LICENSE).
