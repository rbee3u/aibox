//! Claude Code's fixed Config Fields and built-in Named Config template.

use super::{MainConfigCondition, MainConfigField, MainConfigValueKind};
use anyhow::{Result, bail};
use serde_json::{Map, Value};

const PERMISSION_MODES: &[&str] = &[
    "default",
    "acceptEdits",
    "plan",
    "auto",
    "dontAsk",
    "bypassPermissions",
];

pub(super) const MAIN_CONFIG_FIELDS: &[MainConfigField] = &[
    MainConfigField {
        path: &["env", "ANTHROPIC_BASE_URL"],
        value_kind: MainConfigValueKind::String,
        label: "Base URL",
        description: "Optional endpoint override; omit to use Claude's default endpoint.",
        visible_when: None,
        group: "Endpoint & credentials",
        enum_values: &[],
        sensitive: false,
        required: false,
        required_for_custom_provider: false,
        request_proxy_route: true,
    },
    MainConfigField {
        path: &["env", "ANTHROPIC_AUTH_TOKEN"],
        value_kind: MainConfigValueKind::String,
        label: "Custom auth token",
        description: "Custom Bearer credential. Include at most one explicit credential.",
        visible_when: None,
        group: "Endpoint & credentials",
        enum_values: &[],
        sensitive: true,
        required: false,
        required_for_custom_provider: false,
        request_proxy_route: false,
    },
    MainConfigField {
        path: &["env", "ANTHROPIC_API_KEY"],
        value_kind: MainConfigValueKind::String,
        label: "API key",
        description: "Claude Console API key for API usage billing. Include at most one explicit credential.",
        visible_when: None,
        group: "Endpoint & credentials",
        enum_values: &[],
        sensitive: true,
        required: false,
        required_for_custom_provider: false,
        request_proxy_route: false,
    },
    MainConfigField {
        path: &["env", "CLAUDE_CODE_OAUTH_TOKEN"],
        value_kind: MainConfigValueKind::String,
        label: "Subscription OAuth token",
        description: "Subscription token from claude setup-token. Include at most one explicit credential; omit all to use native login.",
        visible_when: None,
        group: "Endpoint & credentials",
        enum_values: &[],
        sensitive: true,
        required: false,
        required_for_custom_provider: false,
        request_proxy_route: false,
    },
    MainConfigField {
        path: &["permissions", "defaultMode"],
        value_kind: MainConfigValueKind::String,
        label: "Default permission mode",
        description: "Claude's native permission mode for tool use.",
        visible_when: None,
        group: "Permissions",
        enum_values: PERMISSION_MODES,
        sensitive: false,
        required: true,
        required_for_custom_provider: false,
        request_proxy_route: false,
    },
    MainConfigField {
        path: &["skipDangerousModePermissionPrompt"],
        value_kind: MainConfigValueKind::Bool,
        label: "Skip dangerous mode prompt",
        description: "Suppress Claude's confirmation prompt for dangerous mode.",
        visible_when: Some(MainConfigCondition {
            path: "permissions.defaultMode",
            value: "bypassPermissions",
        }),
        group: "Permissions",
        enum_values: &[],
        sensitive: false,
        required: false,
        required_for_custom_provider: false,
        request_proxy_route: false,
    },
    MainConfigField {
        path: &["env", "ANTHROPIC_DEFAULT_HAIKU_MODEL"],
        value_kind: MainConfigValueKind::String,
        label: "Default Haiku model",
        description: "Model used for the Haiku class of requests.",
        visible_when: None,
        group: "Model defaults",
        enum_values: &[],
        sensitive: false,
        required: false,
        required_for_custom_provider: false,
        request_proxy_route: false,
    },
    MainConfigField {
        path: &["env", "ANTHROPIC_DEFAULT_SONNET_MODEL"],
        value_kind: MainConfigValueKind::String,
        label: "Default Sonnet model",
        description: "Model used for the Sonnet class of requests.",
        visible_when: None,
        group: "Model defaults",
        enum_values: &[],
        sensitive: false,
        required: false,
        required_for_custom_provider: false,
        request_proxy_route: false,
    },
    MainConfigField {
        path: &["env", "ANTHROPIC_DEFAULT_OPUS_MODEL"],
        value_kind: MainConfigValueKind::String,
        label: "Default Opus model",
        description: "Model used for the Opus class of requests.",
        visible_when: None,
        group: "Model defaults",
        enum_values: &[],
        sensitive: false,
        required: false,
        required_for_custom_provider: false,
        request_proxy_route: false,
    },
    MainConfigField {
        path: &["env", "ANTHROPIC_DEFAULT_FABLE_MODEL"],
        value_kind: MainConfigValueKind::String,
        label: "Default Fable model",
        description: "Model used for the Fable class of requests.",
        visible_when: None,
        group: "Model defaults",
        enum_values: &[],
        sensitive: false,
        required: false,
        required_for_custom_provider: false,
        request_proxy_route: false,
    },
];

/// Validate explicit credentials without inspecting native login state or token contents.
pub(super) fn validate_main_config(main: &Map<String, Value>) -> Result<()> {
    let mut selected = None;
    for field in [
        "ANTHROPIC_AUTH_TOKEN",
        "ANTHROPIC_API_KEY",
        "CLAUDE_CODE_OAUTH_TOKEN",
    ] {
        let Some(value) = main.get("env").and_then(|env| env.get(field)) else {
            continue;
        };
        if value.as_str().is_none_or(|value| value.trim().is_empty()) {
            bail!("Config Field env.{field} must be a nonempty string");
        }
        if let Some(previous) = selected {
            bail!(
                "Claude Named Config permits at most one explicit credential: env.{previous} conflicts with env.{field}"
            );
        }
        selected = Some(field);
    }
    Ok(())
}

pub(super) const DEFAULT_CONFIG: &str = r#"{
  "env": {
    "ANTHROPIC_BASE_URL": "https://example.com",
    "ANTHROPIC_AUTH_TOKEN": "sk-example",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "claude-haiku-5-5",
    "ANTHROPIC_DEFAULT_SONNET_MODEL": "claude-sonnet-5-5[1m]",
    "ANTHROPIC_DEFAULT_OPUS_MODEL": "claude-opus-5-5[1m]",
    "ANTHROPIC_DEFAULT_FABLE_MODEL": "claude-fable-5-1[1m]"
  },
  "permissions": {
    "defaultMode": "bypassPermissions"
  },
  "skipDangerousModePermissionPrompt": true
}
"#;
