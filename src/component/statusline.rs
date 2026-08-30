//! Claude and Codex statusline inspection, installation, and removal.

use super::ComponentStatus;
use super::native::{capture_limited, executable_mode_is_current, parse_json_config, write_atomic};
use crate::agent::AgentKind;
use crate::foundation::safe_fs::FileSnapshot;
use crate::tenant::{Tenant, TenantAgent};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::path::Path;

const CLAUDE_STATUSLINE: &[u8] = include_bytes!("../../assets/claude-statusline.sh");
const CLAUDE_STATUSLINE_SCRIPT: &str = "statusline.sh";
const CODEX_STATUSLINE_ITEMS: [&str; 5] = [
    "model-with-reasoning",
    "current-dir",
    "git-branch",
    "context-window-size",
    "context-used",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StatuslinePartState {
    Absent,
    Current,
    Modified,
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

pub(super) fn inspect_claude_statusline(home: &Path) -> Result<ComponentStatus> {
    let dir = home.join(AgentKind::Claude.state_dir_name());
    if !crate::foundation::safe_fs::real_dir_exists(&dir, "Claude state directory")? {
        return Ok(ComponentStatus::NotInstalled);
    }
    let script = capture_limited(
        &dir.join(CLAUDE_STATUSLINE_SCRIPT),
        "Claude statusline script",
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

pub(super) fn inspect_codex_statusline(home: &Path) -> Result<ComponentStatus> {
    let dir = home.join(AgentKind::Codex.state_dir_name());
    if !crate::foundation::safe_fs::real_dir_exists(&dir, "Codex state directory")? {
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

pub(super) fn install_claude_statusline(tenant: &Tenant) -> Result<i32> {
    let selected = prepare_statusline_install(tenant, AgentKind::Claude)?;

    let script_path = selected.state_file(CLAUDE_STATUSLINE_SCRIPT);
    let settings_path = selected.state_file(AgentKind::Claude.main_config_file());
    let script = capture_limited(&script_path, "Claude statusline script")?;
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

pub(super) fn install_codex_statusline(tenant: &Tenant) -> Result<i32> {
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

pub(super) fn remove_claude_statusline(tenant: &Tenant) -> Result<()> {
    let selected = tenant.for_agent(AgentKind::Claude);
    let script = selected.state_file(CLAUDE_STATUSLINE_SCRIPT);
    crate::foundation::safe_fs::remove_real_file_if_exists(&script, "Claude statusline script")?;

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

pub(super) fn remove_codex_statusline(tenant: &Tenant) -> Result<()> {
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
