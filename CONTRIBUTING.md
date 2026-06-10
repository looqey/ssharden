# Contributing to ssharden

Thanks for your interest. ssharden is a Tauri v2 desktop app — a Vaultwarden-backed
connection manager with embedded SSH, one-click SFTP, and a graphical dual-pane SFTP file
browser. Before touching code, read [`DESIGN.md`](./DESIGN.md) (vision + locked decisions),
[`ARCHITECTURE.md`](./ARCHITECTURE.md) (how the three layers fit together), and
[`PLAN.md`](./PLAN.md) (the original phased plan). The project is MIT licensed
(see [`LICENSE`](./LICENSE)).

## Project shape

A cargo workspace plus a TypeScript frontend:

- `crates/ssharden-core` — **pure Rust, no `tauri`/webkit**. All the real logic (vault
  adapter, host model, SSH PTY launcher, programmatic SFTP client). Compiles and unit-tests
  on any machine.
- `src-tauri` — the thin Tauri shell (the **only** webkit-linked crate); `#[tauri::command]`
  wrappers + event forwarding.
- `src/` — the TypeScript/Vite/xterm.js frontend.

## Setup

You need **Rust** (stable), **bun**, and the **Bitwarden CLI** (`bw`). Building the GUI
shell additionally needs Tauri's Linux system libraries (webkit2gtk etc.) — the one step
that needs root.

The fastest path is the setup script, which installs the Tauri system libraries, the Tauri
CLI, and the frontend dependencies:

```bash
./setup.sh
```

If you prefer to do it by hand:

```bash
# Bitwarden CLI
npm install -g @bitwarden/cli

# Tauri v2 Linux system libraries (needs root)
sudo apt-get install -y libwebkit2gtk-4.1-dev build-essential libxdo-dev \
  libssl-dev libayatana-appindicator3-dev librsvg2-dev pkg-config

# Tauri CLI + frontend deps
cargo install tauri-cli --locked
bun install
```

### Log in to your vault (once)

ssharden only **unlocks** the vault; logging in is done once out-of-band via the `bw` CLI:

```bash
bw config server https://your-vaultwarden.example.com   # self-hosted
bw login
```

After that, the app reaches the vault by spawning `bw serve` on a loopback ephemeral port
and unlocking with your master password typed into the app window.

## Build

```bash
cargo build -p ssharden-core   # core only — no webkit needed
bun run build                  # type-check + bundle the frontend (vite)
cargo build                    # full workspace, including the GUI shell (needs webkit libs)
```

## Run

```bash
cargo tauri dev        # build the GUI shell + frontend and launch the app
```

Enter your master password to unlock; SSH hosts appear in the sidebar. From a host row you
can open an embedded SSH session, a one-click SFTP session, or the graphical dual-pane file
browser.

## Test

```bash
cargo test -p ssharden-core    # the core unit tests — no webkit/GUI required
bunx tsc --noEmit              # type-check the frontend without emitting
```

`cargo test -p ssharden-core` is the primary gate and covers the parts most worth
verifying: the `bw serve` client surface, Host/URI parsing, and cipher round-tripping. It
needs no webkit, so it runs anywhere. The frontend is verified with `bun run build` (which
runs `tsc` + `vite build`) and a standalone `tsc --noEmit`.

## Coding conventions

- **The core stays webkit-free and unit-tested.** Put real logic in `ssharden-core`, behind
  small testable functions, with `#[cfg(test)]` coverage (see `vault/model.rs` for the
  pattern). `src-tauri` should stay a thin shell: resolve state, call core, forward
  events — no business logic. The core must never depend on `tauri` or any webkit/GUI crate
  so it keeps compiling and testing on any machine.
- **Secrets never on argv, never on disk, never in logs.** This is a hard rule, enforced in
  code:
  - The vault session token lives only in `VaultClient` (Rust memory). Never return it to
    JS, never persist it, never log it.
  - The master password flows webview → `vault_unlock` exactly once.
  - Passwords reach `ssh`/`sftp` over the **PTY** (`ssh_write` / the at-most-once auto-auth
    injector), never as a command-line argument.
  - `bw serve` binds `127.0.0.1` on an ephemeral port — never `0.0.0.0`.
  - Host-key checking is never disabled; TOFU surfaces in the UI / terminal and reuses
    `~/.ssh/known_hosts`.
- **Vault strings are untrusted** when rendered into the DOM — HTML-escape them (see the
  `esc`/`escapeHtml` helpers in `src/`).
- **Errors** map to `CoreError` in the core; the shell maps them to user-facing strings
  with the `e()` helper, which must never leak a secret.
- Keep the Rust idiomatic and documented (the existing modules carry `//!`/`///` doc
  comments); keep the TypeScript strict (`tsc --noEmit` clean) and the frontend talking to
  the backend only through `invoke()` / event listeners.
- Manifests (`Cargo.toml`, `tauri.conf.json`, `package.json`, `tsconfig.json`,
  `vite.config.ts`) are deliberately minimal — change them only when a change genuinely
  needs it.

## Proposing changes

1. Branch off `main` (don't commit directly to it).
2. Keep changes scoped; add or update `ssharden-core` unit tests for any logic change.
3. Before opening a PR, make these pass: `cargo test -p ssharden-core`, `bun run build`
   (or `bunx tsc --noEmit`), and — if you touched the shell or its deps — `cargo build`.
4. Confirm every Tauri command you add or change is registered in the `invoke_handler!` in
   `src-tauri/src/main.rs` and that its signature matches the `invoke()` call site in
   `src/`.
5. Open a PR against `main` describing what changed and how you verified it. Note explicitly
   whether the GUI shell was built (webkit libs present) or only the core + frontend were
   verified.

By contributing you agree your work is licensed under the project's MIT license.
