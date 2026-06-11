//! External RDP launcher.
//!
//! Phase 0 of RDP: open the system RDP client (FreeRDP's `xfreerdp` on Linux) in its
//! own window, with the host and password pulled from the vault. The password is fed
//! over stdin (`/from-stdin`) rather than argv, so it isn't visible in `ps`. Embedded
//! in-window RDP (IronRDP) is a future phase.

use crate::error::{CoreError, Result};

/// Parameters for launching an RDP session.
pub struct RdpParams {
    pub host: String,
    pub port: u16,
    pub user: Option<String>,
    pub password: Option<String>,
    pub domain: Option<String>,
}

/// Find an `xfreerdp` binary (FreeRDP 3 first, then 2) on PATH or common dirs.
#[cfg(target_os = "linux")]
fn resolve_freerdp() -> Option<String> {
    use std::path::Path;
    let names = ["xfreerdp3", "xfreerdp"];
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            for n in names {
                let c = dir.join(n);
                if c.is_file() {
                    return Some(c.to_string_lossy().into_owned());
                }
            }
        }
    }
    for dir in ["/usr/bin", "/usr/local/bin", "/bin"] {
        for n in names {
            let c = Path::new(dir).join(n);
            if c.is_file() {
                return Some(c.to_string_lossy().into_owned());
            }
        }
    }
    None
}

/// Launch an external RDP session (Linux/FreeRDP). Returns once the client has started;
/// the client runs in its own window, independent of ssharden.
#[cfg(target_os = "linux")]
pub fn launch(p: &RdpParams) -> Result<()> {
    use std::io::Write;
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};

    let bin = resolve_freerdp().ok_or_else(|| {
        CoreError::Spawn(
            "FreeRDP not found — install it, e.g. `sudo apt install freerdp2-x11` (xfreerdp) \
             or the freerdp3 package"
                .into(),
        )
    })?;
    let is_v3 = std::path::Path::new(&bin)
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.contains("xfreerdp3"))
        .unwrap_or(false);

    let mut cmd = Command::new(&bin);
    cmd.arg(format!("/v:{}:{}", p.host, p.port));
    if let Some(u) = p.user.as_deref().filter(|s| !s.is_empty()) {
        cmd.arg(format!("/u:{u}"));
    }
    if let Some(d) = p.domain.as_deref().filter(|s| !s.is_empty()) {
        cmd.arg(format!("/d:{d}"));
    }
    // Trust-on-first-use for the server cert: self-signed RDP certs are the norm and a
    // detached process can't answer an interactive trust prompt, so FreeRDP pins the
    // cert on first contact (~/.config/freerdp) and refuses a *changed* cert afterwards.
    // A changed cert therefore fails silently (no window opens) — delete the host's
    // entry under ~/.config/freerdp/server/ to re-pin after a legitimate rotation.
    cmd.arg(if is_v3 { "/cert:tofu" } else { "/cert-tofu" });
    cmd.arg("/dynamic-resolution");

    let feed_pw = p.password.as_deref().filter(|s| !s.is_empty());
    if feed_pw.is_some() {
        cmd.arg("/from-stdin"); // read the password from stdin, not argv
        cmd.stdin(Stdio::piped());
    } else {
        cmd.stdin(Stdio::null());
    }
    cmd.stdout(Stdio::null()).stderr(Stdio::null());

    // Detach into its own session so it outlives the spawning context cleanly.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| CoreError::Spawn(format!("failed to launch {bin}: {e}")))?;
    if let Some(pw) = feed_pw {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = writeln!(stdin, "{pw}");
        } // stdin dropped → EOF
    }
    // Reap the child when it exits so it doesn't linger as a zombie.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// RDP launch is currently implemented for Linux (FreeRDP) only.
#[cfg(not(target_os = "linux"))]
pub fn launch(_p: &RdpParams) -> Result<()> {
    Err(CoreError::Spawn(
        "external RDP launch is currently implemented for Linux (FreeRDP) only".into(),
    ))
}
