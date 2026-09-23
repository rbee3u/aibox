//! Fully resolved and validated filesystem inputs for one Run.

use super::{args, mount};
use anyhow::{Context, Result};
use std::path::Path;

/// A workspace source and its container target, resolved before mount checks.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ResolvedWorkspace {
    source: String,
    target: String,
}

impl ResolvedWorkspace {
    fn resolve(workspace: Option<&str>) -> Result<Self> {
        let source = mount::resolve_workspace(workspace)?;
        let name = Path::new(&source)
            .file_name()
            .and_then(|name| name.to_str())
            .with_context(|| format!("workspace has no directory name: {source}"))?;
        Ok(Self {
            target: format!("/workspace/{name}"),
            source,
        })
    }
}

/// An extra bind mount that has completed host-side resolution. Keeping this
/// distinct from the raw CLI string prevents execution from re-parsing or
/// partially validating mounts after the sandbox boundary has been checked.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ExtraMount(String);

/// Fully resolved and validated inputs for an Agent Run.
///
/// Constructing one is the only way to reach mount resolution, so the
/// resolve-then-validate order is a property of the type rather than a rule
/// callers must remember. Runtime Image, Tenant, Component, and environment
/// checks intentionally remain in `execution`; this value owns only the
/// workspace/mount boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RunSpec {
    workspace: ResolvedWorkspace,
    extra_mounts: Vec<ExtraMount>,
}

impl RunSpec {
    /// Resolve and validate all user-controlled filesystem inputs once, in the
    /// same order used by the Run orchestration before Runtime Image lookup.
    pub(crate) fn resolve(
        workspace: Option<&str>,
        mounts: &[String],
        aibox_root: &Path,
    ) -> Result<Self> {
        let workspace = ResolvedWorkspace::resolve(workspace)?;
        let extra_mounts = mount::resolve_mounts(mounts)?
            .into_iter()
            .map(ExtraMount)
            .collect::<Vec<_>>();
        let mount_strings = extra_mounts
            .iter()
            .map(|mount| mount.0.clone())
            .collect::<Vec<_>>();
        mount::validate_extra_mount_targets(&mount_strings, &workspace.target)?;
        mount::validate_aibox_mount_sources(&workspace.source, &mount_strings, aibox_root)?;
        Ok(Self {
            workspace,
            extra_mounts,
        })
    }

    pub(crate) fn assemble_run_args(&self, home_dir: &Path) -> Vec<String> {
        let mounts = self
            .extra_mounts
            .iter()
            .map(|mount| mount.0.clone())
            .collect::<Vec<_>>();
        args::assemble_run_args(
            &self.workspace.source,
            &self.workspace.target,
            home_dir,
            &mounts,
        )
    }
}

#[cfg(test)]
#[path = "spec_tests.rs"]
mod tests;
