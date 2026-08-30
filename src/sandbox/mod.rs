//! Enforce the Filesystem Sandbox boundary for Runs, Debug Shells, and
//! Component installers.
//!
//! [`RunSpec`] is the only way to resolve a Run's Workspace and Extra Mounts;
//! mount parsing and validation are private to [`mount`] so the ordering they
//! depend on cannot be bypassed. [`args`] holds pure `docker run` builders for
//! the two launches that need no user-supplied paths.

mod args;
mod mount;
mod spec;

pub(crate) use args::{assemble_component_run_args, assemble_debug_args};
pub(crate) use mount::reject_colon_in_bind_source;
pub(crate) use spec::RunSpec;
