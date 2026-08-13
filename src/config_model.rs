//! Fixed Named Config schema validation and one-time application.
//!
//! A Named Config may contain only the Config Fields declared by [`AgentKind`]
//! (plus Codex's complete native `auth.json` object). Application iterates that
//! entire fixed field set: present values are set, absent values are removed,
//! and unrelated Current Config values are preserved. This module computes the
//! desired native files without performing filesystem writes or retaining an
//! association with the Named Config.

use crate::agent::{AgentKind, ConfigField, ConfigValueKind};
use anyhow::{Context, Result, bail};
use serde_json::{Map, Value};
use std::str::FromStr;
use toml_edit::{DocumentMut, Item, Table, TableLike};

/// A validated Named Config definition in native main/auth formats.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NamedConfigDefinition {
    agent: AgentKind,
    main: Map<String, Value>,
    auth: Option<Map<String, Value>>,
}

/// Desired native Current Config files after one Config Application.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ApplicationResult {
    /// Desired main file; `None` preserves an absent, semantically empty file.
    pub(crate) main: Option<String>,
    /// Desired Codex auth file; always `None` for Claude.
    pub(crate) auth: Option<String>,
}

impl NamedConfigDefinition {
    /// Parse and validate the fixed Config Fields for one Coding Agent.
    pub(crate) fn parse(agent: AgentKind, main: &str, auth: Option<&str>) -> Result<Self> {
        let main = agent
            .parse_main_config(main)
            .context("parse Named Config main configuration")?;
        validate_config_main(agent, &main)?;

        let auth = match agent {
            AgentKind::Claude => {
                if auth.is_some() {
                    bail!("Claude Named Config does not use auth.json");
                }
                None
            }
            AgentKind::Codex => Some(parse_json_object(
                auth.context("Codex Named Config auth.json is missing")?,
                "Named Config auth.json",
            )?),
        };

        Ok(Self { agent, main, auth })
    }

    /// Validate one independently editable file in a Named Config.
    pub(crate) fn validate_file(agent: AgentKind, file: &str, content: &str) -> Result<()> {
        if file == agent.main_config_file() {
            let main = agent
                .parse_main_config(content)
                .context("parse Named Config main configuration")?;
            return validate_config_main(agent, &main);
        }
        if agent.native_auth_file() == Some(file) {
            parse_json_object(content, "Named Config auth.json")?;
            return Ok(());
        }
        bail!("unsupported Named Config file: {file}")
    }

    /// Apply every fixed Config Field to the current native configuration.
    pub(crate) fn apply(
        &self,
        current_main: Option<&str>,
        current_auth: Option<&str>,
    ) -> Result<ApplicationResult> {
        match self.agent {
            AgentKind::Claude => self.apply_claude(current_main),
            AgentKind::Codex => self.apply_codex(current_main, current_auth),
        }
    }

    fn apply_claude(&self, current_main: Option<&str>) -> Result<ApplicationResult> {
        let original = current_main.unwrap_or("");
        let mut configuration = self
            .agent
            .parse_main_config(current_main.unwrap_or("{}"))
            .context("parse Current Config settings.json")?;
        let mut changed = false;

        for field in self.agent.config_fields() {
            match value_at_path(&self.main, field.path) {
                Some(value) => {
                    changed |= set_json_path(&mut configuration, field.path, value.clone());
                }
                None => changed |= remove_json_path(&mut configuration, field.path),
            }
        }
        let main = if current_main.is_none() && configuration.is_empty() {
            None
        } else if !changed && !original.trim().is_empty() {
            Some(original.to_string())
        } else {
            Some(
                self.agent
                    .render_main_config(&Value::Object(configuration))
                    .context("render Current Config settings.json")?,
            )
        };
        Ok(ApplicationResult { main, auth: None })
    }

    fn apply_codex(
        &self,
        current_main: Option<&str>,
        current_auth: Option<&str>,
    ) -> Result<ApplicationResult> {
        let original_main = current_main.unwrap_or("");
        let mut document = if original_main.trim().is_empty() {
            DocumentMut::new()
        } else {
            DocumentMut::from_str(original_main).context("parse Current Config config.toml")?
        };
        let mut changed = false;
        for field in self.agent.config_fields() {
            match value_at_path(&self.main, field.path) {
                Some(value) => changed |= set_codex_path(&mut document, field.path, value)?,
                None => changed |= remove_codex_path(&mut document, field.path),
            }
        }
        let main = if current_main.is_none() && document.as_table().is_empty() {
            None
        } else if !changed {
            Some(original_main.to_string())
        } else {
            Some(document.to_string())
        };

        let current_auth_object =
            parse_json_object(current_auth.unwrap_or("{}"), "Current Config auth.json")?;
        let desired_auth = self.auth.as_ref().expect("Codex Named Config has auth");
        let auth = if current_auth.is_none() && desired_auth.is_empty() {
            None
        } else if current_auth.is_some()
            && current_auth_object == *desired_auth
            && current_auth.is_some_and(|content| !content.trim().is_empty())
        {
            current_auth.map(str::to_string)
        } else {
            Some(format!(
                "{}\n",
                serde_json::to_string_pretty(&Value::Object(desired_auth.clone()))?
            ))
        };

        Ok(ApplicationResult { main, auth })
    }
}

fn validate_config_main(agent: AgentKind, main: &Map<String, Value>) -> Result<()> {
    let mut path = Vec::new();
    validate_config_object(main, agent.config_fields(), &mut path)
}

fn validate_config_object(
    object: &Map<String, Value>,
    fields: &[ConfigField],
    path: &mut Vec<String>,
) -> Result<()> {
    for (key, value) in object {
        path.push(key.clone());
        let exact = fields.iter().find(|field| path_matches(field.path, path));
        if let Some(field) = exact {
            let valid = match field.value_kind {
                ConfigValueKind::String => value.is_string(),
                ConfigValueKind::Bool => value.is_boolean(),
            };
            if !valid {
                bail!(
                    "Config Field {} must be {}",
                    display_path("config", path),
                    match field.value_kind {
                        ConfigValueKind::String => "a string",
                        ConfigValueKind::Bool => "a boolean",
                    }
                );
            }
        } else if fields.iter().any(|field| path_is_prefix(path, field.path)) {
            let child = value.as_object().with_context(|| {
                format!(
                    "Config Field parent {} must be an object or table",
                    display_path("config", path)
                )
            })?;
            validate_config_object(child, fields, path)?;
        } else {
            bail!("unsupported Config Field {}", display_path("config", path));
        }
        path.pop();
    }
    Ok(())
}

fn parse_json_object(content: &str, label: &str) -> Result<Map<String, Value>> {
    let value = serde_json::from_str::<Value>(content).with_context(|| format!("parse {label}"))?;
    value
        .as_object()
        .cloned()
        .with_context(|| format!("{label} must be a JSON object"))
}

fn path_matches(field: &[&str], path: &[String]) -> bool {
    field.len() == path.len() && field.iter().zip(path).all(|(left, right)| *left == right)
}

fn path_is_prefix(path: &[String], field: &[&str]) -> bool {
    path.len() < field.len() && path.iter().zip(field).all(|(left, right)| left == right)
}

fn display_path(root: &str, path: &[String]) -> String {
    let mut output = format!("/{root}");
    for segment in path {
        output.push('/');
        for character in segment.replace('~', "~0").replace('/', "~1").chars() {
            if character.is_control() {
                output.extend(character.escape_default());
            } else {
                output.push(character);
            }
        }
    }
    output
}

fn value_at_path<'a>(object: &'a Map<String, Value>, path: &[&str]) -> Option<&'a Value> {
    let (first, rest) = path.split_first()?;
    let value = object.get(*first)?;
    if rest.is_empty() {
        Some(value)
    } else {
        value
            .as_object()
            .and_then(|child| value_at_path(child, rest))
    }
}

fn set_json_path(object: &mut Map<String, Value>, path: &[&str], value: Value) -> bool {
    let Some((first, rest)) = path.split_first() else {
        return false;
    };
    if rest.is_empty() {
        if object.get(*first) == Some(&value) {
            return false;
        }
        object.insert((*first).to_string(), value);
        return true;
    }

    let mut changed = !matches!(object.get(*first), Some(Value::Object(_)));
    if changed {
        object.insert((*first).to_string(), Value::Object(Map::new()));
    }
    let child = object
        .get_mut(*first)
        .and_then(Value::as_object_mut)
        .expect("object inserted above");
    changed |= set_json_path(child, rest, value);
    changed
}

fn remove_json_path(object: &mut Map<String, Value>, path: &[&str]) -> bool {
    let Some((first, rest)) = path.split_first() else {
        return false;
    };
    if rest.is_empty() {
        return object.remove(*first).is_some();
    }
    let Some(existing) = object.get_mut(*first) else {
        return false;
    };
    if !existing.is_object() {
        object.remove(*first);
        return true;
    }
    let child = existing.as_object_mut().expect("object checked above");
    let changed = remove_json_path(child, rest);
    if child.is_empty() {
        object.remove(*first);
        true
    } else {
        changed
    }
}

fn set_codex_path(document: &mut DocumentMut, path: &[&str], value: &Value) -> Result<bool> {
    if path.len() == 1 {
        return set_toml_item(document.as_table_mut(), path[0], value);
    }
    debug_assert_eq!(&path[..2], ["model_providers", "custom"]);
    let mut changed = ensure_toml_table(document.as_table_mut(), "model_providers", true);
    let providers = document
        .get_mut("model_providers")
        .and_then(Item::as_table_like_mut)
        .context("model_providers must be a table")?;
    changed |= ensure_toml_table(providers, "custom", false);
    let custom = providers
        .get_mut("custom")
        .and_then(Item::as_table_like_mut)
        .context("model_providers.custom must be a table")?;
    changed |= set_toml_item(custom, path[2], value)?;
    Ok(changed)
}

fn set_toml_item(table: &mut dyn TableLike, key: &str, value: &Value) -> Result<bool> {
    if table
        .get(key)
        .is_some_and(|item| toml_item_matches(item, value))
    {
        return Ok(false);
    }
    let mut item = match value {
        Value::String(value) => toml_edit::value(value.clone()),
        Value::Bool(value) => toml_edit::value(*value),
        _ => bail!("Config Field has an unsupported TOML value type"),
    };
    if let Some(existing) = table.get_mut(key) {
        if let (Some(previous), Some(replacement)) = (existing.as_value(), item.as_value_mut()) {
            *replacement.decor_mut() = previous.decor().clone();
        }
        *existing = item;
    } else {
        table.insert(key, item);
    }
    Ok(true)
}

fn toml_item_matches(item: &Item, value: &Value) -> bool {
    let Some(item) = item.as_value() else {
        return false;
    };
    match value {
        Value::String(value) => item.as_str() == Some(value),
        Value::Bool(value) => item.as_bool() == Some(*value),
        _ => false,
    }
}

fn ensure_toml_table(table: &mut dyn TableLike, key: &str, implicit: bool) -> bool {
    if table.get(key).and_then(Item::as_table_like).is_some() {
        return false;
    }
    let mut child = Table::new();
    child.set_implicit(implicit);
    table.insert(key, Item::Table(child));
    true
}

fn remove_codex_path(document: &mut DocumentMut, path: &[&str]) -> bool {
    if path.len() == 1 {
        return document.as_table_mut().remove(path[0]).is_some();
    }
    debug_assert_eq!(&path[..2], ["model_providers", "custom"]);

    if document.get("model_providers").is_some()
        && document
            .get("model_providers")
            .and_then(Item::as_table_like)
            .is_none()
    {
        document.as_table_mut().remove("model_providers");
        return true;
    }

    let mut changed = false;
    let mut remove_providers = false;
    if let Some(providers) = document
        .get_mut("model_providers")
        .and_then(Item::as_table_like_mut)
    {
        if providers.get("custom").is_some()
            && providers
                .get("custom")
                .and_then(Item::as_table_like)
                .is_none()
        {
            providers.remove("custom");
            changed = true;
        }
        let mut remove_custom = false;
        if let Some(custom) = providers
            .get_mut("custom")
            .and_then(Item::as_table_like_mut)
        {
            changed |= custom.remove(path[2]).is_some();
            remove_custom = custom.iter().next().is_none();
        }
        if remove_custom {
            providers.remove("custom");
            changed = true;
        }
        remove_providers = providers.iter().next().is_none();
    }
    if remove_providers {
        document.as_table_mut().remove("model_providers");
        changed = true;
    }
    changed
}

#[cfg(test)]
#[path = "config_model_tests.rs"]
mod tests;
