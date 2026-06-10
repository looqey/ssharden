//! Programmatic SFTP client (pure Rust: russh + russh-sftp).
//!
//! Powers the graphical dual-pane file browser: connect to a host, list remote
//! directories, and transfer files. Auth uses the host's stored password.
//!
//! Security note (v1): the server host key is accepted on first contact (TOFU).
//! The interactive `ssh`/`sftp` CLI path still does full `~/.ssh/known_hosts`
//! verification; wiring known_hosts checking into this client is a follow-up.

use std::path::Path;
use std::sync::Arc;

use russh::client::{self, Handle};
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::OpenFlags;
use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::error::{CoreError, Result};

/// A single directory entry, used for both local and remote listings.
#[derive(Debug, Clone, Serialize)]
pub struct FsEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
}

fn sftp_err<E: std::fmt::Display>(e: E) -> CoreError {
    CoreError::Sftp(e.to_string())
}

/// russh client callback handler. Verifies the server key against the user's
/// `~/.ssh/known_hosts`: a known, matching key passes; a *changed* key is refused
/// (possible MITM / key rotation); a genuinely new host is recorded on first use
/// (trust-on-first-use), matching OpenSSH's `accept-new` behavior. This shares the
/// same `known_hosts` file the interactive ssh/sftp CLI paths use.
struct ClientHandler {
    host: String,
    port: u16,
}

#[async_trait::async_trait]
impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &russh::keys::key::PublicKey,
    ) -> std::result::Result<bool, Self::Error> {
        match russh::keys::check_known_hosts(&self.host, self.port, server_public_key) {
            // Host is known and the key matches.
            Ok(true) => Ok(true),
            // Unknown host: record it (trust-on-first-use) and accept.
            Ok(false) => {
                let _ = russh::keys::learn_known_hosts(&self.host, self.port, server_public_key);
                Ok(true)
            }
            // Host is known but presented a DIFFERENT key — refuse the connection.
            Err(russh::keys::Error::KeyChanged { .. }) => Ok(false),
            // No known_hosts file yet (or other read issue): treat as first contact.
            Err(_) => {
                let _ = russh::keys::learn_known_hosts(&self.host, self.port, server_public_key);
                Ok(true)
            }
        }
    }
}

/// A live SFTP connection to one host.
pub struct SftpConn {
    /// Kept alive for the duration of the connection (drop closes the transport).
    _session: Handle<ClientHandler>,
    sftp: SftpSession,
}

impl SftpConn {
    /// Connect to `host:port` as `user` with password auth and open the SFTP subsystem.
    pub async fn connect(host: &str, port: u16, user: &str, password: &str) -> Result<SftpConn> {
        let config = Arc::new(client::Config::default());
        let handler = ClientHandler {
            host: host.to_string(),
            port,
        };
        let mut session = client::connect(config, (host, port), handler)
            .await
            .map_err(|e| CoreError::Sftp(format!("connect failed: {e}")))?;

        let authed = session
            .authenticate_password(user, password)
            .await
            .map_err(|e| CoreError::Sftp(format!("auth error: {e}")))?;
        if !authed {
            return Err(CoreError::Sftp("authentication failed".to_string()));
        }

        let channel = session
            .channel_open_session()
            .await
            .map_err(|e| CoreError::Sftp(format!("channel open failed: {e}")))?;
        channel
            .request_subsystem(true, "sftp")
            .await
            .map_err(|e| CoreError::Sftp(format!("sftp subsystem failed: {e}")))?;
        let sftp = SftpSession::new(channel.into_stream())
            .await
            .map_err(sftp_err)?;

        Ok(SftpConn {
            _session: session,
            sftp,
        })
    }

    /// Resolve a path to its absolute form (e.g. `.` → the remote home directory).
    pub async fn canonicalize(&self, path: &str) -> Result<String> {
        self.sftp.canonicalize(path).await.map_err(sftp_err)
    }

    /// List a remote directory (excludes `.`/`..`), directories first.
    pub async fn list(&self, path: &str) -> Result<Vec<FsEntry>> {
        let mut out = Vec::new();
        for entry in self.sftp.read_dir(path).await.map_err(sftp_err)? {
            let name = entry.file_name();
            if name == "." || name == ".." {
                continue;
            }
            let md = entry.metadata();
            out.push(FsEntry {
                name,
                is_dir: md.is_dir(),
                size: md.size.unwrap_or(0),
            });
        }
        out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        Ok(out)
    }

    /// Download a remote file to a local path, streamed in fixed-size chunks so
    /// memory use stays flat regardless of file size. `on_progress(done, total)`
    /// is called as bytes arrive (`total` is 0 if the size is unknown).
    pub async fn download<F: FnMut(u64, u64)>(
        &self,
        remote: &str,
        local: &Path,
        mut on_progress: F,
    ) -> Result<()> {
        let total = self
            .sftp
            .metadata(remote)
            .await
            .map_err(sftp_err)?
            .size
            .unwrap_or(0);
        let mut rf = self
            .sftp
            .open_with_flags(remote, OpenFlags::READ)
            .await
            .map_err(sftp_err)?;
        let mut lf = tokio::fs::File::create(local).await?;
        let mut buf = vec![0u8; 128 * 1024];
        let mut transferred = 0u64;
        on_progress(0, total);
        loop {
            let n = rf.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            lf.write_all(&buf[..n]).await?;
            transferred += n as u64;
            on_progress(transferred, total);
        }
        lf.flush().await?;
        on_progress(transferred, total.max(transferred));
        Ok(())
    }

    /// Create a remote directory.
    pub async fn create_dir(&self, path: &str) -> Result<()> {
        self.sftp.create_dir(path).await.map_err(sftp_err)
    }

    /// Rename/move a remote path.
    pub async fn rename(&self, from: &str, to: &str) -> Result<()> {
        self.sftp.rename(from, to).await.map_err(sftp_err)
    }

    /// Remove a remote file or (empty) directory.
    pub async fn remove(&self, path: &str, is_dir: bool) -> Result<()> {
        if is_dir {
            self.sftp.remove_dir(path).await.map_err(sftp_err)
        } else {
            self.sftp.remove_file(path).await.map_err(sftp_err)
        }
    }

    /// Upload a local file to a remote path, streamed in fixed-size chunks so
    /// memory use stays flat regardless of file size. `on_progress(done, total)`
    /// is called as bytes are sent.
    pub async fn upload<F: FnMut(u64, u64)>(
        &self,
        local: &Path,
        remote: &str,
        mut on_progress: F,
    ) -> Result<()> {
        let total = tokio::fs::metadata(local).await?.len();
        let mut lf = tokio::fs::File::open(local).await?;
        let mut wf = self
            .sftp
            .open_with_flags(
                remote,
                OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE,
            )
            .await
            .map_err(sftp_err)?;
        let mut buf = vec![0u8; 128 * 1024];
        // russh-sftp queues writes without awaiting server ACKs, so write_all returns
        // almost immediately. Flush periodically so progress reflects *confirmed* bytes
        // (an honest bar) and write errors surface mid-stream rather than only at the end.
        const FLUSH_EVERY: u64 = 1024 * 1024;
        let mut written = 0u64;
        let mut acked = 0u64;
        on_progress(0, total);
        loop {
            let n = lf.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            wf.write_all(&buf[..n]).await?;
            written += n as u64;
            if written - acked >= FLUSH_EVERY {
                wf.flush().await?;
                acked = written;
                on_progress(acked, total);
            }
        }
        wf.flush().await?;
        wf.shutdown().await?;
        on_progress(written, total);
        Ok(())
    }
}
