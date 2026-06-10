// Prevents an extra console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! ssharden Tauri shell.
//!
//! Thin wrapper around `ssharden-core`: holds app state (the `VaultClient` and a
//! registry of live SSH sessions) and exposes `#[tauri::command]`s that call core
//! and forward PTY bytes to the webview as `ssh://{id}` events.
//!
//! Security: the vault session token never leaves Rust (it lives inside
//! `VaultClient`); no secret is ever returned to JS or placed on argv.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

use ssharden_core::{FsEntry, Host, HostInput, SftpConn, SshParams, SshSession, VaultClient};
use tauri::{AppHandle, Emitter, State};
use tokio::sync::Mutex as AsyncMutex;

/// A live SSH session plus its PTY writer (kept so typed input never re-opens the pty).
struct LiveSession {
    session: SshSession,
    /// Single PTY writer (portable-pty allows only one), shared between typed input
    /// and the auto-auth injector.
    writer: Arc<StdMutex<Box<dyn Write + Send>>>,
}

/// Managed application state.
#[derive(Default)]
struct AppState {
    /// The vault adapter, present once `vault_start` has run. Async mutex: held across
    /// `await` points while talking to `bw serve`.
    vault: AsyncMutex<Option<VaultClient>>,
    /// Live SSH sessions keyed by session id. Sync mutex: only ever locked for
    /// non-`await` work (write/resize/insert).
    sessions: StdMutex<HashMap<String, LiveSession>>,
    /// Live SFTP connections keyed by conn id (for the file browser).
    sftp_conns: AsyncMutex<HashMap<String, Arc<SftpConn>>>,
    /// Monotonic counter for generating session/conn ids.
    next_id: AtomicU64,
}

/// Map any core error to a user-facing string (never leaks secrets).
fn e<T: std::fmt::Display>(err: T) -> String {
    err.to_string()
}

/// Spawn `bw serve` on a loopback ephemeral port and store the client in state.
#[tauri::command]
async fn vault_start(state: State<'_, AppState>) -> Result<(), String> {
    let mut guard = state.vault.lock().await;
    if guard.is_some() {
        return Ok(()); // already started
    }
    let client = VaultClient::start("bw").await.map_err(e)?;
    *guard = Some(client);
    Ok(())
}

/// Configure the server URL (if provided) and unlock the vault.
///
/// The master password flows webview → here once; the session token stays in Rust.
#[tauri::command]
async fn vault_unlock(
    state: State<'_, AppState>,
    server_url: String,
    master_password: String,
) -> Result<(), String> {
    let mut guard = state.vault.lock().await;
    let client = guard.as_mut().ok_or("vault not started")?;
    // Best-effort: only effective before `bw login`; ignore when already logged in.
    let _ = client.set_server(&server_url).await;
    client.unlock(&master_password).await.map_err(e)
}

/// Lock the vault and zeroize the session token.
#[tauri::command]
async fn vault_lock(state: State<'_, AppState>) -> Result<(), String> {
    let guard = state.vault.lock().await;
    match guard.as_ref() {
        Some(client) => client.lock().await.map_err(e),
        None => Ok(()),
    }
}

/// Which `bw` account is logged in (for the unlock screen). Does not need `bw serve`.
#[tauri::command]
async fn account_status() -> Result<ssharden_core::AccountStatus, String> {
    ssharden_core::account_status("bw").await.map_err(e)
}

/// Sync, then list hosts parsed from Login items.
#[tauri::command]
async fn vault_list_hosts(state: State<'_, AppState>) -> Result<Vec<Host>, String> {
    let guard = state.vault.lock().await;
    let client = guard.as_ref().ok_or("vault not started")?;
    client.list_hosts().await.map_err(e)
}

/// Create a new host (Login item) in the vault.
#[tauri::command]
async fn host_create(state: State<'_, AppState>, input: HostInput) -> Result<(), String> {
    let guard = state.vault.lock().await;
    let client = guard.as_ref().ok_or("vault not started")?;
    client.create_host(&input).await.map_err(e)
}

/// Update an existing host; blank password/folder are preserved.
#[tauri::command]
async fn host_update(
    state: State<'_, AppState>,
    id: String,
    input: HostInput,
) -> Result<(), String> {
    let guard = state.vault.lock().await;
    let client = guard.as_ref().ok_or("vault not started")?;
    client.update_host(&id, &input).await.map_err(e)
}

/// Delete a host by id.
#[tauri::command]
async fn host_delete(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let guard = state.vault.lock().await;
    let client = guard.as_ref().ok_or("vault not started")?;
    client.delete_host(&id).await.map_err(e)
}

/// Fetch a host's password for copy/reveal (user-initiated secret egress).
#[tauri::command]
async fn host_password(state: State<'_, AppState>, id: String) -> Result<Option<String>, String> {
    let guard = state.vault.lock().await;
    let client = guard.as_ref().ok_or("vault not started")?;
    client.host_password(&id).await.map_err(e)
}

/// Resolve a host and open `ssh` or `sftp` to it in a PTY, streaming output as
/// `ssh://{id}` events. Shared by the ssh_connect/sftp_connect commands.
async fn open_session(
    app: AppHandle,
    state: &AppState,
    host_id: String,
    sftp: bool,
) -> Result<String, String> {
    // Resolve the host (and its stored password) under the vault lock, then release it.
    let (params, password) = {
        let guard = state.vault.lock().await;
        let client = guard.as_ref().ok_or("vault not started")?;
        let hosts = client.list_hosts().await.map_err(e)?;
        let host = hosts
            .into_iter()
            .find(|h| h.id == host_id)
            .ok_or("host not found")?;
        let uri = host
            .uris
            .iter()
            .find(|u| u.scheme == "ssh")
            .ok_or("host has no ssh:// uri")?
            .clone();
        // Fetch the stored password for auto-auth (best effort; None = prompt manually).
        let password = client.host_password(&host_id).await.ok().flatten();
        (
            SshParams {
                host: uri.host,
                port: uri.port.unwrap_or(22),
                user: uri.user.or(host.username),
                jump: host.fields.get("jump").cloned(),
            },
            password,
        )
    };

    // Spawn ssh or sftp in a PTY (sync; no locks held).
    let mut session = if sftp {
        SshSession::spawn_sftp(&params)
    } else {
        SshSession::spawn(&params)
    }
    .map_err(e)?;
    let reader = session.take_reader().ok_or("could not open pty reader")?;
    // One writer, shared (portable-pty only allows a single take_writer).
    let writer = Arc::new(StdMutex::new(session.writer().map_err(e)?));
    let auth_writer = Arc::clone(&writer);

    let id = format!("s{}", state.next_id.fetch_add(1, Ordering::Relaxed));
    let event = format!("ssh://{id}");

    // Forward PTY bytes to the webview until EOF; when ssh asks for a password and
    // one is stored, feed it over the PTY (never on argv). Injected at most once so a
    // later in-session prompt (e.g. sudo) is never auto-filled with the SSH password.
    let app_handle = app.clone();
    let mut reader: Box<dyn Read + Send> = reader;
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        let mut tail: Vec<u8> = Vec::new();
        let mut injected = password.is_none();
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if app_handle.emit(&event, buf[..n].to_vec()).is_err() {
                        break;
                    }
                    if !injected {
                        tail.extend_from_slice(&buf[..n]);
                        if tail.len() > 256 {
                            let cut = tail.len() - 256;
                            tail.drain(..cut);
                        }
                        let recent = String::from_utf8_lossy(&tail).to_lowercase();
                        if recent.trim_end().ends_with("password:") {
                            if let Some(pw) = &password {
                                if let Ok(mut w) = auth_writer.lock() {
                                    let _ = w.write_all(pw.as_bytes());
                                    let _ = w.write_all(b"\n");
                                    let _ = w.flush();
                                }
                            }
                            injected = true;
                        }
                    }
                }
            }
        }
    });

    state
        .sessions
        .lock()
        .map_err(|_| "session registry poisoned")?
        .insert(id.clone(), LiveSession { session, writer });

    Ok(id)
}

/// Resolve a host, spawn `ssh` in a PTY, stream output as `ssh://{id}` events.
#[tauri::command]
async fn ssh_connect(
    app: AppHandle,
    state: State<'_, AppState>,
    host_id: String,
) -> Result<String, String> {
    open_session(app, state.inner(), host_id, false).await
}

/// Resolve a host, spawn `sftp` to it in a PTY, stream output as `ssh://{id}` events.
#[tauri::command]
async fn sftp_connect(
    app: AppHandle,
    state: State<'_, AppState>,
    host_id: String,
) -> Result<String, String> {
    open_session(app, state.inner(), host_id, true).await
}

/// Write bytes (including a typed password) to a session's PTY.
#[tauri::command]
async fn ssh_write(
    state: State<'_, AppState>,
    session_id: String,
    data: Vec<u8>,
) -> Result<(), String> {
    let guard = state.sessions.lock().map_err(|_| "session registry poisoned")?;
    let live = guard.get(&session_id).ok_or("session not found")?;
    let mut w = live.writer.lock().map_err(|_| "pty writer poisoned")?;
    w.write_all(&data).map_err(e)?;
    w.flush().map_err(e)
}

/// Resize a session's PTY.
#[tauri::command]
async fn ssh_resize(
    state: State<'_, AppState>,
    session_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let guard = state.sessions.lock().map_err(|_| "session registry poisoned")?;
    let live = guard.get(&session_id).ok_or("session not found")?;
    live.session.resize(cols, rows).map_err(e)
}

// ---------------- Graphical SFTP file browser ----------------

/// Result of opening an SFTP connection: the conn id and the remote home path.
#[derive(serde::Serialize)]
struct SftpOpened {
    conn_id: String,
    home: String,
}

/// Open an SFTP connection to a host and return its id + remote home directory.
#[tauri::command]
async fn sftp_open(state: State<'_, AppState>, host_id: String) -> Result<SftpOpened, String> {
    // Resolve host + password under the vault lock.
    let (host, port, user, password) = {
        let guard = state.vault.lock().await;
        let client = guard.as_ref().ok_or("vault not started")?;
        let hosts = client.list_hosts().await.map_err(e)?;
        let h = hosts.into_iter().find(|h| h.id == host_id).ok_or("host not found")?;
        let uri = h
            .uris
            .iter()
            .find(|u| u.scheme == "ssh")
            .ok_or("host has no ssh:// uri")?
            .clone();
        let password = client.host_password(&host_id).await.ok().flatten();
        (
            uri.host,
            uri.port.unwrap_or(22),
            uri.user.or(h.username).unwrap_or_default(),
            password.unwrap_or_default(),
        )
    };

    let conn = SftpConn::connect(&host, port, &user, &password)
        .await
        .map_err(e)?;
    let home = conn.canonicalize(".").await.unwrap_or_else(|_| ".".to_string());

    let conn_id = format!("f{}", state.next_id.fetch_add(1, Ordering::Relaxed));
    state
        .sftp_conns
        .lock()
        .await
        .insert(conn_id.clone(), Arc::new(conn));
    Ok(SftpOpened { conn_id, home })
}

/// Look up a live SFTP connection by id.
async fn sftp_get_conn(state: &AppState, conn_id: &str) -> Result<Arc<SftpConn>, String> {
    state
        .sftp_conns
        .lock()
        .await
        .get(conn_id)
        .cloned()
        .ok_or_else(|| "sftp connection not found".to_string())
}

/// List a remote directory.
#[tauri::command]
async fn sftp_ls(state: State<'_, AppState>, conn_id: String, path: String) -> Result<Vec<FsEntry>, String> {
    let conn = sftp_get_conn(state.inner(), &conn_id).await?;
    conn.list(&path).await.map_err(e)
}

/// Download a remote file to a local path.
#[tauri::command]
async fn sftp_get(
    state: State<'_, AppState>,
    conn_id: String,
    remote: String,
    local: String,
) -> Result<(), String> {
    let conn = sftp_get_conn(state.inner(), &conn_id).await?;
    conn.download(&remote, std::path::Path::new(&local)).await.map_err(e)
}

/// Upload a local file to a remote path.
#[tauri::command]
async fn sftp_put(
    state: State<'_, AppState>,
    conn_id: String,
    local: String,
    remote: String,
) -> Result<(), String> {
    let conn = sftp_get_conn(state.inner(), &conn_id).await?;
    conn.upload(std::path::Path::new(&local), &remote).await.map_err(e)
}

/// Close an SFTP connection.
#[tauri::command]
async fn sftp_close(state: State<'_, AppState>, conn_id: String) -> Result<(), String> {
    state.sftp_conns.lock().await.remove(&conn_id);
    Ok(())
}

/// The local home directory (starting point for the left pane).
#[tauri::command]
fn local_home() -> String {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| "/".to_string())
}

/// List a local directory (directories first).
#[tauri::command]
fn local_ls(path: String) -> Result<Vec<FsEntry>, String> {
    let mut out = Vec::new();
    let rd = std::fs::read_dir(&path).map_err(|e| e.to_string())?;
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let md = entry.metadata().ok();
        let is_dir = md.as_ref().map(|m| m.is_dir()).unwrap_or(false);
        let size = md.as_ref().map(|m| m.len()).unwrap_or(0);
        out.push(FsEntry { name, is_dir, size });
    }
    out.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(out)
}

fn main() {
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            vault_start,
            vault_unlock,
            vault_lock,
            account_status,
            vault_list_hosts,
            host_create,
            host_update,
            host_delete,
            host_password,
            ssh_connect,
            sftp_connect,
            ssh_write,
            ssh_resize,
            sftp_open,
            sftp_ls,
            sftp_get,
            sftp_put,
            sftp_close,
            local_home,
            local_ls,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
