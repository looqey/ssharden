//! `ssharden-core` — pure-Rust core for ssharden.
//!
//! No `tauri`/webkit dependency: this crate compiles and unit-tests on any machine.
//! It contains the `bw serve` vault adapter, the `Host`/URI model, and the SSH PTY
//! launcher. `src-tauri` is a thin shell that wraps these in `#[tauri::command]`s.

pub mod error;
pub mod rdp;
pub mod sftp;
pub mod ssh;
pub mod vault;

pub use error::{CoreError, Result};
pub use sftp::{FsEntry, SftpConn};
pub use ssh::{SshParams, SshSession};
pub use vault::model::{AccountStatus, Host, HostInput, HostUri};
pub use vault::{account_status, resolve_bw, VaultClient};
