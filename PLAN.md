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

## File layout

```
ssharden/
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
  src-tauri/
    Cargo.toml            # ALL backend deps (single source — implementers don't edit)
    tauri.conf.json
    build.rs
    src/
      main.rs             # entrypoint: app state + register all #[tauri::command]s
      error.rs            # AppError + Result alias
      vault/mod.rs        # bw serve adapter: spawn/supervise, unlock, lock, sync, list/get
      vault/model.rs      # Host parsing: Login item + URI scheme → structured Host
      ssh/mod.rs          # PTY launcher: portable-pty spawns ssh, streams to frontend
      rdp/mod.rs          # STUB — TODO(phase-2): IronRDP
      sftp/mod.rs         # STUB — TODO(phase-3): SFTP file browser
  setup.sh                # installs rustup + tauri prereqs + bw CLI; prints next steps
  README.md               # what it is, how to install toolchain, how to run
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

- `cargo`/`rustc` are **not installed**, so this pass produces source that compiles once
  the toolchain exists — it is **not** compile-verified by the workflow.
- `setup.sh` installs the toolchain; `README.md` documents `bun install` + `cargo tauri dev`.
- Frontend (`bun install`, `bun run build`) *can* be verified since bun is present.

## Workflow shape

1. **Scaffold** (1 agent) — create the tree, write the complete `Cargo.toml` /
   `tauri.conf.json` / `package.json` (the only manifests; implementers don't touch them),
   plus `main.rs` with module declarations + command registration, and typed stubs that
   define every interface above.
2. **Implement** (parallel, against the contracts) — vault adapter, ssh PTY launcher,
   frontend host-list+unlock, frontend terminal+bridge, setup.sh+README.
3. **Integrate & verify** (1 agent) — confirm every command is registered and signatures
   line up, run `bun install`/`bun run build` for the frontend, and report exactly what is
   and isn't verifiable without Rust.

## Out of scope this pass
RDP (IronRDP), SFTP browser, org/collection sharing, multi-OS packaging, auto-update.
Tracked as later phases in `DESIGN.md`.
