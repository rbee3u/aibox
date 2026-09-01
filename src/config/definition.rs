//! Fixed Named Config definition, validation, and one-shot Application projection.
//!
//! A Named Config may contain only the main-configuration Config Fields declared
//! by [`AgentKind`] plus Codex's complete native `auth.json` Config Field.
//! Application iterates that entire fixed field set: present values are set,
//! absent values are removed, and unrelated Current Config values are preserved.
//! This module computes desired native files without performing filesystem writes.

use super::native::{
    parse_json_object, remove_codex_path, remove_json_path, set_codex_path, set_json_path,
    value_at_path,
};
use crate::agent::{AgentKind, MainConfigField, MainConfigValueKind};
use anyhow::{Context, Result, bail};
use serde_json::{Map, Value};
use std::str::FromStr;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use toml_edit::DocumentMut;

/// A validated Named Config definition in native main/auth formats.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NamedConfigDefinition {
    agent: AgentKind,
    main: Map<String, Value>,
    auth: Option<Map<String, Value>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NamedConfigValidation {
    pub(crate) definition: NamedConfigDefinition,
    pub(crate) warnings: Vec<String>,
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
        Ok(Self::parse_with_warnings(agent, main, auth)?.definition)
    }

    pub(crate) fn parse_with_warnings(
        agent: AgentKind,
        main: &str,
        auth: Option<&str>,
    ) -> Result<NamedConfigValidation> {
        let main = agent
            .parse_main_config(main)
            .context("parse Named Config main configuration")?;
        let mut warnings = validate_config_main(agent, &main)?;

        let auth = match agent {
            AgentKind::Claude => {
                if auth.is_some() {
                    bail!("Claude Named Config does not use auth.json");
                }
                None
            }
            AgentKind::Codex => {
                let auth = auth.context("Codex Named Config auth.json is missing")?;
                let (object, auth_warnings) = validate_codex_auth(
                    auth,
                    (value_at_path(&main, &["model_provider"]).and_then(Value::as_str)
                        == Some("custom"))
                    .then(|| {
                        value_at_path(
                            &main,
                            &["model_providers", "custom", "requires_openai_auth"],
                        )
                        .and_then(Value::as_bool)
                    })
                    .flatten(),
                )?;
                warnings.extend(auth_warnings);
                Some(object)
            }
        };

        Ok(NamedConfigValidation {
            definition: Self { agent, main, auth },
            warnings,
        })
    }

    /// Validate one independently editable file in a Named Config.
    pub(crate) fn validate_file(agent: AgentKind, file: &str, content: &str) -> Result<()> {
        Self::validate_file_with_warnings(agent, file, content).map(|_| ())
    }

    pub(crate) fn validate_file_with_warnings(
        agent: AgentKind,
        file: &str,
        content: &str,
    ) -> Result<Vec<String>> {
        if file == agent.main_config_file() {
            let main = agent
                .parse_main_config(content)
                .context("parse Named Config main configuration")?;
            return validate_config_main(agent, &main);
        }
        if agent.native_auth_file() == Some(file) {
            return Ok(validate_codex_auth(content, None)?.1);
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

        for field in self.agent.main_config_fields() {
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
        for field in self.agent.main_config_fields() {
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

pub(super) fn validate_config_main(
    agent: AgentKind,
    main: &Map<String, Value>,
) -> Result<Vec<String>> {
    let warnings = validate_config_main_shape(agent, main)?;
    validate_required_config_main(agent, main)?;
    Ok(warnings)
}

fn validate_config_main_shape(agent: AgentKind, main: &Map<String, Value>) -> Result<Vec<String>> {
    let warnings = validate_config_field_shape(agent, main)?;
    if agent == AgentKind::Codex {
        validate_codex_provider(main)?;
    }
    Ok(warnings)
}

fn validate_config_field_shape(agent: AgentKind, main: &Map<String, Value>) -> Result<Vec<String>> {
    let mut path = Vec::new();
    let warnings = validate_config_object(main, agent.main_config_fields(), &mut path)?;
    Ok(warnings)
}

fn validate_required_config_main(agent: AgentKind, main: &Map<String, Value>) -> Result<()> {
    for field in agent.main_config_fields() {
        if field.required {
            let Some(value) = value_at_path(main, field.path) else {
                bail!("required Config Field {} is missing", field.path.join("."));
            };
            if value.as_str().is_some_and(|value| value.trim().is_empty()) {
                bail!("required Config Field {} is empty", field.path.join("."));
            }
        }
    }
    Ok(())
}

fn validate_codex_provider(main: &Map<String, Value>) -> Result<()> {
    let provider = main.get("model_provider");
    let providers = main.get("model_providers");
    let Some(provider) = provider else {
        if providers.is_some() {
            bail!("model_providers must be absent when model_provider is absent");
        }
        return Ok(());
    };
    if provider.as_str() != Some("custom") {
        bail!("model_provider must be custom when present");
    }

    let providers = providers
        .and_then(Value::as_object)
        .context("model_providers must be a table")?;
    if providers.len() != 1 || !providers.contains_key("custom") {
        bail!("model_providers must contain only custom");
    }
    let custom = providers
        .get("custom")
        .and_then(Value::as_object)
        .context("model_providers.custom must be a table")?;
    const EXPECTED: [&str; 3] = ["name", "base_url", "requires_openai_auth"];
    if custom.len() != EXPECTED.len() || EXPECTED.iter().any(|key| !custom.contains_key(*key)) {
        bail!(
            "model_providers.custom must contain exactly name, base_url, and requires_openai_auth"
        );
    }
    for key in ["name", "base_url"] {
        let value = custom
            .get(key)
            .and_then(Value::as_str)
            .with_context(|| format!("model_providers.custom.{key} must be a string"))?;
        if value.trim().is_empty() {
            bail!("model_providers.custom.{key} must not be empty");
        }
    }
    if custom.get("requires_openai_auth").and_then(Value::as_bool) != Some(true) {
        bail!("model_providers.custom.requires_openai_auth must be true");
    }
    Ok(())
}

fn validate_config_object(
    object: &Map<String, Value>,
    fields: &[MainConfigField],
    path: &mut Vec<String>,
) -> Result<Vec<String>> {
    let mut warnings = Vec::new();
    for (key, value) in object {
        path.push(key.clone());
        let exact = fields.iter().find(|field| path_matches(field.path, path));
        if let Some(field) = exact {
            let valid = match field.value_kind {
                MainConfigValueKind::String => value.is_string(),
                MainConfigValueKind::Bool => value.is_boolean(),
            };
            if !valid {
                bail!(
                    "Config Field {} must be {}",
                    display_path("config", path),
                    match field.value_kind {
                        MainConfigValueKind::String => "a string",
                        MainConfigValueKind::Bool => "a boolean",
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
            warnings.extend(validate_config_object(child, fields, path)?);
        } else {
            warnings.push(format!(
                "Unknown native field {}",
                display_path("config", path)
            ));
        }
        path.pop();
    }
    Ok(warnings)
}

pub(super) fn validate_codex_auth(
    content: &str,
    requires_openai_auth: Option<bool>,
) -> Result<(Map<String, Value>, Vec<String>)> {
    let object = parse_json_object(content, "Named Config auth.json")?;
    let mut warnings = Vec::new();
    if let Some(mode) = object.get("auth_mode") {
        let mode = mode
            .as_str()
            .context("Named Config auth.json auth_mode must be a string")?;
        if mode == "chatgpt" {
            let account_id = object
                .get("tokens")
                .and_then(Value::as_object)
                .and_then(|tokens| tokens.get("account_id"))
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .context("ChatGPT credentials require a non-empty tokens.account_id")?;
            let _ = account_id;
            let last_refresh = object
                .get("last_refresh")
                .and_then(Value::as_str)
                .context("ChatGPT credentials require a string last_refresh")?;
            OffsetDateTime::parse(last_refresh, &Rfc3339)
                .context("ChatGPT credentials have invalid last_refresh")?;
            for key in object.keys() {
                if !matches!(
                    key.as_str(),
                    "auth_mode" | "OPENAI_API_KEY" | "tokens" | "last_refresh"
                ) {
                    warnings.push(format!("Unknown native auth field /auth/{key}"));
                }
            }
            if let Some(api_key) = object.get("OPENAI_API_KEY")
                && !api_key.is_null()
                && !api_key.is_string()
            {
                bail!("ChatGPT auth OPENAI_API_KEY must be a string or null");
            }
            return Ok((object, warnings));
        }
        warnings.push(format!(
            "Unknown native auth field /auth/auth_mode ({mode})"
        ));
    }
    if let Some(api_key) = object.get("OPENAI_API_KEY")
        && !api_key.is_string()
    {
        bail!("API-key auth OPENAI_API_KEY must be a string");
    }
    for key in object.keys() {
        if key != "OPENAI_API_KEY" {
            warnings.push(format!("Unknown native auth field /auth/{key}"));
        }
    }
    if requires_openai_auth == Some(true)
        && object
            .get("OPENAI_API_KEY")
            .and_then(Value::as_str)
            .is_none_or(|value| value.is_empty())
    {
        bail!("OPENAI_API_KEY is required when requires_openai_auth is true");
    }
    Ok((object, warnings))
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

#[cfg(test)]
#[path = "definition_tests.rs"]
mod tests;
