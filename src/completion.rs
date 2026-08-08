//! Dynamic shell completion registration and read-only host-side discovery.

use crate::agent::AgentKind;
use crate::cli::{Cli, CompletionArgs, CompletionShell, SelectionOption};
use crate::tenant::{self, TenantAgent};
use anyhow::{Context, Result};
use clap::{CommandFactory, ValueHint};
use clap_complete::engine::{
    ArgValueCompleter, CompletionCandidate, PathCompleter, ValueCompleter,
};
use clap_complete::env::{Bash, CompleteEnv, EnvCompleter, Fish, Shells, Zsh};
use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

const COMPLETE_ENV: &str = "AIBOX_COMPLETE";
const COMMAND_NAME: &str = "aibox";

/// Process an environment-activated completion request.
pub(crate) fn handle_env() {
    let Some(enabled) = std::env::var_os(COMPLETE_ENV) else {
        return;
    };
    if enabled.is_empty() || enabled == "0" {
        return;
    }
    let argv: Vec<OsString> = std::env::args_os().collect();
    let context = CompletionContext::from_protocol_argv(&argv);
    let executable = current_executable(&argv);
    let current_dir = std::env::current_dir().ok();
    CompleteEnv::with_factory(move || completion_command(context.clone(), current_dir.clone()))
        .var(COMPLETE_ENV)
        .bin(COMMAND_NAME)
        .completer(executable)
        .shells(supported_shells())
        .complete();
}

/// Print a shell registration script without changing shell startup files.
pub(crate) fn dispatch(args: &CompletionArgs) -> Result<i32> {
    let executable = std::env::current_exe().context("resolve current aibox executable")?;
    let executable = executable
        .to_str()
        .context("current aibox executable path is not valid UTF-8")?;
    let mut output = Vec::new();
    shell_completer(args.shell).write_registration(
        COMPLETE_ENV,
        COMMAND_NAME,
        COMMAND_NAME,
        executable,
        &mut output,
    )?;
    crate::print_text(std::str::from_utf8(&output)?)?;
    Ok(0)
}

fn current_executable(argv: &[OsString]) -> String {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.to_str().map(str::to_owned))
        .or_else(|| {
            argv.first()
                .map(|value| value.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| COMMAND_NAME.to_string())
}

fn supported_shells() -> Shells<'static> {
    Shells(&[&DynamicShell::Bash, &DynamicShell::Zsh, &DynamicShell::Fish])
}

fn shell_completer(shell: CompletionShell) -> &'static dyn EnvCompleter {
    match shell {
        CompletionShell::Bash => &DynamicShell::Bash,
        CompletionShell::Zsh => &DynamicShell::Zsh,
        CompletionShell::Fish => &DynamicShell::Fish,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DynamicShell {
    Bash,
    Zsh,
    Fish,
}

impl DynamicShell {
    fn inner(self) -> &'static dyn EnvCompleter {
        match self {
            Self::Bash => &Bash,
            Self::Zsh => &Zsh,
            Self::Fish => &Fish,
        }
    }

    fn completion_index(self, args: &[OsString]) -> Option<usize> {
        match self {
            Self::Fish => args.len().checked_sub(1),
            Self::Bash | Self::Zsh => std::env::var("_CLAP_COMPLETE_INDEX")
                .ok()
                .and_then(|index| index.parse().ok()),
        }
    }
}

impl EnvCompleter for DynamicShell {
    fn name(&self) -> &'static str {
        self.inner().name()
    }

    fn is(&self, name: &str) -> bool {
        self.inner().is(name)
    }

    fn write_registration(
        &self,
        var: &str,
        name: &str,
        bin: &str,
        completer: &str,
        buf: &mut dyn std::io::Write,
    ) -> Result<(), std::io::Error> {
        self.inner()
            .write_registration(var, name, bin, completer, buf)
    }

    fn write_complete(
        &self,
        cmd: &mut clap::Command,
        args: Vec<OsString>,
        current_dir: Option<&Path>,
        buf: &mut dyn std::io::Write,
    ) -> Result<(), std::io::Error> {
        if self
            .completion_index(&args)
            .is_some_and(|index| !crate::cli::short_option_completion_allowed(cmd, &args, index))
        {
            return Ok(());
        }
        self.inner().write_complete(cmd, args, current_dir, buf)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TopCommand {
    Root,
    Run,
    Tenant,
    Component,
    Config,
    Session,
    Traffic,
    Other,
}

#[derive(Clone, Debug)]
struct CompletionContext {
    top: TopCommand,
    agent: AgentKind,
    tenant: String,
    tenant_explicit: bool,
    host: bool,
    selection_valid: bool,
    all: bool,
    current: bool,
    current_position: Option<usize>,
    leaf: Option<String>,
    leaf_position: Option<usize>,
    positionals: BTreeSet<String>,
}

impl Default for CompletionContext {
    fn default() -> Self {
        Self {
            top: TopCommand::Root,
            agent: AgentKind::Codex,
            tenant: "default".to_string(),
            tenant_explicit: false,
            host: false,
            selection_valid: true,
            all: false,
            current: false,
            current_position: None,
            leaf: None,
            leaf_position: None,
            positionals: BTreeSet::new(),
        }
    }
}

impl CompletionContext {
    fn from_protocol_argv(argv: &[OsString]) -> Self {
        let Some(boundary) = argv.iter().position(|arg| arg == "--") else {
            return Self::default();
        };
        Self::from_words(&argv[boundary + 1..])
    }

    fn from_words(words: &[OsString]) -> Self {
        let mut context = Self::default();
        let business_end = words
            .iter()
            .position(|arg| arg == "--")
            .unwrap_or(words.len());
        let Some(values) = words[..business_end]
            .iter()
            .map(|value| value.to_str())
            .collect::<Option<Vec<_>>>()
        else {
            context.selection_valid = false;
            return context;
        };
        context.top = match values.get(1).copied() {
            Some("run") => TopCommand::Run,
            Some("tenant") => TopCommand::Tenant,
            Some("component") => TopCommand::Component,
            Some("config") => TopCommand::Config,
            Some("session") => TopCommand::Session,
            Some("traffic") => TopCommand::Traffic,
            Some("build" | "completion") => TopCommand::Other,
            _ => TopCommand::Root,
        };
        let scoped = matches!(
            context.top,
            TopCommand::Run | TopCommand::Component | TopCommand::Config | TopCommand::Session
        );
        let mut option_parts = BTreeSet::new();
        if scoped {
            context.capture_selection(&values, &mut option_parts);
        }
        context.capture_positionals(&values, &option_parts);
        context.validate_current_position();
        context
    }

    fn capture_selection(&mut self, values: &[&str], option_parts: &mut BTreeSet<usize>) {
        let mut seen_agent = false;
        let mut seen_tenant = false;
        let mut seen_host = false;
        let mut index = 2;
        while index < values.len() {
            let token = values[index];
            if token == "--current" {
                option_parts.insert(index);
                self.current = true;
                self.current_position = Some(index);
                index += 1;
                continue;
            }
            if token == "--host" {
                option_parts.insert(index);
                if seen_host || seen_tenant || self.top == TopCommand::Run {
                    self.selection_valid = false;
                }
                seen_host = true;
                self.host = true;
                index += 1;
                continue;
            }
            let Some((option, inline)) = SelectionOption::parse(token) else {
                index += 1;
                continue;
            };
            option_parts.insert(index);
            let value = match inline {
                Some(value) => value,
                None if index + 1 < values.len() => {
                    index += 1;
                    option_parts.insert(index);
                    values[index]
                }
                None => {
                    self.selection_valid = false;
                    break;
                }
            };
            match option {
                SelectionOption::Agent => {
                    if seen_agent || self.top == TopCommand::Component {
                        self.selection_valid = false;
                    }
                    seen_agent = true;
                    self.agent = match value {
                        "codex" => AgentKind::Codex,
                        "claude" => AgentKind::Claude,
                        _ => {
                            self.selection_valid = false;
                            AgentKind::Codex
                        }
                    };
                }
                SelectionOption::Tenant => {
                    if seen_tenant || seen_host {
                        self.selection_valid = false;
                    }
                    seen_tenant = true;
                    self.tenant_explicit = true;
                    if tenant::validate_name("tenant", value).is_err() {
                        self.selection_valid = false;
                    } else {
                        self.tenant = value.to_string();
                    }
                }
            }
            index += 1;
        }
    }

    fn capture_positionals(&mut self, values: &[&str], option_parts: &BTreeSet<usize>) {
        let leaves: &[&str] = match self.top {
            TopCommand::Tenant => &["list", "create", "delete"],
            TopCommand::Component => &["list", "install", "remove"],
            TopCommand::Config => &[
                "list",
                "get",
                "create",
                "edit",
                "delete",
                "apply",
                "propagate-auth",
            ],
            TopCommand::Session => &["list", "get", "delete"],
            _ => return,
        };
        let Some(leaf) = (2..values.len())
            .find(|index| !option_parts.contains(index) && leaves.contains(&values[*index]))
        else {
            return;
        };
        self.leaf = Some(values[leaf].to_string());
        self.leaf_position = Some(leaf);
        for (index, value) in values.iter().enumerate().skip(leaf + 1) {
            if option_parts.contains(&index) {
                continue;
            }
            if *value == "--all" {
                self.all = true;
            } else if !value.starts_with('-') && !value.is_empty() {
                self.positionals.insert((*value).to_string());
            }
        }
    }

    fn propagate_auth_available(&self) -> bool {
        self.top == TopCommand::Config
            && self.selection_valid
            && !self.tenant_explicit
            && self.agent == AgentKind::Codex
    }

    fn validate_current_position(&mut self) {
        let Some(current) = self.current_position else {
            return;
        };
        let valid_leaf = matches!(
            self.leaf.as_deref(),
            Some("get" | "edit" | "propagate-auth")
        );
        if self.top != TopCommand::Config
            || !valid_leaf
            || self.leaf_position.is_none_or(|leaf| current <= leaf)
        {
            self.selection_valid = false;
        }
    }
}

#[derive(Clone, Copy)]
enum TenantCandidates {
    Select,
    Existing,
}

fn completion_command(context: CompletionContext, current_dir: Option<PathBuf>) -> clap::Command {
    let command = add_run_completers(Cli::command(), current_dir);
    let command = add_tenant_completers(command, context.clone());
    let command = add_component_completers(command, context.clone());
    let command = add_config_completers(command, context.clone());
    add_session_completers(command, context)
}

fn add_run_completers(command: clap::Command, current_dir: Option<PathBuf>) -> clap::Command {
    let workspace_dir = current_dir.clone();
    command.mut_subcommand("run", move |command| {
        command
            .mut_arg("run-tenant", add_tenant_value_completer)
            .mut_arg("workspace", move |arg| {
                arg.value_hint(ValueHint::DirPath)
                    .add(ArgValueCompleter::new(move |current: &OsStr| {
                        complete_workspace(current, workspace_dir.as_deref())
                    }))
            })
            .mut_arg("mount", move |arg| {
                arg.value_hint(ValueHint::Other).add(ArgValueCompleter::new(
                    move |current: &OsStr| complete_mount(current, current_dir.as_deref()),
                ))
            })
    })
}

fn add_tenant_completers(command: clap::Command, context: CompletionContext) -> clap::Command {
    command.mut_subcommand("tenant", move |command| {
        command
            .mut_subcommand("create", |command| {
                command.mut_arg("tenant", |arg| arg.value_hint(ValueHint::Other))
            })
            .mut_subcommand("delete", move |command| {
                command.mut_arg("tenants", move |arg| {
                    arg.value_hint(ValueHint::Other).add(ArgValueCompleter::new(
                        move |current: &OsStr| {
                            if context.all {
                                Vec::new()
                            } else {
                                complete_tenants(
                                    TenantCandidates::Existing,
                                    current,
                                    &context.positionals,
                                )
                            }
                        },
                    ))
                })
            })
    })
}

fn add_component_completers(command: clap::Command, context: CompletionContext) -> clap::Command {
    let install_context = context.clone();
    let remove_context = context;
    command.mut_subcommand("component", move |command| {
        command
            .mut_arg("tenant", add_tenant_value_completer)
            .mut_subcommand("install", |command| {
                let context = install_context.clone();
                command.mut_arg("component", |arg| {
                    arg.value_hint(ValueHint::Other).add(ArgValueCompleter::new(
                        move |current: &OsStr| complete_components(&context, current),
                    ))
                })
            })
            .mut_subcommand("remove", |command| {
                let context = remove_context.clone();
                command.mut_arg("component", |arg| {
                    arg.value_hint(ValueHint::Other).add(ArgValueCompleter::new(
                        move |current: &OsStr| complete_components(&context, current),
                    ))
                })
            })
    })
}

fn add_config_completers(command: clap::Command, context: CompletionContext) -> clap::Command {
    let get = context.clone();
    let edit = context.clone();
    let apply = context.clone();
    let delete = context.clone();
    let propagate_auth_available = context.propagate_auth_available();
    let propagating_auth = context.leaf.as_deref() == Some("propagate-auth");
    command.mut_subcommand("config", move |command| {
        command
            .mut_arg("tenant", move |arg| {
                if propagating_auth {
                    arg.hide(true)
                } else {
                    add_tenant_value_completer(arg)
                }
            })
            .mut_arg("agent", move |arg| {
                if propagating_auth {
                    arg.add(ArgValueCompleter::new(complete_codex_agent))
                } else {
                    arg
                }
            })
            .mut_subcommand("create", |command| {
                command.mut_arg("config", |arg| arg.value_hint(ValueHint::Other))
            })
            .mut_subcommand("get", move |command| {
                add_config_completer(command, get, false)
            })
            .mut_subcommand("edit", move |command| {
                add_config_completer(command, edit, false)
            })
            .mut_subcommand("apply", move |command| {
                add_config_completer(command, apply, false)
            })
            .mut_subcommand("delete", move |command| {
                add_config_completer(command, delete, true)
            })
            .mut_subcommand("propagate-auth", move |command| {
                command.hide(!propagate_auth_available)
            })
    })
}

fn complete_codex_agent(current: &OsStr) -> Vec<CompletionCandidate> {
    filter_candidates(
        [AgentKind::Codex.tag().to_string()],
        current,
        &BTreeSet::new(),
    )
}

fn add_session_completers(command: clap::Command, context: CompletionContext) -> clap::Command {
    let get = context.clone();
    command.mut_subcommand("session", move |command| {
        command
            .mut_arg("tenant", add_tenant_value_completer)
            .mut_subcommand("get", move |command| {
                add_session_completer(command, get, false)
            })
            .mut_subcommand("delete", move |command| {
                add_session_completer(command, context, true)
            })
    })
}

fn add_tenant_value_completer(arg: clap::Arg) -> clap::Arg {
    arg.value_hint(ValueHint::Other)
        .add(ArgValueCompleter::new(move |current: &OsStr| {
            complete_tenants(TenantCandidates::Select, current, &BTreeSet::new())
        }))
}

fn complete_components(context: &CompletionContext, current: &OsStr) -> Vec<CompletionCandidate> {
    let kinds: &[crate::component::ComponentKind] = if context.host {
        &crate::component::ComponentKind::STATUSLINES
    } else {
        &crate::component::ComponentKind::ALL
    };
    filter_candidates(
        kinds.iter().map(|kind| kind.name().to_string()),
        current,
        &BTreeSet::new(),
    )
}

fn add_config_completer(
    command: clap::Command,
    context: CompletionContext,
    repeatable: bool,
) -> clap::Command {
    let id = if repeatable { "configs" } else { "config" };
    command.mut_arg(id, move |arg| {
        arg.value_hint(ValueHint::Other)
            .add(ArgValueCompleter::new(move |current: &OsStr| {
                if context.current || (repeatable && context.all) {
                    Vec::new()
                } else {
                    let excluded = if repeatable {
                        &context.positionals
                    } else {
                        &BTreeSet::new()
                    };
                    complete_named_configs(&context, current, excluded)
                }
            }))
    })
}

fn add_session_completer(
    command: clap::Command,
    context: CompletionContext,
    repeatable: bool,
) -> clap::Command {
    let id = if repeatable { "ids" } else { "id" };
    command.mut_arg(id, move |arg| {
        arg.value_hint(ValueHint::Other)
            .add(ArgValueCompleter::new(move |current: &OsStr| {
                if repeatable && context.all {
                    Vec::new()
                } else {
                    let excluded = if repeatable {
                        &context.positionals
                    } else {
                        &BTreeSet::new()
                    };
                    complete_sessions(&context, current, excluded)
                }
            }))
    })
}

fn complete_tenants(
    mode: TenantCandidates,
    current: &OsStr,
    excluded: &BTreeSet<String>,
) -> Vec<CompletionCandidate> {
    let values = (|| -> Result<Vec<String>> {
        let root = tenant::aibox_root()?;
        tenant_values_at(&root, mode)
    })()
    .unwrap_or_default();
    filter_candidates(values, current, excluded)
}

fn tenant_values_at(root: &Path, mode: TenantCandidates) -> Result<Vec<String>> {
    let mut values: BTreeSet<_> = tenant::list_tenants(root)?.into_iter().collect();
    if matches!(mode, TenantCandidates::Select) {
        values.insert("default".to_string());
    }
    Ok(values.into_iter().collect())
}

fn selected_at(root: &Path, context: &CompletionContext) -> Result<TenantAgent> {
    TenantAgent::resolve(context.agent, root, context.host, &context.tenant)
}

fn complete_named_configs(
    context: &CompletionContext,
    current: &OsStr,
    excluded: &BTreeSet<String>,
) -> Vec<CompletionCandidate> {
    if !context.selection_valid {
        return Vec::new();
    }
    let values = (|| -> Result<Vec<String>> {
        let root = tenant::aibox_root()?;
        crate::config::list_named_configs(&selected_at(&root, context)?)
    })()
    .unwrap_or_default();
    filter_candidates(values, current, excluded)
}

fn complete_sessions(
    context: &CompletionContext,
    current: &OsStr,
    excluded: &BTreeSet<String>,
) -> Vec<CompletionCandidate> {
    if !context.selection_valid {
        return Vec::new();
    }
    let values = (|| -> Result<Vec<String>> {
        let root = tenant::aibox_root()?;
        session_values_at(&root, context)
    })()
    .unwrap_or_default();
    filter_session_candidates(values, current, excluded)
}

fn session_values_at(root: &Path, context: &CompletionContext) -> Result<Vec<String>> {
    let selected = selected_at(root, context)?;
    selected.tenant.validate_session_home()?;
    let backend = crate::session::backend_for(context.agent);
    let mut ids: Vec<_> = backend
        .files(selected.home_dir())?
        .into_iter()
        .map(|path| backend.id_of(&path))
        .filter(|id| !id.is_empty())
        .collect();
    ids.sort();
    ids.dedup();
    Ok(ids)
}

fn filter_candidates(
    values: impl IntoIterator<Item = String>,
    current: &OsStr,
    excluded: &BTreeSet<String>,
) -> Vec<CompletionCandidate> {
    let Some(current) = current.to_str() else {
        return Vec::new();
    };
    let values: BTreeSet<_> = values
        .into_iter()
        .filter(|value| value.starts_with(current) && !excluded.contains(value))
        .collect();
    values.into_iter().map(CompletionCandidate::new).collect()
}

fn filter_session_candidates(
    values: impl IntoIterator<Item = String>,
    current: &OsStr,
    excluded: &BTreeSet<String>,
) -> Vec<CompletionCandidate> {
    let Some(current) = current.to_str() else {
        return Vec::new();
    };
    let values: BTreeSet<_> = values
        .into_iter()
        .filter(|value| value.ends_with(current) && !excluded.contains(value))
        .collect();
    values.into_iter().map(CompletionCandidate::new).collect()
}

fn complete_mount(current: &OsStr, current_dir: Option<&Path>) -> Vec<CompletionCandidate> {
    if current.as_encoded_bytes().contains(&b':') {
        return Vec::new();
    }
    let completer = current_dir.map_or_else(PathCompleter::any, |path| {
        PathCompleter::any().current_dir(path)
    });
    without_colon(completer.complete(current))
}

fn complete_workspace(current: &OsStr, current_dir: Option<&Path>) -> Vec<CompletionCandidate> {
    let completer = current_dir.map_or_else(PathCompleter::dir, |path| {
        PathCompleter::dir().current_dir(path)
    });
    without_colon(completer.complete(current))
}

fn without_colon(candidates: Vec<CompletionCandidate>) -> Vec<CompletionCandidate> {
    candidates
        .into_iter()
        .filter(|candidate| !candidate.get_value().as_encoded_bytes().contains(&b':'))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tenant::ManagedTenant;

    fn words(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    fn candidate_values(candidates: &[CompletionCandidate]) -> Vec<String> {
        candidates
            .iter()
            .map(|candidate| candidate.get_value().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn context_stops_at_agent_passthrough_boundary() {
        let context = CompletionContext::from_words(&words(&[
            "aibox", "run", "--tenant", "work", "--", "--tenant", "ignored",
        ]));
        assert_eq!(context.tenant, "work");

        let protocol = CompletionContext::from_protocol_argv(&words(&[
            "aibox",
            "--shell",
            "bash",
            "--",
            "aibox",
            "run",
            "--agent=claude",
            "--tenant=work",
            "--",
            "--agent=codex",
        ]));
        assert_eq!(protocol.top, TopCommand::Run);
        assert_eq!(protocol.agent, AgentKind::Claude);
        assert_eq!(protocol.tenant, "work");
        assert!(protocol.selection_valid);
    }

    #[test]
    fn host_and_tenant_are_distinct_and_conflicting() {
        let host = CompletionContext::from_words(&words(&["aibox", "config", "--host", "list"]));
        assert!(host.host);
        assert!(host.selection_valid);
        let bad = CompletionContext::from_words(&words(&[
            "aibox", "config", "--host", "--tenant", "host", "list",
        ]));
        assert!(!bad.selection_valid);
    }

    #[test]
    fn selection_context_rejects_the_same_invalid_scopes_as_clap() {
        for args in [
            &["aibox", "run", "--host"][..],
            &["aibox", "component", "--agent", "claude", "list"][..],
            &["aibox", "config", "--agent", "unknown", "list"][..],
            &[
                "aibox",
                "session",
                "--tenant",
                "one",
                "--tenant=two",
                "list",
            ][..],
        ] {
            assert!(Cli::try_parse_from(args).is_err(), "clap accepted {args:?}");
            let context = CompletionContext::from_words(&words(args));
            assert!(
                !context.selection_valid,
                "completion accepted an invalid command scope: {args:?}"
            );
            assert!(
                complete_named_configs(&context, OsStr::new(""), &BTreeSet::new()).is_empty(),
                "invalid selection must not discover host-side candidates: {args:?}"
            );
        }

        let inline = CompletionContext::from_words(&words(&[
            "aibox",
            "config",
            "--agent=claude",
            "--tenant=work",
            "list",
        ]));
        assert!(inline.selection_valid);
        assert_eq!(inline.agent, AgentKind::Claude);
        assert_eq!(inline.tenant, "work");
    }

    #[test]
    fn propagate_auth_completion_matches_the_fixed_host_codex_current_source() {
        for args in [
            &["aibox", "config"][..],
            &["aibox", "config", "--host"][..],
            &["aibox", "config", "--agent", "codex"][..],
            &[
                "aibox",
                "config",
                "--host",
                "--agent=codex",
                "propagate-auth",
                "--current",
            ][..],
        ] {
            let context = CompletionContext::from_words(&words(args));
            assert!(context.propagate_auth_available(), "{args:?}");
            let command = completion_command(context, None);
            let config = command.find_subcommand("config").unwrap();
            let propagate = config.find_subcommand("propagate-auth").unwrap();
            assert!(!propagate.is_hide_set(), "{args:?}");
            let tenant_hidden = config
                .get_arguments()
                .find(|arg| arg.get_id() == "tenant")
                .unwrap()
                .is_hide_set();
            assert_eq!(tenant_hidden, args.contains(&"propagate-auth"), "{args:?}");
        }

        for args in [
            &["aibox", "config", "--tenant", "default"][..],
            &["aibox", "config", "--agent", "claude"][..],
            &["aibox", "config", "--host", "--tenant", "work"][..],
            &["aibox", "config", "--current"][..],
        ] {
            let context = CompletionContext::from_words(&words(args));
            assert!(!context.propagate_auth_available(), "{args:?}");
            let command = completion_command(context, None);
            let propagate = command
                .find_subcommand("config")
                .unwrap()
                .find_subcommand("propagate-auth")
                .unwrap();
            assert!(propagate.is_hide_set(), "{args:?}");
        }

        assert_eq!(
            candidate_values(&complete_codex_agent(OsStr::new(""))),
            ["codex"]
        );
        assert!(complete_codex_agent(OsStr::new("cl")).is_empty());
    }

    #[test]
    fn component_completion_matches_tenant_scope() {
        let context = CompletionContext::from_words(&words(&[
            "aibox",
            "component",
            "--tenant",
            "work",
            "install",
            "",
        ]));
        assert_eq!(context.top, TopCommand::Component);
        assert_eq!(context.tenant, "work");
        assert!(context.selection_valid);
        assert!(!context.host);
        let values = complete_components(&context, OsStr::new(""));
        assert_eq!(
            candidate_values(&values),
            ["claude-statusline", "codex-statusline", "go", "rust"]
        );

        let host = CompletionContext::from_words(&words(&["aibox", "component", "--host", "list"]));
        assert!(host.selection_valid);
        assert!(host.host);
        let values = complete_components(&host, OsStr::new(""));
        assert_eq!(
            candidate_values(&values),
            ["claude-statusline", "codex-statusline"]
        );
    }

    #[test]
    fn candidate_filtering_is_sorted_unique_and_never_completes_mount_targets() {
        let excluded = BTreeSet::from(["alpha".to_string()]);
        let values = filter_candidates(
            ["alpine", "alpha", "alpine", "beta"].map(str::to_string),
            OsStr::new("al"),
            &excluded,
        );
        assert_eq!(candidate_values(&values), ["alpine"]);

        assert!(complete_mount(OsStr::new("src:/container"), None).is_empty());
        let values = without_colon(vec![
            CompletionCandidate::new("safe-path"),
            CompletionCandidate::new("unsafe:path"),
        ]);
        assert_eq!(candidate_values(&values), ["safe-path"]);
    }

    #[test]
    fn dynamic_completion_never_extends_short_flags_into_clusters() {
        let _env_lock = crate::test_env_lock();
        let _index = crate::testutil::EnvGuard::set("_CLAP_COMPLETE_INDEX", "5");
        for current in ["-y", "-yh", "-x"] {
            for shell in [DynamicShell::Bash, DynamicShell::Zsh, DynamicShell::Fish] {
                let args = words(&["aibox", "session", "--agent", "claude", "delete", current]);
                let context = CompletionContext::from_words(&args);
                let mut command = completion_command(context, None);
                command.build();
                let mut output = Vec::new();
                shell
                    .write_complete(&mut command, args, None, &mut output)
                    .unwrap();
                assert!(
                    output.is_empty(),
                    "{shell:?} {current}: {}",
                    String::from_utf8_lossy(&output)
                );
            }
        }

        let args = words(&["aibox", "session", "--agent", "claude", "delete", "-"]);
        let context = CompletionContext::from_words(&args);
        let mut command = completion_command(context, None);
        command.build();
        let mut output = Vec::new();
        DynamicShell::Fish
            .write_complete(&mut command, args, None, &mut output)
            .unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(
            output.lines().any(|line| line.starts_with("-y\t")),
            "{output}"
        );
        assert!(
            output.lines().any(|line| line.starts_with("-h\t")),
            "{output}"
        );
    }

    #[test]
    fn dynamic_completion_preserves_attached_short_option_values() {
        let current_dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(current_dir.path().join("src")).unwrap();
        let args = words(&["aibox", "run", "-wsrc"]);
        let context = CompletionContext::from_words(&args);
        let mut command = completion_command(context, Some(current_dir.path().to_path_buf()));
        command.build();
        let mut output = Vec::new();
        DynamicShell::Fish
            .write_complete(&mut command, args, Some(current_dir.path()), &mut output)
            .unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.lines().any(|line| line == "-wsrc/"), "{output}");
    }

    #[test]
    fn tenant_discovery_is_read_only() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("missing");
        assert_eq!(
            tenant_values_at(&root, TenantCandidates::Select).unwrap(),
            ["default"]
        );
        assert!(!root.exists());
    }

    #[test]
    fn config_candidates_are_tenant_local() {
        let _env_lock = crate::test_env_lock();
        let root = tempfile::tempdir().unwrap();
        let _root = crate::testutil::EnvGuard::set("AIBOX_ROOT", root.path().as_os_str());
        let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();
        tenant.ensure_initialized().unwrap();
        let selected = tenant.for_agent(AgentKind::Codex);
        crate::config::create_named_config(&selected, "custom").unwrap();
        crate::config::create_named_config(&selected, "second").unwrap();
        let context = CompletionContext::from_words(&words(&[
            "aibox", "config", "--tenant", "work", "apply", "",
        ]));
        let values = complete_named_configs(&context, OsStr::new(""), &BTreeSet::new());
        assert_eq!(candidate_values(&values), ["custom", "second"]);

        let deleting = CompletionContext::from_words(&words(&[
            "aibox", "config", "--tenant", "work", "delete", "custom", "",
        ]));
        assert_eq!(deleting.positionals, BTreeSet::from(["custom".to_string()]));
        let values = complete_named_configs(&deleting, OsStr::new(""), &deleting.positionals);
        assert_eq!(candidate_values(&values), ["second"]);

        let current =
            CompletionContext::from_words(&words(&["aibox", "config", "get", "--current", ""]));
        assert!(current.current);
        assert!(complete_named_configs(&current, OsStr::new(""), &BTreeSet::new()).is_empty());
    }

    #[test]
    fn session_candidates_follow_the_selected_tenant_and_agent() {
        let _env_lock = crate::test_env_lock();
        let root = tempfile::tempdir().unwrap();
        let _root = crate::testutil::EnvGuard::set("AIBOX_ROOT", root.path().as_os_str());
        let work = ManagedTenant::resolve(root.path(), "work").unwrap();
        let other = ManagedTenant::resolve(root.path(), "other").unwrap();
        work.ensure_initialized().unwrap();
        other.ensure_initialized().unwrap();
        let codex_id = "019fded0-6b15-7163-8881-458cbf92d123";
        let other_id = "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb";
        crate::testutil::write_jsonl(
            &work.home_dir,
            &format!(".codex/sessions/2026/08/04/rollout-now-{codex_id}.jsonl"),
            &["{}"],
        );
        crate::testutil::write_jsonl(
            &other.home_dir,
            &format!(".codex/sessions/2026/08/04/rollout-now-{other_id}.jsonl"),
            &["{}"],
        );
        crate::testutil::write_jsonl(
            &work.home_dir,
            ".claude/projects/work/claude-session.jsonl",
            &["{}"],
        );

        let codex = CompletionContext::from_words(&words(&[
            "aibox", "session", "--tenant", "work", "list",
        ]));
        assert_eq!(session_values_at(root.path(), &codex).unwrap(), [codex_id]);
        assert_eq!(
            candidate_values(&complete_sessions(
                &codex,
                OsStr::new("d123"),
                &BTreeSet::new()
            )),
            [codex_id],
            "suffix input must complete to the full Session id"
        );
        assert!(
            complete_sessions(&codex, OsStr::new("019fded0"), &BTreeSet::new()).is_empty(),
            "the removed prefix contract must not leak through completion"
        );
        assert!(
            complete_sessions(
                &codex,
                OsStr::new(""),
                &BTreeSet::from([codex_id.to_string()])
            )
            .is_empty(),
            "already selected full ids must remain excluded"
        );

        let claude = CompletionContext::from_words(&words(&[
            "aibox", "session", "--agent", "claude", "--tenant", "work", "list",
        ]));
        assert_eq!(
            session_values_at(root.path(), &claude).unwrap(),
            ["claude-session"]
        );
        assert_eq!(
            candidate_values(&complete_sessions(
                &claude,
                OsStr::new("session"),
                &BTreeSet::new()
            )),
            ["claude-session"]
        );
    }
}
