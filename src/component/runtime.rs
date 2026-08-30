//! Managed Tenant runtime Component installer orchestration.

use super::catalog::inspect;
use super::native::write_atomic;
use super::{ComponentKind, ComponentSpec, ComponentStatus, MAX_CONFIG_BYTES};
use crate::foundation::safe_fs::FileSnapshot;
use crate::tenant::ManagedTenant;
use anyhow::{Context, Result, bail};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

pub(super) const RUST_INSTALLER: &str = include_str!("../../assets/install-rust.sh");
pub(super) const GO_INSTALLER: &str = include_str!("../../assets/install-go.sh");
pub(super) const NODE_INSTALLER: &str = include_str!("../../assets/install-node.sh");
pub(super) const CODEX_INSTALLER: &str = include_str!("../../assets/install-codex.sh");
pub(super) const CLAUDE_INSTALLER: &str = include_str!("../../assets/install-claude.sh");
pub(super) const PYTHON_INSTALLER: &str = include_str!("../../assets/install-python.sh");

/// Install a runtime Component with an injected Docker client and no log sink.
#[cfg(test)]
pub(super) fn install_runtime_component_with(
    tenant: &ManagedTenant,
    component: &ComponentSpec,
    docker: &crate::docker::DockerCli,
) -> Result<i32> {
    install_runtime_component(tenant, component, docker, None)
}

pub(super) fn install_runtime_component(
    tenant: &ManagedTenant,
    component: &ComponentSpec,
    docker: &crate::docker::DockerCli,
    service_log: Option<crate::docker::LogCallback>,
) -> Result<i32> {
    let existing = if tenant.exists()? {
        inspect(component.kind, tenant.home_dir())?
    } else {
        ComponentStatus::NotInstalled
    };
    if let Some(requested) = &component.version
        && matches!(
            existing,
            ComponentStatus::Installed { version: Some(ref current) } if current == requested
        )
    {
        eprintln!(
            ">> {} {requested} is already installed; skipping",
            component.kind.name()
        );
        return Ok(0);
    }
    if existing == ComponentStatus::Unmanaged {
        bail!(
            "{} has unmanaged Component state; remove or normalize it before installation",
            component.kind.name()
        );
    }

    let image = crate::docker::IMAGE;
    if !crate::docker::image_exists_with(docker, image)? {
        bail!(
            "{image} is not present locally; use `aibox console` to build the Runtime Image from Console Overview before installing Components"
        );
    }

    tenant.ensure_initialized()?;
    let home = fs::canonicalize(tenant.home_dir())
        .with_context(|| format!("resolve Tenant Home {}", tenant.home_dir().display()))?;
    crate::sandbox::reject_colon_in_bind_source("Tenant Home", &home)?;
    let run_args = crate::sandbox::assemble_component_run_args(&home);
    let script = match component.kind {
        ComponentKind::Node => NODE_INSTALLER,
        ComponentKind::Codex => CODEX_INSTALLER,
        ComponentKind::Claude => CLAUDE_INSTALLER,
        ComponentKind::Python => PYTHON_INSTALLER,
        ComponentKind::Rust => RUST_INSTALLER,
        ComponentKind::Go => GO_INSTALLER,
        _ => unreachable!("statusline Components are installed on the host"),
    };
    let command = vec![
        OsString::from("bash"),
        // Keep the installer non-interactive and fail-fast, while Bash's
        // xtrace sends every command to stderr. Docker forwards that stream
        // to the Management Operation log alongside installer output.
        OsString::from("-ceux"),
        OsString::from(script),
        OsString::from(format!("aibox-{}-installer", component.kind.name())),
        OsString::from(component.version.as_deref().unwrap_or("")),
    ];
    let profiles = capture_user_shell_profiles(&home)?;
    let run_result = if let Some(log) = service_log {
        let started_log = log.clone();
        let component_name = component.kind.name();
        crate::docker::run_for_service(
            docker,
            &run_args,
            image,
            &command,
            move || started_log(format!("{component_name} installer container started")),
            log,
        )
    } else {
        crate::docker::run_with(docker, &run_args, image, &command, || {})
    };
    let restore_result = restore_user_shell_profiles(&profiles);
    let code = match (run_result, restore_result) {
        (Ok(code), Ok(())) => code,
        (Err(error), Ok(())) => return Err(error),
        (Ok(_), Err(error)) => return Err(error).context("restore user shell profiles"),
        (Err(run_error), Err(restore_error)) => bail!(
            "Component installer failed: {run_error:#}; restoring user shell profiles also failed: {restore_error:#}"
        ),
    };
    if code != 0 {
        bail!(
            "{} Component installer exited with status {code}",
            component.kind.name()
        );
    }
    match inspect(component.kind, &home)? {
        ComponentStatus::Installed { version }
            if component
                .version
                .as_ref()
                .is_none_or(|requested| version.as_ref() == Some(requested)) =>
        {
            Ok(0)
        }
        status => bail!(
            "{} Component did not become healthy after installation: {status:?}",
            component.kind.name()
        ),
    }
}

pub(super) struct UserShellProfile {
    path: PathBuf,
    snapshot: FileSnapshot,
}

pub(super) fn capture_user_shell_profiles(home: &Path) -> Result<Vec<UserShellProfile>> {
    [".bash_profile", ".bashrc"]
        .into_iter()
        .map(|name| {
            let path = home.join(name);
            let snapshot = FileSnapshot::capture_with_limit(&path, MAX_CONFIG_BYTES)
                .with_context(|| format!("capture user shell profile {}", path.display()))?;
            Ok(UserShellProfile { path, snapshot })
        })
        .collect()
}

pub(super) fn restore_user_shell_profiles(profiles: &[UserShellProfile]) -> Result<()> {
    for profile in profiles {
        let metadata = match fs::symlink_metadata(&profile.path) {
            Ok(metadata) => Some(metadata),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("inspect user shell profile {}", profile.path.display())
                });
            }
        };
        if metadata.as_ref().is_some_and(|metadata| {
            let file_type = metadata.file_type();
            !file_type.is_file() && !file_type.is_symlink()
        }) {
            bail!(
                "user shell profile is not a file or symlink: {}",
                profile.path.display()
            );
        }
        if metadata
            .as_ref()
            .is_some_and(|metadata| metadata.file_type().is_symlink())
        {
            fs::remove_file(&profile.path).with_context(|| {
                format!(
                    "remove user shell profile symlink {}",
                    profile.path.display()
                )
            })?;
            crate::foundation::safe_fs::sync_dir(
                profile
                    .path
                    .parent()
                    .context("user shell profile has no parent")?,
            )?;
        }
        if profile.snapshot.present {
            write_atomic(
                &profile.path,
                &profile.snapshot.content,
                profile.snapshot.mode,
            )?;
        } else if metadata
            .as_ref()
            .is_some_and(|metadata| metadata.file_type().is_file())
        {
            fs::remove_file(&profile.path)
                .with_context(|| format!("remove user shell profile {}", profile.path.display()))?;
            crate::foundation::safe_fs::sync_dir(
                profile
                    .path
                    .parent()
                    .context("user shell profile has no parent")?,
            )?;
        }
    }
    Ok(())
}
