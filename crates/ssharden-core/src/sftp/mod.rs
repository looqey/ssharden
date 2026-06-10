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

/// russh client callback handler. Accepts the server key (TOFU) for now.
struct ClientHandler;

#[async_trait::async_trait]
impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::key::PublicKey,
    ) -> std::result::Result<bool, Self::Error> {
        // TODO(security): verify against ~/.ssh/known_hosts instead of trust-on-first-use.
        Ok(true)
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
        let mut session = client::connect(config, (host, port), ClientHandler)
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
    /// memory use stays flat regardless of file size.
    pub async fn download(&self, remote: &str, local: &Path) -> Result<()> {
        let mut rf = self
            .sftp
            .open_with_flags(remote, OpenFlags::READ)
            .await
            .map_err(sftp_err)?;
        let mut lf = tokio::fs::File::create(local).await?;
        let mut buf = vec![0u8; 128 * 1024];
        loop {
            let n = rf.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            lf.write_all(&buf[..n]).await?;
        }
        lf.flush().await?;
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
    /// memory use stays flat regardless of file size.
    pub async fn upload(&self, local: &Path, remote: &str) -> Result<()> {
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
        loop {
            let n = lf.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            wf.write_all(&buf[..n]).await?;
        }
        wf.flush().await?;
        wf.shutdown().await?;
        Ok(())
    }
}
