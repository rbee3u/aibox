//! Resolve and validate user-controlled bind mounts.
//!
//! [`super::RunSpec`] owns resolution and validation for Run Workspace and
//! Extra Mount paths. Debug Shell and container-based Component callers use
//! the shared Tenant Home source check after canonicalizing the path.

use crate::tenant::CONTAINER_HOME;
use anyhow::{Context, Result, bail};
use std::path::{Component, Path, PathBuf};

/// Reject a bind source containing `:` because Docker's `-v` short syntax
/// cannot represent it safely.
pub(crate) fn reject_colon_in_bind_source(kind: &str, path: &Path) -> Result<()> {
    let Some(path_str) = path.to_str() else {
        bail!(
            "{kind} path is not valid UTF-8 and cannot be represented safely for docker: {}",
            path.display()
        );
    };
    if path_str.contains(':') {
        bail!(
            "{kind} path contains ':', which docker -v cannot represent: {}",
            path.display()
        );
    }
    Ok(())
}

/// Resolve the Workspace to an existing absolute UTF-8 path.
///
/// Relative input is anchored to the process working directory.
pub(super) fn resolve_workspace(workspace: Option<&str>) -> Result<String> {
    let cwd = std::env::current_dir().context("get current dir for /workspace")?;
    let path = match workspace {
        Some(workspace) => {
            let path = Path::new(workspace);
            if path.is_absolute() {
                path.to_path_buf()
            } else {
                cwd.join(path)
            }
        }
        None => cwd,
    };
    if !path.is_dir() {
        bail!("workspace is not a directory: {}", path.display());
    }
    let path = std::fs::canonicalize(&path)
        .with_context(|| format!("resolve workspace {}", path.display()))?;
    reject_colon_in_bind_source("workspace", &path)?;
    Ok(path
        .to_str()
        .context("workspace path is not valid UTF-8")?
        .to_string())
}

/// Parse and resolve user bind mounts into Docker `-v` specifications.
///
/// Sources must exist; relative sources are anchored to the process working
/// directory. Targets must be absolute, and the only accepted mode is `ro`.
pub(super) fn resolve_mounts(mounts: &[String]) -> Result<Vec<String>> {
    mounts.iter().map(|mount| resolve_mount(mount)).collect()
}

fn resolve_mount(mount: &str) -> Result<String> {
    let spec = parse_mount_spec(mount)?;
    let source = Path::new(spec.host);
    let source = if source.is_absolute() {
        source.to_path_buf()
    } else {
        std::env::current_dir()
            .context("get current dir for mounts")?
            .join(source)
    };
    if !source.exists() {
        bail!("mount host path does not exist: {}", source.display());
    }
    let source = std::fs::canonicalize(&source)
        .with_context(|| format!("resolve mount host path {}", source.display()))?;
    reject_colon_in_bind_source("mount host", &source)?;
    let source = source
        .to_str()
        .context("mount host path is not valid UTF-8")?;
    let mode = spec.mode.map(|mode| format!(":{mode}")).unwrap_or_default();
    Ok(format!("{source}:{}{mode}", spec.target))
}

struct MountSpec<'a> {
    host: &'a str,
    target: &'a str,
    mode: Option<&'a str>,
}

fn parse_mount_spec(mount: &str) -> Result<MountSpec<'_>> {
    let mut parts = mount.split(':');
    let host = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default();
    let mode = parts.next();
    if parts.next().is_some() {
        bail!("invalid mount (need host:container[:ro]): {mount}");
    }
    if host.is_empty() || target.is_empty() || !target.starts_with('/') {
        bail!("invalid mount (need host:container[:ro]): {mount}");
    }
    match mode {
        None | Some("ro") => Ok(MountSpec { host, target, mode }),
        Some("") => bail!("invalid mount mode in {mount:?}: use :ro or omit the mode"),
        Some(other) => bail!("invalid mount mode {other:?} in {mount:?}: only :ro is supported"),
    }
}

/// Reject extra mounts that replace `/workspace`, the shared container Home,
/// or an ancestor of either managed target.
pub(super) fn validate_extra_mount_targets(mounts: &[String]) -> Result<()> {
    for mount in mounts {
        let target = bind_target(mount)?;
        let target = normalize_container_target(target)?;
        if shadows_managed_target(&target, "/workspace")
            || shadows_managed_target(&target, CONTAINER_HOME)
        {
            bail!(
                "extra mount target {target:?} would override or shadow an AIBox-managed mount; choose a nested target instead: {mount}"
            );
        }
    }
    Ok(())
}

/// Keep host-only AIBox data out of user-selected bind mounts.
///
/// Sources unrelated to `aibox_root` are allowed. Its ancestors are rejected
/// because mounting one would expose the root indirectly. Sources inside the
/// root must resolve beneath a validly named Managed Tenant Home subtree.
/// Named Config catalogs, Requests, Host Tenant catalogs, and internal
/// staging directories are rejected. Sources are resolved before comparison,
/// so a symlink cannot disguise an AIBox-internal target as an unrelated path.
pub(super) fn validate_aibox_mount_sources(
    workspace: &str,
    extra_mounts: &[String],
    aibox_root: &Path,
) -> Result<()> {
    let resolved_root = canonicalize_existing_prefix(aibox_root)
        .with_context(|| format!("resolve AIBox Root {}", aibox_root.display()))?;
    let workspace_source = canonicalize_existing_prefix(Path::new(workspace))
        .with_context(|| format!("resolve workspace {workspace}"))?;
    reject_aibox_internal_source(
        "workspace",
        Path::new(workspace),
        &workspace_source,
        &resolved_root,
        aibox_root,
    )?;

    for mount in extra_mounts {
        let source = bind_source(mount)?;
        let source_path = Path::new(source);
        let resolved_source = canonicalize_existing_prefix(source_path)
            .with_context(|| format!("resolve mount host path {source}"))?;
        reject_aibox_internal_source(
            "mount host",
            source_path,
            &resolved_source,
            &resolved_root,
            aibox_root,
        )?;
    }
    Ok(())
}

fn reject_aibox_internal_source(
    kind: &str,
    display_path: &Path,
    source: &Path,
    resolved_root: &Path,
    aibox_root: &Path,
) -> Result<()> {
    if resolved_root.starts_with(source) {
        bail!(
            "{kind} would expose AIBox internal data: {} overlaps {}",
            display_path.display(),
            aibox_root.display()
        );
    }
    let Ok(relative) = source.strip_prefix(resolved_root) else {
        return Ok(());
    };
    let mut components = relative.components();
    if !matches!(components.next(), Some(Component::Normal(name)) if name == crate::tenant::TENANTS_DIR)
    {
        bail!(
            "{kind} would expose AIBox internal data: {}",
            display_path.display()
        );
    }
    let Some(Component::Normal(tenant_name)) = components.next() else {
        bail!(
            "{kind} would expose AIBox internal data: {}",
            display_path.display()
        );
    };
    let Some(tenant_name) = tenant_name.to_str() else {
        bail!(
            "{kind} would expose AIBox internal data: {}",
            display_path.display()
        );
    };
    if crate::tenant::validate_name("tenant", tenant_name).is_err() {
        bail!(
            "{kind} would expose AIBox internal data: {}; only Managed Tenant Home trees may be mounted from {}",
            display_path.display(),
            aibox_root.display()
        );
    }
    Ok(())
}

fn bind_source(mount: &str) -> Result<&str> {
    let (source, _) = mount
        .split_once(':')
        .with_context(|| format!("invalid resolved mount: {mount}"))?;
    if source.is_empty() {
        bail!("invalid resolved mount source: {mount}");
    }
    Ok(source)
}

fn bind_target(mount: &str) -> Result<&str> {
    let (_, rest) = mount
        .split_once(':')
        .with_context(|| format!("invalid resolved mount: {mount}"))?;
    let target = rest.split_once(':').map_or(rest, |(target, _)| target);
    if target.is_empty() {
        bail!("invalid resolved mount target: {mount}");
    }
    Ok(target)
}

fn normalize_container_target(target: &str) -> Result<String> {
    if !target.starts_with('/') {
        bail!("container mount target must be absolute: {target:?}");
    }

    let mut parts = Vec::new();
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    if parts.is_empty() {
        Ok("/".to_string())
    } else {
        Ok(format!("/{}", parts.join("/")))
    }
}

fn shadows_managed_target(target: &str, managed: &str) -> bool {
    target == managed
        || target == "/"
        || managed
            .strip_prefix(target)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn canonicalize_existing_prefix(path: &Path) -> Result<PathBuf> {
    let mut cursor = path;
    let mut missing = Vec::new();
    loop {
        match std::fs::canonicalize(cursor) {
            Ok(mut resolved) => {
                for component in missing.iter().rev() {
                    resolved.push(component);
                }
                return Ok(normalize_path_components(&resolved));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let name = cursor
                    .file_name()
                    .with_context(|| format!("path does not exist: {}", path.display()))?;
                missing.push(name.to_os_string());
                cursor = cursor
                    .parent()
                    .filter(|parent| !parent.as_os_str().is_empty())
                    .with_context(|| format!("path does not exist: {}", path.display()))?;
            }
            Err(error) => {
                return Err(error).with_context(|| format!("resolve {}", path.display()));
            }
        }
    }
}

fn normalize_path_components(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(_) | Component::RootDir | Component::Prefix(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    normalized
}

#[cfg(test)]
#[path = "mount_tests.rs"]
mod tests;
