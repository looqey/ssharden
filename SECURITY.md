# Security Policy

This document describes the security model of **ssharden**, its known
limitations, and how to report vulnerabilities. It is intended to be honest
about the trust assumptions the app makes so that operators can decide whether
those assumptions hold in their environment.

ssharden is a Tauri v2 desktop app: a Vaultwarden-backed connection manager
with an embedded SSH terminal, one-click `sftp`, and a graphical dual-pane SFTP
file browser. Read `DESIGN.md` and `PLAN.md` for architecture context.

## Security model

### Components and trust boundaries

- **`ssharden-core`** (pure-Rust workspace crate): owns the `bw serve` child,
  the Host/URI model, the SSH PTY launcher, and the russh-based SFTP client.
- **`src-tauri`** (Tauri shell): holds app state (`VaultClient`, live SSH/SFTP
  sessions) and exposes `#[tauri::command]`s to the webview.
- **Webview / frontend** (`src/`, TypeScript + xterm.js): the UI. Treated as
  the *least* trusted in-process component — it may only call the explicitly
  registered commands.

The principal trust boundary is **webview ↔ Rust**. The design goal is that the
webview never holds the vault session token and never receives a long-lived
secret except by explicit, user-initiated action (password reveal/copy).

### Vault access (`bw serve`)

- Login happens **once**, out of band, via the `bw` CLI (`bw login`). ssharden
  never sees the login flow.
- To unlock, ssharden spawns `bw serve` bound to **`127.0.0.1` on an ephemeral
  port** (`crates/ssharden-core/src/vault/mod.rs`, `pick_loopback_port` +
  `--hostname 127.0.0.1`). It never binds `0.0.0.0`.
- The `bw serve` child's lifetime is tied to the app via
  `PR_SET_PDEATHSIG` (Linux), so it is not orphaned if the app is hard-killed.
- The **vault session token** from `/unlock` lives only in Rust process memory
  (`VaultClient.session`). It is never logged, never written to disk, and never
  returned to the webview. The master password flows webview → Rust once during
  `vault_unlock` and is not retained beyond the unlock call.
- `reqwest` is built with `.no_proxy()` so loopback vault traffic cannot be
  redirected through an `HTTP(S)_PROXY` environment variable.

### Secret handling

- **No secret is ever placed on a process argv.** The system `ssh`/`sftp`
  binaries are launched in a PTY (`crates/ssharden-core/src/ssh/mod.rs`) with
  only host/port/user/jump on the command line. When the server prompts for a
  password and one is stored, the password is fed **over the PTY**
  (`src-tauri/src/main.rs`, `open_session`), and only **once** per session, so a
  later in-session prompt (e.g. `sudo`) is not auto-filled.
- **Host-key checking is never disabled** on the interactive `ssh`/`sftp` path;
  prompts surface in the embedded terminal and reuse `~/.ssh/known_hosts`.
- **Password egress** to the webview happens only through the `host_password`
  command, which is a deliberate, user-initiated copy/reveal action.

### Filesystem and command surface

- SSH/SFTP targets are built from structured `SshParams` and passed as discrete
  argv entries via `CommandBuilder` — there is no shell interpretation, so a
  hostname or username cannot inject extra arguments into a shell. (See the
  argument-injection note under Known limitations.)
- The graphical file browser commands (`local_ls`, `local_mkdir`,
  `local_rename`, `local_rm`, `sftp_*`) operate on absolute paths supplied by
  the webview and run with the **full privileges of the desktop user**. They are
  not sandboxed to a root directory.

## Known limitations

These are accepted, documented trade-offs in the current version. Treat them as
the security caveats of running ssharden.

### 1. SFTP host key is Trust-On-First-Use (TOFU)

The russh-based SFTP client used by the **graphical file browser**
(`crates/ssharden-core/src/sftp/mod.rs`, `check_server_key`) currently returns
`Ok(true)` for **any** server key — it does not verify against
`~/.ssh/known_hosts`, and it does not even pin the key for the duration of the
app. This means the graphical SFTP browser provides **no protection against a
man-in-the-middle** between the app and the SSH server, and because this path
authenticates with the host's **stored password**, a MITM can capture that
password. The interactive `ssh`/`sftp` CLI path is unaffected (it does full
`known_hosts` verification). Wiring `known_hosts` verification into the russh
client is the top follow-up.

**Mitigation today:** only use the graphical SFTP browser on networks you
trust, or prefer the interactive `sftp` session (which honours `known_hosts`).

### 2. Loopback trust assumption for `bw serve`

`bw serve` exposes the Vault Management API on a loopback port with **no
additional authentication** on individual requests (Bitwarden's CLI does not
require a per-request token for a locally-bound `bw serve`). Once the vault is
unlocked, **any local process running as the same user** that can reach the
ephemeral loopback port can call the unlocked API (list items, read passwords).
The ephemeral, randomized port is obscurity, not a security control.

This is the standard `bw serve` threat model: ssharden trusts the local user
account. If untrusted code runs as your user, your unlocked vault is already at
risk regardless of ssharden.

### 3. Password-prompt auto-injection heuristic

Auto-fill detects the server's password prompt by checking whether the recent
PTY output ends with `password:` (case-insensitive). A hostile or misconfigured
server could craft banner/MOTD text to elicit the password injection, or to make
the first prompt something other than the intended SSH auth prompt. Injection is
capped at once per session to bound the blast radius.

### 4. No Content-Security-Policy

`src-tauri/tauri.conf.json` sets `app.security.csp` to `null`. The frontend is
local and loads no remote content, but a CSP would harden against any future
injection of attacker-controlled strings (host names, error text) into the DOM.

## Reporting a vulnerability

Please report security issues **privately**:

- Open a [GitHub Security Advisory](https://github.com/looqey/ssharden/security/advisories/new)
  on the repository (preferred), **or**
- Email the maintainer at the address listed in the repository / git history.

Please include a description, affected version/commit, reproduction steps, and
the impact you observed. Do **not** open a public issue for an unfixed
vulnerability. We aim to acknowledge reports within a few days. ssharden is MIT
licensed and maintained on a best-effort basis; there is no formal bug bounty.
