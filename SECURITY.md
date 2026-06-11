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
  registered commands, and runs under a strict Content-Security-Policy
  (`src-tauri/tauri.conf.json`: `default-src 'self'`, no remote origins;
  `'unsafe-inline'` is granted to **styles only**, which xterm.js requires).

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
  later in-session prompt (e.g. `sudo`) is not auto-filled. The external RDP
  launcher feeds the password to FreeRDP over **stdin** (`/from-stdin`), never
  argv.
- **Auto-fill matches the exact expected prompt**, not any text ending in
  `password:` — only `user@host's password:` or the keyboard-interactive
  `(user@host) Password:` for the specific target being connected triggers
  injection. A crafted server banner, a jump host's prompt, or an in-session
  prompt does not match.
- **Host-key checking is never disabled** on the interactive `ssh`/`sftp` path;
  prompts surface in the embedded terminal and reuse `~/.ssh/known_hosts`. The
  graphical SFTP browser's russh client verifies against the same
  `~/.ssh/known_hosts` (changed keys are refused; new hosts are recorded
  `accept-new`-style).
- **Vault SSH keys**: the terminal path materializes a private key to a `0600`
  temp file (removed when the session ends or spawn fails); the graphical SFTP
  browser passes the key to russh **in memory**, with no file at all.
- **RDP server certificates are pinned trust-on-first-use** by FreeRDP
  (`/cert:tofu`): the first-seen cert is stored under `~/.config/freerdp` and a
  changed cert refuses to connect.
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

### 1. First contact is trust-on-first-use (SSH browser and RDP)

The graphical SFTP browser verifies server keys against `~/.ssh/known_hosts`
and **refuses a changed key**, but an *unknown* host is recorded and accepted
on first contact (OpenSSH `accept-new` behavior) without an interactive
fingerprint prompt. Likewise, FreeRDP pins the RDP server certificate on first
contact (`/cert:tofu`) and refuses changes afterwards. In both cases the very
first connection to a host is the unauthenticated window; a MITM present *at
first contact* can pin itself. A changed RDP cert currently fails **silently**
(the detached FreeRDP process exits before opening a window) — remove the
host's entry under `~/.config/freerdp/server/` to re-pin after a legitimate
rotation.

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

### 3. Residual password-prompt mimicry

Auto-fill now requires the **exact** OpenSSH prompt for the connected target
(see Secret handling), which defeats banner tricks and cross-host confusion.
What remains: the server you are connecting to can itself emit that exact
string (e.g. in a post-login MOTD when key auth succeeded without a password
prompt) and capture the stored password — but the secret at risk is only the
password stored *for that same server*. Injection remains capped at once per
session. Servers you connect to are semi-trusted; rotate the stored password if
a host you no longer trust may have elicited it.

## Reporting a vulnerability

Please report security issues **privately**:

- Open a [GitHub Security Advisory](https://github.com/looqey/ssharden/security/advisories/new)
  on the repository (preferred), **or**
- Email the maintainer at the address listed in the repository / git history.

Please include a description, affected version/commit, reproduction steps, and
the impact you observed. Do **not** open a public issue for an unfixed
vulnerability. We aim to acknowledge reports within a few days. ssharden is MIT
licensed and maintained on a best-effort basis; there is no formal bug bounty.
