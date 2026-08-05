//! Fixed Agent Profile schema validation and one-time application.

use crate::agent::{AgentKind, ProfileAuthKind, ProfileField, ProfileValueKind};
use anyhow::{bail, Context, Result};
use serde_json::{Map, Value};
use std::str::FromStr;
use toml_edit::{DocumentMut, Item, Table, TableLike};

const CLAUDE_AUTH_TOKEN: &str = "ANTHROPIC_AUTH_TOKEN";

/// A validated Agent Profile definition in native main/auth formats.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ProfileDefinition {
    agent: AgentKind,
    main: Map<String, Value>,
    auth: Map<String, Value>,
}

/// Desired native Agent Configuration files after one Profile Application.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ApplicationResult {
    /// Desired main file; `None` preserves an absent, semantically empty file.
    pub(crate) main: Option<String>,
    /// Desired Codex auth file; always `None` for Claude.
    pub(crate) auth: Option<String>,
}

impl ProfileDefinition {
    /// Parse and validate the fixed Profile Fields for one Coding Agent.
    pub(crate) fn parse(agent: AgentKind, main: &str, auth: &str) -> Result<Self> {
        let main = agent
            .parse_main_config(main)
            .context("parse Agent Profile main configuration")?;
        validate_profile_main(agent, &main)?;

        let auth = parse_json_object(auth, "Agent Profile auth.json")?;
        match agent.profile_auth_kind() {
            ProfileAuthKind::ClaudeToken => validate_claude_auth(&auth)?,
            ProfileAuthKind::CodexObject => {}
        }

        Ok(Self { agent, main, auth })
    }

    /// Apply every fixed Profile Field to the current native configuration.
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
            .context("parse Agent Configuration settings.json")?;
        let mut changed = false;

        for field in self.agent.profile_fields() {
            match value_at_path(&self.main, field.path) {
                Some(value) => {
                    changed |= set_json_path(&mut configuration, field.path, value.clone())
                }
                None => changed |= remove_json_path(&mut configuration, field.path),
            }
        }
        match self.auth.get(CLAUDE_AUTH_TOKEN) {
            Some(value) => {
                changed |= set_json_path(
                    &mut configuration,
                    &["env", CLAUDE_AUTH_TOKEN],
                    value.clone(),
                );
            }
            None => changed |= remove_json_path(&mut configuration, &["env", CLAUDE_AUTH_TOKEN]),
        }

        let main = if current_main.is_none() && configuration.is_empty() {
            None
        } else if !changed && !original.trim().is_empty() {
            Some(original.to_string())
        } else {
            Some(
                self.agent
                    .render_main_config(&Value::Object(configuration))
                    .context("render Agent Configuration settings.json")?,
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
            DocumentMut::from_str(original_main).context("parse Agent Configuration config.toml")?
        };
        let mut changed = false;
        for field in self.agent.profile_fields() {
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

        let current_auth_object = parse_json_object(
            current_auth.unwrap_or("{}"),
            "Agent Configuration auth.json",
        )?;
        let auth = if current_auth.is_none() && self.auth.is_empty() {
            None
        } else if current_auth.is_some()
            && current_auth_object == self.auth
            && current_auth.is_some_and(|content| !content.trim().is_empty())
        {
            current_auth.map(str::to_string)
        } else {
            Some(format!(
                "{}\n",
                serde_json::to_string_pretty(&Value::Object(self.auth.clone()))?
            ))
        };

        Ok(ApplicationResult { main, auth })
    }
}

fn validate_profile_main(agent: AgentKind, main: &Map<String, Value>) -> Result<()> {
    let mut path = Vec::new();
    validate_profile_object(main, agent.profile_fields(), &mut path)
}

fn validate_profile_object(
    object: &Map<String, Value>,
    fields: &[ProfileField],
    path: &mut Vec<String>,
) -> Result<()> {
    for (key, value) in object {
        path.push(key.clone());
        let exact = fields.iter().find(|field| path_matches(field.path, path));
        if let Some(field) = exact {
            let valid = match field.value_kind {
                ProfileValueKind::String => value.is_string(),
                ProfileValueKind::Bool => value.is_boolean(),
            };
            if !valid {
                bail!(
                    "Agent Profile Field {} must be {}",
                    display_path("config", path),
                    match field.value_kind {
                        ProfileValueKind::String => "a string",
                        ProfileValueKind::Bool => "a boolean",
                    }
                );
            }
        } else if fields.iter().any(|field| path_is_prefix(path, field.path)) {
            let child = value.as_object().with_context(|| {
                format!(
                    "Agent Profile Field parent {} must be an object or table",
                    display_path("config", path)
                )
            })?;
            validate_profile_object(child, fields, path)?;
        } else {
            bail!(
                "unsupported Agent Profile Field {}",
                display_path("config", path)
            );
        }
        path.pop();
    }
    Ok(())
}

fn validate_claude_auth(auth: &Map<String, Value>) -> Result<()> {
    for (key, value) in auth {
        if key != CLAUDE_AUTH_TOKEN {
            bail!(
                "unsupported Agent Profile Field {}",
                display_path("auth", std::slice::from_ref(key))
            );
        }
        if !value.is_string() {
            bail!("Agent Profile Field /auth/{CLAUDE_AUTH_TOKEN} must be a string");
        }
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
    };
    let child = existing.as_object_mut().expect("object checked above");
    let mut changed = remove_json_path(child, rest);
    if child.is_empty() {
        object.remove(*first);
        changed = true;
    }
    changed
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
        _ => bail!("Agent Profile Field has an unsupported TOML value type"),
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
#[path = "profile_model_tests.rs"]
mod tests;
