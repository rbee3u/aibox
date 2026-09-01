//! Assemble `docker run` arguments.
//!
//! These are pure builders. [`super::RunSpec`] supplies validated Run paths;
//! Debug Shell and container-based Component callers must canonicalize the
//! Tenant Home and reject unsupported bind-source syntax before calling them.

use crate::foundation::platform;
use crate::tenant::CONTAINER_HOME;
use std::path::Path;

/// Assemble Docker arguments for the Tenant Home, Workspace, and Extra Mounts.
pub(super) fn assemble_run_args(
    workspace: &str,
    home_dir: &Path,
    extra_mounts: &[String],
) -> Vec<String> {
    let mut args = base_container_args(true);

    args.push("-v".into());
    args.push(format!("{}:{CONTAINER_HOME}", home_dir.display()));
    args.push("-v".into());
    args.push(format!("{workspace}:/workspace"));
    args.extend(["-w".into(), "/workspace".into()]);
    for mount in extra_mounts {
        args.push("-v".into());
        args.push(mount.clone());
    }
    args
}

/// Assemble Docker arguments for a Debug Shell scoped to one Managed Tenant Home.
pub(crate) fn assemble_debug_args(home_dir: &Path) -> Vec<String> {
    assemble_debug_args_for_terminal(home_dir, platform::has_tty())
}

fn assemble_debug_args_for_terminal(home_dir: &Path, has_tty: bool) -> Vec<String> {
    let mut args = base_container_args_for_terminal(true, has_tty);
    args.push("-v".into());
    args.push(format!("{}:{CONTAINER_HOME}", home_dir.display()));
    args.extend(["-w".into(), CONTAINER_HOME.into()]);
    args
}

/// Assemble Docker arguments for a non-interactive task that may write only
/// to one Managed Tenant Home.
pub(crate) fn assemble_component_run_args(home_dir: &Path) -> Vec<String> {
    let mut args = base_container_args(false);
    args.push("-v".into());
    args.push(format!("{}:{CONTAINER_HOME}", home_dir.display()));
    args.extend(["-w".into(), CONTAINER_HOME.into()]);
    args
}

fn base_container_args(interactive: bool) -> Vec<String> {
    base_container_args_for_terminal(interactive, platform::has_tty())
}

fn base_container_args_for_terminal(interactive: bool, has_tty: bool) -> Vec<String> {
    let mut args: Vec<String> = vec!["--rm".into()];
    if interactive {
        args.push(if has_tty { "-it" } else { "-i" }.into());
    }
    args.extend(["--security-opt".into(), "no-new-privileges".into()]);
    args.extend(["--cap-drop".into(), "ALL".into()]);

    if platform::is_linux() {
        let (uid, gid) = platform::uid_gid();
        args.push("--user".into());
        args.push(format!("{uid}:{gid}"));
        args.push("--add-host".into());
        args.push("host.docker.internal:host-gateway".into());
    }
    args
}

#[cfg(test)]
#[path = "args_tests.rs"]
mod tests;
