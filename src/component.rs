//! Optional capabilities derived from native state in a Tenant.
//!
//! Status-line Components can edit native Current Config in a Managed Tenant
//! Home or the Host Home, while runtime Components own Managed Tenant-local
//! executables and SDK directories. There is no Component registry, so
//! inspection derives state directly from native files.

use crate::agent::AgentKind;
use crate::tenant::{self, FileSnapshot, ManagedTenant, Tenant, TenantAgent};
use anyhow::{Context, Result, bail};
use serde_json::{Map, Value, json};
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::str::FromStr;

const CLAUDE_STATUSLINE: &[u8] = include_bytes!("../assets/claude-statusline.sh");
const CLAUDE_STATUSLINE_SCRIPT: &str = "statusline.sh";
const CODEX_STATUSLINE_ITEMS: [&str; 5] = [
    "model-with-reasoning",
    "current-dir",
    "git-branch",
    "context-window-size",
    "context-used",
];
const RUST_INSTALLER: &str = include_str!("../assets/install-rust.sh");
const GO_INSTALLER: &str = include_str!("../assets/install-go.sh");
const NODE_INSTALLER: &str = include_str!("../assets/install-node.sh");
const CODEX_INSTALLER: &str = include_str!("../assets/install-codex.sh");
const CLAUDE_INSTALLER: &str = include_str!("../assets/install-claude.sh");
const PYTHON_INSTALLER: &str = include_str!("../assets/install-python.sh");
const CONTAINER_HOME: &str = "/home/aibox";
// Status-line inspection rewrites native Current Config in memory, so bound
// container- or host-written input before parsing it.
const MAX_CONFIG_BYTES: usize = 16 * 1024 * 1024;

/// One optional capability that aibox can install into a Tenant's native state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComponentKind {
    /// Tenant-local Node.js runtime.
    Node,
    /// Tenant-local OpenAI Codex executable.
    Codex,
    /// Tenant-local Anthropic Claude Code executable.
    Claude,
    /// Tenant-local uv and CPython toolchain.
    Python,
    /// Claude Code status-line integration.
    ClaudeStatusline,
    /// OpenAI Codex status-line integration.
    CodexStatusline,
    /// Tenant-local stable Rust toolchain.
    Rust,
    /// Tenant-local stable Go toolchain.
    Go,
}

impl ComponentKind {
    pub(crate) const ALL: [Self; 8] = [
        Self::Codex,
        Self::CodexStatusline,
        Self::Claude,
        Self::ClaudeStatusline,
        Self::Node,
        Self::Python,
        Self::Rust,
        Self::Go,
    ];
    pub(crate) const STATUSLINES: [Self; 2] = [Self::ClaudeStatusline, Self::CodexStatusline];

    /// Stable Component name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Node => "node",
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Python => "python",
            Self::ClaudeStatusline => "claude-statusline",
            Self::CodexStatusline => "codex-statusline",
            Self::Rust => "rust",
            Self::Go => "go",
        }
    }

    pub(crate) fn supports_version(self) -> bool {
        !self.is_statusline()
    }

    pub(crate) fn is_statusline(self) -> bool {
        matches!(self, Self::ClaudeStatusline | Self::CodexStatusline)
    }

    fn for_agent(agent: AgentKind) -> Self {
        match agent {
            AgentKind::Claude => Self::Claude,
            AgentKind::Codex => Self::Codex,
        }
    }
}

/// State derived from a Component's native files in one Tenant's Home.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ComponentStatus {
    /// The Component exactly matches the current aibox definition.
    Installed {
        /// Stable runtime or toolchain version; absent for status-line Components.
        version: Option<String>,
    },
    /// Some status-line state exists but differs from the current definition.
    Modified,
    /// Recognizable aibox-owned state exists but is only partially installed
    /// or is not healthy enough to run.
    Incomplete,
    /// Component state exists but aibox must not take ownership of it.
    Unmanaged,
    /// No Component-owned state exists.
    NotInstalled,
}

/// Installed Components that own defaults in the Tenant Environment.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct TenantEnvironmentComponents {
    node: bool,
    claude: bool,
    python: bool,
    rust: bool,
    go: bool,
}

impl TenantEnvironmentComponents {
    #[cfg(test)]
    pub(crate) fn for_tests(node: bool, claude: bool, python: bool, rust: bool, go: bool) -> Self {
        Self {
            node,
            claude,
            python,
            rust,
            go,
        }
    }

    pub(crate) fn node(self) -> bool {
        self.node
    }

    pub(crate) fn claude(self) -> bool {
        self.claude
    }

    pub(crate) fn python(self) -> bool {
        self.python
    }

    pub(crate) fn rust(self) -> bool {
        self.rust
    }

    pub(crate) fn go(self) -> bool {
        self.go
    }

    fn mark_installed(&mut self, kind: ComponentKind) {
        match kind {
            ComponentKind::Node => self.node = true,
            ComponentKind::Claude => self.claude = true,
            ComponentKind::Python => self.python = true,
            ComponentKind::Rust => self.rust = true,
            ComponentKind::Go => self.go = true,
            ComponentKind::Codex
            | ComponentKind::ClaudeStatusline
            | ComponentKind::CodexStatusline => {
                unreachable!("Component has no Tenant Environment defaults")
            }
        }
    }
}

#[derive(Debug)]
pub(crate) struct ComponentInspection {
    pub(crate) kind: ComponentKind,
    pub(crate) status: Option<ComponentStatus>,
    pub(crate) error: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StatuslinePartState {
    Absent,
    Current,
    Modified,
}

/// A Component name and optional stable runtime or toolchain version.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComponentSpec {
    /// Selected Component.
    pub kind: ComponentKind,
    /// Requested stable version, or latest stable when absent.
    pub version: Option<String>,
}

impl fmt::Display for ComponentSpec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.kind.name())?;
        if let Some(version) = &self.version {
            write!(formatter, "@{version}")?;
        }
        Ok(())
    }
}

impl FromStr for ComponentSpec {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (name, version) = value
            .split_once('@')
            .map_or((value, None), |(name, version)| (name, Some(version)));
        let kind = name.parse::<ComponentKind>()?;
        if version.is_some() && !kind.supports_version() {
            return Err(format!("{} does not accept a version", kind.name()));
        }
        let version = version.map(validate_stable_version).transpose()?;
        Ok(Self { kind, version })
    }
}

impl FromStr for ComponentKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "node" => Ok(Self::Node),
            "codex" => Ok(Self::Codex),
            "claude" => Ok(Self::Claude),
            "python" => Ok(Self::Python),
            "claude-statusline" => Ok(Self::ClaudeStatusline),
            "codex-statusline" => Ok(Self::CodexStatusline),
            "rust" => Ok(Self::Rust),
            "go" => Ok(Self::Go),
            _ => Err(format!("unknown Component {value:?}")),
        }
    }
}

fn validate_stable_version(version: &str) -> Result<String, String> {
    let parts: Vec<_> = version.split('.').collect();
    if parts.len() != 3
        || parts.iter().any(|part| {
            part.is_empty()
                || !part.bytes().all(|byte| byte.is_ascii_digit())
                || (part.len() > 1 && part.starts_with('0'))
        })
    {
        return Err(format!(
            "invalid stable Component version {version:?}; expected X.Y.Z"
        ));
    }
    Ok(version.to_string())
}

pub(crate) fn inspect_catalog(selected: &Tenant) -> Result<Vec<ComponentInspection>> {
    let exists = tenant_home_exists(selected)?;
    Ok(component_catalog(selected)
        .iter()
        .copied()
        .map(|kind| {
            if !exists {
                return ComponentInspection {
                    kind,
                    status: Some(ComponentStatus::NotInstalled),
                    error: None,
                };
            }
            match inspect(kind, selected.home_dir()) {
                Ok(status) => ComponentInspection {
                    kind,
                    status: Some(status),
                    error: None,
                },
                Err(error) => ComponentInspection {
                    kind,
                    status: None,
                    error: Some(format!("{error:#}")),
                },
            }
        })
        .collect())
}

/// Snapshot healthy Components that own Tenant Environment defaults.
///
/// Inspection failures are returned as warnings rather than failing the
/// caller, so an unrelated damaged Component cannot block a Run or Debug
/// Shell. Recognized non-installed states are intentionally quiet.
pub(crate) fn inspect_tenant_environment_components(
    home: &Path,
) -> (TenantEnvironmentComponents, Vec<String>) {
    let mut installed = TenantEnvironmentComponents::default();
    let mut warnings = Vec::new();
    for kind in [
        ComponentKind::Node,
        ComponentKind::Claude,
        ComponentKind::Python,
        ComponentKind::Rust,
        ComponentKind::Go,
    ] {
        match inspect(kind, home) {
            Ok(ComponentStatus::Installed { .. }) => installed.mark_installed(kind),
            Ok(
                ComponentStatus::Modified
                | ComponentStatus::Incomplete
                | ComponentStatus::Unmanaged
                | ComponentStatus::NotInstalled,
            ) => {}
            Err(error) => warnings.push(format!(
                "could not inspect {} Component; skipping its environment defaults: {error}",
                kind.name()
            )),
        }
    }
    (installed, warnings)
}

/// Require the selected Coding Agent's Tenant-local executable before a Run.
pub(crate) fn require_agent_component(agent: AgentKind, home: &Path) -> Result<()> {
    let kind = ComponentKind::for_agent(agent);
    match inspect(kind, home)? {
        ComponentStatus::Installed { .. } => Ok(()),
        ComponentStatus::NotInstalled => bail!(
            "{} Component is not installed for this Managed Tenant; install it from Console Tenants > Components",
            kind.name()
        ),
        ComponentStatus::Incomplete => bail!(
            "{} Component is incomplete for this Managed Tenant; repair it from Console Tenants > Components",
            kind.name()
        ),
        ComponentStatus::Unmanaged => bail!(
            "{} has unmanaged executable state for this Managed Tenant; resolve it from Console Tenants > Components",
            kind.name()
        ),
        ComponentStatus::Modified => unreachable!("runtime Components never report modified"),
    }
}

pub(crate) fn install_component(selected: &Tenant, component: &ComponentSpec) -> Result<i32> {
    install(selected, component)
}

pub(crate) fn install_component_for_service(
    selected: &Tenant,
    component: &ComponentSpec,
    log: crate::docker::LogCallback,
) -> Result<i32> {
    reject_host_runtime_component(selected, component.kind)?;
    match component.kind {
        ComponentKind::ClaudeStatusline => install_claude_statusline(selected),
        ComponentKind::CodexStatusline => install_codex_statusline(selected),
        ComponentKind::Node
        | ComponentKind::Codex
        | ComponentKind::Claude
        | ComponentKind::Python
        | ComponentKind::Rust
        | ComponentKind::Go => {
            let Tenant::Managed(tenant) = selected else {
                unreachable!("Host runtime Components are rejected above")
            };
            install_runtime_component_with_mode(
                tenant,
                component,
                &crate::docker::DockerCli::system(),
                Some(log),
            )
        }
    }
}

pub(crate) fn remove_component(selected: &Tenant, kind: ComponentKind) -> Result<i32> {
    reject_host_runtime_component(selected, kind)?;
    if !tenant_home_exists(selected)? {
        if matches!(selected, Tenant::Host { .. }) {
            bail!(
                "Host Home does not exist: {}",
                selected.home_dir().display()
            );
        }
        return Ok(0);
    }
    let status = inspect(kind, selected.home_dir())?;
    if status == ComponentStatus::NotInstalled {
        return Ok(0);
    }
    if status == ComponentStatus::Unmanaged {
        bail!(
            "{} has unmanaged Component state; refusing to remove foreign files",
            kind.name()
        );
    }
    match kind {
        ComponentKind::Node => remove_node(selected.home_dir())?,
        ComponentKind::Codex => remove_codex(selected.home_dir())?,
        ComponentKind::Claude => remove_claude(selected.home_dir())?,
        ComponentKind::Python => remove_python(selected.home_dir())?,
        ComponentKind::ClaudeStatusline => remove_claude_statusline(selected)?,
        ComponentKind::CodexStatusline => remove_codex_statusline(selected)?,
        ComponentKind::Rust => remove_rust(selected.home_dir())?,
        ComponentKind::Go => {
            tenant::remove_real_dir_if_exists(&selected.home_dir().join(".goroot"), "Go root")?;
        }
    }
    Ok(0)
}

fn install(selected: &Tenant, component: &ComponentSpec) -> Result<i32> {
    reject_host_runtime_component(selected, component.kind)?;
    match component.kind {
        ComponentKind::ClaudeStatusline => install_claude_statusline(selected),
        ComponentKind::CodexStatusline => install_codex_statusline(selected),
        ComponentKind::Node
        | ComponentKind::Codex
        | ComponentKind::Claude
        | ComponentKind::Python
        | ComponentKind::Rust
        | ComponentKind::Go => {
            let Tenant::Managed(tenant) = selected else {
                unreachable!("Host runtime Components are rejected above")
            };
            install_runtime_component(tenant, component)
        }
    }
}

fn install_runtime_component(tenant: &ManagedTenant, component: &ComponentSpec) -> Result<i32> {
    install_runtime_component_with(tenant, component, &crate::docker::DockerCli::system())
}

fn component_catalog(selected: &Tenant) -> &'static [ComponentKind] {
    match selected {
        Tenant::Managed(_) => &ComponentKind::ALL,
        Tenant::Host { .. } => &ComponentKind::STATUSLINES,
    }
}

fn tenant_home_exists(selected: &Tenant) -> Result<bool> {
    match selected {
        Tenant::Managed(tenant) => tenant.exists(),
        Tenant::Host { home_dir, .. } => tenant::real_dir_exists(home_dir, "Host Home"),
    }
}

fn reject_host_runtime_component(selected: &Tenant, kind: ComponentKind) -> Result<()> {
    if matches!(selected, Tenant::Host { .. }) && !kind.is_statusline() {
        bail!(
            "{} is unavailable to the Host Tenant; it supports only claude-statusline and codex-statusline",
            kind.name()
        );
    }
    Ok(())
}

fn statusline_status_from_parts(
    first: StatuslinePartState,
    second: StatuslinePartState,
) -> ComponentStatus {
    match (first, second) {
        (StatuslinePartState::Absent, StatuslinePartState::Absent) => ComponentStatus::NotInstalled,
        (StatuslinePartState::Current, StatuslinePartState::Current) => {
            ComponentStatus::Installed { version: None }
        }
        (StatuslinePartState::Current, StatuslinePartState::Absent)
        | (StatuslinePartState::Absent, StatuslinePartState::Current) => {
            ComponentStatus::Incomplete
        }
        _ => ComponentStatus::Modified,
    }
}

fn inspect(kind: ComponentKind, home: &Path) -> Result<ComponentStatus> {
    match kind {
        ComponentKind::Node => inspect_node(home),
        ComponentKind::Codex => inspect_codex(home),
        ComponentKind::Claude => inspect_claude(home),
        ComponentKind::Python => inspect_python(home),
        ComponentKind::ClaudeStatusline => inspect_claude_statusline(home),
        ComponentKind::CodexStatusline => inspect_codex_statusline(home),
        ComponentKind::Rust => inspect_rust(home),
        ComponentKind::Go => inspect_go(home),
    }
}

fn inspect_claude_statusline(home: &Path) -> Result<ComponentStatus> {
    let dir = home.join(AgentKind::Claude.state_dir_name());
    if !tenant::real_dir_exists(&dir, "Claude state directory")? {
        return Ok(ComponentStatus::NotInstalled);
    }
    let script = capture_limited(
        &dir.join(CLAUDE_STATUSLINE_SCRIPT),
        "Claude status-line script",
    )?;
    let settings = capture_limited(
        &dir.join(AgentKind::Claude.main_config_file()),
        "Claude settings",
    )?;
    Ok(statusline_status_from_parts(
        claude_statusline_script_state(&script),
        claude_statusline_setting_state(&settings)?,
    ))
}

fn claude_statusline_script_state(script: &FileSnapshot) -> StatuslinePartState {
    if !script.present {
        StatuslinePartState::Absent
    } else if script.content == CLAUDE_STATUSLINE && executable_mode_is_current(script.mode) {
        StatuslinePartState::Current
    } else {
        StatuslinePartState::Modified
    }
}

fn claude_statusline_setting_state(settings: &FileSnapshot) -> Result<StatuslinePartState> {
    let object = parse_json_config(settings, "Claude settings")?;
    let expected = json!({
        "type": "command",
        "command": "bash ~/.claude/statusline.sh"
    });
    Ok(match object.get("statusLine") {
        Some(value) if value == &expected => StatuslinePartState::Current,
        Some(_) => StatuslinePartState::Modified,
        None => StatuslinePartState::Absent,
    })
}

fn inspect_codex_statusline(home: &Path) -> Result<ComponentStatus> {
    let dir = home.join(AgentKind::Codex.state_dir_name());
    if !tenant::real_dir_exists(&dir, "Codex state directory")? {
        return Ok(ComponentStatus::NotInstalled);
    }
    let config = capture_limited(
        &dir.join(AgentKind::Codex.main_config_file()),
        "Codex configuration",
    )?;
    codex_statusline_setting(&config)
}

fn codex_statusline_setting(config: &FileSnapshot) -> Result<ComponentStatus> {
    if !config.present || config.content.iter().all(u8::is_ascii_whitespace) {
        return Ok(ComponentStatus::NotInstalled);
    }
    let content =
        std::str::from_utf8(&config.content).context("Codex configuration is not UTF-8")?;
    let document = content
        .parse::<toml_edit::DocumentMut>()
        .context("parse Codex configuration")?;
    let Some(tui) = document.get("tui").and_then(toml_edit::Item::as_table_like) else {
        return Ok(ComponentStatus::NotInstalled);
    };
    let status_line = match tui.get("status_line") {
        None => StatuslinePartState::Absent,
        Some(item)
            if item.as_array().is_some_and(|array| {
                let actual: Option<Vec<_>> = array.iter().map(toml_edit::Value::as_str).collect();
                actual.as_deref() == Some(CODEX_STATUSLINE_ITEMS.as_slice())
            }) =>
        {
            StatuslinePartState::Current
        }
        Some(_) => StatuslinePartState::Modified,
    };
    let colors = match tui.get("status_line_use_colors") {
        None => StatuslinePartState::Absent,
        Some(item) if item.as_bool() == Some(false) => StatuslinePartState::Current,
        Some(_) => StatuslinePartState::Modified,
    };
    Ok(statusline_status_from_parts(status_line, colors))
}

#[derive(Debug, Eq, PartialEq)]
enum LinkState {
    Absent,
    Symlink(PathBuf),
    Other,
}

fn inspect_node(home: &Path) -> Result<ComponentStatus> {
    let root = home.join(".node");
    if !tenant::real_dir_exists(&root, "Node.js root")? {
        return Ok(ComponentStatus::NotInstalled);
    }
    let releases = root.join("releases");
    if !tenant::real_dir_exists(&releases, "Node.js release collection")? {
        return Ok(ComponentStatus::Incomplete);
    }
    let current = root.join("current");
    let target = match link_state(&current, "Node.js current release")? {
        LinkState::Absent => return Ok(ComponentStatus::Incomplete),
        LinkState::Other => return Ok(ComponentStatus::Unmanaged),
        LinkState::Symlink(target) => target,
    };
    let Some(target) = map_home_symlink_target(home, &current, &target) else {
        return Ok(ComponentStatus::Unmanaged);
    };
    let Some(name) = one_relative_component(&target, &releases) else {
        return Ok(ComponentStatus::Unmanaged);
    };
    let Some(version) = name
        .strip_prefix('v')
        .and_then(|value| validate_stable_version(value).ok())
    else {
        return Ok(ComponentStatus::Unmanaged);
    };
    let release = releases.join(&name);
    if !tenant::real_dir_exists(&release, "Node.js release")? {
        return Ok(ComponentStatus::Incomplete);
    }
    let bin = release.join("bin");
    if !tenant::real_dir_exists(&bin, "Node.js binary directory")?
        || !executable_file_exists(&bin.join("node"), "Node.js executable")?
        || !safe_file_exists_under(&bin.join("npm"), &release, "npm launcher")?
    {
        return Ok(ComponentStatus::Incomplete);
    }
    Ok(ComponentStatus::Installed {
        version: Some(version),
    })
}

fn inspect_codex(home: &Path) -> Result<ComponentStatus> {
    let launcher = home.join(".local/bin/codex");
    let standalone = home.join(".codex/packages/standalone");
    let launcher_state = local_launcher_state(home, "codex", "Codex launcher")?;
    let standalone_exists = codex_standalone_exists(home, &standalone)?;
    if launcher_state == LinkState::Absent && !standalone_exists {
        return Ok(ComponentStatus::NotInstalled);
    }
    let launcher_target = match launcher_state {
        LinkState::Symlink(target) => target,
        LinkState::Other => return Ok(ComponentStatus::Unmanaged),
        LinkState::Absent => return Ok(ComponentStatus::Incomplete),
    };
    if !standalone_exists {
        return Ok(ComponentStatus::Incomplete);
    }

    let current = standalone.join("current");
    let current_target = match link_state(&current, "Codex current release")? {
        LinkState::Absent => return Ok(ComponentStatus::Incomplete),
        LinkState::Other => return Ok(ComponentStatus::Unmanaged),
        LinkState::Symlink(target) => target,
    };
    let Some(current_target) = map_home_symlink_target(home, &current, &current_target) else {
        return Ok(ComponentStatus::Unmanaged);
    };
    let releases = standalone.join("releases");
    if !tenant::real_dir_exists(&releases, "Codex release collection")? {
        return Ok(ComponentStatus::Incomplete);
    }
    let Some(release_name) = one_relative_component(&current_target, &releases) else {
        return Ok(ComponentStatus::Unmanaged);
    };
    let Some(version) = codex_release_version(&release_name) else {
        return Ok(ComponentStatus::Unmanaged);
    };
    let release = releases.join(&release_name);
    if !tenant::real_dir_exists(&release, "Codex release")? {
        return Ok(ComponentStatus::Incomplete);
    }

    let Some(launcher_target) = map_home_symlink_target(home, &launcher, &launcher_target) else {
        return Ok(ComponentStatus::Unmanaged);
    };
    let package_launcher = standalone.join("current/bin/codex");
    let legacy_launcher = standalone.join("current/codex");
    let release_executable = if launcher_target == package_launcher {
        release.join("bin/codex")
    } else if launcher_target == legacy_launcher {
        release.join("codex")
    } else {
        return Ok(ComponentStatus::Unmanaged);
    };
    if !executable_file_exists(&release_executable, "Codex executable")? {
        return Ok(ComponentStatus::Incomplete);
    }
    Ok(ComponentStatus::Installed {
        version: Some(version),
    })
}

fn inspect_claude(home: &Path) -> Result<ComponentStatus> {
    let launcher = home.join(".local/bin/claude");
    let versions = home.join(".local/share/claude/versions");
    let launcher_state = local_launcher_state(home, "claude", "Claude launcher")?;
    let versions_exist = claude_versions_exist(home, &versions)?;
    if launcher_state == LinkState::Absent && !versions_exist {
        return Ok(ComponentStatus::NotInstalled);
    }
    let target = match launcher_state {
        LinkState::Symlink(target) => target,
        LinkState::Other => return Ok(ComponentStatus::Unmanaged),
        LinkState::Absent => return Ok(ComponentStatus::Incomplete),
    };
    if !versions_exist {
        return Ok(ComponentStatus::Incomplete);
    }
    let Some(target) = map_home_symlink_target(home, &launcher, &target) else {
        return Ok(ComponentStatus::Unmanaged);
    };
    let Some(version) = one_relative_component(&target, &versions)
        .and_then(|value| validate_stable_version(&value).ok())
    else {
        return Ok(ComponentStatus::Unmanaged);
    };
    if !executable_file_exists(&versions.join(&version), "Claude executable")? {
        return Ok(ComponentStatus::Incomplete);
    }
    Ok(ComponentStatus::Installed {
        version: Some(version),
    })
}

fn codex_standalone_exists(home: &Path, standalone: &Path) -> Result<bool> {
    let packages = home.join(".codex/packages");
    if !tenant::real_dir_exists(&home.join(".codex"), "Codex state directory")?
        || !tenant::real_dir_exists(&packages, "Codex package directory")?
    {
        return Ok(false);
    }
    tenant::real_dir_exists(standalone, "Codex standalone package")
}

fn claude_versions_exist(home: &Path, versions: &Path) -> Result<bool> {
    let local = home.join(".local");
    let share = local.join("share");
    let claude = share.join("claude");
    if !tenant::real_dir_exists(&local, "Tenant-local data directory")?
        || !tenant::real_dir_exists(&share, "Tenant-local shared data directory")?
        || !tenant::real_dir_exists(&claude, "Claude data directory")?
    {
        return Ok(false);
    }
    tenant::real_dir_exists(versions, "Claude version collection")
}

fn link_state(path: &Path, label: &str) -> Result<LinkState> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Ok(LinkState::Symlink(
            fs::read_link(path).with_context(|| format!("read {label} {}", path.display()))?,
        )),
        Ok(_) => Ok(LinkState::Other),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(LinkState::Absent),
        Err(error) => Err(error).with_context(|| format!("inspect {label} {}", path.display())),
    }
}

fn local_launcher_state(home: &Path, name: &str, label: &str) -> Result<LinkState> {
    let local = home.join(".local");
    if !tenant::real_dir_exists(&local, "Tenant-local data directory")? {
        return Ok(LinkState::Absent);
    }
    let bin = local.join("bin");
    if !tenant::real_dir_exists(&bin, "Tenant-local binary directory")? {
        return Ok(LinkState::Absent);
    }
    link_state(&bin.join(name), label)
}

fn map_home_symlink_target(home: &Path, link: &Path, target: &Path) -> Option<PathBuf> {
    let mapped = if target.is_absolute() {
        if let Ok(relative) = target.strip_prefix(CONTAINER_HOME) {
            home.join(relative)
        } else if target.starts_with(home) {
            target.to_path_buf()
        } else {
            return None;
        }
    } else {
        link.parent()?.join(target)
    };
    normalize_absolute_path(&mapped).filter(|path| path.starts_with(home))
}

fn normalize_absolute_path(path: &Path) -> Option<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::Prefix(_) | std::path::Component::RootDir => {
                normalized.push(component.as_os_str());
            }
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !normalized.pop() {
                    return None;
                }
            }
            std::path::Component::Normal(_) => normalized.push(component.as_os_str()),
        }
    }
    Some(normalized)
}

fn one_relative_component(path: &Path, root: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    let mut components = relative.components();
    let std::path::Component::Normal(name) = components.next()? else {
        return None;
    };
    if components.next().is_some() {
        return None;
    }
    name.to_str().map(str::to_owned)
}

fn codex_release_version(name: &str) -> Option<String> {
    for suffix in [
        "-x86_64-unknown-linux-musl",
        "-aarch64-unknown-linux-musl",
        "-x86_64-unknown-linux-gnu",
        "-aarch64-unknown-linux-gnu",
    ] {
        if let Some(version) = name.strip_suffix(suffix) {
            return validate_stable_version(version).ok();
        }
    }
    None
}

fn safe_file_exists_under(path: &Path, root: &Path, label: &str) -> Result<bool> {
    match fs::canonicalize(path) {
        Ok(resolved) => {
            let resolved_root = fs::canonicalize(root)
                .with_context(|| format!("resolve {label} root {}", root.display()))?;
            if !resolved.starts_with(&resolved_root) {
                bail!("{label} escapes its Component release: {}", path.display());
            }
            Ok(fs::metadata(&resolved)?.file_type().is_file())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).with_context(|| format!("resolve {label} {}", path.display())),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PythonLauncherState {
    Absent,
    Owned,
    Repairable,
    Foreign,
}

fn inspect_python(home: &Path) -> Result<ComponentStatus> {
    let root = home.join(".python");
    let root_exists = match real_directory_entry(&root, "Python toolchain root")? {
        Some(true) => true,
        Some(false) => return Ok(ComponentStatus::Unmanaged),
        None => false,
    };

    let mut launcher_names = vec![
        "uv".to_string(),
        "uvx".to_string(),
        "python".to_string(),
        "python3".to_string(),
        "pip".to_string(),
        "pip3".to_string(),
    ];
    launcher_names.extend(python_versioned_launcher_names(home)?);
    launcher_names.sort();
    launcher_names.dedup();

    let launcher_states = launcher_names
        .iter()
        .map(|name| python_launcher_state(home, name))
        .collect::<Result<Vec<_>>>()?;
    if launcher_states.contains(&PythonLauncherState::Foreign) {
        return Ok(ComponentStatus::Unmanaged);
    }
    let has_owned_launcher = launcher_states.contains(&PythonLauncherState::Owned);
    if !root_exists {
        return Ok(if has_owned_launcher {
            ComponentStatus::Incomplete
        } else {
            ComponentStatus::NotInstalled
        });
    }

    let uv_releases = root.join("uv/releases");
    let python_releases = root.join("cpython/releases");
    let generations = root.join("generations");
    let python_bin = root.join("bin");
    for (path, label) in [
        (&uv_releases, "uv release collection"),
        (&python_releases, "CPython release collection"),
        (&generations, "Python generation collection"),
        (&python_bin, "uv Python launcher directory"),
    ] {
        match real_directory_entry(path, label)? {
            Some(true) => {}
            Some(false) => return Ok(ComponentStatus::Unmanaged),
            None => return Ok(ComponentStatus::Incomplete),
        }
    }

    let current = root.join("current");
    let current_target = match link_state(&current, "Python current generation")? {
        LinkState::Absent => return Ok(ComponentStatus::Incomplete),
        LinkState::Other => return Ok(ComponentStatus::Unmanaged),
        LinkState::Symlink(target) => target,
    };
    let Some(current_target) = map_home_symlink_target(home, &current, &current_target) else {
        return Ok(ComponentStatus::Unmanaged);
    };
    let Some(generation_name) = one_relative_component(&current_target, &generations) else {
        return Ok(ComponentStatus::Unmanaged);
    };
    let Some((python_version, uv_version, platform)) = python_generation_versions(&generation_name)
    else {
        return Ok(ComponentStatus::Unmanaged);
    };
    if Some(platform.as_str()) != expected_python_platform() {
        return Ok(ComponentStatus::Unmanaged);
    }

    let generation = generations.join(&generation_name);
    match real_directory_entry(&generation, "active Python generation")? {
        Some(true) => {}
        Some(false) => return Ok(ComponentStatus::Unmanaged),
        None => return Ok(ComponentStatus::Incomplete),
    }
    let generation_bin = generation.join("bin");
    match real_directory_entry(&generation_bin, "active Python generation binaries")? {
        Some(true) => {}
        Some(false) => return Ok(ComponentStatus::Unmanaged),
        None => return Ok(ComponentStatus::Incomplete),
    }

    let uv_release = uv_releases.join(format!("v{uv_version}"));
    match real_directory_entry(&uv_release, "active uv release")? {
        Some(true) => {}
        Some(false) => return Ok(ComponentStatus::Unmanaged),
        None => return Ok(ComponentStatus::Incomplete),
    }
    for name in ["uv", "uvx"] {
        let path = generation_bin.join(name);
        let target = match link_state(&path, "Python generation uv launcher")? {
            LinkState::Symlink(target) => target,
            LinkState::Absent => return Ok(ComponentStatus::Incomplete),
            LinkState::Other => return Ok(ComponentStatus::Unmanaged),
        };
        let Some(target) = map_home_symlink_target(home, &path, &target) else {
            return Ok(ComponentStatus::Unmanaged);
        };
        if target != uv_release.join(name) {
            return Ok(ComponentStatus::Unmanaged);
        }
        if !executable_file_exists(&target, "active uv executable")? {
            return Ok(ComponentStatus::Incomplete);
        }
    }

    let python_path = generation_bin.join("python");
    let python_target = match link_state(&python_path, "active Python executable")? {
        LinkState::Symlink(target) => target,
        LinkState::Absent => return Ok(ComponentStatus::Incomplete),
        LinkState::Other => return Ok(ComponentStatus::Unmanaged),
    };
    let Some(python_target) = map_home_symlink_target(home, &python_path, &python_target) else {
        return Ok(ComponentStatus::Unmanaged);
    };
    if python_release_version_for_executable(&python_target, &python_releases, &platform).as_deref()
        != Some(&python_version)
    {
        return Ok(ComponentStatus::Unmanaged);
    }
    if !executable_file_exists(&python_target, "active CPython executable")? {
        return Ok(ComponentStatus::Incomplete);
    }

    let minor = python_version
        .rsplit_once('.')
        .map(|(minor, _)| minor)
        .context("validated Python version has no patch component")?;
    match real_file_entry(
        &generation.join("pyvenv.cfg"),
        "Python generation venv marker",
    )? {
        Some(true) => {}
        Some(false) => return Ok(ComponentStatus::Unmanaged),
        None => return Ok(ComponentStatus::Incomplete),
    }
    let pip_package = generation
        .join("lib")
        .join(format!("python{minor}"))
        .join("site-packages/pip");
    match real_directory_entry(&pip_package, "Python generation pip package")? {
        Some(true) => {}
        Some(false) => return Ok(ComponentStatus::Unmanaged),
        None => return Ok(ComponentStatus::Incomplete),
    }
    for name in ["python3", &format!("python{minor}")] {
        let path = generation_bin.join(name);
        let target = match link_state(&path, "Python generation launcher")? {
            LinkState::Symlink(target) => target,
            LinkState::Absent => return Ok(ComponentStatus::Incomplete),
            LinkState::Other => return Ok(ComponentStatus::Unmanaged),
        };
        if map_home_symlink_target(home, &path, &target).as_ref() != Some(&python_target) {
            return Ok(ComponentStatus::Unmanaged);
        }
    }
    for name in ["pip", "pip3"] {
        if !executable_file_exists(&generation_bin.join(name), "pip launcher")? {
            return Ok(ComponentStatus::Incomplete);
        }
    }

    for name in [
        "uv".to_string(),
        "uvx".to_string(),
        "python".to_string(),
        "python3".to_string(),
        format!("python{minor}"),
        "pip".to_string(),
        "pip3".to_string(),
    ] {
        if python_launcher_state(home, &name)? != PythonLauncherState::Owned {
            return Ok(ComponentStatus::Incomplete);
        }
    }
    let active_versioned_launcher = format!("python{minor}");
    for name in launcher_names
        .iter()
        .filter(|name| name.starts_with("python3.") && name.as_str() != active_versioned_launcher)
    {
        let path = generation_bin.join(name);
        let target = match link_state(&path, "historical Python generation launcher")? {
            LinkState::Symlink(target) => target,
            LinkState::Absent => return Ok(ComponentStatus::Incomplete),
            LinkState::Other => return Ok(ComponentStatus::Unmanaged),
        };
        let Some(target) = map_home_symlink_target(home, &path, &target) else {
            return Ok(ComponentStatus::Unmanaged);
        };
        let Some(version) =
            python_release_version_for_executable(&target, &python_releases, &platform)
        else {
            return Ok(ComponentStatus::Unmanaged);
        };
        if format!("python{}", version.rsplit_once('.').unwrap().0) != *name {
            return Ok(ComponentStatus::Unmanaged);
        }
        if !executable_file_exists(&target, "historical CPython executable")? {
            return Ok(ComponentStatus::Incomplete);
        }
    }

    Ok(ComponentStatus::Installed {
        version: Some(python_version),
    })
}

fn real_directory_entry(path: &Path, label: &str) -> Result<Option<bool>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata.file_type().is_dir())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("inspect {label} {}", path.display())),
    }
}

fn real_file_entry(path: &Path, label: &str) -> Result<Option<bool>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata.file_type().is_file())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("inspect {label} {}", path.display())),
    }
}

fn python_versioned_launcher_names(home: &Path) -> Result<Vec<String>> {
    let local = home.join(".local");
    if real_directory_entry(&local, "Tenant-local data directory")? != Some(true) {
        return Ok(Vec::new());
    }
    let bin = local.join("bin");
    if real_directory_entry(&bin, "Tenant-local binary directory")? != Some(true) {
        return Ok(Vec::new());
    }
    let mut names = Vec::new();
    for entry in fs::read_dir(&bin)
        .with_context(|| format!("list Tenant-local binary directory {}", bin.display()))?
    {
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if name.strip_prefix("python3.").is_some_and(|minor| {
            !minor.is_empty() && minor.bytes().all(|byte| byte.is_ascii_digit())
        }) {
            names.push(name);
        }
    }
    Ok(names)
}

fn python_launcher_state(home: &Path, name: &str) -> Result<PythonLauncherState> {
    let local = home.join(".local");
    match real_directory_entry(&local, "Tenant-local data directory")? {
        None => return Ok(PythonLauncherState::Absent),
        Some(false) => return Ok(PythonLauncherState::Foreign),
        Some(true) => {}
    }
    let bin = local.join("bin");
    match real_directory_entry(&bin, "Tenant-local binary directory")? {
        None => return Ok(PythonLauncherState::Absent),
        Some(false) => return Ok(PythonLauncherState::Foreign),
        Some(true) => {}
    }
    let launcher = bin.join(name);
    let metadata = match fs::symlink_metadata(&launcher) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(PythonLauncherState::Absent);
        }
        Err(error) => {
            return Err(error).with_context(|| {
                format!("inspect Python toolchain launcher {}", launcher.display())
            });
        }
        Ok(metadata) => metadata,
    };
    let wrapper = python_launcher_wrapper(name);
    if metadata.file_type().is_file() {
        let snapshot = capture_limited(&launcher, "Python toolchain launcher")?;
        return Ok(
            if wrapper.as_deref() == Some(snapshot.content.as_slice())
                && executable_mode_is_current(snapshot.mode)
            {
                PythonLauncherState::Owned
            } else {
                PythonLauncherState::Foreign
            },
        );
    }
    if !metadata.file_type().is_symlink() {
        return Ok(PythonLauncherState::Foreign);
    }
    let target = fs::read_link(&launcher)
        .with_context(|| format!("read Python toolchain launcher {}", launcher.display()))?;
    let Some(target) = map_home_symlink_target(home, &launcher, &target) else {
        return Ok(PythonLauncherState::Foreign);
    };
    let expected = home.join(".python/current/bin").join(name);
    Ok(if target == expected {
        if wrapper.is_some() {
            PythonLauncherState::Repairable
        } else {
            PythonLauncherState::Owned
        }
    } else {
        PythonLauncherState::Foreign
    })
}

fn python_launcher_wrapper(name: &str) -> Option<Vec<u8>> {
    let versioned = name
        .strip_prefix("python3.")
        .is_some_and(|minor| !minor.is_empty() && minor.bytes().all(|byte| byte.is_ascii_digit()));
    if name != "python" && name != "python3" && !versioned {
        return None;
    }
    Some(
        format!(
            "#!/usr/bin/env bash\nset -euo pipefail\nexec \"$HOME/.python/current/bin/{name}\" \"$@\"\n"
        )
        .into_bytes(),
    )
}

fn python_generation_versions(name: &str) -> Option<(String, String, String)> {
    let mut parts = name.split("__");
    let python = parts.next()?.strip_prefix("python-")?;
    let uv = parts.next()?.strip_prefix("uv-")?;
    let platform = parts.next()?;
    let nonce = parts.next()?;
    if parts.next().is_some()
        || !nonce.split_once('-').is_some_and(|(left, right)| {
            !left.is_empty()
                && !right.is_empty()
                && left.bytes().all(|byte| byte.is_ascii_digit())
                && right.bytes().all(|byte| byte.is_ascii_digit())
        })
    {
        return None;
    }
    Some((
        validate_stable_version(python).ok()?,
        validate_stable_version(uv).ok()?,
        platform.to_string(),
    ))
}

fn expected_python_platform() -> Option<&'static str> {
    match std::env::consts::ARCH {
        "x86_64" => Some("x86_64-unknown-linux-gnu"),
        "aarch64" => Some("aarch64-unknown-linux-gnu"),
        _ => None,
    }
}

fn python_release_version_for_executable(
    executable: &Path,
    releases: &Path,
    platform: &str,
) -> Option<String> {
    let relative = match executable.strip_prefix(releases) {
        Ok(relative) => relative,
        Err(_) => return None,
    };
    let mut parts = relative.components();
    let (
        Some(std::path::Component::Normal(release)),
        Some(std::path::Component::Normal(bin)),
        Some(std::path::Component::Normal(executable_name)),
    ) = (parts.next(), parts.next(), parts.next())
    else {
        return None;
    };
    if parts.next().is_some()
        || bin != "bin"
        || !executable_name.to_string_lossy().starts_with("python")
    {
        return None;
    }
    let architecture = match platform {
        "x86_64-unknown-linux-gnu" => "x86_64",
        "aarch64-unknown-linux-gnu" => "aarch64",
        _ => return None,
    };
    let release = release.to_str()?;
    let version = release
        .strip_prefix("cpython-")?
        .strip_suffix(&format!("-linux-{architecture}-gnu"))?;
    validate_stable_version(version).ok()
}

fn inspect_rust(home: &Path) -> Result<ComponentStatus> {
    let rustup_home = home.join(".rustup");
    if !tenant::real_dir_exists(&rustup_home, "Rustup Home")? {
        return Ok(ComponentStatus::NotInstalled);
    }
    let settings = capture_limited(&rustup_home.join("settings.toml"), "rustup settings")?;
    if !settings.present {
        return Ok(ComponentStatus::Incomplete);
    }
    let content =
        std::str::from_utf8(&settings.content).context("rustup settings are not UTF-8")?;
    let value: Value = toml_edit::de::from_str(content).context("parse rustup settings")?;
    let Some(toolchain) = value.get("default_toolchain").and_then(Value::as_str) else {
        return Ok(ComponentStatus::Unmanaged);
    };
    let Some(version) = stable_version_prefix(toolchain) else {
        return Ok(ComponentStatus::Unmanaged);
    };

    let cargo_home = home.join(".cargo");
    let cargo_exists = tenant::real_dir_exists(&cargo_home, "Cargo Home")?;
    let cargo_bin_exists =
        cargo_exists && tenant::real_dir_exists(&cargo_home.join("bin"), "Cargo binary directory")?;
    let rustup_exists = cargo_bin_exists
        && executable_file_exists(&cargo_home.join("bin/rustup"), "rustup executable")?;

    let toolchains = rustup_home.join("toolchains");
    let toolchains_exist = tenant::real_dir_exists(&toolchains, "Rust toolchain collection")?;
    let toolchain_dir = toolchains.join(toolchain);
    let toolchain_exists =
        toolchains_exist && tenant::real_dir_exists(&toolchain_dir, "Rust toolchain")?;
    let toolchain_bin_exists = toolchain_exists
        && tenant::real_dir_exists(&toolchain_dir.join("bin"), "Rust binary directory")?;
    let rustc_exists = toolchain_bin_exists
        && executable_file_exists(&toolchain_dir.join("bin/rustc"), "rustc executable")?;
    let complete = rustup_exists && rustc_exists;
    if complete {
        Ok(ComponentStatus::Installed {
            version: Some(version),
        })
    } else {
        Ok(ComponentStatus::Incomplete)
    }
}

fn inspect_go(home: &Path) -> Result<ComponentStatus> {
    let goroot = home.join(".goroot");
    if !tenant::real_dir_exists(&goroot, "Go root")? {
        return Ok(ComponentStatus::NotInstalled);
    }
    let version_file = capture_limited(&goroot.join("VERSION"), "Go version file")?;
    if !version_file.present {
        return Ok(ComponentStatus::Incomplete);
    }
    let content = std::str::from_utf8(&version_file.content).context("Go VERSION is not UTF-8")?;
    let Some(version) = content
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("go"))
        .and_then(|version| validate_stable_version(version).ok())
    else {
        return Ok(ComponentStatus::Unmanaged);
    };
    if tenant::real_dir_exists(&goroot.join("bin"), "Go binary directory")?
        && executable_file_exists(&goroot.join("bin/go"), "Go executable")?
    {
        Ok(ComponentStatus::Installed {
            version: Some(version),
        })
    } else {
        Ok(ComponentStatus::Incomplete)
    }
}

fn stable_version_prefix(toolchain: &str) -> Option<String> {
    let version = toolchain.split('-').next()?;
    let version = validate_stable_version(version).ok()?;
    let suffix = toolchain.strip_prefix(&version)?;
    matches!(
        suffix,
        "" | "-x86_64-unknown-linux-gnu" | "-aarch64-unknown-linux-gnu"
    )
    .then_some(version)
}

fn capture_limited(path: &Path, label: &str) -> Result<FileSnapshot> {
    FileSnapshot::capture_with_limit(path, MAX_CONFIG_BYTES as u64)
        .with_context(|| format!("inspect {label}"))
}

fn parse_json_config(snapshot: &FileSnapshot, label: &str) -> Result<Map<String, Value>> {
    if !snapshot.present || snapshot.content.iter().all(u8::is_ascii_whitespace) {
        return Ok(Map::new());
    }
    let value: Value =
        serde_json::from_slice(&snapshot.content).with_context(|| format!("parse {label}"))?;
    value
        .as_object()
        .cloned()
        .with_context(|| format!("{label} must be a JSON object"))
}

fn install_claude_statusline(tenant: &Tenant) -> Result<i32> {
    let selected = prepare_statusline_install(tenant, AgentKind::Claude)?;

    let script_path = selected.state_file(CLAUDE_STATUSLINE_SCRIPT);
    let settings_path = selected.state_file(AgentKind::Claude.main_config_file());
    let script = capture_limited(&script_path, "Claude status-line script")?;
    let settings = capture_limited(&settings_path, "Claude settings")?;
    let setting_state = claude_statusline_setting_state(&settings)?;
    let mut object = parse_json_config(&settings, "Claude settings")?;
    object.insert(
        "statusLine".to_string(),
        json!({
            "type": "command",
            "command": "bash ~/.claude/statusline.sh"
        }),
    );
    let content = format!(
        "{}\n",
        serde_json::to_string_pretty(&Value::Object(object))?
    );

    if claude_statusline_script_state(&script) != StatuslinePartState::Current {
        write_atomic(&script_path, CLAUDE_STATUSLINE, Some(0o755))?;
    }
    if setting_state != StatuslinePartState::Current {
        write_atomic(
            &settings_path,
            content.as_bytes(),
            settings.mode.or(Some(0o600)),
        )?;
    }
    Ok(0)
}

fn install_codex_statusline(tenant: &Tenant) -> Result<i32> {
    let selected = prepare_statusline_install(tenant, AgentKind::Codex)?;

    let path = selected.state_file(AgentKind::Codex.main_config_file());
    let config = capture_limited(&path, "Codex configuration")?;
    let setting_matches = matches!(
        codex_statusline_setting(&config)?,
        ComponentStatus::Installed { version: None }
    );
    let content =
        std::str::from_utf8(&config.content).context("Codex configuration is not UTF-8")?;
    let mut document = if content.trim().is_empty() {
        toml_edit::DocumentMut::new()
    } else {
        content
            .parse::<toml_edit::DocumentMut>()
            .context("parse Codex configuration")?
    };
    if document
        .get("tui")
        .is_some_and(|item| !item.is_table_like())
    {
        bail!("Codex tui configuration is not a table; refusing to replace unowned configuration");
    }
    if document.get("tui").is_none() {
        document["tui"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    let tui = document["tui"]
        .as_table_like_mut()
        .context("Codex tui configuration must be a table")?;
    let mut status_line = toml_edit::Array::new();
    for item in CODEX_STATUSLINE_ITEMS {
        status_line.push(item);
    }
    tui.insert("status_line", toml_edit::value(status_line));
    tui.insert("status_line_use_colors", toml_edit::value(false));
    let desired = document.to_string();
    if !setting_matches {
        write_atomic(&path, desired.as_bytes(), config.mode.or(Some(0o600)))?;
    }
    Ok(0)
}

fn prepare_statusline_install(tenant: &Tenant, agent: AgentKind) -> Result<TenantAgent> {
    let selected = tenant.for_agent(agent);
    selected.ensure_agent_state_dir()?;
    Ok(selected)
}

fn install_runtime_component_with(
    tenant: &ManagedTenant,
    component: &ComponentSpec,
    docker: &crate::docker::DockerCli,
) -> Result<i32> {
    install_runtime_component_with_mode(tenant, component, docker, None)
}

fn install_runtime_component_with_mode(
    tenant: &ManagedTenant,
    component: &ComponentSpec,
    docker: &crate::docker::DockerCli,
    service_log: Option<crate::docker::LogCallback>,
) -> Result<i32> {
    let existing = if tenant.exists()? {
        inspect(component.kind, &tenant.home_dir)?
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
    let home = fs::canonicalize(&tenant.home_dir)
        .with_context(|| format!("resolve Tenant Home {}", tenant.home_dir.display()))?;
    crate::runspec::reject_colon_in_bind_source("Tenant Home", &home)?;
    let run_args = crate::runspec::assemble_component_run_args(&home);
    let script = match component.kind {
        ComponentKind::Node => NODE_INSTALLER,
        ComponentKind::Codex => CODEX_INSTALLER,
        ComponentKind::Claude => CLAUDE_INSTALLER,
        ComponentKind::Python => PYTHON_INSTALLER,
        ComponentKind::Rust => RUST_INSTALLER,
        ComponentKind::Go => GO_INSTALLER,
        _ => unreachable!("status-line Components are installed on the host"),
    };
    let command = vec![
        OsString::from("bash"),
        OsString::from("-ceu"),
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

struct UserShellProfile {
    path: PathBuf,
    snapshot: FileSnapshot,
}

fn capture_user_shell_profiles(home: &Path) -> Result<Vec<UserShellProfile>> {
    [".bash_profile", ".bashrc"]
        .into_iter()
        .map(|name| {
            let path = home.join(name);
            let snapshot = FileSnapshot::capture_with_limit(&path, MAX_CONFIG_BYTES as u64)
                .with_context(|| format!("capture user shell profile {}", path.display()))?;
            Ok(UserShellProfile { path, snapshot })
        })
        .collect()
}

fn restore_user_shell_profiles(profiles: &[UserShellProfile]) -> Result<()> {
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
            tenant::sync_dir(
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
            tenant::sync_dir(
                profile
                    .path
                    .parent()
                    .context("user shell profile has no parent")?,
            )?;
        }
    }
    Ok(())
}

fn remove_claude_statusline(tenant: &Tenant) -> Result<()> {
    let selected = tenant.for_agent(AgentKind::Claude);
    let script = selected.state_file(CLAUDE_STATUSLINE_SCRIPT);
    tenant::remove_real_file_if_exists(&script, "Claude status-line script")?;

    let settings_path = selected.state_file(AgentKind::Claude.main_config_file());
    let settings = capture_limited(&settings_path, "Claude settings")?;
    if settings.present {
        let mut object = parse_json_config(&settings, "Claude settings")?;
        if object.remove("statusLine").is_some() {
            let content = format!(
                "{}\n",
                serde_json::to_string_pretty(&Value::Object(object))?
            );
            write_atomic(&settings_path, content.as_bytes(), settings.mode)?;
        }
    }
    Ok(())
}

fn remove_codex_statusline(tenant: &Tenant) -> Result<()> {
    let selected = tenant.for_agent(AgentKind::Codex);
    let path = selected.state_file(AgentKind::Codex.main_config_file());
    let config = capture_limited(&path, "Codex configuration")?;
    if !config.present || config.content.iter().all(u8::is_ascii_whitespace) {
        return Ok(());
    }
    let content =
        std::str::from_utf8(&config.content).context("Codex configuration is not UTF-8")?;
    let mut document = content
        .parse::<toml_edit::DocumentMut>()
        .context("parse Codex configuration")?;
    let mut changed = false;
    if let Some(tui) = document
        .get_mut("tui")
        .and_then(toml_edit::Item::as_table_like_mut)
    {
        changed |= tui.remove("status_line").is_some();
        changed |= tui.remove("status_line_use_colors").is_some();
    }
    if changed {
        write_atomic(&path, document.to_string().as_bytes(), config.mode)?;
    }
    Ok(())
}

fn remove_node(home: &Path) -> Result<()> {
    tenant::real_dir_exists(home, "Tenant Home")?;
    tenant::remove_real_dir_if_exists(&home.join(".node"), "Node.js root")
}

fn remove_codex(home: &Path) -> Result<()> {
    tenant::real_dir_exists(home, "Tenant Home")?;
    remove_local_launcher(home, "codex", "Codex launcher")?;
    let codex = home.join(".codex");
    if !tenant::real_dir_exists(&codex, "Codex state directory")? {
        return Ok(());
    }
    let packages = codex.join("packages");
    if !tenant::real_dir_exists(&packages, "Codex package directory")? {
        return Ok(());
    }
    tenant::remove_real_dir_if_exists(&packages.join("standalone"), "Codex standalone package")
}

fn remove_claude(home: &Path) -> Result<()> {
    tenant::real_dir_exists(home, "Tenant Home")?;
    remove_local_launcher(home, "claude", "Claude launcher")?;
    let local = home.join(".local");
    if !tenant::real_dir_exists(&local, "Tenant-local data directory")? {
        return Ok(());
    }
    let share = local.join("share");
    if !tenant::real_dir_exists(&share, "Tenant-local shared data directory")? {
        return Ok(());
    }
    let claude = share.join("claude");
    if !tenant::real_dir_exists(&claude, "Claude data directory")? {
        return Ok(());
    }
    tenant::remove_real_dir_if_exists(&claude.join("versions"), "Claude version collection")
}

fn remove_python(home: &Path) -> Result<()> {
    tenant::real_dir_exists(home, "Tenant Home")?;
    let mut launchers = vec![
        "uv".to_string(),
        "uvx".to_string(),
        "python".to_string(),
        "python3".to_string(),
        "pip".to_string(),
        "pip3".to_string(),
    ];
    launchers.extend(python_versioned_launcher_names(home)?);
    launchers.sort();
    launchers.dedup();
    for launcher in launchers {
        remove_local_launcher(home, &launcher, "Python toolchain launcher")?;
    }
    tenant::remove_real_dir_if_exists(&home.join(".python"), "Python toolchain root")
}

fn remove_local_launcher(home: &Path, name: &str, label: &str) -> Result<()> {
    let local = home.join(".local");
    if !tenant::real_dir_exists(&local, "Tenant-local data directory")? {
        return Ok(());
    }
    let bin = local.join("bin");
    if !tenant::real_dir_exists(&bin, "Tenant-local binary directory")? {
        return Ok(());
    }
    let launcher = bin.join(name);
    match fs::symlink_metadata(&launcher) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("inspect {label} {}", launcher.display())),
        Ok(metadata) if metadata.file_type().is_file() || metadata.file_type().is_symlink() => {
            fs::remove_file(&launcher)
                .with_context(|| format!("remove {label} {}", launcher.display()))?;
            tenant::sync_dir(&bin)
        }
        Ok(_) => bail!("{label} is not a file or symlink: {}", launcher.display()),
    }
}

fn remove_rust(home: &Path) -> Result<()> {
    tenant::real_dir_exists(home, "Tenant Home")?;
    let rustup = home.join(".rustup");
    let rustup_exists = tenant::real_dir_exists(&rustup, "Rustup Home")?;
    let cargo = home.join(".cargo");
    let cargo_exists = tenant::real_dir_exists(&cargo, "Cargo Home")?;
    let bin = cargo.join("bin");
    let bin_exists = cargo_exists && tenant::real_dir_exists(&bin, "Cargo binary directory")?;
    let proxies = if bin_exists {
        rustup_proxy_paths(&bin)?
    } else {
        Vec::new()
    };

    // Remove the cross-directory proxies first. If removal is interrupted,
    // `.rustup` remains as recognizable incomplete Component state and a
    // repeated command can finish the operation. Removing `.rustup` first
    // could leave only proxies, which inspection intentionally does not claim
    // as aibox-owned state because they may belong to a manual Rust install.
    for proxy in proxies {
        fs::remove_file(&proxy)
            .with_context(|| format!("remove rustup proxy {}", proxy.display()))?;
    }
    if bin_exists {
        tenant::sync_dir(&bin)?;
    }
    if rustup_exists {
        tenant::remove_real_dir_if_exists(&rustup, "Rustup Home")?;
    }
    Ok(())
}

fn rustup_proxy_paths(bin: &Path) -> Result<Vec<PathBuf>> {
    let rustup = bin.join("rustup");
    let rustup_metadata = match fs::symlink_metadata(&rustup) {
        Ok(metadata) if metadata.file_type().is_file() => Some(metadata),
        Ok(_) => bail!(
            "rustup executable is not a regular file: {}",
            rustup.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(error)
                .with_context(|| format!("inspect rustup executable {}", rustup.display()));
        }
    };
    let mut proxies = Vec::new();
    for entry in fs::read_dir(bin)
        .with_context(|| format!("read Cargo binary directory {}", bin.display()))?
    {
        let entry =
            entry.with_context(|| format!("read Cargo binary entry in {}", bin.display()))?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .with_context(|| format!("inspect Cargo binary entry {}", path.display()))?;
        let owned = if path == rustup {
            rustup_metadata.is_some()
        } else if metadata.file_type().is_symlink() {
            fs::read_link(&path).with_context(|| format!("read rustup proxy {}", path.display()))?
                == Path::new("rustup")
        } else {
            rustup_metadata
                .as_ref()
                .is_some_and(|rustup| same_file_identity(&metadata, rustup))
        };
        if owned {
            proxies.push(path);
        }
    }
    // Keep the executable available until every hard-link proxy is gone so an
    // interrupted removal can rediscover ownership on the next attempt.
    proxies.sort_by_key(|path| path == &rustup);
    Ok(proxies)
}

#[cfg(unix)]
fn same_file_identity(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;

    left.file_type().is_file() && left.dev() == right.dev() && left.ino() == right.ino()
}

#[cfg(not(unix))]
fn same_file_identity(_left: &fs::Metadata, _right: &fs::Metadata) -> bool {
    false
}

fn write_atomic(path: &Path, content: &[u8], mode: Option<u32>) -> Result<()> {
    if content.len() > MAX_CONFIG_BYTES {
        bail!("refusing oversized Component write: {}", path.display());
    }
    let parent = path.parent().context("Component path has no parent")?;
    tenant::ensure_real_dir(parent, "Component parent directory")?;
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.file_type().is_file() => {
            bail!("Component path is not a regular file: {}", path.display())
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }

    let mut temp = tempfile::Builder::new()
        .prefix(".aibox-component-")
        .tempfile_in(parent)
        .with_context(|| format!("create temporary Component file in {}", parent.display()))?;
    temp.write_all(content)?;
    #[cfg(unix)]
    if let Some(mode) = mode {
        use std::os::unix::fs::PermissionsExt;
        temp.as_file()
            .set_permissions(fs::Permissions::from_mode(mode))?;
    }
    #[cfg(not(unix))]
    let _ = mode;
    temp.as_file().sync_all()?;
    temp.persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("replace Component file {}", path.display()))?;
    tenant::sync_dir(parent)
}

#[cfg(unix)]
fn executable_mode_is_current(mode: Option<u32>) -> bool {
    mode.is_some_and(|mode| mode & 0o777 == 0o755)
}

#[cfg(not(unix))]
fn executable_mode_is_current(_mode: Option<u32>) -> bool {
    true
}

fn executable_file_exists(path: &Path, label: &str) -> Result<bool> {
    if !tenant::real_file_exists(path, label)? {
        return Ok(false);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        Ok(fs::symlink_metadata(path)?.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        Ok(true)
    }
}

#[cfg(test)]
#[path = "component_tests.rs"]
mod tests;
