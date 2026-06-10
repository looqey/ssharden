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

pub use model::{host_from_cipher, parse_host_uri, Host, HostUri};

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

impl VaultClient {
    /// Spawn `bw serve` on `127.0.0.1:<ephemeral>` and return a ready client.
    ///
    /// `bw_bin` is the path to the `bw` executable. Never binds `0.0.0.0`.
    pub async fn start(bw_bin: &str) -> Result<VaultClient> {
        let port = pick_loopback_port()?;
        let base_url = format!("http://127.0.0.1:{port}");

        let child = tokio::process::Command::new(bw_bin)
            .args(["serve", "--hostname", "127.0.0.1", "--port", &port.to_string()])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| CoreError::Spawn(format!("failed to spawn `{bw_bin} serve`: {e}")))?;

        let http = reqwest::Client::builder()
            .no_proxy()
            .build()
            .map_err(CoreError::Http)?;

        // Poll /status until bw serve is accepting connections (~10s budget).
        let status_url = format!("{base_url}/status");
        let mut ready = false;
        for _ in 0..50 {
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
        let out = tokio::process::Command::new("bw")
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

        Ok(items.iter().filter_map(host_from_cipher).collect())
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
