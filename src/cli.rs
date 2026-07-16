//! Command-line surface (clap derive) plus the argv pre-split that keeps
//! pass-through agent args away from the parser.
//!
//! Invocation shape — one binary with agent subcommands:
//!
//! ```text
//!   aibox build [claude|codex] [--force]
//!   aibox <claude|codex> [options] [-- <args passed straight to the agent>]
//!   aibox <claude|codex> sync [base|<relay>] [--dry-run]
//!   aibox <claude|codex> [-p <profile>] session [list|get <id>|delete [-y] [id...]]
//! ```
//!
//! ## Why we split argv ourselves
//!
//! The first `--` means "everything after this goes to the agent verbatim".
//! clap's trailing-var-arg has sharp edges when it has to coexist with
//! subcommands (`sync`/`session`), so we sidestep the whole problem:
//! [`split_passthrough`] cuts argv at the first `--` before clap ever sees it.
//! clap parses only the left side; the right side is handed to the agent as-is.
//!
//! Bare positional args (no `--`) are not collected as pass-through: real usage
//! always separates agent args with `--` (`aibox claude -e r -- --model opus`),
//! and requiring it lets clap reject genuine typos instead of forwarding them
//! silently.

use crate::agent::AgentKind;
use clap::{Args, Parser, Subcommand, ValueEnum};

/// Top-level parser. Only the left half of argv (before the first `--`) reaches
/// this; see [`split_passthrough`].
#[derive(Debug, Parser)]
#[command(
    name = "aibox",
    about = "Run coding agents inside a Docker container that is the sandbox boundary",
    long_about = "Run coding agents (Claude Code, OpenAI Codex) inside a Docker container \
                  that is the sandbox boundary — so the agent can skip every permission \
                  prompt and work unrestricted, while the blast radius stays inside the \
                  container.\n\n\
                  Pass args straight to the underlying agent after `--`:\n    \
                  aibox claude -e myrelay -- --model opus",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

/// Top-level commands. Agent commands run or manage one agent profile; `build`
/// owns image construction so normal runs never build implicitly.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Build aibox Docker image(s).
    Build(BuildArgs),
    /// Run Claude Code (wraps `@anthropic-ai/claude-code`).
    Claude(AgentArgs),
    /// Run OpenAI Codex (wraps `@openai/codex`).
    Codex(AgentArgs),
}

impl Command {
    pub fn agent_kind(&self) -> Option<AgentKind> {
        match self {
            Command::Build(_) => None,
            Command::Claude(_) => Some(AgentKind::Claude),
            Command::Codex(_) => Some(AgentKind::Codex),
        }
    }

    pub fn agent_args(&self) -> Option<&AgentArgs> {
        match self {
            Command::Build(_) => None,
            Command::Claude(a) | Command::Codex(a) => Some(a),
        }
    }
}

/// Image build options. A cached build is the default; `--force` refreshes the
/// shared base image and rebuilds the requested agent image(s) without cache.
#[derive(Debug, Args)]
pub struct BuildArgs {
    /// Which agent image to build. Omit to build both.
    #[arg(value_enum)]
    pub target: Option<BuildTarget>,

    /// Disable the Docker build cache and pull a fresh Debian base image.
    #[arg(short, long)]
    pub force: bool,
}

/// Build target for `aibox build`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum BuildTarget {
    /// Build only the Claude image.
    Claude,
    /// Build only the Codex image.
    Codex,
}

/// Everything under an agent subcommand. The `sync` / `session` sub-subcommands
/// are optional; with neither present this is a normal run.
#[derive(Debug, Args)]
pub struct AgentArgs {
    #[command(subcommand)]
    pub action: Option<Action>,

    #[command(flatten)]
    pub run: RunArgs,
}

/// The management sub-subcommands that short-circuit a run (`sync`, `session`).
/// A plain run has no [`Action`].
#[derive(Debug, Subcommand)]
pub enum Action {
    /// Refresh a config file's doc/example comments to the current template,
    /// keeping every real line you added.
    Sync {
        /// `base`, a relay name, or omitted for base + every relay.
        target: Option<String>,
        /// Print the result instead of writing it.
        #[arg(long)]
        dry_run: bool,
    },
    /// Browse this profile's saved chat transcripts (host-side; no container).
    Session {
        /// `list` (default), `get`, or `delete`.
        #[arg(default_value = "list")]
        action: String,
        /// Session short id or unique prefix. `delete` accepts many; none means all.
        #[arg(value_name = "ID")]
        ids: Vec<String>,
        /// Skip delete confirmations.
        #[arg(short = 'y', long)]
        yes: bool,
    },
}

/// Flags shared by a normal run. `--exec` is Codex-only and rejected for Claude
/// in [`crate::run`] rather than in the type, so the two subcommands can share
/// one struct.
#[derive(Debug, Args)]
pub struct RunArgs {
    /// Config profile name.
    #[arg(short, long, default_value = "default")]
    pub profile: String,

    /// Relay endpoint (required for a run): a name under `<profile>/envs/`, or a
    /// path. Merged onto `base`.
    #[arg(short, long)]
    pub env: Option<String>,

    /// Project dir mounted at /work (default: current dir).
    #[arg(short, long)]
    pub work: Option<String>,

    /// Extra bind mount, Docker syntax `host:container[:ro]` (repeatable).
    #[arg(short, long)]
    pub mount: Vec<String>,

    /// Keep the agent's normal permission prompts / sandbox instead of bypassing.
    #[arg(long)]
    pub safe: bool,

    /// Codex only: run headless `codex exec`. Pass the prompt after `--`.
    #[arg(long)]
    pub exec: bool,
}

/// Split argv at the first `--`: everything before is parsed by clap, everything
/// after is pass-through for the agent. The `--` itself is dropped. Returns
/// `(left, passthrough)`.
///
/// `argv` should include `argv[0]` (the program name) as clap expects.
pub fn split_passthrough(argv: Vec<String>) -> (Vec<String>, Vec<String>) {
    match argv.iter().position(|a| a == "--") {
        Some(i) => {
            let mut left = argv;
            let right = left.split_off(i + 1); // after the `--`
            left.pop(); // drop the `--` itself
            (left, right)
        }
        None => (argv, Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn split_no_dashdash() {
        let (l, r) = split_passthrough(v(&["aibox", "claude", "-e", "r"]));
        assert_eq!(l, v(&["aibox", "claude", "-e", "r"]));
        assert!(r.is_empty());
    }

    #[test]
    fn split_at_dashdash() {
        let (l, r) = split_passthrough(v(&["aibox", "claude", "-e", "r", "--", "--model", "opus"]));
        assert_eq!(l, v(&["aibox", "claude", "-e", "r"]));
        assert_eq!(r, v(&["--model", "opus"]));
    }

    #[test]
    fn split_empty_passthrough() {
        let (l, r) = split_passthrough(v(&["aibox", "codex", "--"]));
        assert_eq!(l, v(&["aibox", "codex"]));
        assert!(r.is_empty());
    }

    #[test]
    fn split_keeps_later_dashdash_in_passthrough() {
        // Only the first `--` splits; a second is part of the agent args.
        let (l, r) = split_passthrough(v(&["aibox", "codex", "--", "a", "--", "b"]));
        assert_eq!(l, v(&["aibox", "codex"]));
        assert_eq!(r, v(&["a", "--", "b"]));
    }

    #[test]
    fn parses_claude_run() {
        let (l, _) = split_passthrough(v(&["aibox", "claude", "-e", "openrouter"]));
        let cli = Cli::try_parse_from(l).unwrap();
        assert_eq!(cli.command.agent_kind(), Some(AgentKind::Claude));
        assert_eq!(
            cli.command.agent_args().unwrap().run.env.as_deref(),
            Some("openrouter")
        );
    }

    #[test]
    fn parses_codex_exec() {
        let (l, _) = split_passthrough(v(&["aibox", "codex", "-e", "r", "--exec"]));
        let cli = Cli::try_parse_from(l).unwrap();
        assert_eq!(cli.command.agent_kind(), Some(AgentKind::Codex));
        assert!(cli.command.agent_args().unwrap().run.exec);
    }

    #[test]
    fn parses_build_defaults_to_all_cached() {
        let (l, _) = split_passthrough(v(&["aibox", "build"]));
        let cli = Cli::try_parse_from(l).unwrap();
        match cli.command {
            Command::Build(BuildArgs { target, force }) => {
                assert_eq!(target, None);
                assert!(!force);
            }
            _ => panic!("expected build command"),
        }
    }

    #[test]
    fn parses_build_codex_force() {
        let (l, _) = split_passthrough(v(&["aibox", "build", "codex", "--force"]));
        let cli = Cli::try_parse_from(l).unwrap();
        match cli.command {
            Command::Build(BuildArgs { target, force }) => {
                assert_eq!(target, Some(BuildTarget::Codex));
                assert!(force);
            }
            _ => panic!("expected build command"),
        }
    }

    #[test]
    fn parses_build_short_force() {
        let (l, _) = split_passthrough(v(&["aibox", "build", "-f"]));
        let cli = Cli::try_parse_from(l).unwrap();
        match cli.command {
            Command::Build(BuildArgs { target, force }) => {
                assert_eq!(target, None);
                assert!(force);
            }
            _ => panic!("expected build command"),
        }
    }

    #[test]
    fn build_all_is_not_exposed() {
        let (l, _) = split_passthrough(v(&["aibox", "build", "all"]));
        assert!(Cli::try_parse_from(l).is_err());
    }

    #[test]
    fn agent_build_flag_is_rejected() {
        let (l, _) = split_passthrough(v(&["aibox", "codex", "--build"]));
        assert!(Cli::try_parse_from(l).is_err());
    }

    #[test]
    fn parses_sync_dry_run() {
        let (l, _) = split_passthrough(v(&["aibox", "claude", "sync", "base", "--dry-run"]));
        let cli = Cli::try_parse_from(l).unwrap();
        match &cli.command.agent_args().unwrap().action {
            Some(Action::Sync { target, dry_run }) => {
                assert_eq!(target.as_deref(), Some("base"));
                assert!(dry_run);
            }
            _ => panic!("expected sync action"),
        }
    }

    #[test]
    fn parses_session_get() {
        let (l, _) = split_passthrough(v(&["aibox", "codex", "session", "get", "3f2a"]));
        let cli = Cli::try_parse_from(l).unwrap();
        match &cli.command.agent_args().unwrap().action {
            Some(Action::Session { action, ids, yes }) => {
                assert_eq!(action, "get");
                assert_eq!(ids, &v(&["3f2a"]));
                assert!(!yes);
            }
            _ => panic!("expected session action"),
        }
    }

    #[test]
    fn parses_session_default_list() {
        let (l, _) = split_passthrough(v(&["aibox", "codex", "session"]));
        let cli = Cli::try_parse_from(l).unwrap();
        match &cli.command.agent_args().unwrap().action {
            Some(Action::Session { action, ids, yes }) => {
                assert_eq!(action, "list");
                assert!(ids.is_empty());
                assert!(!yes);
            }
            _ => panic!("expected session action"),
        }
    }

    #[test]
    fn parses_session_delete_many_yes() {
        let (l, _) = split_passthrough(v(&[
            "aibox", "codex", "session", "delete", "-y", "3f2a", "9d0e",
        ]));
        let cli = Cli::try_parse_from(l).unwrap();
        match &cli.command.agent_args().unwrap().action {
            Some(Action::Session { action, ids, yes }) => {
                assert_eq!(action, "delete");
                assert_eq!(ids, &v(&["3f2a", "9d0e"]));
                assert!(*yes);
            }
            _ => panic!("expected session action"),
        }
    }

    #[test]
    fn parses_session_delete_without_ids() {
        let (l, _) = split_passthrough(v(&["aibox", "codex", "session", "delete"]));
        let cli = Cli::try_parse_from(l).unwrap();
        match &cli.command.agent_args().unwrap().action {
            Some(Action::Session { action, ids, yes }) => {
                assert_eq!(action, "delete");
                assert!(ids.is_empty());
                assert!(!yes);
            }
            _ => panic!("expected session action"),
        }
    }

    #[test]
    fn parses_session_profile_before_action() {
        let (l, _) = split_passthrough(v(&[
            "aibox", "codex", "-p", "risky", "session", "get", "3f2a",
        ]));
        let cli = Cli::try_parse_from(l).unwrap();
        let args = cli.command.agent_args().unwrap();
        assert_eq!(args.run.profile, "risky");
        match &args.action {
            Some(Action::Session { action, ids, yes }) => {
                assert_eq!(action, "get");
                assert_eq!(ids, &v(&["3f2a"]));
                assert!(!yes);
            }
            _ => panic!("expected session action"),
        }
    }
}
