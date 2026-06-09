# ssharden — Phase 0 implementation plan

Scope of this build: the **Phase 0 spike** from `DESIGN.md` — prove the chain
*Vaultwarden → bw serve → host list → embedded SSH terminal* end to end. RDP (IronRDP)
and the SFTP browser are explicitly **out of scope** for this pass; their modules are
stubbed with clear `TODO(phase-2/3)` markers so the architecture is ready for them.

## Target stack

- **Tauri v2** (Rust backend + web frontend).
- Frontend: TypeScript + Vite + **xterm.js** (`@xterm/xterm`, `@xterm/addon-fit`),
  `@tauri-apps/api`. Package manager: **bun** (present).
- Backend crates: `tauri`, `tokio`, `portable-pty`, `reqwest` (talk to bw serve),
  `serde`/`serde_json`, `url`, `thiserror`.
- Vault engine: official **`bw` CLI** in `bw serve` mode, bound to `127.0.0.1:<ephemeral>`.

## File layout (cargo workspace: verifiable core + thin GUI shell)

The real logic lives in a **pure-Rust `ssharden-core` crate** with **no `tauri`/webkit
dependency**, so it compiles and unit-tests on any machine. `src-tauri` is a thin shell
that wraps core in `#[tauri::command]`s and needs the webkit system libs only to build the
GUI. This makes the core verifiable *now* and isolates the part that waits on `sudo apt`.

```
ssharden/
  Cargo.toml              # workspace: members = ["crates/ssharden-core", "src-tauri"]
  package.json            # frontend deps + scripts
  vite.config.ts
  index.html
  tsconfig.json
  src/                    # frontend (TypeScript)
    main.ts               # bootstrap: unlock screen → host list → terminal tabs
    vault.ts              # invoke() bridge to Rust vault commands
    hosts.ts              # host-list rendering, grouped by folder, ssh:// filter
    terminal.ts           # xterm.js wiring; PTY byte stream over Tauri events
    styles.css
  crates/ssharden-core/   # PURE Rust — no tauri, no webkit; compiles + tests anywhere
    Cargo.toml            # reqwest, tokio, portable-pty, serde, serde_json, url, thiserror
    src/
      lib.rs              # re-exports public API
      error.rs            # CoreError + Result alias
      vault/mod.rs        # bw serve adapter: spawn/supervise, unlock, lock, sync, list/get
      vault/model.rs      # Host parsing: Login item + URI scheme → Host  (#[cfg(test)] unit tests)
      ssh/mod.rs          # PTY launcher: portable-pty spawns ssh, yields bytes over a channel
      rdp/mod.rs          # STUB — TODO(phase-2): IronRDP
      sftp/mod.rs         # STUB — TODO(phase-3): SFTP file browser
  src-tauri/
    Cargo.toml            # depends on ssharden-core + tauri; the ONLY webkit-linked crate
    tauri.conf.json
    build.rs
    src/main.rs           # thin: app state + #[tauri::command]s that call core + forward
                          #       PTY bytes to the webview as `ssh://{id}` events
  setup.sh                # apt deps (webkit2gtk-4.1 etc.) + reminders; rust/bw already in
  README.md               # what it is, how to install the GUI system libs, how to run
```

## Backend contract (Tauri commands)

| Command | Signature | Behavior |
|---|---|---|
| `vault_start` | `() -> Result<()>` | Spawn `bw serve` on `127.0.0.1:<ephemeral>`, store port in state. Never `0.0.0.0`. |
| `vault_unlock` | `(server_url, master_password) -> Result<()>` | `bw config server` if needed; `POST /unlock`; keep session token in Rust state only. |
| `vault_lock` | `() -> Result<()>` | `POST /lock`; zeroize token; reset auto-lock timer. |
| `vault_list_hosts` | `() -> Result<Vec<Host>>` | `POST /sync` then `GET /list/object/items`; parse Login items → `Host`. |
| `ssh_connect` | `(host_id) -> Result<SessionId>` | Resolve host; `portable-pty` spawn `ssh`; stream stdout as `ssh://{id}` Tauri events. |
| `ssh_write` | `(session_id, data) -> Result<()>` | Write bytes (incl. typed password) to the PTY. |
| `ssh_resize` | `(session_id, cols, rows) -> Result<()>` | Resize PTY. |

`Host` = `{ id, name, folder_id, username, uris: [{scheme, host, port, user}], fields: map }`.

## Security rules (enforced in code, from DESIGN.md)

- bw serve bound to **loopback + ephemeral port**; port held in Rust state, never logged.
- Master password flows webview → `vault_unlock` once; **session token stays in Rust**,
  never returned to JS, never written to disk.
- **Auto-lock timer** (idle); lock drops/zeroizes the token.
- SSH password fed to the **PTY via `ssh_write`**, never on argv. Host-key prompts surface
  in the terminal (reuse `~/.ssh/known_hosts`); host-key checking never disabled.
- No secret ever appears in logs or command arguments.

## Build / verify reality

Toolchain status on this machine: **Rust 1.96.0 installed**, **bw CLI installed**, **bun
present**. Missing: Tauri's webkit system libs (`webkit2gtk-4.1`, `gtk-3.0`, `libsoup-3.0`,
…) which need `sudo apt` (no passwordless sudo here).

- **`ssharden-core` IS compile- and test-verified** by the workflow: `cargo test -p
  ssharden-core` (no webkit needed). This covers the bw-serve client, Host/URI parsing, and
  PTY launcher logic — the parts most worth verifying.
- **Frontend IS verified**: `bun install` + `bun run build`.
- **`src-tauri` GUI shell is NOT built** until the webkit libs are installed. `setup.sh`
  prints the exact `sudo apt install …` line; once run, `cargo tauri dev` builds the app.

## Workflow shape

1. **Scaffold** (1 agent) — create the workspace tree; write the complete workspace
   `Cargo.toml`, both crate `Cargo.toml`s, `tauri.conf.json`, `package.json`, `tsconfig`,
   `vite.config` (the only manifests; implementers don't touch them); write `lib.rs` /
   `main.rs` with module decls + command registration, and typed stubs defining every
   interface above so the parallel agents have hard contracts.
2. **Implement** (parallel, against the contracts) — (a) core vault adapter + Host/URI
   model with unit tests, (b) core ssh PTY launcher, (c) src-tauri command shell + event
   forwarding, (d) frontend host-list+unlock, (e) frontend terminal+bridge, (f)
   setup.sh+README.
3. **Integrate & verify** (1 agent) — `cargo test -p ssharden-core` (must pass), `bun
   install` + `bun run build` (must pass), confirm every Tauri command is registered and
   signatures line up with the frontend `invoke()` calls, and report exactly what is built
   vs. pending the webkit `sudo apt` step.

## Out of scope this pass
RDP (IronRDP), SFTP browser, org/collection sharing, multi-OS packaging, auto-update.
Tracked as later phases in `DESIGN.md`.
