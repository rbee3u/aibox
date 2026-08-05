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
    Shells(&[&Bash, &Zsh, &Fish])
}

fn shell_completer(shell: CompletionShell) -> &'static dyn EnvCompleter {
    match shell {
        CompletionShell::Bash => &Bash,
        CompletionShell::Zsh => &Zsh,
        CompletionShell::Fish => &Fish,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TopCommand {
    Root,
    Run,
    Tenant,
    Component,
    Profile,
    Session,
    Other,
}

#[derive(Clone, Debug)]
struct CompletionContext {
    top: TopCommand,
    agent: AgentKind,
    tenant: String,
    host: bool,
    selection_valid: bool,
    all: bool,
    positionals: BTreeSet<String>,
}

impl Default for CompletionContext {
    fn default() -> Self {
        Self {
            top: TopCommand::Root,
            agent: AgentKind::Codex,
            tenant: "default".to_string(),
            host: false,
            selection_valid: true,
            all: false,
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
            Some("profile") => TopCommand::Profile,
            Some("session") => TopCommand::Session,
            Some("build" | "completion") => TopCommand::Other,
            _ => TopCommand::Root,
        };
        let scoped = matches!(
            context.top,
            TopCommand::Run | TopCommand::Component | TopCommand::Profile | TopCommand::Session
        );
        let mut option_parts = BTreeSet::new();
        if scoped {
            context.capture_selection(&values, &mut option_parts);
        }
        context.capture_positionals(&values, &option_parts);
        context
    }

    fn capture_selection(&mut self, values: &[&str], option_parts: &mut BTreeSet<usize>) {
        let mut seen_agent = false;
        let mut seen_tenant = false;
        let mut seen_host = false;
        let mut index = 2;
        while index < values.len() {
            let token = values[index];
            if token == "--host" {
                option_parts.insert(index);
                if seen_host
                    || seen_tenant
                    || matches!(self.top, TopCommand::Run | TopCommand::Component)
                {
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
            TopCommand::Profile => &["list", "get", "create", "edit", "delete", "apply"],
            TopCommand::Session => &["list", "get", "delete"],
            _ => return,
        };
        let Some(leaf) = (2..values.len())
            .find(|index| !option_parts.contains(index) && leaves.contains(&values[*index]))
        else {
            return;
        };
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
}

#[derive(Clone, Copy)]
enum TenantCandidates {
    Select,
    Existing,
}

fn completion_command(context: CompletionContext, current_dir: Option<PathBuf>) -> clap::Command {
    let command = add_run_completers(Cli::command(), current_dir);
    let command = add_tenant_completers(command, context.clone());
    let command = add_component_completers(command);
    let command = add_profile_completers(command, context.clone());
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

fn add_component_completers(command: clap::Command) -> clap::Command {
    command.mut_subcommand("component", move |command| {
        command
            .mut_arg("component-tenant", add_tenant_value_completer)
            .mut_subcommand("install", |command| {
                command.mut_arg("component", |arg| {
                    arg.value_hint(ValueHint::Other)
                        .add(ArgValueCompleter::new(complete_components))
                })
            })
            .mut_subcommand("remove", |command| {
                command.mut_arg("component", |arg| {
                    arg.value_hint(ValueHint::Other)
                        .add(ArgValueCompleter::new(complete_components))
                })
            })
    })
}

fn add_profile_completers(command: clap::Command, context: CompletionContext) -> clap::Command {
    let get = context.clone();
    let edit = context.clone();
    let apply = context.clone();
    command.mut_subcommand("profile", move |command| {
        command
            .mut_arg("tenant", add_tenant_value_completer)
            .mut_subcommand("create", |command| {
                command.mut_arg("profile", |arg| arg.value_hint(ValueHint::Other))
            })
            .mut_subcommand("get", move |command| {
                add_profile_completer(command, get, false)
            })
            .mut_subcommand("edit", move |command| {
                add_profile_completer(command, edit, false)
            })
            .mut_subcommand("apply", move |command| {
                add_profile_completer(command, apply, false)
            })
            .mut_subcommand("delete", move |command| {
                add_profile_completer(command, context, true)
            })
    })
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

fn complete_components(current: &OsStr) -> Vec<CompletionCandidate> {
    filter_candidates(
        crate::component::ComponentKind::ALL.map(|kind| kind.name().to_string()),
        current,
        &BTreeSet::new(),
    )
}

fn add_profile_completer(
    command: clap::Command,
    context: CompletionContext,
    repeatable: bool,
) -> clap::Command {
    let id = if repeatable { "profiles" } else { "profile" };
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
                    complete_profiles(&context, current, excluded)
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

fn complete_profiles(
    context: &CompletionContext,
    current: &OsStr,
    excluded: &BTreeSet<String>,
) -> Vec<CompletionCandidate> {
    if !context.selection_valid {
        return Vec::new();
    }
    let values = (|| -> Result<Vec<String>> {
        let root = tenant::aibox_root()?;
        crate::profile::list_profiles(&selected_at(&root, context)?)
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
    filter_candidates(values, current, excluded)
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
    }

    #[test]
    fn host_and_tenant_are_distinct_and_conflicting() {
        let host = CompletionContext::from_words(&words(&["aibox", "profile", "--host", "list"]));
        assert!(host.host);
        assert!(host.selection_valid);
        let bad = CompletionContext::from_words(&words(&[
            "aibox", "profile", "--host", "--tenant", "host", "list",
        ]));
        assert!(!bad.selection_valid);
    }

    #[test]
    fn selection_context_rejects_the_same_invalid_scopes_as_clap() {
        for args in [
            &["aibox", "run", "--host"][..],
            &["aibox", "component", "--agent", "claude", "list"][..],
            &["aibox", "profile", "--agent", "unknown", "list"][..],
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
                complete_profiles(&context, OsStr::new(""), &BTreeSet::new()).is_empty(),
                "invalid selection must not discover host-side candidates: {args:?}"
            );
        }

        let inline = CompletionContext::from_words(&words(&[
            "aibox",
            "profile",
            "--agent=claude",
            "--tenant=work",
            "list",
        ]));
        assert!(inline.selection_valid);
        assert_eq!(inline.agent, AgentKind::Claude);
        assert_eq!(inline.tenant, "work");
    }

    #[test]
    fn component_completion_is_managed_tenant_scoped_and_static() {
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

        let host = CompletionContext::from_words(&words(&["aibox", "component", "--host", "list"]));
        assert!(!host.selection_valid);
        let values = complete_components(OsStr::new("co"));
        assert_eq!(candidate_values(&values), ["codex-statusline"]);
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
    fn profile_candidates_are_tenant_local() {
        let _env_lock = crate::test_env_lock();
        let root = tempfile::tempdir().unwrap();
        let _root = crate::testutil::EnvGuard::set("AIBOX_ROOT", root.path().as_os_str());
        let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();
        tenant.ensure_initialized().unwrap();
        let selected = tenant.for_agent(AgentKind::Codex);
        crate::profile::create_profile(&selected, "custom").unwrap();
        crate::profile::create_profile(&selected, "second").unwrap();
        let context = CompletionContext::from_words(&words(&[
            "aibox", "profile", "--tenant", "work", "apply", "",
        ]));
        let values = complete_profiles(&context, OsStr::new(""), &BTreeSet::new());
        assert_eq!(candidate_values(&values), ["custom", "second"]);

        let deleting = CompletionContext::from_words(&words(&[
            "aibox", "profile", "--tenant", "work", "delete", "custom", "",
        ]));
        assert_eq!(deleting.positionals, BTreeSet::from(["custom".to_string()]));
        let values = complete_profiles(&deleting, OsStr::new(""), &deleting.positionals);
        assert_eq!(candidate_values(&values), ["second"]);
    }

    #[test]
    fn session_candidates_follow_the_selected_tenant_and_agent() {
        let root = tempfile::tempdir().unwrap();
        let work = ManagedTenant::resolve(root.path(), "work").unwrap();
        let other = ManagedTenant::resolve(root.path(), "other").unwrap();
        work.ensure_initialized().unwrap();
        other.ensure_initialized().unwrap();
        let codex_id = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
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

        let claude = CompletionContext::from_words(&words(&[
            "aibox", "session", "--agent", "claude", "--tenant", "work", "list",
        ]));
        assert_eq!(
            session_values_at(root.path(), &claude).unwrap(),
            ["claude-session"]
        );
    }
}
