//! Command-line surface (clap derive) plus the argv pre-split that keeps
//! pass-through agent args away from the parser.
//!
//! Invocation shape, unchanged in spirit from the two Bash scripts but now under
//! one binary with agent subcommands:
//!
//! ```text
//!   aibox <claude|codex> [options] [-- <args passed straight to the agent>]
//!   aibox <claude|codex> sync [base|<relay>] [--dry-run]
//!   aibox <claude|codex> session [list|get <id>|delete <id>] [-p <profile>]
//! ```
//!
//! ## Why we split argv ourselves
//!
//! The Bash loop treats the first `--` as "everything after this goes to the
//! agent verbatim" (`claude_args+=("$@"); break`). clap's trailing-var-arg has
//! sharp edges when it has to coexist with subcommands (`sync`/`session`), so we
//! sidestep the whole problem: [`split_passthrough`] cuts argv at the first `--`
//! before clap ever sees it. clap parses only the left side; the right side is
//! handed to the agent as-is.
//!
//! One deliberate deviation from Bash: the old scripts also collected *bare*
//! positional args (no `--`) into the pass-through array (`*) claude_args+=…`).
//! We don't — real usage always separates agent args with `--`
//! (`aibox claude -e r -- --model opus`), and requiring it lets clap reject
//! genuine typos instead of forwarding them silently.

use crate::agent::AgentKind;
use clap::{Args, Parser, Subcommand};

/// Top-level parser. Only the left half of argv (before the first `--`) reaches
/// this; see [`split_passthrough`].
#[derive(Debug, Parser)]
#[command(
    name = "aibox",
    about = "Run coding agents inside a Docker container that IS the sandbox boundary",
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
    pub agent: AgentCmd,
}

/// The agent selector — the first positional word (`claude` / `codex`).
#[derive(Debug, Subcommand)]
pub enum AgentCmd {
    /// Run Claude Code (wraps `@anthropic-ai/claude-code`).
    Claude(AgentArgs),
    /// Run OpenAI Codex (wraps `@openai/codex`).
    Codex(AgentArgs),
}

impl AgentCmd {
    pub fn kind(&self) -> AgentKind {
        match self {
            AgentCmd::Claude(_) => AgentKind::Claude,
            AgentCmd::Codex(_) => AgentKind::Codex,
        }
    }

    pub fn args(&self) -> &AgentArgs {
        match self {
            AgentCmd::Claude(a) | AgentCmd::Codex(a) => a,
        }
    }
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
        action: Option<String>,
        /// Session short id or unique prefix (for `get` / `delete`).
        id: Option<String>,
    },
}

/// Flags shared by a normal run. `--exec` is Codex-only and rejected for Claude
/// in [`crate::run`] rather than in the type, so the two subcommands can share
/// one struct (matching the Bash scripts' shared arg loop).
#[derive(Debug, Args, Default)]
pub struct RunArgs {
    /// Config profile name.
    #[arg(short, long, default_value = "default")]
    pub profile: String,

    /// Relay endpoint (required for a run): a name under <profile>/envs/, or a
    /// path. Merged onto `base`.
    #[arg(short, long)]
    pub env: Option<String>,

    /// Project dir mounted at /work (default: current dir).
    #[arg(short, long)]
    pub work: Option<String>,

    /// Extra bind mount, docker syntax host:container[:ro] (repeatable).
    #[arg(short, long)]
    pub mount: Vec<String>,

    /// Keep the agent's normal permission prompts / sandbox instead of bypassing.
    #[arg(long)]
    pub safe: bool,

    /// Rebuild the image from scratch (--no-cache --pull) before running.
    #[arg(long)]
    pub build: bool,

    /// Codex only: run headless `codex exec`. Pass the prompt after `--`.
    #[arg(long)]
    pub exec: bool,
}

/// Split argv at the first `--`: everything before is parsed by clap, everything
/// after is pass-through for the agent. The `--` itself is dropped. Returns
/// `(left, passthrough)`.
///
/// `argv` should include argv[0] (the program name) as clap expects.
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
        assert_eq!(cli.agent.kind(), AgentKind::Claude);
        assert_eq!(cli.agent.args().run.env.as_deref(), Some("openrouter"));
    }

    #[test]
    fn parses_codex_exec() {
        let (l, _) = split_passthrough(v(&["aibox", "codex", "-e", "r", "--exec"]));
        let cli = Cli::try_parse_from(l).unwrap();
        assert_eq!(cli.agent.kind(), AgentKind::Codex);
        assert!(cli.agent.args().run.exec);
    }

    #[test]
    fn parses_sync_dry_run() {
        let (l, _) = split_passthrough(v(&["aibox", "claude", "sync", "base", "--dry-run"]));
        let cli = Cli::try_parse_from(l).unwrap();
        match &cli.agent.args().action {
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
        match &cli.agent.args().action {
            Some(Action::Session { action, id }) => {
                assert_eq!(action.as_deref(), Some("get"));
                assert_eq!(id.as_deref(), Some("3f2a"));
            }
            _ => panic!("expected session action"),
        }
    }
}
