//! Direct Config file reveal, validation, editing, and filesystem writes.

use super::catalog::{
    ensure_named_config_main, ensure_safe_named_config, inspect_named_config_directory,
};
use super::definition::NamedConfigDefinition;
use super::files::{capture_optional_agent_file, file_revision, write_atomic};
use super::visual::{
    CodexAuthInspection, CustomProviderInput, VisualAuthInput, VisualConfigOptionInput,
    VisualConfigState, inspect_codex_auth, inspect_visual_config, render_visual_auth,
    render_visual_main,
};
use super::{
    ConfigDiagnostic, ConfigEdit, ConfigFile, ConfigFileSnapshot, ConfigSaveResult, ConfigTarget,
    MAX_CONFIG_BYTES, NamedConfigName,
};
use crate::application_error::{ApplicationErrorKind, application_error};
use crate::foundation::safe_fs::FileSnapshot;
use crate::tenant::TenantAgent;
use anyhow::{Context, Result, bail};
use serde_json::{Map, Value};

pub(crate) fn visual_config_state(
    selected: &TenantAgent,
    config: &NamedConfigName,
    content: &str,
) -> Result<VisualConfigState> {
    ensure_named_config_main(selected, config)?;
    inspect_visual_config(selected.agent(), content)
}

pub(crate) fn inspect_named_codex_auth(
    selected: &TenantAgent,
    config: &NamedConfigName,
    content: &str,
) -> Result<CodexAuthInspection> {
    ensure_safe_named_config(selected, config)?;
    inspect_codex_auth(content, None)
}

pub(crate) fn config_file_warnings(
    selected: &TenantAgent,
    _config: &NamedConfigName,
    file: &str,
    content: &[u8],
) -> Result<Vec<String>> {
    let text = std::str::from_utf8(content)
        .with_context(|| format!("Named Config {file} is not valid UTF-8"))?;
    NamedConfigDefinition::validate_file_with_warnings(selected.agent(), file, text)
}

pub(crate) fn diagnose_config_file(
    selected: &TenantAgent,
    target: &ConfigTarget,
    file: ConfigFile,
    content: &[u8],
) -> Result<Vec<ConfigDiagnostic>> {
    let _ = read_config_file_target(selected, target, file)?;
    let text = match std::str::from_utf8(content) {
        Ok(text) => text,
        Err(error) => {
            return Ok(vec![ConfigDiagnostic {
                message: format!("configuration is not valid UTF-8: {error}"),
                line: 1,
                column: 1,
            }]);
        }
    };
    let result = if target.is_current() {
        if file == ConfigFile::Main {
            selected.agent().parse_main_config(text).map(|_| ())
        } else {
            serde_json::from_str::<Value>(text)
                .context("parse Current Config auth.json")
                .map(|_| ())
        }
    } else {
        NamedConfigDefinition::validate_file(selected.agent(), file.as_str(selected.agent()), text)
    };
    Ok(result.err().map_or_else(Vec::new, |error| {
        let (line, column) = diagnostic_position(&error, text);
        vec![ConfigDiagnostic {
            message: format!("{error:#}"),
            line,
            column,
        }]
    }))
}

fn diagnostic_position(error: &anyhow::Error, source: &str) -> (usize, usize) {
    if let Some(json) = error.downcast_ref::<serde_json::Error>() {
        return (json.line(), json.column());
    }
    if let Some(toml) = error.downcast_ref::<toml_edit::TomlError>()
        && let Some(span) = toml.span()
    {
        let offset = span.start.min(source.len());
        let line = source[..offset]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count()
            + 1;
        let column = source[..offset]
            .rsplit('\n')
            .next()
            .map(|line| line.chars().count() + 1)
            .unwrap_or(1);
        return (line, column);
    }
    (1, 1)
}

pub(crate) fn read_config_file_target(
    selected: &TenantAgent,
    target: &ConfigTarget,
    file: ConfigFile,
) -> Result<ConfigFileSnapshot> {
    let file_name = file.as_str(selected.agent());
    let snapshot = if target.is_current() {
        capture_optional_agent_file(selected, file_name)?
    } else {
        let config = target
            .named()
            .expect("non-current ConfigTarget must have a name");
        ensure_safe_named_config(selected, config)?;
        let path = super::layout::named_config_file(selected, config, file);
        if crate::foundation::safe_fs::real_file_exists(&path, "Named Config file")? {
            FileSnapshot::capture_with_limit(&path, MAX_CONFIG_BYTES)?
        } else {
            FileSnapshot {
                present: false,
                content: Vec::new(),
                mode: None,
            }
        }
    };
    let content = if snapshot.present {
        snapshot.content.clone()
    } else {
        selected
            .agent()
            .empty_config_file(file_name)
            .context("Agent Config file contract is incomplete")?
            .as_bytes()
            .to_vec()
    };
    Ok(ConfigFileSnapshot {
        file: file_name.to_string(),
        exists: snapshot.present,
        revision: file_revision(snapshot.present, &snapshot.content),
        content,
    })
}

#[allow(dead_code)]
pub(crate) fn read_config_file(
    selected: &TenantAgent,
    config: Option<&str>,
    current: bool,
    file: &str,
) -> Result<ConfigFileSnapshot> {
    let target = ConfigTarget::from_wire(config, current)?;
    let file = ConfigFile::parse(selected.agent(), file)?;
    read_config_file_target(selected, &target, file)
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn save_config_file(
    selected: &TenantAgent,
    config: Option<&str>,
    current: bool,
    file: &str,
    expected_revision: &str,
    content: &[u8],
    visual: Option<&[VisualConfigOptionInput]>,
    visual_auth: Option<&VisualAuthInput>,
) -> Result<ConfigFileSnapshot> {
    save_config_file_with_linked(
        selected,
        config,
        current,
        file,
        expected_revision,
        content,
        None,
        visual,
        visual_auth,
    )
    .map(|result| result.snapshot)
}

#[allow(clippy::too_many_arguments)]
#[allow(dead_code)]
pub(crate) fn save_config_file_with_linked(
    selected: &TenantAgent,
    config: Option<&str>,
    current: bool,
    file: &str,
    expected_revision: &str,
    content: &[u8],
    custom_provider: Option<&CustomProviderInput>,
    visual: Option<&[VisualConfigOptionInput]>,
    visual_auth: Option<&VisualAuthInput>,
) -> Result<ConfigSaveResult> {
    let target = ConfigTarget::from_wire(config, current)?;
    let file = ConfigFile::parse(selected.agent(), file)?;
    let edit = ConfigEdit::from_wire(
        content.to_vec(),
        custom_provider.cloned(),
        visual.map(<[VisualConfigOptionInput]>::to_vec),
        visual_auth.cloned(),
    )?;
    save_config_file_target(selected, &target, file, expected_revision, edit)
}

pub(crate) fn save_config_file_target(
    selected: &TenantAgent,
    target: &ConfigTarget,
    file: ConfigFile,
    expected_revision: &str,
    edit: ConfigEdit,
) -> Result<ConfigSaveResult> {
    let before = read_config_file_target(selected, target, file)?;
    let file_name = file.as_str(selected.agent());
    if before.revision != expected_revision {
        return Err(application_error(
            ApplicationErrorKind::Conflict,
            "configuration file changed since it was revealed",
        ));
    }
    let (path, mode, content) = if target.is_current() {
        selected.ensure_agent_state_dir()?;
        let snapshot = capture_optional_agent_file(selected, file_name)?;
        let ConfigEdit::Raw { content, .. } = &edit else {
            bail!("Visual editing is only available for a Named Config");
        };
        (
            selected.state_file(file_name),
            snapshot.mode.unwrap_or(0o600),
            content.clone(),
        )
    } else {
        let config = target
            .named()
            .expect("non-current ConfigTarget must have a name");
        ensure_safe_named_config(selected, config)?;
        let content = match &edit {
            ConfigEdit::VisualMain {
                options,
                custom_provider,
            } => {
                if file != ConfigFile::Main {
                    bail!("Visual main fields are only available for the main Config file");
                }
                let original = std::str::from_utf8(&before.content)
                    .with_context(|| format!("Named Config {file_name} is not valid UTF-8"))?;
                render_visual_main(
                    selected.agent(),
                    original,
                    options,
                    custom_provider.as_ref(),
                )?
                .into_bytes()
            }
            ConfigEdit::VisualAuth(auth) => {
                if file != ConfigFile::Auth {
                    bail!("Visual auth is only available for Codex auth.json");
                }
                render_visual_auth(auth)?.into_bytes()
            }
            ConfigEdit::Raw { content, .. } => content.clone(),
        };
        if content.len() as u64 > MAX_CONFIG_BYTES {
            return Err(application_error(
                ApplicationErrorKind::InputTooLarge,
                format!("configuration file exceeds {MAX_CONFIG_BYTES} bytes"),
            ));
        }
        let content_text = std::str::from_utf8(&content)
            .with_context(|| format!("Named Config {file_name} is not valid UTF-8"))?;
        let layout = inspect_named_config_directory(selected, config)?
            .context("Named Config directory disappeared while saving")?;
        let _ = layout;
        NamedConfigDefinition::validate_file(selected.agent(), file_name, content_text)
            .with_context(|| format!("validate Named Config '{config}' {file_name}"))?;
        (
            super::layout::named_config_file(selected, config, file),
            0o600,
            content,
        )
    };
    write_atomic(&path, &content, mode)?;
    let snapshot = read_config_file_target(selected, target, file)?;
    let linked = if !target.is_current()
        && file == ConfigFile::Main
        && edit
            .custom_provider()
            .is_some_and(|provider| provider.included)
        && selected.agent().native_auth_file().is_some()
    {
        let auth_kind = ConfigFile::Auth;
        let auth_before = read_config_file_target(selected, target, auth_kind)?;
        let empty_auth = if !auth_before.exists {
            true
        } else {
            serde_json::from_slice::<Value>(&auth_before.content)
                .ok()
                .and_then(|value| value.as_object().map(Map::is_empty))
                .unwrap_or(false)
        };
        if empty_auth {
            let placeholder = selected
                .agent()
                .config_auth_template()
                .context("Codex auth template is missing")?
                .as_bytes();
            let config = target.named().expect("Named Config");
            let auth_path = super::layout::named_config_file(selected, config, auth_kind);
            write_atomic(&auth_path, placeholder, 0o600)?;
            Some(read_config_file_target(selected, target, auth_kind)?)
        } else {
            None
        }
    } else {
        None
    };
    Ok(ConfigSaveResult { snapshot, linked })
}
