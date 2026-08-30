//! OpenAI Codex's fixed Config Fields and built-in Named Config templates.

use super::{MainConfigField, MainConfigValueKind, NO_ENUM_VALUES};

const APPROVAL_POLICIES: &[&str] = &["untrusted", "on-request", "never"];
const SANDBOX_MODES: &[&str] = &["read-only", "workspace-write", "danger-full-access"];
const REASONING_EFFORTS: &[&str] = &["minimal", "low", "medium", "high", "xhigh"];
const PLAN_REASONING_EFFORTS: &[&str] = &["none", "minimal", "low", "medium", "high", "xhigh"];

pub(super) const MAIN_CONFIG_FIELDS: &[MainConfigField] = &[
    MainConfigField {
        path: &["approval_policy"],
        value_kind: MainConfigValueKind::String,
        label: "Approval policy",
        description: "Controls when Codex pauses before executing commands.",
        group: "Execution & permissions",
        enum_values: APPROVAL_POLICIES,
        sensitive: false,
        required: true,
        required_for_custom_provider: false,
        request_proxy_route: false,
    },
    MainConfigField {
        path: &["sandbox_mode"],
        value_kind: MainConfigValueKind::String,
        label: "Sandbox mode",
        description: "Filesystem and network access policy for command execution.",
        group: "Execution & permissions",
        enum_values: SANDBOX_MODES,
        sensitive: false,
        required: true,
        required_for_custom_provider: false,
        request_proxy_route: false,
    },
    MainConfigField {
        path: &["model_reasoning_effort"],
        value_kind: MainConfigValueKind::String,
        label: "Model reasoning effort",
        description: "Reasoning effort for supported models.",
        group: "Model & reasoning",
        enum_values: REASONING_EFFORTS,
        sensitive: false,
        required: false,
        required_for_custom_provider: false,
        request_proxy_route: false,
    },
    MainConfigField {
        path: &["plan_mode_reasoning_effort"],
        value_kind: MainConfigValueKind::String,
        label: "Plan mode reasoning effort",
        description: "Reasoning effort override used in Plan mode.",
        group: "Model & reasoning",
        enum_values: PLAN_REASONING_EFFORTS,
        sensitive: false,
        required: false,
        required_for_custom_provider: false,
        request_proxy_route: false,
    },
    MainConfigField {
        path: &["model"],
        value_kind: MainConfigValueKind::String,
        label: "Model",
        description: "Model selected for Codex sessions.",
        group: "Model & reasoning",
        enum_values: NO_ENUM_VALUES,
        sensitive: false,
        required: true,
        required_for_custom_provider: false,
        request_proxy_route: false,
    },
    MainConfigField {
        path: &["model_provider"],
        value_kind: MainConfigValueKind::String,
        label: "Model provider",
        description: "Provider id selected from the model_providers table.",
        group: "Provider",
        enum_values: &["openai", "custom"],
        sensitive: false,
        required: false,
        required_for_custom_provider: false,
        request_proxy_route: false,
    },
    MainConfigField {
        path: &["model_providers", "custom", "name"],
        value_kind: MainConfigValueKind::String,
        label: "Custom provider name",
        description: "Display name for the fixed custom provider.",
        group: "Provider",
        enum_values: NO_ENUM_VALUES,
        sensitive: false,
        required: false,
        required_for_custom_provider: true,
        request_proxy_route: false,
    },
    MainConfigField {
        path: &["model_providers", "custom", "base_url"],
        value_kind: MainConfigValueKind::String,
        label: "Custom provider base URL",
        description: "API base URL for the fixed custom provider.",
        group: "Provider",
        enum_values: NO_ENUM_VALUES,
        sensitive: false,
        required: false,
        required_for_custom_provider: true,
        request_proxy_route: true,
    },
    MainConfigField {
        path: &["model_providers", "custom", "requires_openai_auth"],
        value_kind: MainConfigValueKind::Bool,
        label: "Use OpenAI authentication",
        description: "Whether the custom provider uses OpenAI authentication.",
        group: "Provider",
        enum_values: NO_ENUM_VALUES,
        sensitive: false,
        required: false,
        required_for_custom_provider: true,
        request_proxy_route: false,
    },
];

pub(super) const DEFAULT_CONFIG: &str = r#"approval_policy = "never"
sandbox_mode = "danger-full-access"
model_reasoning_effort = "xhigh"
plan_mode_reasoning_effort = "xhigh"
model = "gpt-5.6-sol"
model_provider = "custom"

[model_providers.custom]
name = "custom"
base_url = "https://example.com/v1"
requires_openai_auth = true
"#;

pub(super) const DEFAULT_AUTH: &str = r#"{
  "OPENAI_API_KEY": "sk-example"
}
"#;
