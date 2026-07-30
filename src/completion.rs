//! Dynamic shell completion registration and host-side candidate discovery.
//!
//! Completion requests are handled before aibox splits its own `--`
//! pass-through boundary. The completion-only [`clap::Command`] reuses the
//! normal CLI definition and adds filesystem-backed completers without
//! changing ordinary parsing.

use crate::agent::AgentKind;
use crate::cli::{Cli, CompletionArgs, CompletionShell};
use crate::profile::{self, Profile};
use anyhow::{Context, Result};
use clap::{CommandFactory, ValueHint};
use clap_complete::engine::{
    ArgValueCompleter, CompletionCandidate, PathCompleter, ValueCompleter,
};
use clap_complete::env::{Bash, CompleteEnv, EnvCompleter, Fish, Shells, Zsh};
use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::{Path, PathBuf};

const COMPLETE_ENV: &str = "AIBOX_COMPLETE";
const COMMAND_NAME: &str = "aibox";

/// Process a generated shell script's environment-activated completion call.
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

/// Print the selected shell's registration script without changing shell
/// startup files.
pub(crate) fn dispatch(args: &CompletionArgs) -> Result<i32> {
    let executable = std::env::current_exe().context("resolve current aibox executable")?;
    let executable = executable
        .to_str()
        .context("current aibox executable path is not valid UTF-8")?;
    let mut output = Vec::new();
    write_registration(args.shell, executable, &mut output)
        .context("generate shell completion registration")?;
    let output = std::str::from_utf8(&output).context("completion registration is not UTF-8")?;
    crate::print_text(output)?;
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

fn write_registration(
    shell: CompletionShell,
    executable: &str,
    output: &mut dyn Write,
) -> std::io::Result<()> {
    shell_completer(shell).write_registration(
        COMPLETE_ENV,
        COMMAND_NAME,
        COMMAND_NAME,
        executable,
        output,
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TopCommand {
    Run,
    Build,
    Completion,
    Profile,
    Config,
    Session,
}

#[derive(Clone, Debug)]
struct CompletionContext {
    top: TopCommand,
    agent: AgentKind,
    profile: String,
    selection_valid: bool,
    all: bool,
    positionals: BTreeSet<String>,
}

impl Default for CompletionContext {
    fn default() -> Self {
        Self {
            top: TopCommand::Run,
            agent: AgentKind::Codex,
            profile: "default".to_string(),
            selection_valid: true,
            all: false,
            positionals: BTreeSet::new(),
        }
    }
}

impl CompletionContext {
    fn from_protocol_argv(argv: &[OsString]) -> Self {
        let Some(protocol_boundary) = argv.iter().position(|arg| arg == "--") else {
            return Self::default();
        };
        Self::from_words(&argv[protocol_boundary + 1..])
    }

    fn from_words(words: &[OsString]) -> Self {
        let mut context = Self::default();
        let business_end = words
            .iter()
            .position(|arg| arg == "--")
            .unwrap_or(words.len());
        let mut values = Vec::with_capacity(business_end);
        for word in &words[..business_end] {
            let Some(word) = word.to_str() else {
                context.selection_valid = false;
                return context;
            };
            values.push(word);
        }
        if values.is_empty() {
            return context;
        }

        let (top, top_index) = find_top_command(&values);
        context.top = top;
        if !matches!(top, TopCommand::Config | TopCommand::Session) {
            context.capture_leaf_and_positionals(&values, top_index);
            return context;
        }

        let option_parts = context.capture_scoped_options(&values, top_index);
        context.capture_scoped_leaf_and_positionals(&values, top_index, &option_parts);
        context
    }

    fn capture_scoped_options(&mut self, values: &[&str], top_index: usize) -> BTreeSet<usize> {
        let mut option_parts = BTreeSet::new();
        let mut seen_agent = false;
        let mut seen_profile = false;
        let mut index = top_index + 1;
        while index < values.len() {
            let token = values[index];
            if token == "--agent" {
                option_parts.insert(index);
                if index + 1 >= values.len() {
                    self.selection_valid = false;
                    break;
                }
                option_parts.insert(index + 1);
                self.record_agent(values[index + 1], &mut seen_agent);
                index += 2;
                continue;
            }
            if let Some(value) = token.strip_prefix("--agent=") {
                option_parts.insert(index);
                self.record_agent(value, &mut seen_agent);
                index += 1;
                continue;
            }
            if token == "--profile" || token == "-p" {
                option_parts.insert(index);
                if index + 1 >= values.len() {
                    self.selection_valid = false;
                    break;
                }
                option_parts.insert(index + 1);
                self.record_profile(values[index + 1], &mut seen_profile);
                index += 2;
                continue;
            }
            if let Some(value) = token.strip_prefix("--profile=") {
                option_parts.insert(index);
                self.record_profile(value, &mut seen_profile);
                index += 1;
                continue;
            }
            if let Some(value) = token.strip_prefix("-p") {
                if !value.is_empty() && !token.starts_with("--") {
                    option_parts.insert(index);
                    self.record_profile(value, &mut seen_profile);
                }
            }
            index += 1;
        }
        option_parts
    }

    fn record_agent(&mut self, value: &str, seen: &mut bool) {
        if *seen {
            self.selection_valid = false;
            return;
        }
        *seen = true;
        match value {
            "codex" => self.agent = AgentKind::Codex,
            "claude" => self.agent = AgentKind::Claude,
            _ => self.selection_valid = false,
        }
    }

    fn record_profile(&mut self, value: &str, seen: &mut bool) {
        if *seen {
            self.selection_valid = false;
            return;
        }
        *seen = true;
        if profile::validate_name("profile", value).is_err() {
            self.selection_valid = false;
        } else {
            self.profile = value.to_string();
        }
    }

    fn capture_leaf_and_positionals(&mut self, values: &[&str], top_index: usize) {
        let leaves: &[&str] = match self.top {
            TopCommand::Profile => &["list", "create", "delete"],
            TopCommand::Run
            | TopCommand::Build
            | TopCommand::Completion
            | TopCommand::Config
            | TopCommand::Session => return,
        };
        let Some(leaf_index) =
            ((top_index + 1)..values.len()).find(|index| leaves.contains(&values[*index]))
        else {
            return;
        };
        self.capture_positionals(values, leaf_index, &BTreeSet::new());
    }

    fn capture_scoped_leaf_and_positionals(
        &mut self,
        values: &[&str],
        top_index: usize,
        option_parts: &BTreeSet<usize>,
    ) {
        let leaves: &[&str] = match self.top {
            TopCommand::Config => &["list", "get", "create", "apply", "edit", "delete"],
            TopCommand::Session => &["list", "get", "delete"],
            TopCommand::Run | TopCommand::Build | TopCommand::Completion | TopCommand::Profile => {
                return
            }
        };
        let Some(leaf_index) = ((top_index + 1)..values.len())
            .find(|index| !option_parts.contains(index) && leaves.contains(&values[*index]))
        else {
            return;
        };
        self.capture_positionals(values, leaf_index, option_parts);
    }

    fn capture_positionals(
        &mut self,
        values: &[&str],
        leaf_index: usize,
        option_parts: &BTreeSet<usize>,
    ) {
        for (index, value) in values.iter().enumerate().skip(leaf_index + 1) {
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

fn find_top_command(values: &[&str]) -> (TopCommand, usize) {
    let mut index = 1;
    while index < values.len() {
        let value = values[index];
        let top = match value {
            "build" => Some(TopCommand::Build),
            "completion" => Some(TopCommand::Completion),
            "profile" => Some(TopCommand::Profile),
            "config" => Some(TopCommand::Config),
            "session" => Some(TopCommand::Session),
            _ => None,
        };
        if let Some(top) = top {
            return (top, index);
        }

        if matches!(
            value,
            "--agent" | "--profile" | "-p" | "--work" | "-w" | "--mount" | "-m"
        ) {
            index += 2;
        } else {
            index += 1;
        }
    }
    (TopCommand::Run, 0)
}

#[derive(Clone, Copy)]
enum ProfileCandidates {
    Run,
    Management,
    Existing,
}

fn completion_command(context: CompletionContext, current_dir: Option<PathBuf>) -> clap::Command {
    let profile_delete_context = context.clone();
    let config_context = context.clone();
    let config_delete_context = context.clone();
    let session_context = context.clone();
    let session_delete_context = context;
    let work_dir = current_dir.clone();
    let mount_dir = current_dir;

    Cli::command()
        .mut_arg("run-profile", |arg| {
            arg.value_hint(ValueHint::Other)
                .add(ArgValueCompleter::new(move |current: &OsStr| {
                    complete_profiles(ProfileCandidates::Run, current, &BTreeSet::new())
                }))
        })
        .mut_arg("work", move |arg| {
            arg.value_hint(ValueHint::DirPath)
                .add(ArgValueCompleter::new(move |current: &OsStr| {
                    complete_work(current, work_dir.as_deref())
                }))
        })
        .mut_arg("mount", move |arg| {
            arg.value_hint(ValueHint::Other)
                .add(ArgValueCompleter::new(move |current: &OsStr| {
                    complete_mount(current, mount_dir.as_deref())
                }))
        })
        .mut_subcommand("profile", |command| {
            command
                .mut_subcommand("create", |command| {
                    command.mut_arg("profile", |arg| arg.value_hint(ValueHint::Other))
                })
                .mut_subcommand("delete", move |command| {
                    command.mut_arg("profiles", move |arg| {
                        let context = profile_delete_context.clone();
                        arg.value_hint(ValueHint::Other).add(ArgValueCompleter::new(
                            move |current: &OsStr| {
                                if context.all {
                                    Vec::new()
                                } else {
                                    complete_profiles(
                                        ProfileCandidates::Existing,
                                        current,
                                        &context.positionals,
                                    )
                                }
                            },
                        ))
                    })
                })
        })
        .mut_subcommand("config", |command| {
            command
                .mut_arg("config-profile", |arg| {
                    arg.value_hint(ValueHint::Other).add(ArgValueCompleter::new(
                        move |current: &OsStr| {
                            complete_profiles(
                                ProfileCandidates::Management,
                                current,
                                &BTreeSet::new(),
                            )
                        },
                    ))
                })
                .mut_subcommand("create", |command| {
                    command.mut_arg("provider", |arg| arg.value_hint(ValueHint::Other))
                })
                .mut_subcommand("get", {
                    let context = config_context.clone();
                    move |command| add_provider_completer(command, context, false)
                })
                .mut_subcommand("apply", {
                    let context = config_context.clone();
                    move |command| add_provider_completer(command, context, false)
                })
                .mut_subcommand("edit", move |command| {
                    add_provider_completer(command, config_context, false)
                })
                .mut_subcommand("delete", move |command| {
                    add_provider_completer(command, config_delete_context, true)
                })
        })
        .mut_subcommand("session", |command| {
            command
                .mut_arg("session-profile", |arg| {
                    arg.value_hint(ValueHint::Other).add(ArgValueCompleter::new(
                        move |current: &OsStr| {
                            complete_profiles(
                                ProfileCandidates::Management,
                                current,
                                &BTreeSet::new(),
                            )
                        },
                    ))
                })
                .mut_subcommand("get", move |command| {
                    add_session_completer(command, session_context, false)
                })
                .mut_subcommand("delete", move |command| {
                    add_session_completer(command, session_delete_context, true)
                })
        })
}

fn add_provider_completer(
    command: clap::Command,
    context: CompletionContext,
    repeatable: bool,
) -> clap::Command {
    let arg_id = if repeatable { "providers" } else { "provider" };
    command.mut_arg(arg_id, move |arg| {
        arg.value_hint(ValueHint::Other)
            .add(ArgValueCompleter::new(move |current: &OsStr| {
                if repeatable && context.all {
                    return Vec::new();
                }
                if repeatable {
                    complete_providers(&context, current, &context.positionals)
                } else {
                    complete_providers(&context, current, &BTreeSet::new())
                }
            }))
    })
}

fn add_session_completer(
    command: clap::Command,
    context: CompletionContext,
    repeatable: bool,
) -> clap::Command {
    let arg_id = if repeatable { "ids" } else { "id" };
    command.mut_arg(arg_id, move |arg| {
        arg.value_hint(ValueHint::Other)
            .add(ArgValueCompleter::new(move |current: &OsStr| {
                if repeatable && context.all {
                    return Vec::new();
                }
                if repeatable {
                    complete_sessions(&context, current, &context.positionals)
                } else {
                    complete_sessions(&context, current, &BTreeSet::new())
                }
            }))
    })
}

fn complete_profiles(
    mode: ProfileCandidates,
    current: &OsStr,
    excluded: &BTreeSet<String>,
) -> Vec<CompletionCandidate> {
    let values = (|| -> Result<Vec<String>> {
        let root = profile::config_root()?;
        profile_values_at(&root, mode)
    })()
    .unwrap_or_default();
    filter_candidates(values, current, excluded)
}

fn profile_values_at(root: &Path, mode: ProfileCandidates) -> Result<Vec<String>> {
    let profiles = profile::list_profiles(root)?;
    let mut values: BTreeSet<String> = profiles.into_iter().collect();
    match mode {
        ProfileCandidates::Run => {
            values.insert("default".to_string());
        }
        ProfileCandidates::Management => {
            values.insert("default".to_string());
            if Profile::resolve(AgentKind::Codex, root, profile::HOST_PROFILE).is_ok() {
                values.insert(profile::HOST_PROFILE.to_string());
            }
        }
        ProfileCandidates::Existing => {}
    }
    Ok(values.into_iter().collect())
}

fn complete_providers(
    context: &CompletionContext,
    current: &OsStr,
    excluded: &BTreeSet<String>,
) -> Vec<CompletionCandidate> {
    if !context.selection_valid {
        return Vec::new();
    }
    let values = (|| -> Result<Vec<String>> {
        let root = profile::config_root()?;
        provider_values_at(&root, context)
    })()
    .unwrap_or_default();
    filter_candidates(values, current, excluded)
}

fn provider_values_at(root: &Path, context: &CompletionContext) -> Result<Vec<String>> {
    let selected = Profile::resolve(context.agent, root, &context.profile)?;
    crate::config::list_provider_names(&selected)
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
        let root = profile::config_root()?;
        session_values_at(&root, context)
    })()
    .unwrap_or_default();
    filter_candidates(values, current, excluded)
}

fn session_values_at(root: &Path, context: &CompletionContext) -> Result<Vec<String>> {
    let selected = Profile::resolve(context.agent, root, &context.profile)?;
    selected.validate_session_home()?;
    let backend = crate::session::backend_for(context.agent);
    let mut ids: Vec<String> = backend
        .files(&selected.home_dir)?
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
    let values: BTreeSet<String> = values
        .into_iter()
        .filter(|value| value.starts_with(current) && !excluded.contains(value))
        .collect();
    values.into_iter().map(CompletionCandidate::new).collect()
}

fn complete_mount(current: &OsStr, current_dir: Option<&Path>) -> Vec<CompletionCandidate> {
    if current.as_encoded_bytes().contains(&b':') {
        Vec::new()
    } else {
        let completer = match current_dir {
            Some(path) => PathCompleter::any().current_dir(path),
            None => PathCompleter::any(),
        };
        without_colon(completer.complete(current))
    }
}

fn complete_work(current: &OsStr, current_dir: Option<&Path>) -> Vec<CompletionCandidate> {
    let completer = match current_dir {
        Some(path) => PathCompleter::dir().current_dir(path),
        None => PathCompleter::dir(),
    };
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
    use crate::testutil::{write_jsonl, EnvGuard};
    use clap_complete::engine;

    fn words(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    fn context(values: &[&str]) -> CompletionContext {
        CompletionContext::from_words(&words(values))
    }

    #[test]
    fn protocol_context_uses_only_business_arguments_after_its_boundary() {
        let context = CompletionContext::from_protocol_argv(&words(&[
            "/opt/aibox",
            "--shell-protocol-flag",
            "--",
            "aibox",
            "config",
            "apply",
            "openai",
            "--agent=claude",
            "--profile=work",
        ]));

        assert_eq!(context.top, TopCommand::Config);
        assert_eq!(context.agent, AgentKind::Claude);
        assert_eq!(context.profile, "work");
        assert_eq!(context.positionals, BTreeSet::from(["openai".to_string()]));

        let no_boundary =
            CompletionContext::from_protocol_argv(&words(&["aibox", "config", "--agent=claude"]));
        assert_eq!(no_boundary.top, TopCommand::Run);
        assert_eq!(no_boundary.agent, AgentKind::Codex);
        assert_eq!(no_boundary.profile, "default");
    }

    fn candidate_values(candidates: Vec<CompletionCandidate>) -> Vec<String> {
        candidates
            .into_iter()
            .map(|candidate| candidate.get_value().to_string_lossy().into_owned())
            .collect()
    }

    fn engine_values(values: &[&str], index: usize, current_dir: &Path) -> Vec<String> {
        let argv = words(values);
        let mut command = completion_command(
            CompletionContext::from_words(&argv),
            Some(current_dir.to_path_buf()),
        );
        candidate_values(
            engine::complete(&mut command, argv, index, Some(current_dir))
                .expect("complete command"),
        )
    }

    #[test]
    fn context_reads_scoped_options_in_every_supported_form_and_position() {
        for argv in [
            &[
                "aibox", "config", "--agent", "claude", "-p", "work", "apply", "provider",
            ][..],
            &[
                "aibox",
                "config",
                "apply",
                "provider",
                "--agent=claude",
                "--profile=work",
            ][..],
            &[
                "aibox", "config", "apply", "provider", "--agent", "claude", "-pwork",
            ][..],
        ] {
            let context = context(argv);
            assert_eq!(context.top, TopCommand::Config);
            assert_eq!(context.agent, AgentKind::Claude);
            assert_eq!(context.profile, "work");
            assert!(context.selection_valid);
            assert_eq!(
                context.positionals,
                BTreeSet::from(["provider".to_string()])
            );
        }
    }

    #[test]
    fn context_rejects_duplicate_or_invalid_selection_and_stops_at_boundary() {
        let duplicate = context(&[
            "aibox",
            "session",
            "-p",
            "work",
            "get",
            "id",
            "--profile=other",
        ]);
        assert!(!duplicate.selection_valid);

        let invalid = context(&["aibox", "config", "--agent", "other", "list"]);
        assert!(!invalid.selection_valid);

        let escaped = context(&[
            "aibox", "config", "delete", "openai", "--", "--agent", "claude",
        ]);
        assert_eq!(escaped.agent, AgentKind::Codex);
        assert_eq!(escaped.positionals, BTreeSet::from(["openai".to_string()]));
    }

    #[test]
    fn context_does_not_treat_run_option_values_as_subcommands() {
        for argv in [
            &["aibox", "--work", "config"][..],
            &["aibox", "-w", "session"][..],
            &["aibox", "--mount", "profile"][..],
            &["aibox", "-m", "completion"][..],
            &["aibox", "--profile", "build"][..],
            &["aibox", "-p", "config"][..],
            &["aibox", "--agent", "session"][..],
        ] {
            let context = context(argv);
            assert_eq!(
                context.top,
                TopCommand::Run,
                "{argv:?} must remain a run while its option value is incomplete or subcommand-shaped"
            );
        }
    }

    #[test]
    fn context_tracks_delete_all_and_selected_values() {
        let context = context(&[
            "aibox",
            "config",
            "delete",
            "openai",
            "anthropic",
            "--all",
            "--profile",
            "work",
        ]);
        assert!(context.all);
        assert_eq!(context.profile, "work");
        assert_eq!(
            context.positionals,
            BTreeSet::from(["anthropic".to_string(), "openai".to_string()])
        );
    }

    #[test]
    fn registrations_cover_only_supported_shells_and_use_the_current_binary() {
        for shell in [
            CompletionShell::Bash,
            CompletionShell::Zsh,
            CompletionShell::Fish,
        ] {
            let mut output = Vec::new();
            write_registration(shell, "/opt/aibox/bin/aibox", &mut output).unwrap();
            let output = String::from_utf8(output).unwrap();
            assert!(output.contains("AIBOX_COMPLETE"), "{output}");
            assert!(output.contains("/opt/aibox/bin/aibox"), "{output}");
            assert!(output.contains("aibox"), "{output}");
        }
    }

    #[test]
    fn profile_values_follow_run_management_and_delete_semantics() {
        let _env_lock = crate::test_env_lock();
        let root = tempfile::tempdir().unwrap();
        let host_home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("HOME", host_home.path().as_os_str());
        profile::create_ordinary_profile(root.path(), "work").unwrap();
        profile::create_ordinary_profile(root.path(), "zeta").unwrap();

        assert_eq!(
            profile_values_at(root.path(), ProfileCandidates::Run).unwrap(),
            ["default", "work", "zeta"]
        );
        assert_eq!(
            profile_values_at(root.path(), ProfileCandidates::Management).unwrap(),
            ["default", "host", "work", "zeta"]
        );
        assert_eq!(
            profile_values_at(root.path(), ProfileCandidates::Existing).unwrap(),
            ["work", "zeta"]
        );
    }

    #[test]
    fn management_profile_values_keep_ordinary_profiles_without_a_host_home() {
        let _env_lock = crate::test_env_lock();
        let root = tempfile::tempdir().unwrap();
        profile::create_ordinary_profile(root.path(), "work").unwrap();
        let _home = EnvGuard::remove("HOME");

        assert_eq!(
            profile_values_at(root.path(), ProfileCandidates::Management).unwrap(),
            ["default", "work"]
        );
    }

    #[test]
    fn provider_values_follow_agent_and_profile_selection() {
        let root = tempfile::tempdir().unwrap();
        profile::create_ordinary_profile(root.path(), "work").unwrap();
        let codex = Profile::resolve(AgentKind::Codex, root.path(), "work").unwrap();
        let claude = Profile::resolve(AgentKind::Claude, root.path(), "work").unwrap();
        crate::config::create_provider(&codex, "openai").unwrap();
        crate::config::create_provider(&claude, "anthropic").unwrap();

        let codex_context = context(&["aibox", "config", "-pwork", "apply", ""]);
        assert_eq!(
            provider_values_at(root.path(), &codex_context).unwrap(),
            ["openai"]
        );

        let claude_context = context(&[
            "aibox",
            "config",
            "apply",
            "",
            "--profile=work",
            "--agent=claude",
        ]);
        assert_eq!(
            provider_values_at(root.path(), &claude_context).unwrap(),
            ["anthropic"]
        );
    }

    #[test]
    fn provider_values_do_not_depend_on_last_applied_state() {
        let root = tempfile::tempdir().unwrap();
        profile::create_ordinary_profile(root.path(), "work").unwrap();
        let selected = Profile::resolve(AgentKind::Codex, root.path(), "work").unwrap();
        crate::config::create_provider(&selected, "openai").unwrap();
        std::fs::write(selected.state_path(), "not json\n").unwrap();
        let context = context(&["aibox", "config", "-pwork", "get", ""]);

        assert_eq!(
            provider_values_at(root.path(), &context).unwrap(),
            ["openai"]
        );
    }

    #[test]
    fn session_values_use_full_ids_without_parsing_transcript_content() {
        let root = tempfile::tempdir().unwrap();
        profile::create_ordinary_profile(root.path(), "work").unwrap();
        let codex_id = "11111111-1111-1111-1111-111111111111";
        let claude_id = "22222222-2222-2222-2222-222222222222";
        write_jsonl(
            root.path(),
            &format!("work/home/.codex/sessions/2026/07/29/rollout-bad-{codex_id}.jsonl"),
            &["not-json"],
        );
        write_jsonl(
            root.path(),
            &format!("work/home/.claude/projects/repo/{claude_id}.jsonl"),
            &["also-not-json"],
        );

        let codex_context = context(&["aibox", "session", "-p", "work", "get", ""]);
        assert_eq!(
            session_values_at(root.path(), &codex_context).unwrap(),
            [codex_id]
        );
        let claude_context = context(&[
            "aibox",
            "session",
            "get",
            "",
            "--agent",
            "claude",
            "--profile",
            "work",
        ]);
        assert_eq!(
            session_values_at(root.path(), &claude_context).unwrap(),
            [claude_id]
        );
    }

    #[test]
    fn filtering_sorts_deduplicates_and_excludes_selected_values() {
        let excluded = BTreeSet::from(["alpha".to_string()]);
        let values = filter_candidates(
            ["beta", "alpha", "beta", "alpine"]
                .into_iter()
                .map(str::to_string),
            OsStr::new(""),
            &excluded,
        );
        assert_eq!(candidate_values(values), ["alpine", "beta"]);

        let values = filter_candidates(
            ["beta", "alpha", "alpine"].into_iter().map(str::to_string),
            OsStr::new("al"),
            &BTreeSet::new(),
        );
        assert_eq!(candidate_values(values), ["alpha", "alpine"]);
    }

    #[test]
    fn completion_engine_handles_structure_paths_and_business_boundary() {
        let current = tempfile::tempdir().unwrap();
        std::fs::create_dir(current.path().join("project")).unwrap();
        std::fs::create_dir(current.path().join("bad:project")).unwrap();
        std::fs::write(current.path().join("notes.txt"), "notes").unwrap();
        std::fs::write(current.path().join("bad:notes.txt"), "notes").unwrap();

        let values = engine_values(&["aibox", "co"], 1, current.path());
        assert!(values.contains(&"completion".to_string()), "{values:?}");
        assert!(values.contains(&"config".to_string()), "{values:?}");

        let values = engine_values(&["aibox", "--agent", "cl"], 2, current.path());
        assert_eq!(values, ["claude"]);

        let values = engine_values(&["aibox", "--work", "pro"], 2, current.path());
        assert!(
            values.iter().any(|value| value.starts_with("project")),
            "{values:?}"
        );
        let values = engine_values(&["aibox", "--work", "bad"], 2, current.path());
        assert!(values.is_empty(), "{values:?}");

        let values = engine_values(&["aibox", "--mount", "not"], 2, current.path());
        assert!(values.contains(&"notes.txt".to_string()), "{values:?}");
        let values = engine_values(&["aibox", "--mount", "bad"], 2, current.path());
        assert!(values.is_empty(), "{values:?}");

        let values = engine_values(&["aibox", "--mount", "notes.txt:/work"], 2, current.path());
        assert!(values.is_empty(), "{values:?}");

        let values = engine_values(&["aibox", "--", "--agent"], 2, current.path());
        assert!(values.is_empty(), "{values:?}");
    }

    #[test]
    fn dynamic_delete_completion_respects_existing_values_and_all() {
        let _env_lock = crate::test_env_lock();
        let root = tempfile::tempdir().unwrap();
        let _aibox_root = EnvGuard::set("AIBOX_ROOT", root.path().as_os_str());
        profile::create_ordinary_profile(root.path(), "work").unwrap();
        profile::create_ordinary_profile(root.path(), "zeta").unwrap();

        let values = engine_values(&["aibox", "profile", "delete", "work", ""], 4, root.path());
        assert!(!values.contains(&"work".to_string()), "{values:?}");
        assert!(values.contains(&"zeta".to_string()), "{values:?}");

        let values = engine_values(&["aibox", "profile", "delete", "--all", ""], 4, root.path());
        assert!(!values.contains(&"work".to_string()), "{values:?}");
        assert!(!values.contains(&"zeta".to_string()), "{values:?}");
    }

    #[test]
    fn provider_and_session_delete_completion_use_context_and_exclusions() {
        let _env_lock = crate::test_env_lock();
        let root = tempfile::tempdir().unwrap();
        let _aibox_root = EnvGuard::set("AIBOX_ROOT", root.path().as_os_str());
        profile::create_ordinary_profile(root.path(), "work").unwrap();

        let selected = Profile::resolve(AgentKind::Codex, root.path(), "work").unwrap();
        crate::config::create_provider(&selected, "alpha").unwrap();
        crate::config::create_provider(&selected, "beta").unwrap();

        let values = engine_values(
            &["aibox", "config", "delete", "alpha", "", "--profile=work"],
            4,
            root.path(),
        );
        assert!(!values.contains(&"alpha".to_string()), "{values:?}");
        assert!(values.contains(&"beta".to_string()), "{values:?}");

        let values = engine_values(
            &["aibox", "config", "delete", "--all", "", "--profile=work"],
            4,
            root.path(),
        );
        assert!(!values.contains(&"alpha".to_string()), "{values:?}");
        assert!(!values.contains(&"beta".to_string()), "{values:?}");

        let first_id = "44444444-4444-4444-4444-444444444444";
        let second_id = "55555555-5555-5555-5555-555555555555";
        for id in [first_id, second_id] {
            write_jsonl(
                root.path(),
                &format!("work/home/.codex/sessions/2026/07/29/rollout-bad-{id}.jsonl"),
                &["not-json"],
            );
        }

        let values = engine_values(
            &["aibox", "session", "delete", first_id, "", "--profile=work"],
            4,
            root.path(),
        );
        assert!(!values.contains(&first_id.to_string()), "{values:?}");
        assert!(values.contains(&second_id.to_string()), "{values:?}");
    }

    #[cfg(unix)]
    #[test]
    fn unsafe_session_layout_silently_removes_dynamic_candidates() {
        use std::os::unix::fs::symlink;

        let _env_lock = crate::test_env_lock();
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let _aibox_root = EnvGuard::set("AIBOX_ROOT", root.path().as_os_str());
        profile::create_ordinary_profile(root.path(), "work").unwrap();
        write_jsonl(
            outside.path(),
            "rollout-bad-33333333-3333-3333-3333-333333333333.jsonl",
            &["not-json"],
        );
        symlink(
            outside.path(),
            root.path().join("work/home/.codex/sessions"),
        )
        .unwrap();
        let context = context(&["aibox", "session", "-pwork", "get", ""]);

        assert!(session_values_at(root.path(), &context).is_err());
        assert!(complete_sessions(&context, OsStr::new(""), &BTreeSet::new()).is_empty());
    }

    #[test]
    fn invalid_root_layout_silently_removes_profile_candidates() {
        let _env_lock = crate::test_env_lock();
        let root = tempfile::tempdir().unwrap();
        let _aibox_root = EnvGuard::set("AIBOX_ROOT", root.path().as_os_str());
        std::fs::write(root.path().join("unexpected"), "bad layout").unwrap();

        assert!(profile_values_at(root.path(), ProfileCandidates::Run).is_err());
        assert!(
            complete_profiles(ProfileCandidates::Run, OsStr::new(""), &BTreeSet::new()).is_empty()
        );
    }
}
