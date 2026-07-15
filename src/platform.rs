//! Host-platform probes: uid/gid, TTY detection, OS gate.
//!
//! These decide the Linux-only `--user`/`--add-host` flags and the `-it` vs `-i`
//! docker flag, so they must reflect the *host* the wrapper runs on — not the
//! container.

use std::io::IsTerminal;

/// True when the host is Linux. Gates the `--user host-uid:gid` and
/// `--add-host host.docker.internal:host-gateway` docker flags (Docker Desktop on
/// macOS/Windows handles ownership and that hostname on its own).
pub fn is_linux() -> bool {
    cfg!(target_os = "linux")
}

/// Host uid/gid, for `docker run --user uid:gid` on Linux so files created in
/// `/work` stay owned by the invoking user. Only meaningful on Linux; callers
/// gate on [`is_linux`] first.
#[cfg(unix)]
pub fn uid_gid() -> (u32, u32) {
    use rustix::process::{getgid, getuid};
    (getuid().as_raw(), getgid().as_raw())
}

#[cfg(not(unix))]
pub fn uid_gid() -> (u32, u32) {
    (0, 0)
}

/// True only when both stdin and stdout are TTYs. Decides `-it` (interactive)
/// vs `-i` (piped) so that piping into the agent still works.
pub fn has_tty() -> bool {
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}
