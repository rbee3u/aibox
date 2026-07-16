//! The one place the two agents diverge.
//!
//! Everything the wrapper does that differs between Claude Code and Codex is
//! funnelled through [`AgentKind`] and its methods. Shared logic (profile
//! resolution, env merge, sync, session resolve/delete, docker hardening) lives
//! elsewhere and takes an `AgentKind` only to ask these questions. A divergence
//! that isn't expressed here is a compile error, not a copy-paste someone forgot.

use crate::creds::{GuardedPath, StagedFile};
use crate::runspec::{Invocation, RunOpts};
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

/// Container path the Codex model-instructions file is mounted at (read-only).
const CODEX_INSTRUCTIONS_CTR: &str = "/aibox-instructions.md";

/// Which agent a run targets. Selected by the `aibox claude` / `aibox codex`
/// subcommand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentKind {
    Claude,
    Codex,
}

/// Bump when a template in [`crate::template`] changes. Existing env files carry
/// the version they were written with (first line `# aibox-template: vN`); a run
/// whose file lags this prints a hint to `sync`. Shared by both agents.
pub const TEMPLATE_VERSION: u32 = 3;

impl AgentKind {
    /// Short lowercase tag used in paths and messages: `claude` / `codex`.
    pub fn tag(self) -> &'static str {
        match self {
            AgentKind::Claude => "claude",
            AgentKind::Codex => "codex",
        }
    }

    /// Default image tag, overridable by `$AIBOX_IMAGE`.
    pub fn image_default(self) -> &'static str {
        match self {
            AgentKind::Claude => "aibox-claude:latest",
            AgentKind::Codex => "aibox-codex:latest",
        }
    }

    /// Default config root, overridable by `$AIBOX_CONFIG_ROOT`. Returns the
    /// `$HOME/.aibox/<tag>` path; caller resolves `$HOME`.
    pub fn config_root_default(self, home: &str) -> PathBuf {
        PathBuf::from(home).join(".aibox").join(self.tag())
    }

    /// Whether the agent has a headless `exec` subcommand (the `--exec` flag).
    /// Codex does (`codex exec`); Claude doesn't. The flag is shared in the CLI
    /// struct so the subcommands can share one arg type, so the rejection lives
    /// here rather than as an inline Claude/Codex branch in [`crate::run`].
    pub fn supports_exec(self) -> bool {
        match self {
            AgentKind::Claude => false,
            AgentKind::Codex => true,
        }
    }

    /// The agent's home *inside the container* — the mount target and, for Codex,
    /// the CODEX_HOME parent. `/home/claude` vs `/home/codex`.
    pub fn container_home(self) -> &'static str {
        match self {
            AgentKind::Claude => "/home/claude",
            AgentKind::Codex => "/home/codex",
        }
    }

    /// The Dockerfile for this agent, embedded at compile time. It extends the
    /// shared `aibox-base:latest` image and has no `COPY`, so the build context
    /// is irrelevant and we can feed this straight to `docker build -f -` on
    /// stdin with an empty context.
    pub fn dockerfile(self) -> &'static str {
        match self {
            AgentKind::Claude => include_str!("../assets/claude.Dockerfile"),
            AgentKind::Codex => include_str!("../assets/codex.Dockerfile"),
        }
    }

    /// Translate a run's merged relay config into the agent-specific docker
    /// extras and command line. This is where the two agents genuinely diverge:
    ///
    /// - **Claude** is configured entirely by env vars, which already reached the
    ///   container via the merged `--env-file` the caller staged. So there are no
    ///   extra run args; the command line is just the permission toggle plus
    ///   pass-through.
    /// - **Codex** needs `-c key=value` overrides, a chosen auth mode, and
    ///   possibly extra mounts.
    pub fn build_invocation(self, opts: &RunOpts) -> Result<Invocation> {
        match self {
            AgentKind::Claude => build_claude(opts),
            AgentKind::Codex => build_codex(opts),
        }
    }
}

/// Claude is configured entirely by env vars. The merged `base` + relay config
/// is staged in a 0600 temp file (host-side only, never mounted) and delivered
/// via `docker run --env-file`. The command line bypasses permissions by default
/// (the container is the boundary), or keeps prompts under `--safe`, then appends
/// pass-through. Prepending the default flag means an explicit `--permission-mode`
/// in the pass-through still wins.
fn build_claude(opts: &RunOpts) -> Result<Invocation> {
    // Stage the merged env as a 0600 file docker reads with --env-file. Held in
    // `staged` so it's unlinked once the run returns (or on signal; see creds).
    let env_file = crate::creds::StagedFile::create("aibox-env.", &opts.env.to_env_file())?;
    let extra_run_args = vec![
        "--env-file".to_string(),
        env_file.path().display().to_string(),
    ];

    let mut agent_cmd = Vec::new();
    if opts.safe {
        eprintln!(">> permissions: prompting (--safe)");
    } else {
        agent_cmd.push("--dangerously-skip-permissions".to_string());
        eprintln!(">> permissions: SKIPPED (agent runs unrestricted; use --safe to prompt)");
    }
    agent_cmd.extend(opts.passthrough.iter().cloned());

    Ok(Invocation {
        extra_run_args,
        agent_cmd,
        staged: vec![env_file],
        guarded: Vec::new(),
    })
}

/// Codex is NOT configured by env vars: a custom model provider lives in
/// `config.toml`, and only the API *key* comes from an env var. We never write
/// the mounted `config.toml` (it holds the user's `codex login`, trust levels,
/// MCP servers); instead the whole provider is injected ephemerally via Codex's
/// own `-c key=value` overrides, and the key is delivered one of two mutually
/// exclusive ways (see below).
fn build_codex(opts: &RunOpts) -> Result<Invocation> {
    // Read the keys we translate out of the merge. Unknown keys are ignored —
    // this understands exactly the set below.
    let env = opts.env;
    let base_url = env.get("CODEX_BASE_URL").unwrap_or("");
    let api_key = env.get("CODEX_API_KEY").unwrap_or("");
    let model = env.get("CODEX_MODEL").unwrap_or("");
    let reasoning = env.get("CODEX_REASONING").unwrap_or("");
    let plan_reasoning = env.get("CODEX_PLAN_REASONING").unwrap_or("");
    let query_params = env.get("CODEX_QUERY_PARAMS").unwrap_or("");
    let requires_openai_auth = env.get("CODEX_REQUIRES_OPENAI_AUTH").unwrap_or("");
    let instructions_file = env.get("CODEX_INSTRUCTIONS_FILE").unwrap_or("");

    // Required keys.
    let mut missing = Vec::new();
    if base_url.is_empty() {
        missing.push("CODEX_BASE_URL");
    }
    if api_key.is_empty() {
        missing.push("CODEX_API_KEY");
    }
    if model.is_empty() {
        missing.push("CODEX_MODEL");
    }
    if !missing.is_empty() {
        bail!("relay is missing required keys: {}", missing.join(", "));
    }

    let mut extra_run_args: Vec<String> = Vec::new();
    let mut staged: Vec<StagedFile> = Vec::new();
    let mut guarded: Vec<GuardedPath> = Vec::new();

    // --- how the key reaches codex: two mutually-exclusive modes ---------
    // env_key mode (default): key crosses as OPENAI_API_KEY via a 0600 --env-file.
    // auth.json mode (CODEX_REQUIRES_OPENAI_AUTH truthy): key delivered as a
    // throwaway {"OPENAI_API_KEY":"…"} mounted read-only over CODEX_HOME.
    let use_auth_json = matches!(
        requires_openai_auth.to_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    );

    if use_auth_json {
        // Pre-create the mount target so docker over-mounts an existing file
        // (virtiofs can't create a target nested in the /home/codex mount). Only
        // a placeholder we create is removed later; a real login auth.json stays.
        let auth_mount = opts.home_dir.join(".codex").join("auth.json");
        guarded.push(GuardedPath::ensure(auth_mount.clone(), "{}\n")?);

        let auth_json = StagedFile::create(
            "aibox-codex-auth.",
            &format!("{{\"OPENAI_API_KEY\": \"{api_key}\"}}\n"),
        )?;
        extra_run_args.push("-v".to_string());
        extra_run_args.push(format!(
            "{}:/home/codex/.codex/auth.json:ro",
            auth_json.path().display()
        ));
        staged.push(auth_json);
    } else {
        let key_env =
            StagedFile::create("aibox-codex-key.", &format!("OPENAI_API_KEY={api_key}\n"))?;
        extra_run_args.push("--env-file".to_string());
        extra_run_args.push(key_env.path().display().to_string());
        staged.push(key_env);
    }

    // --- model_instructions_file: a host path bind-mounted read-only -----
    if !instructions_file.is_empty() {
        let host = resolve_instructions_path(instructions_file)?;
        if !host.is_file() {
            bail!("CODEX_INSTRUCTIONS_FILE not found: {}", host.display());
        }
        extra_run_args.push("-v".to_string());
        extra_run_args.push(format!("{}:{CODEX_INSTRUCTIONS_CTR}:ro", host.display()));
    }

    // --- build the codex invocation --------------------------------------
    // The endpoint is injected ephemerally via codex's own `-c` overrides;
    // nothing lands in the mounted config.toml. `aibox` is our provider id.
    let mut cmd: Vec<String> = Vec::new();

    // `codex exec` is the headless subcommand; the interactive TUI is the bare cmd.
    if opts.exec {
        cmd.push("exec".to_string());
    }

    push_c(&mut cmd, "model_provider=aibox");
    push_c(&mut cmd, "model_providers.aibox.name=aibox relay");
    push_c(
        &mut cmd,
        format!("model_providers.aibox.base_url={base_url}"),
    );
    push_c(&mut cmd, "model_providers.aibox.wire_api=responses");
    push_c(&mut cmd, format!("model={model}"));

    // Exactly one auth wiring — the two conflict in codex's provider.validate(),
    // and its first-party (auth.json) path only engages when env_key is unset.
    if use_auth_json {
        push_c(&mut cmd, "model_providers.aibox.requires_openai_auth=true");
    } else {
        push_c(&mut cmd, "model_providers.aibox.env_key=OPENAI_API_KEY");
    }

    if !reasoning.is_empty() {
        push_c(&mut cmd, format!("model_reasoning_effort={reasoning}"));
    }
    if !plan_reasoning.is_empty() {
        push_c(
            &mut cmd,
            format!("plan_mode_reasoning_effort={plan_reasoning}"),
        );
    }
    if !instructions_file.is_empty() {
        push_c(
            &mut cmd,
            format!("model_instructions_file={CODEX_INSTRUCTIONS_CTR}"),
        );
    }

    // query_params is a TOML inline table: split k=v[,k=v…] into per-key overrides.
    if !query_params.is_empty() {
        for pair in query_params.split(',') {
            let (pk, pv) = pair.split_once('=').unwrap_or((pair, ""));
            push_c(
                &mut cmd,
                format!("model_providers.aibox.query_params.{pk}={pv}"),
            );
        }
    }

    // Wide-open by default: bypass BOTH codex's approval prompts and its OS
    // sandbox. --safe puts the normal approvals + workspace-write sandbox back.
    // Prepended so an explicit flag in pass-through (e.g. -a/-s) still wins.
    if opts.safe {
        cmd.push("-a".into());
        cmd.push("on-request".into());
        cmd.push("-s".into());
        cmd.push("workspace-write".into());
        eprintln!(">> permissions: prompting + workspace-write sandbox (--safe)");
    } else {
        cmd.push("--dangerously-bypass-approvals-and-sandbox".into());
        eprintln!(">> permissions: BYPASSED (agent runs unrestricted; use --safe to prompt)");
    }

    cmd.extend(opts.passthrough.iter().cloned());

    Ok(Invocation {
        extra_run_args,
        agent_cmd: cmd,
        staged,
        guarded,
    })
}

/// Push a codex `-c key=value` override as the two argv tokens it takes. Folds
/// the repeated `cmd.push("-c"); cmd.push(kv)` pair that wiring the ephemeral
/// provider needs into one call.
fn push_c(cmd: &mut Vec<String>, kv: impl Into<String>) {
    cmd.push("-c".to_string());
    cmd.push(kv.into());
}

/// Resolve a `CODEX_INSTRUCTIONS_FILE` value (a host path) to an absolute path.
/// A leading `~/` expands against `$HOME`; an absolute path is taken as-is; a
/// relative path is taken against the launch dir (`$PWD`). Note this is the
/// launch cwd, *not* the `-w` work dir — the two differ when `-w` is passed.
fn resolve_instructions_path(value: &str) -> Result<PathBuf> {
    if let Some(rest) = value.strip_prefix("~/") {
        let home = std::env::var("HOME").context("$HOME is not set for ~/ expansion")?;
        Ok(Path::new(&home).join(rest))
    } else if value.starts_with('/') {
        Ok(PathBuf::from(value))
    } else {
        let cwd = std::env::current_dir().context("get $PWD for relative instructions path")?;
        Ok(cwd.join(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_codex_supports_exec() {
        assert!(!AgentKind::Claude.supports_exec());
        assert!(AgentKind::Codex.supports_exec());
    }
}
