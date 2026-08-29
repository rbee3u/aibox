//! Fixed Named Config schema validation and one-time application.
//!
//! A Named Config may contain only the main-configuration Config Fields declared
//! by [`AgentKind`] plus Codex's complete native `auth.json` Config Field.
//! Application iterates that entire fixed field set: present values are set,
//! absent values are removed, and unrelated Current Config values are preserved.
//! This module computes the desired native files without performing filesystem
//! writes. The Config module separately records the last successful
//! application and derives drift without changing this projection model.

use crate::agent::{AgentKind, MainConfigField, MainConfigValueKind};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::str::FromStr;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use toml_edit::{DocumentMut, Item, Table, TableLike};

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CodexAuthInspection {
    pub(crate) mode: &'static str,
    pub(crate) api_key: Option<String>,
    pub(crate) extra_fields: bool,
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

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub(crate) struct VisualConfigOptionInput {
    pub(crate) path: String,
    pub(crate) included: bool,
    pub(crate) value: Option<Value>,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub(crate) struct CustomProviderInput {
    pub(crate) included: bool,
    pub(crate) name: String,
    pub(crate) base_url: String,
    #[serde(default)]
    pub(crate) proxy_routed: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub(crate) struct VisualAuthInput {
    pub(crate) included: bool,
    pub(crate) value: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct VisualConfigOptionState {
    pub(crate) path: String,
    pub(crate) label: &'static str,
    pub(crate) description: &'static str,
    pub(crate) group: &'static str,
    pub(crate) value_kind: &'static str,
    pub(crate) enum_values: Vec<&'static str>,
    pub(crate) sensitive: bool,
    pub(crate) required: bool,
    pub(crate) request_proxy_route: bool,
    pub(crate) included: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) value: Option<Value>,
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct CustomProviderState {
    pub(crate) included: bool,
    pub(crate) name: String,
    pub(crate) base_url: String,
    pub(crate) request_proxy_route: bool,
    pub(crate) proxy_routed: bool,
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct VisualConfigState {
    pub(crate) options: Vec<VisualConfigOptionState>,
    pub(crate) custom_provider: Option<CustomProviderState>,
}

fn path_string(path: &[&str]) -> String {
    path.join(".")
}

/// Return the fixed-field schema and values represented by a native main file.
pub(crate) fn inspect_visual_config(agent: AgentKind, content: &str) -> Result<VisualConfigState> {
    let object = if content.trim().is_empty() {
        Map::new()
    } else {
        agent
            .parse_main_config(content)
            .context("parse Visual Editor source")?
    };
    validate_config_main(agent, &object)?;
    let custom_provider = agent == AgentKind::Codex
        && value_at_path(&object, &["model_provider"]).and_then(Value::as_str) == Some("custom");
    let options = agent
        .main_config_fields()
        .iter()
        .filter(|field| {
            !(agent == AgentKind::Codex
                && (field.path == ["model_provider"]
                    || field.path.starts_with(&["model_providers", "custom"])))
        })
        .map(|field| VisualConfigOptionState {
            path: path_string(field.path),
            label: field.label,
            description: field.description,
            group: field.group,
            value_kind: match field.value_kind {
                MainConfigValueKind::String => "string",
                MainConfigValueKind::Bool => "bool",
            },
            enum_values: field.enum_values.to_vec(),
            sensitive: field.sensitive,
            required: field.required || (custom_provider && field.required_for_custom_provider),
            request_proxy_route: field.request_proxy_route,
            included: value_at_path(&object, field.path).is_some()
                || field.required
                || (custom_provider && field.required_for_custom_provider),
            value: value_at_path(&object, field.path).cloned(),
        })
        .collect();
    let custom_provider = if agent == AgentKind::Codex {
        let custom =
            value_at_path(&object, &["model_providers", "custom"]).and_then(Value::as_object);
        Some(CustomProviderState {
            included: custom.is_some(),
            name: custom
                .and_then(|value| value.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("custom")
                .to_string(),
            base_url: custom
                .and_then(|value| value.get("base_url"))
                .and_then(Value::as_str)
                .unwrap_or("https://example.com/v1")
                .to_string(),
            request_proxy_route: true,
            proxy_routed: false,
        })
    } else {
        None
    };
    Ok(VisualConfigState {
        options,
        custom_provider,
    })
}

/// Apply Visual Editor values to a native main file while preserving unrelated data.
pub(crate) fn render_visual_main(
    agent: AgentKind,
    original: &str,
    inputs: &[VisualConfigOptionInput],
    provider_input: Option<&CustomProviderInput>,
) -> Result<String> {
    let mut seen = std::collections::HashSet::new();
    let original_object = if original.trim().is_empty() {
        Map::new()
    } else {
        agent
            .parse_main_config(original)
            .context("parse Visual Editor source")?
    };
    let mut values = Map::new();
    for input in inputs {
        if !seen.insert(input.path.as_str()) {
            bail!("duplicate Visual Config Option: {}", input.path);
        }
        let field = agent
            .main_config_fields()
            .iter()
            .find(|field| path_string(field.path) == input.path)
            .with_context(|| format!("unsupported Visual Config Option: {}", input.path))?;
        if input.included {
            let value = input
                .value
                .clone()
                .with_context(|| format!("Visual Config Option {} has no value", input.path))?;
            let valid = match field.value_kind {
                MainConfigValueKind::String => value.is_string(),
                MainConfigValueKind::Bool => value.is_boolean(),
            };
            if !valid {
                bail!(
                    "Visual Config Option {} must be {}",
                    input.path,
                    match field.value_kind {
                        MainConfigValueKind::String => "a string",
                        MainConfigValueKind::Bool => "a boolean",
                    }
                );
            }
            if !field.enum_values.is_empty() {
                let value = value.as_str().expect("string enum field validated above");
                let original_value =
                    value_at_path(&original_object, field.path).and_then(Value::as_str);
                if !field.enum_values.contains(&value) && original_value != Some(value) {
                    bail!(
                        "Visual Config Option {} must use a supported enum value",
                        input.path
                    );
                }
            }
            values.insert(input.path.clone(), value);
        }
    }
    let expected_fields = agent
        .main_config_fields()
        .iter()
        .filter(|field| {
            !(agent == AgentKind::Codex
                && (field.path == ["model_provider"]
                    || field.path.starts_with(&["model_providers", "custom"])))
        })
        .count();
    if seen.len() != expected_fields {
        bail!("Visual Editor must provide every fixed Config Field");
    }

    match agent {
        AgentKind::Claude => {
            let mut object = if original.trim().is_empty() {
                Map::new()
            } else {
                agent
                    .parse_main_config(original)
                    .context("parse Visual Editor source")?
            };
            for field in agent.main_config_fields().iter().filter(|field| {
                !(agent == AgentKind::Codex
                    && (field.path == ["model_provider"]
                        || field.path.starts_with(&["model_providers", "custom"])))
            }) {
                let key = path_string(field.path);
                match values.get(&key) {
                    Some(value) => {
                        set_json_path(&mut object, field.path, value.clone());
                    }
                    None => {
                        remove_json_path(&mut object, field.path);
                    }
                }
            }
            validate_config_main(agent, &object)?;
            agent.render_main_config(&Value::Object(object))
        }
        AgentKind::Codex => {
            let mut document = if original.trim().is_empty() {
                DocumentMut::new()
            } else {
                DocumentMut::from_str(original).context("parse Visual Editor source")?
            };
            for field in agent.main_config_fields().iter().filter(|field| {
                !(agent == AgentKind::Codex
                    && (field.path == ["model_provider"]
                        || field.path.starts_with(&["model_providers", "custom"])))
            }) {
                let key = path_string(field.path);
                match values.get(&key) {
                    Some(value) => {
                        set_codex_path(&mut document, field.path, value)?;
                    }
                    None => {
                        remove_codex_path(&mut document, field.path);
                    }
                }
            }
            let Some(provider) = provider_input else {
                bail!("Codex Visual Editor requires Custom provider state");
            };
            let _ = provider.proxy_routed;
            if provider.included {
                if provider.name.trim().is_empty() || provider.base_url.trim().is_empty() {
                    bail!("Custom provider name and base URL must not be empty");
                }
                set_codex_path(
                    &mut document,
                    &["model_provider"],
                    &Value::String("custom".to_string()),
                )?;
                set_codex_path(
                    &mut document,
                    &["model_providers", "custom", "name"],
                    &Value::String(provider.name.clone()),
                )?;
                set_codex_path(
                    &mut document,
                    &["model_providers", "custom", "base_url"],
                    &Value::String(provider.base_url.clone()),
                )?;
                set_codex_path(
                    &mut document,
                    &["model_providers", "custom", "requires_openai_auth"],
                    &Value::Bool(true),
                )?;
            } else {
                remove_codex_provider(&mut document);
            }
            let rendered = document.to_string();
            let object = agent
                .parse_main_config(&rendered)
                .context("render Visual Editor source")?;
            validate_config_main(agent, &object)?;
            Ok(rendered)
        }
    }
}

pub(crate) fn render_visual_auth(input: &VisualAuthInput) -> Result<String> {
    let value = if input.included {
        let value = input
            .value
            .as_deref()
            .context("OPENAI_API_KEY is required when included")?;
        if value.is_empty() {
            Value::Object(Map::new())
        } else {
            let mut object = Map::new();
            object.insert(
                "OPENAI_API_KEY".to_string(),
                Value::String(value.to_string()),
            );
            Value::Object(object)
        }
    } else {
        Value::Object(Map::new())
    };
    Ok(format!("{}\n", serde_json::to_string_pretty(&value)?))
}

pub(crate) fn inspect_codex_auth(
    content: &str,
    requires_openai_auth: Option<bool>,
) -> Result<CodexAuthInspection> {
    let (object, warnings) = validate_codex_auth(content, requires_openai_auth)?;
    let chatgpt = object.get("auth_mode").and_then(Value::as_str) == Some("chatgpt");
    let api_key = object
        .get("OPENAI_API_KEY")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let extra_fields = if chatgpt {
        object.keys().any(|key| {
            !matches!(
                key.as_str(),
                "auth_mode" | "OPENAI_API_KEY" | "tokens" | "last_refresh"
            )
        })
    } else {
        object.keys().any(|key| key != "OPENAI_API_KEY")
    };
    Ok(CodexAuthInspection {
        mode: if chatgpt { "chatgpt" } else { "api-key" },
        api_key,
        extra_fields,
        warnings,
    })
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

fn validate_config_main(agent: AgentKind, main: &Map<String, Value>) -> Result<Vec<String>> {
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

fn validate_codex_auth(
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

fn remove_codex_provider(document: &mut DocumentMut) -> bool {
    let mut changed = document.as_table_mut().remove("model_provider").is_some();
    changed |= document.as_table_mut().remove("model_providers").is_some();
    changed
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
