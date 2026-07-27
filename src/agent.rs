//! The one place the two agents diverge.

use crate::runspec::{Invocation, RunOpts};
use anyhow::Result;

/// Which agent a command targets. Selected by `--agent` on agent-scoped commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum AgentKind {
    Claude,
    Codex,
}

impl AgentKind {
    pub fn tag(self) -> &'static str {
        match self {
            AgentKind::Claude => "claude",
            AgentKind::Codex => "codex",
        }
    }

    pub fn image_default(self) -> &'static str {
        match self {
            AgentKind::Claude => "aibox-claude:latest",
            AgentKind::Codex => "aibox-codex:latest",
        }
    }

    pub fn supports_exec(self) -> bool {
        matches!(self, AgentKind::Codex)
    }

    pub fn container_home(self) -> &'static str {
        match self {
            AgentKind::Claude => "/home/claude",
            AgentKind::Codex => "/home/codex",
        }
    }

    pub fn active_dir_name(self) -> &'static str {
        match self {
            AgentKind::Claude => ".claude",
            AgentKind::Codex => ".codex",
        }
    }

    pub fn main_config_file(self) -> &'static str {
        match self {
            AgentKind::Claude => "settings.json",
            AgentKind::Codex => "config.toml",
        }
    }

    pub fn auth_file(self) -> Option<&'static str> {
        match self {
            AgentKind::Claude => None,
            AgentKind::Codex => Some("auth.json"),
        }
    }

    pub fn managed_config_files(self) -> &'static [&'static str] {
        match self {
            AgentKind::Claude => &["settings.json"],
            AgentKind::Codex => &["config.toml", "auth.json"],
        }
    }

    pub fn dockerfile(self) -> &'static str {
        match self {
            AgentKind::Claude => include_str!("../assets/claude.Dockerfile"),
            AgentKind::Codex => include_str!("../assets/codex.Dockerfile"),
        }
    }

    pub fn build_invocation(self, opts: &RunOpts) -> Result<Invocation> {
        let mut agent_cmd = Vec::new();
        match self {
            AgentKind::Claude => {
                if opts.safe {
                    eprintln!(">> permissions: prompting (--safe)");
                } else {
                    agent_cmd.push("--dangerously-skip-permissions".to_string());
                    eprintln!(
                        ">> permissions: SKIPPED (agent runs unrestricted; use --safe to prompt)"
                    );
                }
            }
            AgentKind::Codex => {
                if opts.exec {
                    agent_cmd.push("exec".to_string());
                }
                push_codex_permissions(&mut agent_cmd, opts.safe, opts.exec);
            }
        }
        agent_cmd.extend(opts.passthrough.iter().cloned());
        Ok(Invocation {
            extra_run_args: Vec::new(),
            agent_cmd,
        })
    }
}

fn push_codex_permissions(cmd: &mut Vec<String>, safe: bool, exec: bool) {
    if safe {
        if exec {
            push_c_string(cmd, "approval_policy", "on-request");
        } else {
            cmd.extend(["-a".into(), "on-request".into()]);
        }
        cmd.extend(["-s".into(), "workspace-write".into()]);
        eprintln!(">> permissions: prompting + workspace-write sandbox (--safe)");
    } else {
        cmd.push("--dangerously-bypass-approvals-and-sandbox".into());
        eprintln!(">> permissions: BYPASSED (agent runs unrestricted; use --safe to prompt)");
    }
}

fn push_c_string(cmd: &mut Vec<String>, key: &str, value: &str) {
    cmd.push("-c".to_string());
    cmd.push(format!("{key}={}", codex_config_string(value)));
}

fn codex_config_string(value: &str) -> String {
    serde_json::Value::String(value.to_string())
        .to_string()
        .replace('\u{7f}', "\\u007F")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runspec::RunOpts;

    fn opts(passthrough: &[String]) -> RunOpts<'_> {
        RunOpts {
            safe: false,
            exec: false,
            passthrough,
        }
    }

    #[test]
    fn agent_kind_carries_agent_contracts() {
        assert_eq!(AgentKind::Claude.tag(), "claude");
        assert_eq!(AgentKind::Codex.tag(), "codex");
        assert_eq!(AgentKind::Claude.image_default(), "aibox-claude:latest");
        assert_eq!(AgentKind::Codex.image_default(), "aibox-codex:latest");
        assert_eq!(AgentKind::Claude.container_home(), "/home/claude");
        assert_eq!(AgentKind::Codex.container_home(), "/home/codex");
        assert_eq!(AgentKind::Claude.active_dir_name(), ".claude");
        assert_eq!(AgentKind::Codex.active_dir_name(), ".codex");
        assert_eq!(AgentKind::Claude.main_config_file(), "settings.json");
        assert_eq!(AgentKind::Codex.main_config_file(), "config.toml");
        assert_eq!(AgentKind::Codex.auth_file(), Some("auth.json"));
        assert_eq!(AgentKind::Claude.auth_file(), None);
    }

    #[test]
    fn build_invocation_no_longer_injects_provider_config() {
        let pass = vec!["--model".to_string(), "opus".to_string()];
        let inv = AgentKind::Claude.build_invocation(&opts(&pass)).unwrap();
        assert_eq!(
            inv.agent_cmd,
            ["--dangerously-skip-permissions", "--model", "opus"]
        );
        assert!(inv.extra_run_args.is_empty());

        let inv = AgentKind::Codex.build_invocation(&opts(&[])).unwrap();
        assert_eq!(
            inv.agent_cmd,
            ["--dangerously-bypass-approvals-and-sandbox"]
        );
        assert!(inv.extra_run_args.is_empty());
    }

    #[test]
    fn codex_safe_exec_uses_config_override_for_approval_policy() {
        let mut o = opts(&[]);
        o.safe = true;
        o.exec = true;

        let inv = AgentKind::Codex.build_invocation(&o).unwrap();

        assert_eq!(
            inv.agent_cmd,
            [
                "exec",
                "-c",
                "approval_policy=\"on-request\"",
                "-s",
                "workspace-write"
            ]
        );
    }
}
