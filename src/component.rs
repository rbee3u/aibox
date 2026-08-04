//! Optional capabilities derived from native state in a Managed Tenant Home.
//!
//! Status-line Components own a narrow set of Agent Configuration paths;
//! toolchains own their Tenant-local SDK directories. There is no Component
//! registry, so inspection must distinguish healthy, partial, modified, and
//! unmanaged native state before installation or removal.

use crate::agent::AgentKind;
use crate::agent_config::Pointer;
use crate::cli::{ComponentArgs, ComponentCommand};
use crate::tenant::{self, FileSnapshot, ManagedTenant, Tenant, TenantAgent};
use anyhow::{bail, Context, Result};
use serde_json::{json, Map, Value};
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::Path;
use std::str::FromStr;

const CLAUDE_STATUSLINE: &[u8] = include_bytes!("../assets/claude-statusline.sh");
const CLAUDE_STATUSLINE_SCRIPT: &str = "statusline.sh";
const CODEX_STATUSLINE_ITEMS: [&str; 3] = ["model-with-reasoning", "current-dir", "git-branch"];
const RUST_INSTALLER: &str = include_str!("../assets/install-rust.sh");
const GO_INSTALLER: &str = include_str!("../assets/install-go.sh");
const MAX_CONFIG_BYTES: usize = 16 * 1024 * 1024;

/// One optional capability that aibox can install into a Managed Tenant Home.
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
}

/// State derived from a Component's native files in one Managed Tenant Home.
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
    discard_changes: bool,
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
    match &args.command {
        ComponentCommand::List => list(args.tenant_name()),
        ComponentCommand::Install { component } => install(args.tenant_name(), component),
        ComponentCommand::Remove {
            component,
            discard_changes,
            yes,
        } => remove(args.tenant_name(), *component, *discard_changes, *yes),
    }
}

fn list(tenant_name: &str) -> Result<i32> {
    let root = tenant::aibox_root()?;
    let tenant = ManagedTenant::resolve(&root, tenant_name)?;
    let exists = tenant.exists()?;
    for kind in ComponentKind::ALL {
        let status = if exists {
            inspect(kind, &tenant.home_dir)?
        } else {
            ComponentStatus::NotInstalled
        };
        if !crate::print_line(&format_status(kind, &status))? {
            break;
        }
    }
    Ok(0)
}

fn install(tenant_name: &str, component: &ComponentSpec) -> Result<i32> {
    let root = tenant::aibox_root()?;
    let tenant = ManagedTenant::resolve(&root, tenant_name)?;
    match component.kind {
        ComponentKind::ClaudeStatusline => install_claude_statusline(&tenant),
        ComponentKind::CodexStatusline => install_codex_statusline(&tenant),
        ComponentKind::Rust | ComponentKind::Go => install_toolchain(&tenant, component),
    }
}

fn remove(tenant_name: &str, kind: ComponentKind, discard_changes: bool, yes: bool) -> Result<i32> {
    let root = tenant::aibox_root()?;
    let tenant = ManagedTenant::resolve(&root, tenant_name)?;
    if !tenant.exists()? {
        return Ok(0);
    }
    remove_from_tenant(
        &tenant,
        kind,
        RemovalOptions {
            discard_changes,
            skip_confirmation: yes,
        },
    )
}

fn remove_from_tenant(
    tenant: &ManagedTenant,
    kind: ComponentKind,
    options: RemovalOptions,
) -> Result<i32> {
    let status = inspect(kind, &tenant.home_dir)?;
    if status == ComponentStatus::NotInstalled {
        return Ok(0);
    }
    if matches!(
        status,
        ComponentStatus::Modified | ComponentStatus::Unmanaged
    ) && !options.discard_changes
    {
        bail!(
            "{} is {}; use --discard-changes to remove it",
            kind.name(),
            status_label(&status)
        );
    }
    if !options.skip_confirmation && !confirm_remove(kind)? {
        bail!("aborted");
    }
    match kind {
        ComponentKind::ClaudeStatusline => remove_claude_statusline(tenant)?,
        ComponentKind::CodexStatusline => remove_codex_statusline(tenant)?,
        ComponentKind::Rust => remove_rust(&tenant.home_dir)?,
        ComponentKind::Go => {
            tenant::remove_real_dir_if_exists(&tenant.home_dir.join(".goroot"), "Go root")?;
        }
    }
    Ok(0)
}

fn status_label(status: &ComponentStatus) -> &'static str {
    match status {
        ComponentStatus::Installed { .. } => "installed",
        ComponentStatus::Modified => "modified",
        ComponentStatus::Incomplete => "incomplete",
        ComponentStatus::Unmanaged => "unmanaged",
        ComponentStatus::NotInstalled => "not installed",
    }
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

/// Component-owned logical Agent Configuration paths that Agent Profile
/// operations must preserve for this scope.
pub(crate) fn protected_config_paths(selected: &TenantAgent) -> Result<Vec<Pointer>> {
    let Tenant::Managed(tenant) = &selected.tenant else {
        return Ok(Vec::new());
    };
    if !tenant.exists()? {
        return Ok(Vec::new());
    }
    let kind = statusline_component(selected.agent);
    if inspect(kind, &tenant.home_dir)? == ComponentStatus::NotInstalled {
        return Ok(Vec::new());
    }
    component_owned_paths(kind)
        .iter()
        .map(|path| Pointer::parse(path))
        .collect()
}

fn statusline_component(agent: AgentKind) -> ComponentKind {
    match agent {
        AgentKind::Claude => ComponentKind::ClaudeStatusline,
        AgentKind::Codex => ComponentKind::CodexStatusline,
    }
}

fn component_owned_paths(kind: ComponentKind) -> &'static [&'static str] {
    match kind {
        ComponentKind::ClaudeStatusline => &["/config/statusLine"],
        ComponentKind::CodexStatusline => &[
            "/config/tui/status_line",
            "/config/tui/status_line_use_colors",
        ],
        ComponentKind::Rust | ComponentKind::Go => &[],
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
        Some(item) if item.as_bool() == Some(true) => StatuslinePartState::Current,
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

fn install_claude_statusline(tenant: &ManagedTenant) -> Result<i32> {
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
            settings.mode.or(Some(0o644)),
        )?;
    }
    Ok(0)
}

fn install_codex_statusline(tenant: &ManagedTenant) -> Result<i32> {
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
    tui.insert("status_line_use_colors", toml_edit::value(true));
    let desired = document.to_string();
    if !setting_matches {
        write_atomic(&path, desired.as_bytes(), config.mode.or(Some(0o644)))?;
    }
    Ok(0)
}

fn prepare_statusline_install(tenant: &ManagedTenant, agent: AgentKind) -> Result<TenantAgent> {
    tenant.ensure_initialized()?;
    let selected = tenant.for_agent(agent);
    crate::profile::recover_pending(&selected)?;
    crate::profile::ensure_component_paths_available(
        &selected,
        component_owned_paths(statusline_component(agent)),
    )?;
    selected.ensure_agent_state_dir()?;
    Ok(selected)
}

fn install_toolchain(tenant: &ManagedTenant, component: &ComponentSpec) -> Result<i32> {
    let existing = if tenant.exists()? {
        inspect(component.kind, &tenant.home_dir)?
    } else {
        ComponentStatus::NotInstalled
    };
    if let Some(requested) = &component.version {
        if matches!(
            existing,
            ComponentStatus::Installed { version: Some(ref current) } if current == requested
        ) {
            eprintln!(
                ">> {} {requested} is already installed; skipping",
                component.kind.name()
            );
            return Ok(0);
        }
    }
    if existing == ComponentStatus::Unmanaged {
        bail!(
            "{} has unmanaged toolchain state; remove or normalize it before installation",
            component.kind.name()
        );
    }

    let image_override = crate::env_override("AIBOX_IMAGE")?;
    let image = crate::image_for(image_override.as_deref())?;
    if image_override.is_some() {
        eprintln!(">> image overridden by $AIBOX_IMAGE: {image}");
    }
    if !crate::docker::image_exists(&image)? {
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
    crate::docker::run(&run_args, &image, &command, || {})
}

fn remove_claude_statusline(tenant: &ManagedTenant) -> Result<()> {
    let selected = tenant.for_agent(AgentKind::Claude);
    crate::profile::recover_pending(&selected)?;
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

fn remove_codex_statusline(tenant: &ManagedTenant) -> Result<()> {
    let selected = tenant.for_agent(AgentKind::Codex);
    crate::profile::recover_pending(&selected)?;
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
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;

    fn initialized_tenant() -> (tempfile::TempDir, ManagedTenant) {
        let root = tempfile::tempdir().unwrap();
        let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();
        tenant.ensure_initialized().unwrap();
        (root, tenant)
    }

    fn remove_confirmed(tenant: &ManagedTenant, kind: ComponentKind) -> Result<i32> {
        remove_from_tenant(
            tenant,
            kind,
            RemovalOptions {
                discard_changes: false,
                skip_confirmation: true,
            },
        )
    }

    fn remove_discarding(tenant: &ManagedTenant, kind: ComponentKind) -> Result<i32> {
        remove_from_tenant(
            tenant,
            kind,
            RemovalOptions {
                discard_changes: true,
                skip_confirmation: true,
            },
        )
    }

    fn write_rust_state(home: &Path, toolchain: &str, complete: bool) {
        let rustup = home.join(".rustup");
        fs::create_dir_all(&rustup).unwrap();
        fs::write(
            rustup.join("settings.toml"),
            format!("version = \"12\"\ndefault_toolchain = \"{toolchain}\"\n"),
        )
        .unwrap();
        if complete {
            fs::create_dir_all(home.join(".cargo/bin")).unwrap();
            fs::write(home.join(".cargo/bin/rustup"), "rustup").unwrap();
            let rustc = rustup.join("toolchains").join(toolchain).join("bin");
            fs::create_dir_all(&rustc).unwrap();
            fs::write(rustc.join("rustc"), "rustc").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(
                    home.join(".cargo/bin/rustup"),
                    fs::Permissions::from_mode(0o755),
                )
                .unwrap();
                fs::set_permissions(rustc.join("rustc"), fs::Permissions::from_mode(0o755))
                    .unwrap();
            }
        }
    }

    fn write_go_state(home: &Path, version: &str, complete: bool) {
        let goroot = home.join(".goroot");
        fs::create_dir_all(&goroot).unwrap();
        fs::write(
            goroot.join("VERSION"),
            format!("{version}\ntime 2026-01-01T00:00:00Z\n"),
        )
        .unwrap();
        if complete {
            fs::create_dir_all(goroot.join("bin")).unwrap();
            fs::write(goroot.join("bin/go"), "go").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(goroot.join("bin/go"), fs::Permissions::from_mode(0o755))
                    .unwrap();
            }
        }
    }

    #[test]
    fn component_specs_accept_supported_shapes_and_explain_rejections() {
        for (input, kind, version) in [
            ("claude-statusline", ComponentKind::ClaudeStatusline, None),
            ("rust", ComponentKind::Rust, None),
            ("go@1.25.6", ComponentKind::Go, Some("1.25.6")),
        ] {
            assert_eq!(
                input.parse::<ComponentSpec>().unwrap(),
                ComponentSpec {
                    kind,
                    version: version.map(str::to_string),
                },
                "{input}"
            );
        }

        for (input, expected) in [
            ("statusline", "unknown Component"),
            ("rust@stable", "expected X.Y.Z"),
            ("rust@1.90", "expected X.Y.Z"),
            ("rust@01.90.0", "expected X.Y.Z"),
            ("go@1.25.6@extra", "expected X.Y.Z"),
            ("codex-statusline@1.0.0", "does not accept a version"),
        ] {
            let error = input.parse::<ComponentSpec>().unwrap_err();
            assert!(error.contains(expected), "{input:?}: {error}");
        }
    }

    #[test]
    fn status_format_is_stable_and_versioned_only_for_toolchains() {
        assert_eq!(
            format_status(
                ComponentKind::ClaudeStatusline,
                &ComponentStatus::Installed { version: None }
            ),
            "claude-statusline installed"
        );
        assert_eq!(
            format_status(
                ComponentKind::Rust,
                &ComponentStatus::Installed {
                    version: Some("1.90.0".to_string())
                }
            ),
            "rust installed 1.90.0"
        );
        assert_eq!(
            format_status(ComponentKind::Go, &ComponentStatus::Unmanaged),
            "go unmanaged"
        );
    }

    #[cfg(unix)]
    #[test]
    fn claude_statusline_install_overwrites_owned_state_and_preserves_other_settings() {
        use std::os::unix::fs::PermissionsExt;

        let (_root, tenant) = initialized_tenant();
        let claude = tenant.home_dir.join(".claude");
        fs::write(claude.join("statusline.sh"), "#!/bin/sh\necho custom\n").unwrap();
        fs::write(
            claude.join("settings.json"),
            r#"{"keep":true,"statusLine":{"type":"command","command":"custom"}}"#,
        )
        .unwrap();
        assert_eq!(
            inspect(ComponentKind::ClaudeStatusline, &tenant.home_dir).unwrap(),
            ComponentStatus::Modified
        );

        install_claude_statusline(&tenant).unwrap();

        assert_eq!(
            inspect(ComponentKind::ClaudeStatusline, &tenant.home_dir).unwrap(),
            ComponentStatus::Installed { version: None }
        );
        assert_eq!(
            fs::read(claude.join("statusline.sh")).unwrap(),
            CLAUDE_STATUSLINE
        );
        assert_eq!(
            fs::metadata(claude.join("statusline.sh"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o755
        );
        let settings: Value =
            serde_json::from_slice(&fs::read(claude.join("settings.json")).unwrap()).unwrap();
        assert_eq!(settings["keep"], true);
        assert_eq!(
            settings["statusLine"],
            json!({
                "type": "command",
                "command": "bash ~/.claude/statusline.sh"
            })
        );

        let script_before = fs::read(claude.join("statusline.sh")).unwrap();
        let settings_before = fs::read(claude.join("settings.json")).unwrap();
        install_claude_statusline(&tenant).unwrap();
        assert_eq!(
            fs::read(claude.join("statusline.sh")).unwrap(),
            script_before
        );
        assert_eq!(
            fs::read(claude.join("settings.json")).unwrap(),
            settings_before
        );
    }

    #[test]
    fn codex_statusline_install_preserves_unrelated_toml_and_comments() {
        let (_root, tenant) = initialized_tenant();
        let config = tenant.home_dir.join(".codex/config.toml");
        fs::write(
            &config,
            "# keep this comment\nmodel = \"custom\"\n\n[tui]\nanimations = false\nstatus_line = [\"old\"]\nstatus_line_use_colors = false\n",
        )
        .unwrap();

        install_codex_statusline(&tenant).unwrap();

        let content = fs::read_to_string(&config).unwrap();
        assert!(content.contains("# keep this comment"), "{content}");
        assert!(content.contains("model = \"custom\""), "{content}");
        assert!(content.contains("animations = false"), "{content}");
        assert_eq!(
            inspect(ComponentKind::CodexStatusline, &tenant.home_dir).unwrap(),
            ComponentStatus::Installed { version: None }
        );
    }

    #[test]
    fn codex_statusline_install_rejects_an_unowned_non_table_tui() {
        let (_root, tenant) = initialized_tenant();
        let config = tenant.home_dir.join(".codex/config.toml");
        let original = "model = \"custom\"\ntui = \"keep\"\n";
        fs::write(&config, original).unwrap();

        assert_eq!(
            inspect(ComponentKind::CodexStatusline, &tenant.home_dir).unwrap(),
            ComponentStatus::NotInstalled
        );
        let error = install_codex_statusline(&tenant).unwrap_err().to_string();
        assert!(error.contains("refusing to replace unowned"), "{error}");
        assert_eq!(fs::read_to_string(config).unwrap(), original);
    }

    #[cfg(unix)]
    #[test]
    fn partial_statusline_installations_are_incomplete() {
        use std::os::unix::fs::PermissionsExt;

        let (_root, tenant) = initialized_tenant();
        let claude_script = tenant.home_dir.join(".claude/statusline.sh");
        fs::write(&claude_script, CLAUDE_STATUSLINE).unwrap();
        fs::set_permissions(&claude_script, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            inspect(ComponentKind::ClaudeStatusline, &tenant.home_dir).unwrap(),
            ComponentStatus::Incomplete
        );

        fs::write(
            tenant.home_dir.join(".codex/config.toml"),
            "[tui]\nstatus_line_use_colors = true\n",
        )
        .unwrap();
        assert_eq!(
            inspect(ComponentKind::CodexStatusline, &tenant.home_dir).unwrap(),
            ComponentStatus::Incomplete
        );
    }

    #[test]
    fn profile_and_statusline_double_ownership_is_rejected_both_ways() {
        let (_root, tenant) = initialized_tenant();
        let selected = tenant.for_agent(AgentKind::Codex);

        install_codex_statusline(&tenant).unwrap();
        crate::profile::create_profile(&selected, "overlap").unwrap();
        fs::write(
            selected.profile_file("overlap", "config.toml"),
            "[tui]\nstatus_line = [\"profile\"]\n",
        )
        .unwrap();
        let error = crate::profile::activate_profile(&selected, "overlap", false)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("Component path /config/tui/status_line"),
            "{error}"
        );

        remove_confirmed(&tenant, ComponentKind::CodexStatusline).unwrap();
        crate::profile::activate_profile(&selected, "overlap", false).unwrap();
        let error = install_codex_statusline(&tenant).unwrap_err().to_string();
        assert!(error.contains("Active Agent Profile 'overlap'"), "{error}");
    }

    #[test]
    fn statusline_survives_profile_reconcile_switch_and_deactivate() {
        let (_root, tenant) = initialized_tenant();
        let selected = tenant.for_agent(AgentKind::Codex);
        for (name, model) in [("one", "one"), ("two", "two")] {
            crate::profile::create_profile(&selected, name).unwrap();
            fs::write(
                selected.profile_file(name, "config.toml"),
                format!("model = \"{model}\"\n"),
            )
            .unwrap();
        }

        crate::profile::activate_profile(&selected, "one", false).unwrap();
        install_codex_statusline(&tenant).unwrap();
        assert_eq!(
            inspect(ComponentKind::CodexStatusline, &tenant.home_dir).unwrap(),
            ComponentStatus::Installed { version: None }
        );
        let config = selected.state_file("config.toml");
        let mut working = fs::read_to_string(&config).unwrap();
        working.push_str("working_only = true\n");
        fs::write(&config, working).unwrap();
        crate::profile::reconcile_profile(
            &selected,
            &crate::cli::ReconcileArgs {
                take_profile: Vec::new(),
                take_config: Vec::new(),
                take_profile_all: false,
                take_config_all: false,
            },
        )
        .unwrap();
        assert!(
            fs::read_to_string(selected.profile_file("one", "config.toml"))
                .unwrap()
                .contains("working_only")
        );
        assert!(
            !fs::read_to_string(selected.profile_file("one", "config.toml"))
                .unwrap()
                .contains("status_line")
        );

        crate::profile::activate_profile(&selected, "two", false).unwrap();
        assert_eq!(
            inspect(ComponentKind::CodexStatusline, &tenant.home_dir).unwrap(),
            ComponentStatus::Installed { version: None }
        );
        crate::profile::deactivate_profile(&selected, false).unwrap();
        assert_eq!(
            inspect(ComponentKind::CodexStatusline, &tenant.home_dir).unwrap(),
            ComponentStatus::Installed { version: None }
        );
    }

    #[cfg(unix)]
    #[test]
    fn profile_deactivation_with_a_component_restores_the_base_file_mode() {
        use std::os::unix::fs::PermissionsExt;

        let (_root, tenant) = initialized_tenant();
        let selected = tenant.for_agent(AgentKind::Codex);
        let config = selected.state_file("config.toml");
        fs::write(&config, "model = \"base\"\n").unwrap();
        fs::set_permissions(&config, fs::Permissions::from_mode(0o640)).unwrap();
        install_codex_statusline(&tenant).unwrap();

        crate::profile::create_profile(&selected, "custom").unwrap();
        fs::write(
            selected.profile_file("custom", "config.toml"),
            "model = \"profile\"\n",
        )
        .unwrap();
        crate::profile::activate_profile(&selected, "custom", false).unwrap();
        assert_eq!(
            fs::metadata(&config).unwrap().permissions().mode() & 0o777,
            0o600
        );

        crate::profile::deactivate_profile(&selected, false).unwrap();

        assert_eq!(
            fs::metadata(&config).unwrap().permissions().mode() & 0o777,
            0o640
        );
        assert_eq!(
            inspect(ComponentKind::CodexStatusline, &tenant.home_dir).unwrap(),
            ComponentStatus::Installed { version: None }
        );
    }

    #[cfg(unix)]
    #[test]
    fn absent_component_configuration_stays_absent_after_profile_deactivation() {
        use std::os::unix::fs::PermissionsExt;

        let (_root, tenant) = initialized_tenant();
        let selected = tenant.for_agent(AgentKind::Claude);
        let script = selected.state_file("statusline.sh");
        let settings = selected.state_file("settings.json");
        fs::write(&script, CLAUDE_STATUSLINE).unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            inspect(ComponentKind::ClaudeStatusline, &tenant.home_dir).unwrap(),
            ComponentStatus::Incomplete
        );

        crate::profile::create_profile(&selected, "custom").unwrap();
        crate::profile::activate_profile(&selected, "custom", false).unwrap();
        assert!(settings.is_file());

        crate::profile::deactivate_profile(&selected, false).unwrap();

        assert!(!settings.exists());
        assert!(script.is_file());
        assert_eq!(
            inspect(ComponentKind::ClaudeStatusline, &tenant.home_dir).unwrap(),
            ComponentStatus::Incomplete
        );
    }

    #[test]
    fn component_remove_is_guarded_selective_and_idempotent() {
        let (_root, tenant) = initialized_tenant();
        let claude = tenant.home_dir.join(".claude");
        fs::write(claude.join("settings.json"), "{\"keep\":true}\n").unwrap();
        install_claude_statusline(&tenant).unwrap();

        if !io::stdin().is_terminal() {
            let error = remove_from_tenant(
                &tenant,
                ComponentKind::ClaudeStatusline,
                RemovalOptions {
                    discard_changes: false,
                    skip_confirmation: false,
                },
            )
            .unwrap_err()
            .to_string();
            assert!(error.contains("without --yes"), "{error}");
            assert_eq!(
                inspect(ComponentKind::ClaudeStatusline, &tenant.home_dir).unwrap(),
                ComponentStatus::Installed { version: None },
                "refusing non-interactive removal must preserve the Component"
            );
        }

        remove_confirmed(&tenant, ComponentKind::ClaudeStatusline).unwrap();
        remove_confirmed(&tenant, ComponentKind::ClaudeStatusline).unwrap();
        assert!(!claude.join("statusline.sh").exists());
        let settings: Value =
            serde_json::from_slice(&fs::read(claude.join("settings.json")).unwrap()).unwrap();
        assert_eq!(settings["keep"], true);
        assert!(settings.get("statusLine").is_none());

        fs::write(claude.join("statusline.sh"), "custom\n").unwrap();
        assert_eq!(
            inspect(ComponentKind::ClaudeStatusline, &tenant.home_dir).unwrap(),
            ComponentStatus::Modified
        );
        let error = remove_confirmed(&tenant, ComponentKind::ClaudeStatusline)
            .unwrap_err()
            .to_string();
        assert!(error.contains("is modified"), "{error}");
        assert!(error.contains("--discard-changes"), "{error}");
        remove_discarding(&tenant, ComponentKind::ClaudeStatusline).unwrap();
    }

    #[test]
    fn codex_statusline_discard_removes_only_component_owned_keys() {
        let (_root, tenant) = initialized_tenant();
        let config = tenant.home_dir.join(".codex/config.toml");
        fs::write(
            &config,
            "# keep this comment\nmodel = \"custom\"\n\n[tui]\nanimations = false\n",
        )
        .unwrap();
        install_codex_statusline(&tenant).unwrap();
        let mut document = fs::read_to_string(&config)
            .unwrap()
            .parse::<toml_edit::DocumentMut>()
            .unwrap();
        let mut customized = toml_edit::Array::new();
        customized.push("user-customized");
        document["tui"]["status_line"] = toml_edit::value(customized);
        fs::write(&config, document.to_string()).unwrap();

        let error = remove_confirmed(&tenant, ComponentKind::CodexStatusline)
            .unwrap_err()
            .to_string();
        assert!(error.contains("modified"), "{error}");

        remove_discarding(&tenant, ComponentKind::CodexStatusline).unwrap();

        let content = fs::read_to_string(&config).unwrap();
        assert!(content.contains("# keep this comment"), "{content}");
        let document = content.parse::<toml_edit::DocumentMut>().unwrap();
        assert_eq!(document["model"].as_str(), Some("custom"));
        let tui = document["tui"].as_table_like().unwrap();
        assert_eq!(
            tui.get("animations").and_then(toml_edit::Item::as_bool),
            Some(false)
        );
        assert!(tui.get("status_line").is_none());
        assert!(tui.get("status_line_use_colors").is_none());
    }

    #[test]
    fn unowned_toolchain_paths_are_not_claimed_or_removed() {
        let (_root, tenant) = initialized_tenant();
        let manual_rust = tenant.home_dir.join(".cargo/bin/rustc");
        let manual_go = tenant.home_dir.join(".gopath/bin/custom-go-tool");
        fs::create_dir_all(manual_rust.parent().unwrap()).unwrap();
        fs::create_dir_all(manual_go.parent().unwrap()).unwrap();
        fs::write(&manual_rust, b"manual rust").unwrap();
        fs::write(&manual_go, b"manual go").unwrap();

        assert_eq!(
            inspect(ComponentKind::Rust, &tenant.home_dir).unwrap(),
            ComponentStatus::NotInstalled
        );
        assert_eq!(
            inspect(ComponentKind::Go, &tenant.home_dir).unwrap(),
            ComponentStatus::NotInstalled
        );

        remove_confirmed(&tenant, ComponentKind::Rust).unwrap();
        remove_confirmed(&tenant, ComponentKind::Go).unwrap();
        assert_eq!(fs::read(&manual_rust).unwrap(), b"manual rust");
        assert_eq!(fs::read(&manual_go).unwrap(), b"manual go");
    }

    #[test]
    fn toolchain_remove_preserves_user_caches_and_unrelated_commands() {
        let (_root, tenant) = initialized_tenant();
        let cargo_bin = tenant.home_dir.join(".cargo/bin");
        fs::create_dir_all(&cargo_bin).unwrap();
        fs::write(cargo_bin.join("custom-command"), "keep").unwrap();
        fs::write(cargo_bin.join("cargo"), "proxy").unwrap();
        fs::create_dir_all(tenant.home_dir.join(".rustup")).unwrap();
        fs::write(
            tenant.home_dir.join(".rustup/settings.toml"),
            "default_toolchain = \"nightly-x86_64-unknown-linux-gnu\"\n",
        )
        .unwrap();
        let error = remove_confirmed(&tenant, ComponentKind::Rust)
            .unwrap_err()
            .to_string();
        assert!(error.contains("is unmanaged"), "{error}");
        assert!(error.contains("--discard-changes"), "{error}");
        remove_discarding(&tenant, ComponentKind::Rust).unwrap();
        assert!(!tenant.home_dir.join(".rustup").exists());
        assert!(!cargo_bin.join("cargo").exists());
        assert_eq!(
            fs::read_to_string(cargo_bin.join("custom-command")).unwrap(),
            "keep"
        );

        fs::create_dir_all(tenant.home_dir.join(".goroot")).unwrap();
        fs::create_dir_all(tenant.home_dir.join(".gopath")).unwrap();
        fs::write(tenant.home_dir.join(".gopath/keep"), "keep").unwrap();
        remove_confirmed(&tenant, ComponentKind::Go).unwrap();
        assert!(!tenant.home_dir.join(".goroot").exists());
        assert_eq!(
            fs::read_to_string(tenant.home_dir.join(".gopath/keep")).unwrap(),
            "keep"
        );
    }

    #[cfg(unix)]
    #[test]
    fn rust_remove_rejects_a_symlinked_cargo_ancestor_before_deleting_anything() {
        use std::os::unix::fs::symlink;

        let (_root, tenant) = initialized_tenant();
        fs::create_dir(tenant.home_dir.join(".rustup")).unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::create_dir(outside.path().join("bin")).unwrap();
        let outside_proxy = outside.path().join("bin/rustup");
        fs::write(&outside_proxy, "keep").unwrap();
        symlink(outside.path(), tenant.home_dir.join(".cargo")).unwrap();

        let error = remove_confirmed(&tenant, ComponentKind::Rust)
            .unwrap_err()
            .to_string();

        assert!(
            error.contains("Cargo Home is not a real directory"),
            "{error}"
        );
        assert!(tenant.home_dir.join(".rustup").is_dir());
        assert_eq!(fs::read_to_string(outside_proxy).unwrap(), "keep");
    }

    #[cfg(unix)]
    #[test]
    fn rust_remove_prevalidates_every_proxy_before_removing_anything() {
        use std::os::unix::fs::symlink;

        let (_root, tenant) = initialized_tenant();
        let rustup_home = tenant.home_dir.join(".rustup");
        let cargo_bin = tenant.home_dir.join(".cargo/bin");
        fs::create_dir(&rustup_home).unwrap();
        fs::create_dir_all(&cargo_bin).unwrap();
        let rustup_proxy = cargo_bin.join("rustup");
        fs::write(&rustup_proxy, "keep proxy").unwrap();
        let outside = tempfile::tempdir().unwrap();
        let outside_rustc = outside.path().join("rustc");
        fs::write(&outside_rustc, "outside").unwrap();
        symlink(&outside_rustc, cargo_bin.join("rustc")).unwrap();

        let error = remove_confirmed(&tenant, ComponentKind::Rust)
            .unwrap_err()
            .to_string();

        assert!(
            error.contains("rustup proxy is not a regular file"),
            "{error}"
        );
        assert!(rustup_home.is_dir());
        assert_eq!(fs::read_to_string(rustup_proxy).unwrap(), "keep proxy");
        assert_eq!(fs::read_to_string(outside_rustc).unwrap(), "outside");
    }

    #[cfg(unix)]
    #[test]
    fn go_remove_rejects_a_symlinked_sdk_without_touching_its_target() {
        use std::os::unix::fs::symlink;

        let (_root, tenant) = initialized_tenant();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("VERSION"), b"go1.25.6\n").unwrap();
        fs::write(outside.path().join("keep"), b"outside sdk").unwrap();
        symlink(outside.path(), tenant.home_dir.join(".goroot")).unwrap();

        let error = remove_discarding(&tenant, ComponentKind::Go)
            .unwrap_err()
            .to_string();

        assert!(error.contains("Go root is not a real directory"), "{error}");
        assert_eq!(
            fs::read(outside.path().join("keep")).unwrap(),
            b"outside sdk"
        );
    }

    #[test]
    fn statusline_install_does_not_rewrite_active_profile_metadata() {
        let (_root, tenant) = initialized_tenant();
        let selected = tenant.for_agent(AgentKind::Codex);
        crate::profile::create_profile(&selected, "custom").unwrap();
        crate::profile::activate_profile(&selected, "custom", false).unwrap();
        let metadata = fs::read(selected.metadata_file()).unwrap();

        install_codex_statusline(&tenant).unwrap();

        assert_eq!(fs::read(selected.metadata_file()).unwrap(), metadata);
        let config = fs::read_to_string(selected.state_file("config.toml")).unwrap();
        assert!(config.contains("status_line ="), "{config}");
    }

    #[test]
    fn toolchain_statuses_are_derived_from_native_files() {
        let (_root, tenant) = initialized_tenant();
        assert_eq!(
            inspect(ComponentKind::Rust, &tenant.home_dir).unwrap(),
            ComponentStatus::NotInstalled
        );
        assert_eq!(
            inspect(ComponentKind::Go, &tenant.home_dir).unwrap(),
            ComponentStatus::NotInstalled
        );

        let toolchain = "1.90.0-x86_64-unknown-linux-gnu";
        write_rust_state(&tenant.home_dir, toolchain, true);
        write_go_state(&tenant.home_dir, "go1.25.6", true);
        assert_eq!(
            inspect(ComponentKind::Rust, &tenant.home_dir).unwrap(),
            ComponentStatus::Installed {
                version: Some("1.90.0".to_string())
            }
        );
        assert_eq!(
            inspect(ComponentKind::Go, &tenant.home_dir).unwrap(),
            ComponentStatus::Installed {
                version: Some("1.25.6".to_string())
            }
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                tenant
                    .home_dir
                    .join(".rustup/toolchains")
                    .join(toolchain)
                    .join("bin/rustc"),
                fs::Permissions::from_mode(0o644),
            )
            .unwrap();
            fs::set_permissions(
                tenant.home_dir.join(".goroot/bin/go"),
                fs::Permissions::from_mode(0o644),
            )
            .unwrap();
            assert_eq!(
                inspect(ComponentKind::Rust, &tenant.home_dir).unwrap(),
                ComponentStatus::Incomplete
            );
            assert_eq!(
                inspect(ComponentKind::Go, &tenant.home_dir).unwrap(),
                ComponentStatus::Incomplete
            );
        }

        fs::write(
            tenant.home_dir.join(".rustup/settings.toml"),
            "default_toolchain = \"nightly-x86_64-unknown-linux-gnu\"\n",
        )
        .unwrap();
        fs::write(tenant.home_dir.join(".goroot/VERSION"), "go1.26rc1\n").unwrap();
        assert_eq!(
            inspect(ComponentKind::Rust, &tenant.home_dir).unwrap(),
            ComponentStatus::Unmanaged
        );
        assert_eq!(
            inspect(ComponentKind::Go, &tenant.home_dir).unwrap(),
            ComponentStatus::Unmanaged
        );

        write_rust_state(&tenant.home_dir, "1.90.0-custom", true);
        assert_eq!(
            inspect(ComponentKind::Rust, &tenant.home_dir).unwrap(),
            ComponentStatus::Unmanaged
        );
    }

    #[test]
    fn incomplete_stable_toolchains_are_reported_as_incomplete() {
        let (_root, tenant) = initialized_tenant();
        write_rust_state(&tenant.home_dir, "1.90.0-x86_64-unknown-linux-gnu", false);
        write_go_state(&tenant.home_dir, "go1.25.6", false);
        assert_eq!(
            inspect(ComponentKind::Rust, &tenant.home_dir).unwrap(),
            ComponentStatus::Incomplete
        );
        assert_eq!(
            inspect(ComponentKind::Go, &tenant.home_dir).unwrap(),
            ComponentStatus::Incomplete
        );
    }

    #[test]
    fn explicit_healthy_toolchain_version_skips_before_docker_lookup() {
        let (_root, tenant) = initialized_tenant();
        write_rust_state(&tenant.home_dir, "1.90.0-x86_64-unknown-linux-gnu", true);
        let component = "rust@1.90.0".parse::<ComponentSpec>().unwrap();

        assert_eq!(install_toolchain(&tenant, &component).unwrap(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn status_inspection_rejects_symlinked_owned_paths() {
        use std::os::unix::fs::symlink;

        let (_root, tenant) = initialized_tenant();
        let outside = tempfile::NamedTempFile::new().unwrap();
        symlink(
            outside.path(),
            tenant.home_dir.join(".claude/statusline.sh"),
        )
        .unwrap();
        let error = format!(
            "{:#}",
            inspect(ComponentKind::ClaudeStatusline, &tenant.home_dir).unwrap_err()
        );
        assert!(error.contains("not a regular file"), "{error}");
    }

    #[cfg(unix)]
    fn write_fake_docker(dir: &Path) {
        crate::testutil::write_stub_script(
            dir,
            "docker",
            r#"#!/bin/sh
if [ -n "$AIBOX_FAKE_DOCKER_LOG" ]; then
    printf '%s\n' "$*" >> "$AIBOX_FAKE_DOCKER_LOG"
fi
if [ "$1" = image ] && [ "$2" = inspect ]; then
    [ "$AIBOX_FAKE_DOCKER_MODE" = missing ] && exit 1
    printf 'sha256:fake\n'
    exit 0
fi
if [ "$1" = image ] && [ "$2" = ls ]; then
    exit 0
fi
if [ "$1" = container ] && [ "$2" = ls ]; then
    exit 0
fi
if [ "$1" = run ]; then
    shift
    while [ "$#" -gt 0 ]; do
        if [ "$1" = --cidfile ]; then
            printf 'fake-container\n' > "$2"
            exit 0
        fi
        shift
    done
fi
exit 99
"#,
        );
    }

    #[cfg(unix)]
    fn run_installer(script: &str, home: &Path, bin: &Path, version: &str) -> std::process::Output {
        let path = std::env::join_paths(std::iter::once(bin.to_path_buf()).chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
        ))
        .unwrap();
        Command::new("bash")
            .arg(script)
            .arg(version)
            .env("HOME", home)
            .env("PATH", path)
            .output()
            .unwrap()
    }

    #[cfg(unix)]
    #[test]
    fn rust_installer_skips_same_version_and_uninstalls_before_switching() {
        let _env_lock = crate::test_env_lock();
        let scratch = tempfile::tempdir().unwrap();
        let home = scratch.path().join("home");
        let bin = scratch.path().join("bin");
        let log = scratch.path().join("rustup.log");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&bin).unwrap();
        crate::testutil::write_stub_script(
            &bin,
            "curl",
            r#"#!/bin/sh
out=
while [ "$#" -gt 0 ]; do
    if [ "$1" = -o ]; then out=$2; shift 2; else shift; fi
done
cat > "$out" <<'BOOTSTRAP'
#!/bin/sh
mkdir -p "$CARGO_HOME/bin" "$RUSTUP_HOME"
cp "$AIBOX_FAKE_RUSTUP" "$CARGO_HOME/bin/rustup"
chmod +x "$CARGO_HOME/bin/rustup"
BOOTSTRAP
"#,
        );
        crate::testutil::write_stub_script(
            &bin,
            "fake-rustup",
            r#"#!/bin/sh
printf '%s\n' "$*" >> "$AIBOX_FAKE_RUSTUP_LOG"
case "$1 $2" in
    "toolchain list")
        old=$(sed -n 's/^default_toolchain = "\(.*\)"/\1/p' "$RUSTUP_HOME/settings.toml" 2>/dev/null)
        [ -n "$old" ] && [ -d "$RUSTUP_HOME/toolchains/$old" ] && printf '%s (default)\n' "$old"
        ;;
    "toolchain uninstall")
        rm -rf "$RUSTUP_HOME/toolchains/$3"
        ;;
    "toolchain install")
        mkdir -p "$RUSTUP_HOME/toolchains/$3-x86_64-unknown-linux-gnu/bin"
        cat > "$RUSTUP_HOME/toolchains/$3-x86_64-unknown-linux-gnu/bin/rustc" <<EOF
#!/bin/sh
printf 'rustc $3\n'
EOF
        chmod +x "$RUSTUP_HOME/toolchains/$3-x86_64-unknown-linux-gnu/bin/rustc"
        cp "$RUSTUP_HOME/toolchains/$3-x86_64-unknown-linux-gnu/bin/rustc" "$CARGO_HOME/bin/rustc"
        ;;
    "default "*)
        printf 'version = "12"\ndefault_toolchain = "%s-x86_64-unknown-linux-gnu"\n' "$2" > "$RUSTUP_HOME/settings.toml"
        ;;
esac
"#,
        );
        let _rustup = crate::testutil::EnvGuard::set(
            "AIBOX_FAKE_RUSTUP",
            bin.join("fake-rustup").as_os_str(),
        );
        let _log = crate::testutil::EnvGuard::set("AIBOX_FAKE_RUSTUP_LOG", log.as_os_str());

        let first = run_installer(
            &format!("{}/assets/install-rust.sh", env!("CARGO_MANIFEST_DIR")),
            &home,
            &bin,
            "1.90.0",
        );
        assert!(
            first.status.success(),
            "{}",
            String::from_utf8_lossy(&first.stderr)
        );
        let first_log = fs::read_to_string(&log).unwrap();

        let same = run_installer(
            &format!("{}/assets/install-rust.sh", env!("CARGO_MANIFEST_DIR")),
            &home,
            &bin,
            "1.90.0",
        );
        assert!(
            same.status.success(),
            "{}",
            String::from_utf8_lossy(&same.stderr)
        );
        assert!(String::from_utf8_lossy(&same.stdout).contains("already installed"));
        let same_log = fs::read_to_string(&log).unwrap();
        assert_eq!(
            same_log,
            format!("{first_log}run 1.90.0-x86_64-unknown-linux-gnu rustc --version\n")
        );

        let switch = run_installer(
            &format!("{}/assets/install-rust.sh", env!("CARGO_MANIFEST_DIR")),
            &home,
            &bin,
            "1.89.0",
        );
        assert!(
            switch.status.success(),
            "{}",
            String::from_utf8_lossy(&switch.stderr)
        );
        let switched = fs::read_to_string(&log).unwrap();
        let uninstall = switched.find("toolchain uninstall 1.90.0-").unwrap();
        let install = switched.rfind("toolchain install 1.89.0").unwrap();
        assert!(uninstall < install, "{switched}");
        assert!(home.join(".cargo").is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn go_installer_verifies_and_replaces_only_goroot() {
        let _env_lock = crate::test_env_lock();
        let scratch = tempfile::tempdir().unwrap();
        let home = scratch.path().join("home");
        let bin = scratch.path().join("bin");
        let fixture = scratch.path().join("fixture");
        let archive = scratch.path().join("go.tar.gz");
        let metadata = scratch.path().join("releases.json");
        fs::create_dir_all(home.join(".goroot")).unwrap();
        fs::create_dir_all(home.join(".gopath")).unwrap();
        fs::write(home.join(".goroot/old"), "old").unwrap();
        fs::write(home.join(".gopath/keep"), "keep").unwrap();
        fs::create_dir_all(fixture.join("go/bin")).unwrap();
        fs::write(fixture.join("go/VERSION"), "go1.25.6\n").unwrap();
        crate::testutil::write_stub_script(
            &fixture.join("go/bin"),
            "go",
            "#!/bin/sh\nprintf 'go version go1.25.6 linux/amd64\n'\n",
        );
        let status = Command::new("tar")
            .args(["-C", fixture.to_str().unwrap(), "-czf"])
            .arg(&archive)
            .arg("go")
            .status()
            .unwrap();
        assert!(status.success());
        let checksum = Command::new("sha256sum").arg(&archive).output().unwrap();
        let checksum = String::from_utf8(checksum.stdout)
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .to_string();
        fs::write(
            &metadata,
            format!(
                r#"[{{"version":"go1.25.6","stable":true,"files":[{{"filename":"go1.25.6.linux-amd64.tar.gz","os":"linux","arch":"amd64","kind":"archive","sha256":"{checksum}"}}]}}]"#
            ),
        )
        .unwrap();
        fs::create_dir_all(&bin).unwrap();
        crate::testutil::write_stub_script(&bin, "dpkg", "#!/bin/sh\nprintf 'amd64\n'\n");
        crate::testutil::write_stub_script(
            &bin,
            "curl",
            r#"#!/bin/sh
url=
out=
while [ "$#" -gt 0 ]; do
    case "$1" in
        -o) out=$2; shift 2 ;;
        http*) url=$1; shift ;;
        *) shift ;;
    esac
done
case "$url" in
    *mode=json*) cp "$AIBOX_FAKE_GO_METADATA" "$out" ;;
    *) cp "$AIBOX_FAKE_GO_ARCHIVE" "$out" ;;
esac
"#,
        );
        let _metadata =
            crate::testutil::EnvGuard::set("AIBOX_FAKE_GO_METADATA", metadata.as_os_str());
        let _archive = crate::testutil::EnvGuard::set("AIBOX_FAKE_GO_ARCHIVE", archive.as_os_str());

        let output = run_installer(
            &format!("{}/assets/install-go.sh", env!("CARGO_MANIFEST_DIR")),
            &home,
            &bin,
            "1.25.6",
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            fs::read_to_string(home.join(".goroot/VERSION")).unwrap(),
            "go1.25.6\n"
        );
        assert!(!home.join(".goroot/old").exists());
        assert_eq!(
            fs::read_to_string(home.join(".gopath/keep")).unwrap(),
            "keep"
        );

        let same = run_installer(
            &format!("{}/assets/install-go.sh", env!("CARGO_MANIFEST_DIR")),
            &home,
            &bin,
            "1.25.6",
        );
        assert!(
            same.status.success(),
            "{}",
            String::from_utf8_lossy(&same.stderr)
        );
        assert!(String::from_utf8_lossy(&same.stdout).contains("already installed"));
    }

    #[cfg(unix)]
    #[test]
    fn toolchain_install_uses_the_shared_image_and_home_only_mount() {
        let _env_lock = crate::test_env_lock();
        let root = tempfile::tempdir().unwrap();
        let bin = tempfile::tempdir().unwrap();
        let log = root.path().join("docker.log");
        write_fake_docker(bin.path());
        let _root = crate::testutil::EnvGuard::set("AIBOX_ROOT", root.path().as_os_str());
        let _path = crate::testutil::EnvGuard::prepend_path(bin.path());
        let _log = crate::testutil::EnvGuard::set("AIBOX_FAKE_DOCKER_LOG", log.as_os_str());
        let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();
        let component = "rust@1.90.0".parse::<ComponentSpec>().unwrap();

        assert_eq!(install_toolchain(&tenant, &component).unwrap(), 0);

        let log = fs::read_to_string(log).unwrap();
        assert!(log.contains("image inspect"), "{log}");
        assert!(
            log.contains(&format!(
                "{}:/home/aibox",
                root.path().join("tenants/work").display()
            )),
            "{log}"
        );
        assert!(!log.contains("/workspace"), "{log}");
        assert!(log.contains("aibox-rust-installer 1.90.0"), "{log}");
    }

    #[cfg(unix)]
    #[test]
    fn missing_image_does_not_initialize_a_toolchain_tenant() {
        let _env_lock = crate::test_env_lock();
        let root = tempfile::tempdir().unwrap();
        let bin = tempfile::tempdir().unwrap();
        write_fake_docker(bin.path());
        let _root = crate::testutil::EnvGuard::set("AIBOX_ROOT", root.path().as_os_str());
        let _path = crate::testutil::EnvGuard::prepend_path(bin.path());
        let _mode = crate::testutil::EnvGuard::set("AIBOX_FAKE_DOCKER_MODE", "missing");
        let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();
        let component = "go@1.25.6".parse::<ComponentSpec>().unwrap();

        let error = install_toolchain(&tenant, &component)
            .unwrap_err()
            .to_string();

        assert!(error.contains("build it first"), "{error}");
        assert!(!tenant.home_dir.exists());
    }
}
