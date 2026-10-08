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

/// Validated Workspace and Extra Mounts for a Run. Construction enforces
/// resolve-before-validate; execution owns Image, Tenant, and Component preflight.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RunSpec {
    workspace: ResolvedWorkspace,
    extra_mounts: Vec<String>,
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
        let extra_mounts = mount::resolve_mounts(mounts)?;
        mount::validate_extra_mount_targets(&extra_mounts, &workspace.target)?;
        mount::validate_aibox_mount_sources(&workspace.source, &extra_mounts, aibox_root)?;
        Ok(Self {
            workspace,
            extra_mounts,
        })
    }

    pub(crate) fn assemble_run_args(&self, home_dir: &Path) -> Vec<String> {
        args::assemble_run_args(
            &self.workspace.source,
            &self.workspace.target,
            home_dir,
            &self.extra_mounts,
        )
    }
}

#[cfg(test)]
#[path = "spec_tests.rs"]
mod tests;
