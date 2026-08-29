//! Host-platform probes for Linux-specific flags, uid/gid, and TTY detection.
//!
//! These decide the Linux-only `--user`/`--add-host` flags and the `-it` vs `-i`
//! Docker flag, so they must reflect the *host* the wrapper runs on — not the
//! container.

use std::io::IsTerminal;

/// True when the host is Linux. Gates the `--user host-uid:gid` and
/// `--add-host host.docker.internal:host-gateway` Docker flags. Docker Desktop
/// on macOS handles ownership and that hostname without these flags.
pub fn is_linux() -> bool {
    cfg!(target_os = "linux")
}

/// Host uid/gid, for `docker run --user uid:gid` on Linux so files created in
/// `/workspace` stay owned by the invoking user. Only meaningful on Linux;
/// callers gate on [`is_linux`] first.
#[cfg(unix)]
pub fn uid_gid() -> (u32, u32) {
    use rustix::process::{getgid, getuid};
    (getuid().as_raw(), getgid().as_raw())
}

/// Compatibility fallback for non-Unix builds. Run assembly calls this only on
/// Linux.
#[cfg(not(unix))]
pub fn uid_gid() -> (u32, u32) {
    (0, 0)
}

/// True only when both stdin and stdout are TTYs. Decides `-it` (interactive)
/// vs `-i` (piped) so that piping into the agent or Debug Shell still works.
pub fn has_tty() -> bool {
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn uid_gid_reports_the_invoking_process_identity() {
        assert_eq!(
            uid_gid(),
            (
                rustix::process::getuid().as_raw(),
                rustix::process::getgid().as_raw()
            )
        );
    }
}
