# Embedded RDP via IronRDP — design note

Status: **research done; spike step 0 PASSED** (see results below). Today RDP launches an external
FreeRDP window (`crates/ssharden-core/src/rdp/mod.rs`); this doc captures the
plan for rendering RDP sessions *inside* the app, based on a June 2026 survey
of IronRDP (Devolutions' pure-Rust RDP stack — production-proven in
Devolutions Gateway and Teleport).

## TL;DR

Run the IronRDP session natively in the existing tokio runtime inside
`ssharden-core`, decode into a `DecodedImage` framebuffer, ship **dirty-rect
binary frames to the webview over a Tauri v2 `ipc::Channel`** (raw bytes, no
JSON), and paint with `canvas.putImageData()`. Input flows the other way
through a single `rdp_input` command using `ironrdp-input`. NLA/CredSSP works
out of the box (pure-Rust NTLM via sspi-rs). Credentials stay in Rust next to
the vault — same posture as the SSH paths — and the TLS upgrade hands us the
server cert, so TOFU pinning becomes ours to implement properly.

## Crate set (verify exact versions with `cargo add --dry-run` at spike time)

| Crate | ~Version | Role |
|---|---|---|
| `ironrdp` (meta) | 0.14 | features: `connector`, `session`, `graphics`, `input`, `pdu`, `core`, `dvc`, `displaycontrol` |
| `ironrdp-connector` | ^0.8 | connection sequence state machine (incl. CredSSP step) |
| `ironrdp-session` | ^0.8 | `ActiveStage`, `DecodedImage` |
| `ironrdp-input` | ^0.5 | keyboard/mouse state DB → FastPath input events |
| `ironrdp-tokio` | latest | `Framed` I/O over tokio (we already use tokio) |
| `ironrdp-tls` | 0.1–0.2 | TLS upgrade; feature **`rustls`** (prefer the `ring` provider to avoid cmake) |
| `sspi` | 0.17 (transitive) | CredSSP/NLA: pure-Rust NTLM, optional Kerberos |

All 0.x with real breaking churn between releases — **pin exact versions** and
copy from examples at the pinned tag, not master.

## Reference examples in the IronRDP repo

- `crates/ironrdp/examples/screenshot.rs` — minimal blocking client (~300
  lines): connect → decode → dump framebuffer. The canonical starting point.
- `crates/ironrdp-client` — full tokio+winit GUI client: async loop shape,
  input mapping, resize.
- `crates/ironrdp-web` / `iron-remote-desktop` — WASM browser client (the
  fallback architecture, see below).

## Connection flow

```
connector::Config { credentials, domain, desktop_size, ... }
ClientConnector::new(config, client_addr)
TcpStream → Framed → connect_begin            // X.224 negotiation
  → ShouldUpgrade → ironrdp_tls::upgrade      // returns server public key
  → connect_finalize                          // CredSSP/NLA, MCS, capabilities
  → ConnectionResult
DecodedImage::new(RgbA32, w, h); ActiveStage::new(result)
loop { read_pdu → stage.process(&mut image, ...) →
       ResponseFrame | GraphicsUpdate(dirty_rect) | Terminate }
```

`GraphicsUpdate` yields the exact damaged region — that's the rect we crop
from the RGBA framebuffer and send to the canvas.

## Frame transport decision

**Chosen: native session + binary frames over a Tauri `ipc::Channel`.**
Rationale: credentials and CredSSP stay in `ssharden-core` next to the vault;
no listening socket; mirrors the existing PTY→emit pattern (upgraded from
events to a raw-payload channel, which Tauri documents for streaming and which
skips JSON/base64). JS side is one `onmessage` → `putImageData(rect)`.

Throughput: RDP sends deltas, not full frames — steady-state desktop use is
tens of KB to a few MB/s, fine for raw IPC. Full-screen video inside a session
will degrade; acceptable for a connection manager, mitigable later via frame
coalescing or the EGFX/H.264 pipeline.

Fallbacks, in order:
1. **Localhost WebSocket** (tokio-tungstenite on `127.0.0.1:0` + random
   token) if webkit2gtk IPC throughput disappoints.
2. **`ironrdp-web` WASM in the webview + local RDCleanPath proxy** (what
   electerm and Devolutions Gateway do). Reuses their finished renderer and
   input mapping, but moves credentials into JS — breaks our "secrets stay in
   Rust" posture, so it's last resort.

## Input path

- JS: `pointermove/down/up`, `wheel`, `keydown/keyup` on the canvas
  (`KeyboardEvent.code`, scale coords canvas→desktop), batched into
  `invoke("rdp_input", { sessionId, events })`.
- Rust: `ironrdp_input::Database` keeps modifier/button state;
  `database.apply(ops)` → `FastPathInputEvent`s → write to the framed stream.
- Fiddly part: `KeyboardEvent.code` → RDP scancode table; crib from
  `iron-remote-desktop` or ironrdp-client's winit mapping.

## NLA and certificates

- CredSSP/NLA works against standalone and AD boxes via pure-Rust NTLM
  (`sspi`); Kerberos available but has realm/SPN rough edges — NTLM fallback
  covers it.
- `ironrdp-tls` deliberately skips chain validation (mstsc-style, self-signed
  certs are the RDP norm) but exposes the server cert: implement **TOFU
  pinning in ssharden** (hash leaf cert, store per host, surface a real
  "cert changed" error in the UI) — strictly better than the external
  launcher's silent `/cert:tofu` failure.

## Module sketch

```
crates/ssharden-core/src/rdp/
  mod.rs        external FreeRDP launcher (stays as fallback)
  embedded.rs   RdpSession: tokio task — connect, read loop,
                mpsc<InputBatch> in, FrameSink trait out

src-tauri/src/main.rs
  rdp_connect(host_id, channel) -> session_id   // creds never cross IPC
  rdp_input(session_id, events)
  rdp_resize(session_id, w, h)                  // displaycontrol DVC, later
  rdp_disconnect(session_id)

src/rdp.ts    canvas per session tab; channel.onmessage → putImageData;
              pointer/keyboard → batched rdp_input; scancode map module
```

## Step 0 results (2026-06-11, this machine, software-rendered Xephyr)

The probe lives in the tree behind `SSHARDEN_PROBE=1` (`probe_frames`/`probe_report`
commands + `src/probe.ts`); run `SSHARDEN_PROBE=1 ./ssharden` to repeat it.

| Scenario | Result |
|---|---|
| 64 KB dirty rects (128×128) × 300 | **1863 fps, 122 MB/s**, paint avg 0.04 ms |
| 4 MB full frames (1280×800) × 30 | **43 fps, 175 MB/s**, paint avg 0.83 ms, max 6 ms |

**Gate passed decisively.** Even worst-case full-frame streaming sustains 43 fps over
the raw channel on webkit2gtk *without* GPU acceleration; steady-state dirty-rect
traffic is orders of magnitude inside budget. The `ipc::Channel` → canvas transport
is locked in; no WebSocket fallback needed.

## Spike plan (~3–4 days to a demoable session)

| Step | Work | Est. |
|---|---|---|
| 0 | ~~IPC throughput probe~~ — **done, passed** (results above) | ~~2–3 h~~ |
| 1 | Pin crates; port `screenshot.rs` into a core integration bin: connect to a Windows VM (NLA on), save first frame as PNG | 4–6 h |
| 2 | `RdpSession` tokio task + `rdp_connect` resolving creds from the vault; stream dirty rects to a canvas tab — **live view-only desktop** | 6–8 h |
| 3 | Mouse (move/click/wheel) — *milestone: usable point-and-click* | 3–4 h |
| 4 | Keyboard scancode map, disconnect/teardown, error surfacing | 4–6 h |
| 5 | TOFU cert pinning via custom TLS upgrade; cursor style, reconnect | 4–6 h |

Gates: step 0 fails → WebSocket transport; step 2 too slow on webkit →
evaluate the WASM fallback.

## Risks

1. **webkit2gtk IPC/paint throughput** — unbenchmarked, hence spike step 0.
2. **0.x API churn** — pin versions, follow tagged examples.
3. EGFX/H.264 coverage unverified (negotiate it off for the spike).
4. Dynamic resize needs the `displaycontrol` DVC — defer.
5. Non-US keyboard layouts / AltGr scancode mapping.
6. No clipboard/audio in the spike (`ironrdp-cliprdr` / `ironrdp-rdpsnd`
   exist for later).
