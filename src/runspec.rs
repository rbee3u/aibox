//! Resolve bind mounts, enforce the Filesystem Sandbox mount boundary, and
//! assemble the `docker run` arguments shared by both Coding Agents.

#[cfg(test)]
use crate::agent::AgentKind;
use crate::platform;
use anyhow::{bail, Context, Result};
use std::path::{Component, Path, PathBuf};

const CONTAINER_HOME: &str = "/home/aibox";

/// Reject a bind source containing `:` because Docker's `-v` short syntax
/// cannot represent it safely.
pub fn reject_colon_in_bind_source(kind: &str, path: &Path) -> Result<()> {
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
pub fn resolve_workspace(workspace: Option<&str>) -> Result<String> {
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
/// This function does not enforce managed mount-target or aibox-root boundaries;
/// callers must subsequently use [`validate_extra_mount_targets`] and
/// [`validate_aibox_mount_sources`].
pub fn resolve_mounts(mounts: &[String]) -> Result<Vec<String>> {
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
///
/// `mounts` must contain the resolved specifications returned by
/// [`resolve_mounts`].
pub fn validate_extra_mount_targets(mounts: &[String]) -> Result<()> {
    for mount in mounts {
        let target = bind_target(mount)?;
        let target = normalize_container_target(target)?;
        if shadows_managed_target(&target, "/workspace")
            || shadows_managed_target(&target, CONTAINER_HOME)
        {
            bail!(
                "extra mount target {target:?} would override or shadow an aibox-managed mount; choose a nested target instead: {mount}"
            );
        }
    }
    Ok(())
}

/// Keep host-only aibox data out of user-selected bind mounts.
///
/// Sources unrelated to `aibox_root` are allowed. Its ancestors are rejected
/// because mounting one would expose the root indirectly. Sources inside the
/// root must resolve beneath a validly named Managed Tenant Home subtree.
/// Agent/Tenant metadata, internal staging directories, and Host Tenant
/// metadata are rejected.
/// Sources are resolved before comparison, so a symlink cannot disguise an
/// aibox-internal target as an unrelated path.
///
/// `workspace` and `extra_mounts` must be the resolved values returned by
/// [`resolve_workspace`] and [`resolve_mounts`].
pub fn validate_aibox_mount_sources(
    workspace: &str,
    extra_mounts: &[String],
    aibox_root: &Path,
) -> Result<()> {
    let resolved_root = canonicalize_existing_prefix(aibox_root)
        .with_context(|| format!("resolve aibox root {}", aibox_root.display()))?;
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
            "{kind} would expose aibox internal data: {} overlaps {}",
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
            "{kind} would expose aibox internal data: {}",
            display_path.display()
        );
    }
    let Some(Component::Normal(tenant_name)) = components.next() else {
        bail!(
            "{kind} would expose aibox internal data: {}",
            display_path.display()
        );
    };
    let Some(tenant_name) = tenant_name.to_str() else {
        bail!(
            "{kind} would expose aibox internal data: {}",
            display_path.display()
        );
    };
    if crate::tenant::validate_name("tenant", tenant_name).is_err() {
        bail!(
            "{kind} would expose aibox internal data: {}; only Managed Tenant Home trees may be mounted from {}",
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

/// Seed runtime state required by a Coding Agent because the Tenant Home mount
/// shadows the image's home directory.
///
/// `home_dir` must be an already validated Tenant Home; creation protects the
/// final state-directory entry but does not recursively validate ancestors.
#[cfg(test)]
pub fn seed_home(agent: AgentKind, home_dir: &Path) -> Result<()> {
    crate::tenant::ensure_agent_state(agent, home_dir)
}

/// Assemble Docker arguments for the Tenant Home, Workspace, and Extra Mounts.
///
/// Callers must resolve `workspace` and `extra_mounts` with
/// [`resolve_workspace`] and [`resolve_mounts`], then apply
/// [`validate_extra_mount_targets`] and [`validate_aibox_mount_sources`].
/// `home_dir` must also pass [`reject_colon_in_bind_source`]. This function is
/// only a pure argument builder and repeats none of those checks.
pub fn assemble_run_args(workspace: &str, home_dir: &Path, extra_mounts: &[String]) -> Vec<String> {
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

/// Assemble Docker arguments for a non-interactive task that may write only
/// to one Managed Tenant Home.
pub fn assemble_component_run_args(home_dir: &Path) -> Vec<String> {
    let mut args = base_container_args(false);
    args.push("-v".into());
    args.push(format!("{}:{CONTAINER_HOME}", home_dir.display()));
    args.extend(["-w".into(), CONTAINER_HOME.into()]);
    args
}

fn base_container_args(interactive: bool) -> Vec<String> {
    let mut args: Vec<String> = vec!["--rm".into()];
    if interactive {
        args.push(if platform::has_tty() { "-it" } else { "-i" }.into());
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
mod tests {
    use super::*;
    use crate::testutil::contains_pair;
    use std::fs;

    #[test]
    fn resolve_workspace_none_uses_cwd() {
        let got = resolve_workspace(None).unwrap();
        assert_eq!(
            got,
            std::fs::canonicalize(std::env::current_dir().unwrap())
                .unwrap()
                .to_string_lossy()
        );
    }

    #[test]
    fn resolve_workspace_absolutizes_relative_paths() {
        let cwd = std::env::current_dir().unwrap();

        let got = resolve_workspace(Some("src")).unwrap();

        assert_eq!(
            got,
            std::fs::canonicalize(cwd.join("src"))
                .unwrap()
                .to_string_lossy()
        );
    }

    #[test]
    fn resolve_workspace_rejects_missing_and_non_directory_paths() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("file");
        let missing = dir.path().join("missing");
        fs::write(&file, "not a directory\n").unwrap();

        for path in [&file, &missing] {
            let error = resolve_workspace(Some(path.to_str().unwrap()))
                .unwrap_err()
                .to_string();
            assert!(error.contains("workspace is not a directory"), "{error}");
            assert!(
                error.contains(&path.display().to_string()),
                "the rejected path should be identifiable: {error}"
            );
        }
    }

    #[test]
    fn resolve_mounts_absolutizes_and_validates_host_side() {
        let cwd = std::env::current_dir().unwrap();
        let got =
            resolve_mounts(&["src:/src".to_string(), "src:/readonly:ro".to_string()]).unwrap();
        assert_eq!(
            got,
            vec![
                format!(
                    "{}:/src",
                    std::fs::canonicalize(cwd.join("src")).unwrap().display()
                ),
                format!(
                    "{}:/readonly:ro",
                    std::fs::canonicalize(cwd.join("src")).unwrap().display()
                )
            ]
        );

        let error = resolve_mounts(&["/no/such/dir:/data".to_string()])
            .unwrap_err()
            .to_string();
        assert!(error.contains("mount host path does not exist"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn resolved_bind_sources_do_not_leave_symlinks_for_docker() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        let link = dir.path().join("link");
        fs::create_dir(&target).unwrap();
        symlink(&target, &link).unwrap();
        let canonical_target = fs::canonicalize(&target).unwrap();

        assert_eq!(
            resolve_workspace(Some(link.to_str().unwrap())).unwrap(),
            canonical_target.to_string_lossy()
        );
        assert_eq!(
            resolve_mounts(&[format!("{}:/data:ro", link.display())]).unwrap(),
            [format!("{}:/data:ro", canonical_target.display())]
        );
    }

    #[test]
    fn resolve_mounts_rejects_malformed_short_syntax() {
        for (mount, expected) in [
            ("src", "invalid mount"),
            (":/cache", "invalid mount"),
            ("src:", "invalid mount"),
            ("src:relative", "invalid mount"),
            ("src:/cache:", "invalid mount mode"),
            ("src:/cache:rw", "only :ro is supported"),
            ("src:/cache:ro:extra", "invalid mount"),
        ] {
            let err = resolve_mounts(&[mount.to_string()])
                .unwrap_err()
                .to_string();
            assert!(
                err.contains(expected),
                "{mount:?} should fail with {expected:?}, got {err:?}"
            );
        }
    }

    #[test]
    fn extra_mounts_must_not_replace_managed_targets() {
        for target in [
            "/workspace",
            "/workspace/",
            "/workspace/.",
            "/tmp/../workspace",
            "/",
            "/home",
            "/home/aibox",
            "/home/aibox/",
            "/home/aibox/.",
            "/home/aibox/..",
            "/home/aibox/.cache/../..",
        ] {
            let err = validate_extra_mount_targets(&[format!("/host:{target}:ro")])
                .unwrap_err()
                .to_string();
            assert!(err.contains("would override or shadow"));
        }
        validate_extra_mount_targets(&[
            "/host:/legacy-work:ro".to_string(),
            "/host:/workspace-cache:ro".to_string(),
            "/host:/workspace/.cache:ro".to_string(),
            "/host:/home/aibox-cache:ro".to_string(),
            "/host:/home/aibox2:ro".to_string(),
            "/host:/home/aibox/.cache:ro".to_string(),
        ])
        .unwrap();
    }

    #[test]
    fn aibox_mount_sources_only_allow_managed_tenant_home_trees() {
        let root = tempfile::tempdir().unwrap();
        crate::tenant::ManagedTenant::resolve(root.path(), "default")
            .unwrap()
            .ensure_initialized()
            .unwrap();
        let tenant_home = root.path().join("tenants/default");
        let home_child = tenant_home.join("projects/demo");
        let codex_catalog = root.path().join("codex/default");
        let host_catalog = root.path().join("claude/__host");
        fs::create_dir_all(&home_child).unwrap();
        fs::create_dir_all(&codex_catalog).unwrap();
        fs::create_dir_all(&host_catalog).unwrap();

        for rejected in [
            root.path(),
            &root.path().join("tenants"),
            &codex_catalog,
            &root.path().join("codex"),
            &root.path().join("claude"),
            &host_catalog,
            root.path().parent().unwrap(),
        ] {
            let err = validate_aibox_mount_sources(rejected.to_str().unwrap(), &[], root.path())
                .unwrap_err()
                .to_string();
            assert!(err.contains("aibox internal data"), "{rejected:?}: {err}");
        }

        validate_aibox_mount_sources(tenant_home.to_str().unwrap(), &[], root.path()).unwrap();
        validate_aibox_mount_sources(home_child.to_str().unwrap(), &[], root.path()).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn aibox_mount_check_resolves_symlinked_sources() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let links = tempfile::tempdir().unwrap();
        crate::tenant::ManagedTenant::resolve(root.path(), "default")
            .unwrap()
            .ensure_initialized()
            .unwrap();
        let catalog = root.path().join("codex/default");
        fs::create_dir_all(&catalog).unwrap();
        let linked_config = links.path().join("config");
        symlink(&catalog, &linked_config).unwrap();

        let err = validate_aibox_mount_sources(linked_config.to_str().unwrap(), &[], root.path())
            .unwrap_err()
            .to_string();

        assert!(err.contains("aibox internal data"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn aibox_mount_check_rejects_invalid_tenant_collection_entries() {
        let root = tempfile::tempdir().unwrap();
        let invalid_home = root.path().join("tenants/bad_name");
        fs::create_dir_all(&invalid_home).unwrap();

        let err = validate_aibox_mount_sources(invalid_home.to_str().unwrap(), &[], root.path())
            .unwrap_err()
            .to_string();

        assert!(
            err.contains("only Managed Tenant Home trees may be mounted"),
            "{err}"
        );
    }

    #[test]
    fn aibox_mount_check_normalizes_dotdot_sources() {
        let root = tempfile::tempdir().unwrap();
        crate::tenant::ManagedTenant::resolve(root.path(), "default")
            .unwrap()
            .ensure_initialized()
            .unwrap();
        let tenant_home = root.path().join("tenants/default");
        let catalog = root.path().join("codex/default");
        fs::create_dir_all(&catalog).unwrap();

        let workspace = tenant_home.join("..");
        let err = validate_aibox_mount_sources(workspace.to_str().unwrap(), &[], root.path())
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("aibox internal data"),
            "a dotdot Workspace path that resolves to the tenant root must fail: {err}"
        );

        let mount_source = tenant_home.join("../../codex/default");
        let err = validate_aibox_mount_sources(
            tenant_home.to_str().unwrap(),
            &[format!("{}:/secrets:ro", mount_source.display())],
            root.path(),
        )
        .unwrap_err()
        .to_string();
        assert!(
            err.contains("aibox internal data"),
            "a dotdot mount source that resolves into a Named Config catalog must be rejected: {err}"
        );
    }

    #[test]
    fn aibox_mount_check_allows_unrelated_paths() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();

        validate_aibox_mount_sources(
            outside.path().to_str().unwrap(),
            &[format!("{}:/archive:ro", outside.path().display())],
            root.path(),
        )
        .unwrap();
    }

    #[test]
    fn aibox_mount_check_rejects_an_ancestor_before_the_root_exists() {
        let parent = tempfile::tempdir().unwrap();
        let future_root = parent.path().join("future/.aibox");

        let err = validate_aibox_mount_sources(parent.path().to_str().unwrap(), &[], &future_root)
            .unwrap_err()
            .to_string();

        assert!(err.contains("aibox internal data"), "{err}");
        assert!(!future_root.exists());
    }

    #[test]
    fn tenant_home_children_are_valid_extra_mount_sources() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        crate::tenant::ManagedTenant::resolve(root.path(), "work")
            .unwrap()
            .ensure_initialized()
            .unwrap();
        let home_child = root.path().join("tenants/work/projects/demo");
        fs::create_dir_all(&home_child).unwrap();

        validate_aibox_mount_sources(
            outside.path().to_str().unwrap(),
            &[format!("{}:/demo:ro", home_child.display())],
            root.path(),
        )
        .unwrap();
    }

    #[test]
    fn assemble_run_args_keeps_sandbox_flags_and_mount_order() {
        let args = assemble_run_args(
            "/abs/workspace",
            Path::new("/abs/tenant"),
            &["/abs/cache:/cache:ro".to_string()],
        );

        assert_eq!(args.first().map(String::as_str), Some("--rm"));
        assert_eq!(
            args.get(1).map(String::as_str),
            Some(if platform::has_tty() { "-it" } else { "-i" })
        );
        assert!(contains_pair(&args, "--security-opt", "no-new-privileges"));
        assert!(contains_pair(&args, "--cap-drop", "ALL"));
        assert!(contains_pair(&args, "-v", "/abs/tenant:/home/aibox"));
        assert!(contains_pair(&args, "-v", "/abs/workspace:/workspace"));
        assert!(contains_pair(&args, "-v", "/abs/cache:/cache:ro"));
        assert!(contains_pair(&args, "-w", "/workspace"));
        assert!(
            crate::testutil::pair_pos(&args, "-v", "/abs/tenant:/home/aibox")
                < crate::testutil::pair_pos(&args, "-v", "/abs/workspace:/workspace")
        );
        assert!(
            crate::testutil::pair_pos(&args, "-v", "/abs/workspace:/workspace")
                < crate::testutil::pair_pos(&args, "-v", "/abs/cache:/cache:ro")
        );
        assert!(!args.iter().any(|arg| arg == "--env-file"));

        if platform::is_linux() {
            let (uid, gid) = platform::uid_gid();
            assert!(contains_pair(&args, "--user", &format!("{uid}:{gid}")));
            assert!(contains_pair(
                &args,
                "--add-host",
                "host.docker.internal:host-gateway"
            ));
        } else {
            assert!(!args.iter().any(|arg| arg == "--user"));
            assert!(!args.iter().any(|arg| arg == "--add-host"));
        }
    }

    #[test]
    fn seed_home_creates_only_the_selected_agent_state_directory() {
        let home = tempfile::tempdir().unwrap();
        seed_home(AgentKind::Codex, home.path()).unwrap();
        assert!(home.path().join(".codex").is_dir());
        assert!(!home.path().join(".codex/statusline.sh").exists());

        let home = tempfile::tempdir().unwrap();
        seed_home(AgentKind::Claude, home.path()).unwrap();
        assert!(home.path().join(".claude").is_dir());
        assert!(!home.path().join(".claude/statusline.sh").exists());
    }

    #[test]
    fn component_run_args_mount_only_the_tenant_home() {
        let args = assemble_component_run_args(Path::new("/abs/tenant"));
        assert_eq!(args.first().map(String::as_str), Some("--rm"));
        assert!(contains_pair(&args, "--security-opt", "no-new-privileges"));
        assert!(contains_pair(&args, "--cap-drop", "ALL"));
        assert!(contains_pair(&args, "-v", "/abs/tenant:/home/aibox"));
        assert!(contains_pair(&args, "-w", "/home/aibox"));
        assert!(!args.iter().any(|arg| arg.contains("/workspace")));
    }

    #[test]
    fn reject_bind_sources_that_short_syntax_cannot_represent() {
        let parent = tempfile::tempdir().unwrap();
        let colon_dir = parent.path().join("a:b");
        fs::create_dir(&colon_dir).unwrap();
        let err = resolve_workspace(Some(colon_dir.to_str().unwrap()))
            .unwrap_err()
            .to_string();
        assert!(err.contains("contains ':'"));

        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;

            let opaque = PathBuf::from(std::ffi::OsString::from_vec(vec![
                b'w', b'o', b'r', b'k', 0xff,
            ]));
            let err = reject_colon_in_bind_source("mount host", &opaque)
                .unwrap_err()
                .to_string();
            assert!(err.contains("not valid UTF-8"), "{err}");
            assert!(
                err.contains("cannot be represented safely for docker"),
                "{err}"
            );
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn resolve_workspace_rejects_a_symlink_to_a_non_utf8_bind_source() {
        use std::os::unix::ffi::OsStringExt;
        use std::os::unix::fs::symlink;

        let parent = tempfile::tempdir().unwrap();
        let opaque_dir = parent.path().join(std::ffi::OsString::from_vec(vec![
            b'w', b'o', b'r', b'k', 0xff,
        ]));
        let link = parent.path().join("work-link");
        fs::create_dir(&opaque_dir).unwrap();
        symlink(&opaque_dir, &link).unwrap();

        let err = resolve_workspace(Some(link.to_str().unwrap()))
            .unwrap_err()
            .to_string();

        assert!(err.contains("not valid UTF-8"), "{err}");
        assert!(
            err.contains("cannot be represented safely for docker"),
            "{err}"
        );
    }
}
