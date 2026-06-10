//! SSH PTY launcher.
//!
//! Spawns the system `ssh` binary inside a pseudo-terminal via `portable-pty`,
//! so the session renders embedded (xterm.js) and secrets are fed over the PTY —
//! never on argv. Host-key checking is never disabled; prompts surface in the
//! terminal and reuse `~/.ssh/known_hosts`.

use std::io::{Read, Write};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

use crate::error::{CoreError, Result};

/// Parameters for launching an SSH session.
pub struct SshParams {
    /// Target hostname or IP.
    pub host: String,
    /// Target port.
    pub port: u16,
    /// Optional login user.
    pub user: Option<String>,
    /// Optional jump host (`ssh -J`).
    pub jump: Option<String>,
    /// Optional path to a private key file (`ssh -i`), e.g. one materialized from a
    /// vault SSH Key item. When set, only this identity is offered.
    pub identity_file: Option<String>,
}

/// A live SSH session: an owned PTY master plus the `ssh` child process.
pub struct SshSession {
    /// PTY master side; source of the reader/writer and target of resizes.
    master: Box<dyn MasterPty + Send>,
    /// The spawned `ssh` child process.
    #[allow(dead_code)]
    child: Box<dyn Child + Send>,
}

impl SshSession {
    /// Spawn `ssh` in a PTY using the given parameters.
    ///
    /// Never places a secret on argv; host-key checking is never disabled.
    pub fn spawn(p: &SshParams) -> Result<SshSession> {
        // ssh uses lowercase -p for the port.
        Self::spawn_program("ssh", "-p", p)
    }

    /// Spawn an interactive `sftp` session to the same target, in a PTY.
    ///
    /// Reuses the SSH host's params (port, user, jump); auth (key/agent/password)
    /// works exactly as for `ssh`.
    pub fn spawn_sftp(p: &SshParams) -> Result<SshSession> {
        // sftp uses uppercase -P for the port.
        Self::spawn_program("sftp", "-P", p)
    }

    /// Shared launcher for the `ssh`/`sftp` family. `port_flag` is `-p` (ssh) or `-P` (sftp).
    fn spawn_program(program: &str, port_flag: &str, p: &SshParams) -> Result<SshSession> {
        let pty = native_pty_system();
        let pair = pty
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| CoreError::Spawn(format!("openpty failed: {e}")))?;

        let mut cmd = CommandBuilder::new(program);
        cmd.arg(port_flag);
        cmd.arg(p.port.to_string());
        if let Some(jump) = &p.jump {
            if !jump.trim().is_empty() {
                cmd.arg("-J");
                cmd.arg(jump);
            }
        }
        if let Some(identity) = &p.identity_file {
            if !identity.is_empty() {
                cmd.arg("-i");
                cmd.arg(identity);
                // Offer only this key, so ssh doesn't fall back to ~/.ssh defaults.
                cmd.arg("-o");
                cmd.arg("IdentitiesOnly=yes");
            }
        }
        let target = match &p.user {
            Some(u) if !u.is_empty() => format!("{u}@{}", p.host),
            _ => p.host.clone(),
        };
        cmd.arg(target);

        let child: Box<dyn Child + Send> = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| CoreError::Spawn(format!("failed to spawn {program}: {e}")))?;

        // Drop the slave so the master observes EOF when the child exits.
        drop(pair.slave);

        Ok(SshSession {
            master: pair.master,
            child,
        })
    }

    /// Clone the PTY reader (the terminal byte stream). Returns `None` on failure.
    pub fn take_reader(&mut self) -> Option<Box<dyn Read + Send>> {
        self.master.try_clone_reader().ok()
    }

    /// Get a writer to the PTY (typed input, including a password fed over the PTY).
    pub fn writer(&self) -> Result<Box<dyn Write + Send>> {
        self.master
            .take_writer()
            .map_err(|e| CoreError::Spawn(format!("pty writer unavailable: {e}")))
    }

    /// Resize the PTY to the given terminal dimensions.
    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        self.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| CoreError::Spawn(format!("pty resize failed: {e}")))
    }
}
