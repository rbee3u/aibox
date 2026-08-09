//! Optional capabilities derived from native state in a Tenant Home.
//!
//! Status-line Components directly edit native Current Config while
//! toolchains own Managed Tenant-local SDK directories. There is no Component
//! registry, so inspection derives state directly from native files.

use crate::agent::AgentKind;
use crate::cli::{ComponentArgs, ComponentCommand};
use crate::tenant::{self, FileSnapshot, ManagedTenant, Tenant, TenantAgent};
use anyhow::{Context, Result, bail};
use serde_json::{Map, Value, json};
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::Path;
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
const MAX_CONFIG_BYTES: usize = 16 * 1024 * 1024;

/// One optional capability that aibox can install into a Tenant Home.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComponentKind {
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
    pub(crate) const ALL: [Self; 4] = [
        Self::ClaudeStatusline,
        Self::CodexStatusline,
        Self::Rust,
        Self::Go,
    ];
    pub(crate) const STATUSLINES: [Self; 2] = [Self::ClaudeStatusline, Self::CodexStatusline];

    /// Stable CLI name.
    pub fn name(self) -> &'static str {
        match self {
            Self::ClaudeStatusline => "claude-statusline",
            Self::CodexStatusline => "codex-statusline",
            Self::Rust => "rust",
            Self::Go => "go",
        }
    }

    fn supports_version(self) -> bool {
        matches!(self, Self::Rust | Self::Go)
    }

    fn is_statusline(self) -> bool {
        matches!(self, Self::ClaudeStatusline | Self::CodexStatusline)
    }
}

/// State derived from a Component's native files in one Tenant Home.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ComponentStatus {
    /// The Component exactly matches the current aibox definition.
    Installed {
        /// Stable toolchain version; absent for status-line Components.
        version: Option<String>,
    },
    /// Some status-line state exists but differs from the current definition.
    Modified,
    /// Recognizable aibox-owned state exists but is only partially installed
    /// or is not healthy enough to run.
    Incomplete,
    /// Toolchain state exists but aibox must not take ownership of it.
    Unmanaged,
    /// No Component-owned state exists.
    NotInstalled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StatuslinePartState {
    Absent,
    Current,
    Modified,
}

#[derive(Clone, Copy)]
struct RemovalOptions {
    skip_confirmation: bool,
}

/// A Component name and optional stable toolchain version.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComponentSpec {
    /// Selected Component.
    pub kind: ComponentKind,
    /// Requested stable toolchain version, or latest stable when absent.
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
            "invalid stable toolchain version {version:?}; expected X.Y.Z"
        ));
    }
    Ok(version.to_string())
}

/// Execute one parsed Component command.
pub fn dispatch(args: &ComponentArgs) -> Result<i32> {
    let root = tenant::aibox_root()?;
    let selected = Tenant::resolve(&root, args.tenant.host, args.tenant.tenant_name())?;
    match &args.command {
        ComponentCommand::List => list(&selected),
        ComponentCommand::Install { component } => install(&selected, component),
        ComponentCommand::Remove { component, yes } => remove(&selected, *component, *yes),
    }
}

#[cfg(test)]
pub(crate) fn dispatch_with(
    args: &ComponentArgs,
    root: &Path,
    host_home: &Path,
    image_override: Option<&str>,
    docker: &crate::docker::DockerCli,
) -> Result<i32> {
    let selected =
        Tenant::resolve_with_home(root, args.tenant.host, args.tenant.tenant_name(), host_home)?;
    match &args.command {
        ComponentCommand::List => list(&selected),
        ComponentCommand::Install { component } => {
            install_with(&selected, component, image_override, docker)
        }
        ComponentCommand::Remove { component, yes } => remove(&selected, *component, *yes),
    }
}

fn list(selected: &Tenant) -> Result<i32> {
    let exists = tenant_home_exists(selected)?;
    for &kind in component_catalog(selected) {
        let status = if exists {
            inspect(kind, selected.home_dir())?
        } else {
            ComponentStatus::NotInstalled
        };
        if !crate::print_line(&format_status(kind, &status))? {
            break;
        }
    }
    Ok(0)
}

fn install(selected: &Tenant, component: &ComponentSpec) -> Result<i32> {
    reject_host_toolchain(selected, component.kind)?;
    match component.kind {
        ComponentKind::ClaudeStatusline => install_claude_statusline(selected),
        ComponentKind::CodexStatusline => install_codex_statusline(selected),
        ComponentKind::Rust | ComponentKind::Go => {
            let Tenant::Managed(tenant) = selected else {
                unreachable!("Host toolchains are rejected above")
            };
            install_toolchain(tenant, component)
        }
    }
}

#[cfg(test)]
fn install_with(
    selected: &Tenant,
    component: &ComponentSpec,
    image_override: Option<&str>,
    docker: &crate::docker::DockerCli,
) -> Result<i32> {
    reject_host_toolchain(selected, component.kind)?;
    match component.kind {
        ComponentKind::ClaudeStatusline => install_claude_statusline(selected),
        ComponentKind::CodexStatusline => install_codex_statusline(selected),
        ComponentKind::Rust | ComponentKind::Go => {
            let Tenant::Managed(tenant) = selected else {
                unreachable!("Host toolchains are rejected above")
            };
            install_toolchain_with(tenant, component, image_override, docker)
        }
    }
}

fn install_toolchain(tenant: &ManagedTenant, component: &ComponentSpec) -> Result<i32> {
    let image_override = crate::env_override("AIBOX_IMAGE")?;
    install_toolchain_with(
        tenant,
        component,
        image_override.as_deref(),
        &crate::docker::DockerCli::system(),
    )
}

fn remove(selected: &Tenant, kind: ComponentKind, yes: bool) -> Result<i32> {
    reject_host_toolchain(selected, kind)?;
    if !tenant_home_exists(selected)? {
        if matches!(selected, Tenant::Host { .. }) {
            bail!(
                "Host Home does not exist: {}",
                selected.home_dir().display()
            );
        }
        return Ok(0);
    }
    remove_from_tenant(
        selected,
        kind,
        RemovalOptions {
            skip_confirmation: yes,
        },
    )
}

fn remove_from_tenant(
    selected: &Tenant,
    kind: ComponentKind,
    options: RemovalOptions,
) -> Result<i32> {
    reject_host_toolchain(selected, kind)?;
    let status = inspect(kind, selected.home_dir())?;
    if status == ComponentStatus::NotInstalled {
        return Ok(0);
    }
    if !options.skip_confirmation && !confirm_remove(kind)? {
        bail!("aborted");
    }
    match kind {
        ComponentKind::ClaudeStatusline => remove_claude_statusline(selected)?,
        ComponentKind::CodexStatusline => remove_codex_statusline(selected)?,
        ComponentKind::Rust => remove_rust(selected.home_dir())?,
        ComponentKind::Go => {
            tenant::remove_real_dir_if_exists(&selected.home_dir().join(".goroot"), "Go root")?;
        }
    }
    Ok(0)
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

fn reject_host_toolchain(selected: &Tenant, kind: ComponentKind) -> Result<()> {
    if matches!(selected, Tenant::Host { .. }) && !kind.is_statusline() {
        bail!(
            "{} is unavailable to the Host Tenant; --host supports only claude-statusline and codex-statusline",
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

fn format_status(kind: ComponentKind, status: &ComponentStatus) -> String {
    match status {
        ComponentStatus::Installed {
            version: Some(version),
        } => format!("{} installed {version}", kind.name()),
        ComponentStatus::Installed { version: None } => {
            format!("{} installed", kind.name())
        }
        ComponentStatus::Modified => format!("{} modified", kind.name()),
        ComponentStatus::Incomplete => format!("{} incomplete", kind.name()),
        ComponentStatus::Unmanaged => format!("{} unmanaged", kind.name()),
        ComponentStatus::NotInstalled => format!("{} not-installed", kind.name()),
    }
}

fn inspect(kind: ComponentKind, home: &Path) -> Result<ComponentStatus> {
    match kind {
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
    let selected = match tenant {
        Tenant::Managed(tenant) => tenant.for_agent(agent),
        Tenant::Host { .. } => tenant.for_agent(agent),
    };
    selected.ensure_agent_state_dir()?;
    Ok(selected)
}

fn install_toolchain_with(
    tenant: &ManagedTenant,
    component: &ComponentSpec,
    image_override: Option<&str>,
    docker: &crate::docker::DockerCli,
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
            "{} has unmanaged toolchain state; remove or normalize it before installation",
            component.kind.name()
        );
    }

    let image = crate::image_for(image_override)?;
    if image_override.is_some() {
        eprintln!(">> image overridden by $AIBOX_IMAGE: {image}");
    }
    if !crate::docker::image_exists_with(docker, &image)? {
        bail!("{image} is not present locally; build it first with `aibox build`");
    }

    tenant.ensure_initialized()?;
    let home = fs::canonicalize(&tenant.home_dir)
        .with_context(|| format!("resolve Tenant Home {}", tenant.home_dir.display()))?;
    crate::runspec::reject_colon_in_bind_source("Tenant Home", &home)?;
    let run_args = crate::runspec::assemble_component_run_args(&home);
    let script = match component.kind {
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
    crate::docker::run_with(docker, &run_args, &image, &command, || {})
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

fn remove_rust(home: &Path) -> Result<()> {
    tenant::real_dir_exists(home, "Tenant Home")?;
    let rustup = home.join(".rustup");
    let rustup_exists = tenant::real_dir_exists(&rustup, "Rustup Home")?;
    let cargo = home.join(".cargo");
    let cargo_exists = tenant::real_dir_exists(&cargo, "Cargo Home")?;
    let bin = cargo.join("bin");
    let bin_exists = cargo_exists && tenant::real_dir_exists(&bin, "Cargo binary directory")?;
    let proxies = [
        "rustup",
        "rustc",
        "cargo",
        "rustdoc",
        "rustfmt",
        "cargo-fmt",
        "clippy-driver",
        "cargo-clippy",
    ];
    if bin_exists {
        for proxy in proxies {
            tenant::real_file_exists(&bin.join(proxy), "rustup proxy")?;
        }
    }

    // Remove the cross-directory proxies first. If removal is interrupted,
    // `.rustup` remains as recognizable incomplete Component state and a
    // repeated command can finish the operation. Removing `.rustup` first
    // could leave only proxies, which inspection intentionally does not claim
    // as aibox-owned state because they may belong to a manual Rust install.
    if bin_exists {
        for proxy in proxies {
            tenant::remove_real_file_if_exists(&bin.join(proxy), "rustup proxy")?;
        }
    }
    if rustup_exists {
        tenant::remove_real_dir_if_exists(&rustup, "Rustup Home")?;
    }
    Ok(())
}

fn confirm_remove(kind: ComponentKind) -> Result<bool> {
    if !io::stdin().is_terminal() {
        bail!(
            "refusing to remove Component '{}' without --yes in a non-interactive shell",
            kind.name()
        );
    }
    eprint!("Remove Component '{}'? [y/N] ", kind.name());
    io::stderr().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(matches!(input.trim(), "y" | "Y" | "yes" | "YES"))
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
