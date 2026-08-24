//! Coding Agent-specific runtime and configuration contracts.
//!
//! Shared orchestration asks [`AgentKind`] for paths, Named Config files, and
//! command construction. Transcript parsing remains in the two Session backend
//! modules because the Coding Agents use different on-disk formats.

use anyhow::{Context, Result};
use serde_json::{Map, Value};
use std::ffi::OsString;

/// Primitive value accepted by one fixed main-configuration Config Field.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MainConfigValueKind {
    String,
    Bool,
}

/// One fixed main-configuration field that every Config Application updates.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MainConfigField {
    pub(crate) path: &'static [&'static str],
    pub(crate) value_kind: MainConfigValueKind,
    pub(crate) label: &'static str,
    pub(crate) description: &'static str,
    pub(crate) group: &'static str,
    pub(crate) enum_values: &'static [&'static str],
    pub(crate) sensitive: bool,
    pub(crate) required: bool,
    pub(crate) required_for_custom_provider: bool,
    pub(crate) request_proxy_route: bool,
}

const NO_ENUM_VALUES: &[&str] = &[];
const APPROVAL_POLICIES: &[&str] = &["untrusted", "on-request", "never"];
const SANDBOX_MODES: &[&str] = &["read-only", "workspace-write", "danger-full-access"];
const REASONING_EFFORTS: &[&str] = &["minimal", "low", "medium", "high", "xhigh"];
const PLAN_REASONING_EFFORTS: &[&str] = &["none", "minimal", "low", "medium", "high", "xhigh"];

const CLAUDE_MAIN_CONFIG_FIELDS: &[MainConfigField] = &[
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

const CODEX_MAIN_CONFIG_FIELDS: &[MainConfigField] = &[
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

const DEFAULT_CODEX_CONFIG: &str = r#"approval_policy = "never"
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

const DEFAULT_CODEX_AUTH: &str = r#"{
  "OPENAI_API_KEY": "sk-example"
}
"#;

const DEFAULT_CLAUDE_CONFIG: &str = r#"{
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

/// Which Coding Agent a command targets.
///
/// Selected by `--agent` on Coding Agent-scoped commands.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum, serde::Deserialize, serde::Serialize,
)]
#[serde(rename_all = "lowercase")]
pub enum AgentKind {
    /// Anthropic Claude Code.
    Claude,
    /// OpenAI Codex.
    Codex,
}

impl AgentKind {
    /// Every Coding Agent supported by aibox.
    pub const ALL: [Self; 2] = [Self::Claude, Self::Codex];

    /// Lowercase name used by the CLI, paths, and executable.
    pub const fn tag(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }

    /// Agent state directory relative to the selected Tenant Home or Host Home.
    pub const fn state_dir_name(self) -> &'static str {
        match self {
            Self::Claude => ".claude",
            Self::Codex => ".codex",
        }
    }

    /// Primary native Current Config file.
    pub const fn main_config_file(self) -> &'static str {
        match self {
            Self::Claude => "settings.json",
            Self::Codex => "config.toml",
        }
    }

    /// Native authentication file in the Current Config, when separate.
    pub const fn native_auth_file(self) -> Option<&'static str> {
        match self {
            Self::Claude => None,
            Self::Codex => Some("auth.json"),
        }
    }

    /// Native files comprising a Named Config or Current Config.
    pub const fn config_files(self) -> &'static [&'static str] {
        match self {
            Self::Claude => &["settings.json"],
            Self::Codex => &["config.toml", "auth.json"],
        }
    }

    /// Empty native content used when editing a missing Current Config file.
    pub fn empty_config_file(self, file: &str) -> Option<&'static str> {
        match (self, file) {
            (Self::Claude, "settings.json") | (Self::Codex, "auth.json") => Some("{}\n"),
            (Self::Codex, "config.toml") => Some(""),
            _ => None,
        }
    }

    /// Fixed main-configuration fields accepted by a Named Config.
    pub(crate) const fn main_config_fields(self) -> &'static [MainConfigField] {
        match self {
            Self::Claude => CLAUDE_MAIN_CONFIG_FIELDS,
            Self::Codex => CODEX_MAIN_CONFIG_FIELDS,
        }
    }

    /// Built-in native main configuration used when the Console creates a Named Config.
    pub const fn config_template(self) -> &'static str {
        match self {
            Self::Claude => DEFAULT_CLAUDE_CONFIG,
            Self::Codex => DEFAULT_CODEX_CONFIG,
        }
    }

    /// Built-in native credential template used when the Console creates a Named Config.
    pub const fn config_auth_template(self) -> Option<&'static str> {
        match self {
            Self::Claude => None,
            Self::Codex => Some(DEFAULT_CODEX_AUTH),
        }
    }

    /// Parse the Coding Agent's native main configuration (Claude JSON or
    /// Codex TOML) into a generic object map.
    pub(crate) fn parse_main_config(self, content: &str) -> Result<Map<String, Value>> {
        if self == Self::Codex && content.trim().is_empty() {
            return Ok(Map::new());
        }
        let value = match self {
            Self::Codex => toml_edit::de::from_str::<Value>(content)?,
            Self::Claude => serde_json::from_str::<Value>(content)?,
        };
        value
            .as_object()
            .cloned()
            .with_context(|| format!("{} main configuration must be an object", self.tag()))
    }

    /// Render a JSON object in the Coding Agent's native main format.
    pub(crate) fn render_main_config(self, value: &Value) -> Result<String> {
        if !value.is_object() {
            anyhow::bail!("{} main configuration must be an object", self.tag());
        }
        match self {
            Self::Codex => Ok(toml_edit::ser::to_string_pretty(value)?),
            Self::Claude => Ok(format!("{}\n", serde_json::to_string_pretty(value)?)),
        }
    }

    /// Build the Coding Agent command without adding Named Config data.
    pub fn build_command(self, passthrough: &[OsString]) -> Vec<OsString> {
        let script = match self {
            Self::Claude => {
                r#". "$HOME/.config/aibox/env.sh"; exec "$HOME/.local/bin/claude" "$@""#
            }
            Self::Codex => r#". "$HOME/.config/aibox/env.sh"; exec "$HOME/.local/bin/codex" "$@""#,
        };
        let mut command = vec![
            OsString::from("/bin/bash"),
            OsString::from("--login"),
            OsString::from("-c"),
            OsString::from(script),
            OsString::from(format!("aibox-{}", self.tag())),
        ];
        command.extend(passthrough.iter().cloned());
        command
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_kind_carries_agent_contracts() {
        for (
            agent,
            tag,
            state_dir,
            main,
            native_auth,
            config_files,
            empty_files,
            main_config_fields,
            auth,
        ) in [
            (
                AgentKind::Claude,
                "claude",
                ".claude",
                "settings.json",
                None,
                &["settings.json"][..],
                &[("settings.json", "{}\n")][..],
                &[
                    (
                        &["env", "ANTHROPIC_BASE_URL"][..],
                        MainConfigValueKind::String,
                    ),
                    (
                        &["env", "ANTHROPIC_AUTH_TOKEN"][..],
                        MainConfigValueKind::String,
                    ),
                    (
                        &["env", "ANTHROPIC_DEFAULT_HAIKU_MODEL"][..],
                        MainConfigValueKind::String,
                    ),
                    (
                        &["env", "ANTHROPIC_DEFAULT_SONNET_MODEL"][..],
                        MainConfigValueKind::String,
                    ),
                    (
                        &["env", "ANTHROPIC_DEFAULT_OPUS_MODEL"][..],
                        MainConfigValueKind::String,
                    ),
                    (
                        &["env", "ANTHROPIC_DEFAULT_FABLE_MODEL"][..],
                        MainConfigValueKind::String,
                    ),
                    (
                        &["permissions", "defaultMode"][..],
                        MainConfigValueKind::String,
                    ),
                    (
                        &["skipDangerousModePermissionPrompt"][..],
                        MainConfigValueKind::Bool,
                    ),
                ][..],
                None,
            ),
            (
                AgentKind::Codex,
                "codex",
                ".codex",
                "config.toml",
                Some("auth.json"),
                &["config.toml", "auth.json"][..],
                &[("config.toml", ""), ("auth.json", "{}\n")][..],
                &[
                    (&["approval_policy"][..], MainConfigValueKind::String),
                    (&["sandbox_mode"][..], MainConfigValueKind::String),
                    (&["model_reasoning_effort"][..], MainConfigValueKind::String),
                    (
                        &["plan_mode_reasoning_effort"][..],
                        MainConfigValueKind::String,
                    ),
                    (&["model"][..], MainConfigValueKind::String),
                    (&["model_provider"][..], MainConfigValueKind::String),
                    (
                        &["model_providers", "custom", "name"][..],
                        MainConfigValueKind::String,
                    ),
                    (
                        &["model_providers", "custom", "base_url"][..],
                        MainConfigValueKind::String,
                    ),
                    (
                        &["model_providers", "custom", "requires_openai_auth"][..],
                        MainConfigValueKind::Bool,
                    ),
                ][..],
                Some("{\n  \"OPENAI_API_KEY\": \"sk-example\"\n}\n"),
            ),
        ] {
            assert_eq!(agent.tag(), tag, "{agent:?}");
            assert_eq!(agent.state_dir_name(), state_dir, "{agent:?}");
            assert_eq!(agent.main_config_file(), main, "{agent:?}");
            assert_eq!(agent.native_auth_file(), native_auth, "{agent:?}");
            assert_eq!(agent.config_files(), config_files, "{agent:?}");
            assert_eq!(agent.config_auth_template(), auth, "{agent:?}");
            for (file, expected) in empty_files {
                assert_eq!(
                    agent.empty_config_file(file),
                    Some(*expected),
                    "{agent:?} {file}"
                );
            }
            assert_eq!(agent.empty_config_file("unknown"), None, "{agent:?}");
            let actual_fields: Vec<_> = agent
                .main_config_fields()
                .iter()
                .map(|field| (field.path, field.value_kind))
                .collect();
            assert_eq!(actual_fields, main_config_fields, "{agent:?}");
        }
    }

    #[test]
    fn build_command_preserves_passthrough_without_injecting_named_config() {
        let pass = vec![OsString::from("--model"), OsString::from("opus")];
        let command = AgentKind::Claude.build_command(&pass);
        assert_eq!(
            command,
            [
                "/bin/bash",
                "--login",
                "-c",
                r#". "$HOME/.config/aibox/env.sh"; exec "$HOME/.local/bin/claude" "$@""#,
                "aibox-claude",
                "--model",
                "opus",
            ]
        );

        let command = AgentKind::Codex.build_command(&[]);
        assert_eq!(
            command,
            [
                "/bin/bash",
                "--login",
                "-c",
                r#". "$HOME/.config/aibox/env.sh"; exec "$HOME/.local/bin/codex" "$@""#,
                "aibox-codex",
            ]
        );
    }

    #[test]
    fn claude_template_uses_fables_native_one_megacontext_model_id() {
        let template: Value = serde_json::from_str(AgentKind::Claude.config_template()).unwrap();
        assert_eq!(
            template["env"]["ANTHROPIC_DEFAULT_FABLE_MODEL"],
            "claude-fable-5"
        );
    }

    #[cfg(unix)]
    #[test]
    fn command_preserves_non_utf8_passthrough_arguments() {
        use std::os::unix::ffi::OsStringExt;

        let opaque = OsString::from_vec(vec![b'f', 0x80, b'o']);
        let pass = vec![opaque.clone()];

        let command = AgentKind::Codex.build_command(&pass);

        assert_eq!(command.last(), Some(&opaque));
    }

    #[cfg(unix)]
    #[test]
    fn login_launch_loads_profile_then_aibox_environment_without_forcing_bashrc() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        use std::process::Command;

        let home = tempfile::tempdir().unwrap();
        fs::create_dir_all(home.path().join(".config/aibox")).unwrap();
        fs::create_dir_all(home.path().join(".local/bin")).unwrap();
        fs::write(
            home.path().join(".config/aibox/env.sh"),
            include_bytes!("../assets/aibox-env.sh"),
        )
        .unwrap();
        fs::write(
            home.path().join(".bash_profile"),
            b"export PROFILE_VALUE=profile\n\
export GOPATH=\"$HOME/custom-go\"\n\
export UV_PYTHON_INSTALL_DIR=\"$HOME/custom-python\"\n\
export UV_PYTHON_BIN_DIR=\"$HOME/custom-python-bin\"\n\
export UV_MANAGED_PYTHON=0\n\
export UV_PYTHON_DOWNLOADS=automatic\n",
        )
        .unwrap();
        fs::write(home.path().join(".bashrc"), b"export BASHRC_VALUE=bashrc\n").unwrap();
        let launcher = home.path().join(".local/bin/codex");
        fs::write(
            &launcher,
            br#"#!/bin/bash
if env | grep -q '^UV_NO_MANAGED_PYTHON='; then exit 9; fi
printf '%s|%s|%s|%s|%s|%s|%s|%s|%s\n' "${PROFILE_VALUE-}" "${BASHRC_VALUE-}" "$GOPATH" "$DISABLE_AUTOUPDATER" "$UV_PYTHON_INSTALL_DIR" "$UV_PYTHON_BIN_DIR" "$UV_MANAGED_PYTHON" "$UV_PYTHON_DOWNLOADS" "$1"
printf '%s\n' "$PATH"
"#,
        )
        .unwrap();
        fs::set_permissions(&launcher, fs::Permissions::from_mode(0o755)).unwrap();

        let command = AgentKind::Codex.build_command(&[OsString::from("argument with spaces")]);
        let output = Command::new(&command[0])
            .args(&command[1..])
            .env("HOME", home.path())
            .env("SHELL", "/bin/bash")
            .env_remove("PROFILE_VALUE")
            .env_remove("BASHRC_VALUE")
            .env_remove("GOPATH")
            .env_remove("DISABLE_AUTOUPDATER")
            .env_remove("UV_PYTHON_INSTALL_DIR")
            .env_remove("UV_PYTHON_BIN_DIR")
            .env_remove("UV_MANAGED_PYTHON")
            .env("UV_NO_MANAGED_PYTHON", "1")
            .env_remove("UV_PYTHON_DOWNLOADS")
            .output()
            .unwrap();

        assert!(output.status.success(), "{:?}", output);
        let stdout = String::from_utf8(output.stdout).unwrap();
        let mut lines = stdout.lines();
        assert_eq!(
            lines.next().unwrap(),
            format!(
                "profile||{0}/custom-go|1|{0}/custom-python|{0}/custom-python-bin|1|manual|argument with spaces",
                home.path().display(),
            )
        );
        let path = lines.next().unwrap();
        assert!(
            path.starts_with(&format!(
                "{0}/.local/bin:{0}/custom-python-bin:",
                home.path().display()
            )),
            "{path}"
        );
        for expected in [
            home.path().join(".local/bin"),
            home.path().join("custom-python-bin"),
        ] {
            assert_eq!(
                path.split(':')
                    .filter(|entry| *entry == expected.to_string_lossy())
                    .count(),
                1,
                "{path}"
            );
        }
    }
}
