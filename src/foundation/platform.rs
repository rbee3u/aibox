//! Host-platform probes for Linux-specific flags, uid/gid, and TTY detection,
//! plus process file-descriptor limit adjustments.
//!
//! These decide the Linux-only `--user`/`--add-host` flags and the `-it` vs `-i`
//! Docker flag, so they must reflect the *host* the wrapper runs on — not the
//! container. The Service also raises `RLIMIT_NOFILE` here before it opens
//! sockets or Request files.

use std::io::IsTerminal;

/// Soft `RLIMIT_NOFILE` the Service tries to reach when the inherited ceiling
/// is lower. macOS and some service-manager environments start processes at
/// 256, which a busy Request Proxy exhausts.
const DESIRED_NOFILE_SOFT: u64 = 65_536;

/// True when the host is Linux. Gates the `--user host-uid:gid` and
/// `--add-host host.docker.internal:host-gateway` Docker flags. Docker Desktop
/// on macOS handles ownership and that hostname without these flags.
pub(crate) fn is_linux() -> bool {
    cfg!(target_os = "linux")
}

/// Host uid/gid, for `docker run --user uid:gid` on Linux so files created in
/// `/workspace` stay owned by the invoking user. Only meaningful on Linux;
/// callers gate on [`is_linux`] first.
#[cfg(unix)]
pub(crate) fn uid_gid() -> (u32, u32) {
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
pub(crate) fn has_tty() -> bool {
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

/// Raise the process file-descriptor soft limit when the inherited ceiling is
/// too low.
///
/// macOS and some service-manager environments start processes with a soft
/// `RLIMIT_NOFILE` of 256. Each in-flight proxied Request holds several
/// descriptors, so that ceiling is easy to hit; the next `mkdir` or directory
/// walk then fails with `Too many open files`. This never lowers the current
/// limit and never fails the caller.
pub(crate) fn raise_nofile_limit() {
    #[cfg(unix)]
    {
        let current = rustix::process::getrlimit(rustix::process::Resource::Nofile);
        if let Some(next) = nofile_soft_to_apply(current.current, current.maximum) {
            let _ = rustix::process::setrlimit(
                rustix::process::Resource::Nofile,
                rustix::process::Rlimit {
                    current: Some(next),
                    maximum: current.maximum,
                },
            );
        }
    }
}

/// Next soft `RLIMIT_NOFILE` to apply, or `None` when the current limit is
/// already unlimited or at least the reachable ceiling.
fn nofile_soft_to_apply(current: Option<u64>, maximum: Option<u64>) -> Option<u64> {
    let soft = current?;
    let ceiling = maximum.unwrap_or(DESIRED_NOFILE_SOFT);
    let next = DESIRED_NOFILE_SOFT.min(ceiling);
    (next > soft).then_some(next)
}

#[cfg(test)]
#[path = "platform_tests.rs"]
mod tests;
