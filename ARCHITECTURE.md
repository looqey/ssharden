# Architecture

ssharden is a Tauri v2 desktop app that keeps your host inventory in a **Vaultwarden**
(Bitwarden-compatible) vault and opens **embedded** SSH, SFTP, and a graphical dual-pane
SFTP file browser to those hosts. The single source of truth is your own vault; there is
no proprietary cloud.

This document describes how the app is actually built. For the product vision and locked
decisions see [`DESIGN.md`](./DESIGN.md); for the original Phase 0 plan see
[`PLAN.md`](./PLAN.md). Where this document and those disagree, this document reflects the
code as it stands (notably: the SFTP file browser, programmatic SFTP client, and host
create/edit/delete have all shipped past the original Phase 0 scope).

## The three layers

```
┌───────────────────────────────────────────────────────────────────────┐
│  Tauri app                                                             │
│                                                                        │
│  Frontend (webview)              Tauri shell (Rust)                    │
│  src/  TS + Vite + xterm.js      src-tauri/  #[tauri::command]s        │
│  ┌────────────────────┐  invoke  ┌──────────────────────┐             │
│  │ unlock screen      │◄────────►│ AppState:            │             │
│  │ host list / form   │          │  - VaultClient       │──┐          │
│  │ xterm SSH/SFTP tabs │◄──events─│  - SSH sessions     │  │          │
│  │ dual-pane browser  │          │  - SFTP connections  │  │          │
│  └────────────────────┘          └──────────┬───────────┘  │          │
│                                              │ calls        │          │
│                                  ssharden-core (pure Rust)  │          │
│                                  crates/ssharden-core/      │          │
│                                  ┌──────────────────────┐   │          │
│                                  │ vault  bw serve client │──┼─HTTP─┐  │
│                                  │ ssh    PTY launcher    │  │      │  │
│                                  │ sftp   russh client    │──┼─SSH──┼─►remote
│                                  │ rdp    stub (phase 2)  │  │      │  │
│                                  └──────────────────────┘   │   ┌──▼──────────┐
└──────────────────────────────────────────────────────────────┤ bw serve     │─►Vaultwarden
                                                                 │ 127.0.0.1    │
                                                                 │ :ephemeral   │
                                                                 └─────────────┘
```

### 1. `ssharden-core` (pure Rust, `crates/ssharden-core`)

The real logic. It has **no `tauri` or webkit dependency**, so it compiles and unit-tests
on any machine (`cargo test -p ssharden-core`). It owns:

- the `bw serve` vault adapter (spawn/supervise, unlock/lock/sync, list/get/create/update/
  delete items),
- the `Host`/URI model and the parser that turns Bitwarden Login items into structured
  hosts,
- the SSH/SFTP **PTY launcher** (spawns the system `ssh`/`sftp` binaries), and
- a **programmatic SFTP client** (pure Rust `russh` + `russh-sftp`) that powers the file
  browser.

`rdp` is a stub reserved for a later phase (IronRDP).

### 2. `src-tauri` (Tauri shell, the only webkit-linked crate)

A thin wrapper. It holds `AppState` (a `VaultClient` plus registries of live SSH sessions
and SFTP connections) and exposes `#[tauri::command]`s that call into core. It is the trust
boundary: the vault session token lives here (inside `VaultClient`) and never crosses back
to the webview. It also forwards PTY bytes and transfer progress to the frontend as Tauri
events.

### 3. Frontend (`src/`, TypeScript + Vite + xterm.js)

The UI: unlock screen → host list (with create/edit/delete) → workspace tabs that hold
either an xterm.js terminal (SSH or interactive SFTP) or the graphical dual-pane SFTP
browser. It talks to the backend only through `invoke()` and Tauri event listeners. It
never sees the master password after submitting it once, and never receives the session
token.

## Data flow

### Unlock

1. The frontend (`src/main.ts`) shows the unlock screen and calls `account_status`
   (`ssharden_core::account_status`, runs `bw status`) to display which `bw` account is
   logged in — login is done once out-of-band via the `bw` CLI; the app only **unlocks**.
2. On submit it calls `vault_start`, which spawns `bw serve` bound to
   `127.0.0.1:<ephemeral port>` (`VaultClient::start`). The port is picked by binding an
   OS-assigned ephemeral port and releasing it; `bw serve` is never bound to `0.0.0.0`. On
   Linux the child gets `PR_SET_PDEATHSIG`/`SIGTERM` so it can never be orphaned, and
   `kill_on_drop` ties its lifetime to the app.
3. `vault_unlock` POSTs the master password once to `bw serve`'s `/unlock`. The returned
   session token (`data.raw`) is stored only in `VaultClient.session` (Rust memory) — never
   returned to JS, never written to disk, never logged.

### List hosts

`vault_list_hosts` → `VaultClient::list_hosts`: POSTs `/sync`, GETs `/list/object/items`,
then runs each cipher through `host_from_cipher`. Only Bitwarden **Login items** (`type ==
1`) that carry at least one URI in a recognized host scheme (`ssh`/`sftp`/`rdp`/`ftp`)
become a `Host`. The URI **scheme** selects the launcher; host/port/user come from the URI;
protocol extras (e.g. `jump` → `ssh -J`) come from known-named custom fields. The frontend
currently filters the rendered list to hosts with an `ssh` URI.

### SSH / interactive SFTP (PTY → xterm.js)

1. The frontend calls `ssh_connect` (or `sftp_connect`) with a `host_id`.
2. The shell resolves the host under the vault lock, builds `SshParams`, and fetches the
   stored password for best-effort auto-auth. It then releases the lock and spawns
   `SshSession::spawn` / `spawn_sftp` — the system `ssh`/`sftp` binary inside a
   `portable-pty` PTY. **No secret is ever placed on argv.**
3. A dedicated reader thread forwards PTY bytes to the webview as `ssh://{id}` events. When
   the stream ends with a `password:` prompt and a password is stored, it injects the
   password over the PTY **at most once** (so a later in-session `sudo` prompt is never
   auto-filled).
4. The frontend (`src/terminal.ts`) mounts an xterm.js terminal, writes incoming bytes,
   sends typed input back via `ssh_write`, and keeps the remote PTY sized via `ssh_resize`.
   Host-key checking is never disabled — TOFU prompts surface in the terminal and reuse
   `~/.ssh/known_hosts`.

### Graphical SFTP file browser (programmatic SFTP)

1. `sftp_open` resolves host + password, opens an `SftpConn` (`russh` password auth +
   `russh-sftp` subsystem), canonicalizes the remote home, and stores the connection in
   `AppState.sftp_conns`, returning `{ conn_id, home }`.
2. The frontend (`src/sftpui.ts`) renders a dual pane: the local filesystem (left, via
   `local_ls`/`local_home`/`local_mkdir`/`local_rename`/`local_rm`) and the remote host
   (right, via `sftp_ls`/`sftp_mkdir`/`sftp_rename`/`sftp_rm`).
3. Transfers use `sftp_get` (download) and `sftp_put` (upload), streamed in 128 KB chunks
   so memory stays flat. Throttled progress is emitted as `xfer://{transfer_id}` events
   carrying `(done, total)`.

   > Security note: the programmatic SFTP client currently accepts the server host key on
   > first contact (TOFU without persistence) — wiring `~/.ssh/known_hosts` verification
   > into it is a follow-up. The interactive `ssh`/`sftp` CLI path already does full
   > `known_hosts` checking.

### Lock / auto-lock

`vault_lock` → `VaultClient::lock` POSTs `/lock` and drops the in-memory token. The
frontend has a 10-minute idle auto-lock (reset on key/mouse activity) and a manual Lock
button; locking disposes all open tabs and returns to the unlock screen. Dropping
`VaultClient` kills the owned `bw serve` child.

## Module map

### `crates/ssharden-core/src`

| Module | Responsibility |
|---|---|
| `lib.rs` | Crate root; re-exports the public API (`VaultClient`, `Host`, `HostInput`, `HostUri`, `AccountStatus`, `SshParams`, `SshSession`, `SftpConn`, `FsEntry`, `CoreError`, …). |
| `error.rs` | `CoreError` enum (`Http`, `Io`, `Json`, `Bw`, `NotFound`, `Spawn`, `Sftp`) and the `Result` alias. |
| `vault/mod.rs` | `VaultClient`: owns the `bw serve` child; `start`/`set_server`/`unlock`/`lock`/`sync`/`list_hosts`/`get_item`/`create_host`/`update_host`/`delete_host`/`host_password`/`shutdown`. Also `account_status` (standalone `bw status`). |
| `vault/model.rs` | The `Host`/`HostUri`/`HostInput`/`AccountStatus` types; `parse_host_uri`, `host_from_cipher`, `login_cipher_json`. Carries the unit tests for URI parsing and cipher round-tripping. |
| `ssh/mod.rs` | `SshParams` and `SshSession`: the `portable-pty` launcher for the system `ssh`/`sftp` binaries (`spawn`, `spawn_sftp`, reader/writer/`resize`). |
| `sftp/mod.rs` | `SftpConn` and `FsEntry`: the pure-Rust `russh`/`russh-sftp` client for the file browser (`connect`, `canonicalize`, `list`, `download`, `upload`, `create_dir`, `rename`, `remove`). |
| `rdp/mod.rs` | Stub. `TODO(phase-2)`: embedded RDP via IronRDP. |

### `src/` (frontend)

| File | Responsibility |
|---|---|
| `main.ts` | Bootstrap and orchestration: unlock screen, sidebar layout, workspace tab management, host CRUD wiring, lock / idle auto-lock, toasts. |
| `vault.ts` | The `invoke()` bridge to the vault/host Rust commands; mirrors the command signatures and declares the `Host`/`HostUri`/`HostInput`/`AccountStatus` TS types. |
| `hosts.ts` | Host-list rendering (grouped by folder, filtered to `ssh` URIs) with per-row actions (connect / SFTP / edit / copy password / delete). |
| `form.ts` | Host create/edit modal; resolves to a `HostInput`. |
| `terminal.ts` | `TerminalSession`: xterm.js wiring for SSH/interactive-SFTP — connects, streams the `ssh://{id}` PTY events, sends `ssh_write`/`ssh_resize`. |
| `sftpui.ts` | `SftpBrowser`: the graphical dual-pane file browser, transfer queue, and `xfer://{id}` progress handling. |
| `styles.css` | Styles (referenced from `main.ts`). |

### `src-tauri`

| File | Responsibility |
|---|---|
| `src/main.rs` | `AppState`, every `#[tauri::command]`, the shared `open_session` PTY/auto-auth helper, the `xfer_progress` throttle, and the `invoke_handler` registration. The only webkit-linked crate. |
| `tauri.conf.json`, `build.rs`, `capabilities/` | Tauri app/build configuration and capability allowlist. |

## Tauri commands

Defined in `src-tauri/src/main.rs`, registered in `tauri::generate_handler!`.

**Vault & account**

| Command | Signature (Rust) | Behavior |
|---|---|---|
| `vault_start` | `() -> Result<()>` | Spawn `bw serve` on a loopback ephemeral port; idempotent. |
| `vault_unlock` | `(server_url, master_password) -> Result<()>` | Best-effort `bw config server`, then `/unlock`; token stays in Rust. |
| `vault_lock` | `() -> Result<()>` | `/lock`; drop the in-memory token. |
| `account_status` | `() -> Result<AccountStatus>` | `bw status` — which account is logged in (no `bw serve` needed). |
| `vault_list_hosts` | `() -> Result<Vec<Host>>` | `/sync` then list items → parsed `Host`s. |

**Host CRUD**

| Command | Signature | Behavior |
|---|---|---|
| `host_create` | `(input: HostInput) -> Result<()>` | Create a Login item from user input. |
| `host_update` | `(id, input) -> Result<()>` | Update, preserving blank password/folder. |
| `host_delete` | `(id) -> Result<()>` | Delete the vault item. |
| `host_password` | `(id) -> Result<Option<String>>` | Fetch the stored password for copy/reveal (user-initiated). |

**SSH / interactive SFTP (PTY)**

| Command | Signature | Behavior |
|---|---|---|
| `ssh_connect` | `(host_id) -> Result<SessionId>` | Spawn `ssh` in a PTY; stream `ssh://{id}` events. |
| `sftp_connect` | `(host_id) -> Result<SessionId>` | Spawn interactive `sftp` in a PTY; same event stream. |
| `ssh_write` | `(session_id, data) -> Result<()>` | Write bytes (incl. typed password) to the PTY. |
| `ssh_resize` | `(session_id, cols, rows) -> Result<()>` | Resize the PTY. |

**Graphical SFTP browser**

| Command | Signature | Behavior |
|---|---|---|
| `sftp_open` | `(host_id) -> Result<{conn_id, home}>` | Open a programmatic SFTP connection; return its id + remote home. |
| `sftp_ls` | `(conn_id, path) -> Result<Vec<FsEntry>>` | List a remote directory. |
| `sftp_get` | `(conn_id, remote, local, transfer_id) -> Result<()>` | Download; emits `xfer://{id}` progress. |
| `sftp_put` | `(conn_id, local, remote, transfer_id) -> Result<()>` | Upload; emits `xfer://{id}` progress. |
| `sftp_mkdir` / `sftp_rename` / `sftp_rm` | `(conn_id, …) -> Result<()>` | Remote file ops (rm takes `is_dir`). |
| `sftp_close` | `(conn_id) -> Result<()>` | Drop the connection. |
| `local_home` | `() -> String` | Local home directory (left pane start). |
| `local_ls` | `(path) -> Result<Vec<FsEntry>>` | List a local directory (dirs first). |
| `local_mkdir` / `local_rename` / `local_rm` | `(…) -> Result<()>` | Local file ops (rm takes `is_dir`). |

`Host` = `{ id, name, folder_id, username, uris: [{ scheme, host, port, user, raw }],
fields: map }`. `FsEntry` = `{ name, is_dir, size }`.
