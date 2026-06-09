# ssharden — Vaultwarden-backed connection manager

A cross-platform desktop app that stores host requisites in a Vaultwarden vault and
opens **embedded** SSH / SFTP / RDP sessions to those hosts — "your host fleet,
unlocked everywhere, sourced from your own Vaultwarden."

Think *Termius / Royal TSO*, but the single source of truth is your Bitwarden-compatible
vault, and there is no separate proprietary cloud.

---

## Locked decisions

| Decision | Choice | Why |
|---|---|---|
| Form factor | Cross-platform desktop GUI | "Open it everywhere" with full embedded UX |
| Shell | **Tauri** (Rust backend + web frontend) | Light (~10MB), strong security posture for a creds app, Rust-native engines drop in |
| Vault integration | **`bw serve`** localhost adapter (official Bitwarden CLI) | Near-zero crypto code; persistent unlock; JSON over loopback HTTP; swappable for the native SDK later |
| Host data model | **Login items + protocol URIs** | Portable, editable from any Bitwarden client; scheme encodes protocol |
| SSH/SFTP render | Embedded (PTY + xterm.js / file-browser) | Coherent in-app experience, secret injection we control |
| RDP render | **IronRDP** (pure-Rust, WASM canvas) — spike early | Embedded RDP without a C daemon; native fit for Tauri; Guacamole is the documented fallback |
| FTP shape | **SFTP file-browser panel** (dual-pane) | Modern, secure, most common; classic FTP/FTPS later if needed |
| First OS | **Linux** | Native `ssh`; cleanest launch story; `guacd`-free path via IronRDP |
| Multi-user / org sharing | **Deferred** (model stays collection-aware) | MVP reads personal vault; org collections = shared fleets later, drop-in |

---

## Architecture

```
┌──────────────────────────────────────────────────────────┐
│  Tauri app                                                │
│                                                           │
│  Web frontend (webview)         Rust backend              │
│  ┌───────────────────┐          ┌──────────────────────┐  │
│  │ Host list / search│◄────────►│ Vault adapter        │──┼─HTTP─┐
│  │ (grouped by       │          │  (manages bw serve,  │  │      │
│  │  folder/collection)│          │   unlock/lock/sync)  │  │   ┌──▼─────────┐
│  ├───────────────────┤          ├──────────────────────┤  │   │ bw serve    │──► Vaultwarden
│  │ xterm.js  (SSH)   │◄────PTY──►│ Launcher: SSH (pty)  │  │   │ 127.0.0.1   │   (self-hosted)
│  │ file browser(SFTP)│◄────────►│ Launcher: SFTP       │  │   │ :ephemeral  │
│  │ canvas    (RDP)   │◄─IronRDP─►│ Launcher: RDP        │  │   └─────────────┘
│  └───────────────────┘          └──────────────────────┘  │
└──────────────────────────────────────────────────────────┘
```

Three independently testable modules:

1. **Vault adapter** — spawns/owns the `bw serve` child, performs unlock/lock/sync,
   parses items → `Host` objects. Internal interface is engine-agnostic so the native
   Bitwarden SDK can replace `bw serve` later without touching callers.
2. **Host model / parser** — turns a Login item into a structured `Host` (below).
3. **Launcher** — per-protocol strategy that opens a session and renders it embedded,
   without ever putting a secret on a command line.

---

## Host data model

A host is a normal Bitwarden **Login item** by convention — still fully readable/editable
from any Bitwarden client:

```jsonc
{
  "name": "prod-db-01",
  "folderId": "…",                 // → grouping in the UI (and collectionId later for orgs)
  "login": {
    "username": "admin",
    "password": "…",               // SSH password auth, or RDP/SFTP password
    "totp": "…",                   // optional per-host 2FA
    "uris": [
      { "uri": "ssh://admin@10.0.0.5:22" },   // scheme selects the launcher
      { "uri": "rdp://10.0.0.5:3389" }        // one host may expose several protocols
    ]
  },
  "fields": [                      // custom fields carry protocol-specific extras
    { "name": "jump",   "value": "bastion.corp" },  // ssh -J
    { "name": "domain", "value": "CORP" },          // RDP domain
    { "name": "sshkey", "value": "<itemId of SSH Key item>" }
  ]
}
```

**Convention rules**
- URI **scheme** → which launcher runs (`ssh` / `sftp` / `rdp`).
- host / port / user come from the URI.
- protocol-specific extras live in **known-named custom fields**.
- SSH private keys live in native **SSH Key items**, referenced by id (served via ssh-agent,
  never written to disk).

---

## Security model

The one net-new attack surface is the `bw serve` port, which exposes the **decrypted,
unlocked vault over localhost with no auth** (by design — it assumes a trusted machine).
Treatment:

- **Bind `127.0.0.1` on an ephemeral random port.** Never `0.0.0.0`. Move to a unix socket
  if/when `bw` supports it.
- **Tauri owns unlock.** Master password typed into the Tauri window, sent once to
  `/unlock`; session token lives only in Rust process memory — never on disk, kept out of
  the webview.
- **Aggressive auto-lock** — idle timeout, lock-on-sleep, optional lock-on-focus-loss.
  Lock = `/lock` + drop/zeroize token.
- **Secrets never touch argv or logs.** Passwords go via PTY/stdin (`ssh`, `xfreerdp
  /from-stdin`), keys via ssh-agent. Hard rule enforced in the launcher.
- **Host-key verification** reuses `~/.ssh/known_hosts`; first-contact keys surface a
  **TOFU prompt in our UI** — never silently disabled.
- Plaintext creds stay in narrow Rust scopes, zeroized after the launcher consumes them.

Honest caveat: a tool that decrypts your whole host inventory is a juicy local target.
We are not more exposed than Bitwarden desktop itself; the `bw serve` port gets the
loopback + ephemeral + aggressive-lock treatment to keep it that way.

---

## Roadmap

- **Phase 0 — spike.** `bw serve` adapter + parse one `ssh://` Login item → **embedded
  PTY SSH session** in the Tauri window. Proves vault → launch → embedded render
  end-to-end. **Also spike IronRDP early** to de-risk the hard milestone before we depend
  on it.
- **Phase 1 — SSH MVP.** Host list, unlock/auto-lock, polished embedded SSH (tabs,
  `known_hosts` + in-UI TOFU, ssh-agent for keys).
- **Phase 2 — embedded RDP (make-or-break).** IronRDP canvas: video, input, clipboard.
- **Phase 3 — SFTP file-browser, jump-host chaining, per-host TOTP.**
- **Later — org collections = shared fleets** (RBAC/sharing for free via Vaultwarden orgs;
  host model already collection-aware).

---

## Open risks / things to watch

- **IronRDP maturity** — some enterprise RDP features (specific GFX codecs, RemoteApp,
  smartcard/device redirection) may lag mature clients. Fine for "open a desktop and use
  it." Fallback: Apache Guacamole (`guacd` + HTML5 canvas), at the cost of a C daemon
  that's painful to ship on Windows/macOS.
- **`bw serve` lifecycle** — robust supervision (crash/restart, unlock-state recovery),
  and a `bw` version pin so the local API doesn't shift under us.
- **Cross-platform after Linux** — Windows/macOS bring `ssh` path quirks, RDP secret
  handling differences, and packaging of any native deps.
- **Offline behavior** — `bw sync` cadence; usable host list when Vaultwarden is
  unreachable but the vault is cached/unlocked.
```
