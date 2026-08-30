//! Claude Code's fixed Config Fields and built-in Named Config template.

use super::{MainConfigField, MainConfigValueKind, NO_ENUM_VALUES};

pub(super) const MAIN_CONFIG_FIELDS: &[MainConfigField] = &[
    MainConfigField {
        path: &["env", "ANTHROPIC_BASE_URL"],
        value_kind: MainConfigValueKind::String,
        label: "Anthropic base URL",
        description: "Endpoint used for Claude requests.",
        group: "Endpoint & credentials",
        enum_values: NO_ENUM_VALUES,
        sensitive: false,
        required: true,
        required_for_custom_provider: false,
        request_proxy_route: true,
    },
    MainConfigField {
        path: &["env", "ANTHROPIC_AUTH_TOKEN"],
        value_kind: MainConfigValueKind::String,
        label: "Anthropic auth token",
        description: "Credential passed to Claude as ANTHROPIC_AUTH_TOKEN.",
        group: "Endpoint & credentials",
        enum_values: NO_ENUM_VALUES,
        sensitive: true,
        required: true,
        required_for_custom_provider: false,
        request_proxy_route: false,
    },
    MainConfigField {
        path: &["env", "ANTHROPIC_DEFAULT_HAIKU_MODEL"],
        value_kind: MainConfigValueKind::String,
        label: "Default Haiku model",
        description: "Model used for the Haiku class of requests.",
        group: "Model defaults",
        enum_values: NO_ENUM_VALUES,
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
        group: "Model defaults",
        enum_values: NO_ENUM_VALUES,
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
        group: "Model defaults",
        enum_values: NO_ENUM_VALUES,
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
        group: "Model defaults",
        enum_values: NO_ENUM_VALUES,
        sensitive: false,
        required: false,
        required_for_custom_provider: false,
        request_proxy_route: false,
    },
    MainConfigField {
        path: &["permissions", "defaultMode"],
        value_kind: MainConfigValueKind::String,
        label: "Default permission mode",
        description: "Claude's native permission mode for tool use.",
        group: "Permissions",
        enum_values: NO_ENUM_VALUES,
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
        group: "Permissions",
        enum_values: NO_ENUM_VALUES,
        sensitive: false,
        required: false,
        required_for_custom_provider: false,
        request_proxy_route: false,
    },
];

pub(super) const DEFAULT_CONFIG: &str = r#"{
  "env": {
    "ANTHROPIC_BASE_URL": "https://example.com",
    "ANTHROPIC_AUTH_TOKEN": "sk-example",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "claude-haiku-4-5",
    "ANTHROPIC_DEFAULT_SONNET_MODEL": "claude-sonnet-5[1m]",
    "ANTHROPIC_DEFAULT_OPUS_MODEL": "claude-opus-5[1m]",
    "ANTHROPIC_DEFAULT_FABLE_MODEL": "claude-fable-5"
  },
  "permissions": {
    "defaultMode": "bypassPermissions"
  },
  "skipDangerousModePermissionPrompt": true
}
"#;
