//! Visual Editor inputs, projections, inspection, and native rendering.

use super::definition::{validate_codex_auth, validate_config_main};
use super::native::{
    remove_codex_path, remove_codex_provider, remove_json_path, set_codex_path, set_json_path,
    value_at_path,
};
use crate::agent::{AgentKind, MainConfigValueKind};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::str::FromStr;
use toml_edit::DocumentMut;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CodexAuthInspection {
    pub(crate) mode: &'static str,
    pub(crate) api_key: Option<String>,
    pub(crate) extra_fields: bool,
    pub(crate) warnings: Vec<String>,
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

#[cfg(test)]
#[path = "visual_tests.rs"]
mod tests;
