//! `bw serve` vault adapter.
//!
//! Spawns and owns the `bw serve` child process bound to `127.0.0.1:<ephemeral>`,
//! and performs unlock / lock / sync / list over loopback HTTP. The session token
//! lives only here, in Rust process memory — never returned to the webview, never
//! written to disk.
//!
//! The interface is engine-agnostic so the native Bitwarden SDK can replace
//! `bw serve` later without touching callers.

pub mod model;

pub use model::{
    host_from_cipher, login_cipher_json, parse_host_uri, AccountStatus, Host, HostInput, HostUri,
};

/// Read the `bw` CLI account status (`bw status`) without needing `bw serve`.
///
/// Used to show which account a user is unlocking before the vault is started.
pub async fn account_status(bw_bin: &str) -> Result<AccountStatus> {
    let out = bw_command(bw_bin)
        .arg("status")
        .output()
        .await
        .map_err(|e| CoreError::Spawn(format!("`bw status` failed: {e}")))?;
    let v: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    Ok(AccountStatus {
        server_url: v.get("serverUrl").and_then(|x| x.as_str()).map(str::to_string),
        user_email: v.get("userEmail").and_then(|x| x.as_str()).map(str::to_string),
        status: v
            .get("status")
            .and_then(|x| x.as_str())
            .unwrap_or("unknown")
            .to_string(),
    })
}

use std::net::TcpListener;
use std::time::Duration;

use crate::error::{CoreError, Result};

/// Owns a `bw serve` child and talks to it over loopback HTTP.
pub struct VaultClient {
    /// Base URL of the local `bw serve` instance, e.g. `http://127.0.0.1:<port>`.
    base_url: String,
    /// HTTP client used for all loopback requests.
    http: reqwest::Client,
    /// The `bw serve` child process, owned for supervision/shutdown.
    child: Option<tokio::process::Child>,
    /// Session token from `/unlock`. Stays in Rust memory only; never logged.
    session: Option<String>,
    /// Resolved absolute path to the `bw` binary (used for `bw config server`).
    bw_bin: String,
}

/// Reserve an ephemeral TCP port on loopback, then release it for `bw serve` to bind.
fn pick_loopback_port() -> Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

/// Treat a Vault Management API response as success, or surface its message as an error.
fn check_success(body: &serde_json::Value) -> Result<()> {
    if body.get("success").and_then(|s| s.as_bool()) == Some(true) {
        Ok(())
    } else {
        let msg = body
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("bw serve reported failure");
        Err(CoreError::Bw(msg.to_string()))
    }
}

/// Candidate executable names for the Bitwarden CLI per platform.
#[cfg(windows)]
const BW_NAMES: &[&str] = &["bw.exe", "bw.cmd", "bw"];
#[cfg(not(windows))]
const BW_NAMES: &[&str] = &["bw"];

/// Resolve the Bitwarden CLI to an absolute path.
///
/// Desktop launchers (a `.desktop` file from the `.deb`, the dock, etc.) start the app
/// with a minimal `PATH` that usually omits nvm / npm-global / bun bin dirs, so a bare
/// `Command::new("bw")` fails with "No such file or directory". Check an explicit
/// override, then `PATH`, then the common install locations.
pub fn resolve_bw() -> String {
    use std::path::PathBuf;

    if let Ok(p) = std::env::var("SSHARDEN_BW") {
        if !p.is_empty() && PathBuf::from(&p).is_file() {
            return p;
        }
    }

    let try_dir = |dir: PathBuf| -> Option<String> {
        for name in BW_NAMES {
            let c = dir.join(name);
            if c.is_file() {
                return Some(c.to_string_lossy().into_owned());
            }
        }
        None
    };

    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            if let Some(found) = try_dir(dir) {
                return found;
            }
        }
    }

    let mut dirs: Vec<PathBuf> =
        ["/usr/local/bin", "/usr/bin", "/bin", "/opt/homebrew/bin", "/snap/bin"]
            .iter()
            .map(PathBuf::from)
            .collect();
    if let Ok(home) = std::env::var("HOME") {
        let home = PathBuf::from(home);
        dirs.push(home.join(".local/bin"));
        dirs.push(home.join(".bun/bin"));
        // nvm installs node (and npm-global bins) under ~/.nvm/versions/node/<ver>/bin
        if let Ok(entries) = std::fs::read_dir(home.join(".nvm/versions/node")) {
            for e in entries.flatten() {
                dirs.push(e.path().join("bin"));
            }
        }
    }
    for d in dirs {
        if let Some(found) = try_dir(d) {
            return found;
        }
    }

    BW_NAMES[0].to_string() // fallback: let the OS surface the error if truly missing
}

/// Build a `Command` for the `bw` binary with its own directory prepended to `PATH`,
/// so the Node interpreter `bw`'s shebang needs (`#!/usr/bin/env node`) resolves even
/// under a minimal desktop-launch environment.
fn bw_command(bw_bin: &str) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(bw_bin);
    if let Some(dir) = std::path::Path::new(bw_bin).parent() {
        if !dir.as_os_str().is_empty() {
            let mut paths = vec![dir.to_path_buf()];
            if let Some(existing) = std::env::var_os("PATH") {
                paths.extend(std::env::split_paths(&existing));
            }
            if let Ok(joined) = std::env::join_paths(paths) {
                cmd.env("PATH", joined);
            }
        }
    }
    cmd
}

impl VaultClient {
    /// Spawn `bw serve` on `127.0.0.1:<ephemeral>` and return a ready client.
    ///
    /// `bw_bin` is the path to the `bw` executable. Never binds `0.0.0.0`.
    pub async fn start(bw_bin: &str) -> Result<VaultClient> {
        use tokio::io::AsyncReadExt;

        let port = pick_loopback_port()?;
        let base_url = format!("http://127.0.0.1:{port}");

        let mut cmd = bw_command(bw_bin);
        cmd.args(["serve", "--hostname", "127.0.0.1", "--port", &port.to_string()])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);

        // Tie `bw serve`'s lifetime to ours: if this process dies (even by SIGKILL),
        // the kernel sends `bw serve` SIGTERM, so it can never be orphaned. Without
        // this, hard-killing the app leaves stray `bw serve` instances that fight
        // over the shared `bw` data file.
        #[cfg(target_os = "linux")]
        unsafe {
            cmd.pre_exec(|| {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| CoreError::Spawn(format!("failed to spawn `{bw_bin} serve`: {e}")))?;

        let mut stderr = child.stderr.take();

        let http = reqwest::Client::builder()
            .no_proxy()
            .build()
            .map_err(CoreError::Http)?;

        // Poll /status until bw serve accepts connections (~10s budget), but fail
        // fast with a helpful message if the process exits first. The most common
        // cause is that the Bitwarden CLI is not logged in (`bw serve` then prints
        // "You are not logged in." and exits immediately).
        let status_url = format!("{base_url}/status");
        let mut ready = false;
        for _ in 0..50 {
            if let Some(status) = child.try_wait()? {
                let mut msg = String::new();
                if let Some(mut err) = stderr.take() {
                    let _ = err.read_to_string(&mut msg).await;
                }
                let msg = msg.trim();
                let hint = if msg.to_lowercase().contains("not logged in") {
                    " — log in first: `bw config server <url>` (self-hosted) then `bw login`"
                } else {
                    ""
                };
                return Err(CoreError::Bw(format!("bw serve exited ({status}): {msg}{hint}")));
            }
            if http.get(&status_url).send().await.is_ok() {
                ready = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        if !ready {
            return Err(CoreError::Bw(
                "bw serve did not become reachable on loopback".to_string(),
            ));
        }

        Ok(VaultClient {
            base_url,
            http,
            child: Some(child),
            session: None,
            bw_bin: bw_bin.to_string(),
        })
    }

    /// Configure the Vaultwarden server URL via the `bw` CLI.
    ///
    /// Best-effort: must run before `bw login`, so it is a no-op for an empty URL and
    /// the caller may ignore its error when the CLI is already logged in.
    pub async fn set_server(&self, url: &str) -> Result<()> {
        if url.trim().is_empty() {
            return Ok(());
        }
        let out = bw_command(&self.bw_bin)
            .args(["config", "server", url])
            .output()
            .await
            .map_err(|e| CoreError::Spawn(format!("`bw config server` failed: {e}")))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(CoreError::Bw(
                String::from_utf8_lossy(&out.stderr).trim().to_string(),
            ))
        }
    }

    /// Unlock the vault with the master password; store the session token in memory.
    pub async fn unlock(&mut self, password: &str) -> Result<()> {
        let body = self
            .http
            .post(format!("{}/unlock", self.base_url))
            .json(&serde_json::json!({ "password": password }))
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;
        check_success(&body)?;
        // The session key lives at data.raw; keep it in memory, never log it.
        self.session = body
            .get("data")
            .and_then(|d| d.get("raw"))
            .and_then(|r| r.as_str())
            .map(str::to_string);
        Ok(())
    }

    /// Lock the vault and drop the in-memory session token.
    pub async fn lock(&self) -> Result<()> {
        let body = self
            .http
            .post(format!("{}/lock", self.base_url))
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;
        check_success(&body)
    }

    /// Sync the vault from the server.
    pub async fn sync(&self) -> Result<()> {
        let body = self
            .http
            .post(format!("{}/sync", self.base_url))
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;
        check_success(&body)
    }

    /// Sync, then list all hosts parsed from Login items in the vault.
    pub async fn list_hosts(&self) -> Result<Vec<Host>> {
        self.sync().await?;
        let body = self
            .http
            .get(format!("{}/list/object/items", self.base_url))
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;
        check_success(&body)?;

        let items = body
            .get("data")
            .and_then(|d| d.get("data"))
            .and_then(|a| a.as_array())
            .ok_or_else(|| CoreError::Bw("unexpected item list shape".to_string()))?;

        let mut hosts: Vec<Host> = items.iter().filter_map(host_from_cipher).collect();

        // Attach friendly folder names (best effort — grouping still works without them).
        let folders = self.list_folders().await.unwrap_or_default();
        for h in &mut hosts {
            if let Some(fid) = &h.folder_id {
                h.folder_name = folders.get(fid).cloned();
            }
        }
        Ok(hosts)
    }

    /// Fetch a `folder id -> name` map from the vault.
    pub async fn list_folders(&self) -> Result<std::collections::BTreeMap<String, String>> {
        let body = self
            .http
            .get(format!("{}/list/object/folders", self.base_url))
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;
        check_success(&body)?;
        let mut map = std::collections::BTreeMap::new();
        if let Some(arr) = body
            .get("data")
            .and_then(|d| d.get("data"))
            .and_then(|a| a.as_array())
        {
            for f in arr {
                if let (Some(id), Some(name)) = (
                    f.get("id").and_then(|x| x.as_str()),
                    f.get("name").and_then(|x| x.as_str()),
                ) {
                    map.insert(id.to_string(), name.to_string());
                }
            }
        }
        Ok(map)
    }

    /// Fetch a single item's full (decrypted) cipher JSON by id.
    pub async fn get_item(&self, id: &str) -> Result<serde_json::Value> {
        let body = self
            .http
            .get(format!("{}/object/item/{id}", self.base_url))
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;
        check_success(&body)?;
        body.get("data").cloned().ok_or(CoreError::NotFound)
    }

    /// Create a vault folder.
    pub async fn folder_create(&self, name: &str) -> Result<()> {
        let body = self
            .http
            .post(format!("{}/object/folder", self.base_url))
            .json(&serde_json::json!({ "name": name }))
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;
        check_success(&body)
    }

    /// Rename a vault folder.
    pub async fn folder_rename(&self, id: &str, name: &str) -> Result<()> {
        let body = self
            .http
            .put(format!("{}/object/folder/{id}", self.base_url))
            .json(&serde_json::json!({ "name": name }))
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;
        check_success(&body)
    }

    /// Delete a vault folder (its items become unfiled).
    pub async fn folder_delete(&self, id: &str) -> Result<()> {
        let resp = self
            .http
            .delete(format!("{}/object/folder/{id}", self.base_url))
            .send()
            .await?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if status.is_success() {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                if v.get("success").and_then(|s| s.as_bool()) == Some(false) {
                    return check_success(&v);
                }
            }
            Ok(())
        } else {
            Err(CoreError::Bw(format!("delete failed ({status}): {}", text.trim())))
        }
    }

    /// Create a new host (Login item) from user input.
    pub async fn create_host(&self, input: &HostInput) -> Result<()> {
        let cipher = login_cipher_json(input, None);
        let body = self
            .http
            .post(format!("{}/object/item", self.base_url))
            .json(&cipher)
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;
        check_success(&body)
    }

    /// Update an existing host, preserving fields the input leaves blank.
    pub async fn update_host(&self, id: &str, input: &HostInput) -> Result<()> {
        let existing = self.get_item(id).await?;
        let cipher = login_cipher_json(input, Some(&existing));
        let body = self
            .http
            .put(format!("{}/object/item/{id}", self.base_url))
            .json(&cipher)
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;
        check_success(&body)
    }

    /// Delete a host by id.
    pub async fn delete_host(&self, id: &str) -> Result<()> {
        let resp = self
            .http
            .delete(format!("{}/object/item/{id}", self.base_url))
            .send()
            .await?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if status.is_success() {
            // Some bw serve builds still return a wrapper; surface an explicit failure.
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                if v.get("success").and_then(|s| s.as_bool()) == Some(false) {
                    return check_success(&v);
                }
            }
            Ok(())
        } else {
            Err(CoreError::Bw(format!("delete failed ({status}): {}", text.trim())))
        }
    }

    /// Fetch the private key of a Bitwarden SSH Key item (type 5) by id, if present.
    pub async fn ssh_private_key(&self, item_id: &str) -> Result<Option<String>> {
        let item = self.get_item(item_id).await?;
        Ok(item
            .get("sshKey")
            .and_then(|k| k.get("privateKey"))
            .and_then(|p| p.as_str())
            .filter(|p| !p.is_empty())
            .map(str::to_string))
    }

    /// Fetch just the (decrypted) password for a host, for copy/reveal.
    pub async fn host_password(&self, id: &str) -> Result<Option<String>> {
        let item = self.get_item(id).await?;
        Ok(item
            .get("login")
            .and_then(|l| l.get("password"))
            .and_then(|p| p.as_str())
            .filter(|p| !p.is_empty())
            .map(str::to_string))
    }

    /// Terminate the owned `bw serve` child, if any, and clear the session.
    pub fn shutdown(&mut self) {
        self.session = None;
        if let Some(mut child) = self.child.take() {
            let _ = child.start_kill();
        }
    }
}

impl Drop for VaultClient {
    fn drop(&mut self) {
        self.shutdown();
    }
}
