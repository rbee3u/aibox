//! The one place the two agents diverge.
//!
//! Everything the wrapper does that differs between Claude Code and Codex is
//! funneled through [`AgentKind`] and its methods. Shared logic (profile
//! resolution, env merge, refresh, session resolve/delete, Docker hardening)
//! lives elsewhere and takes an `AgentKind` only to ask these questions. A divergence
//! that isn't expressed here is visible at the type boundary instead of hidden
//! in copy-pasted run paths.

use crate::creds::{GuardedPath, StagedFile};
use crate::runspec::{Invocation, RunOpts};
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

/// Container path the Codex model-instructions file is mounted at (read-only).
const CODEX_INSTRUCTIONS_CTR: &str = "/aibox-instructions.md";

/// The placeholder written to the pre-created auth.json mount target. Also the
/// ownership marker: a file holding exactly this is ours (a leftover from a
/// SIGKILL'd run), anything else is the user's real login file.
const AUTH_PLACEHOLDER: &str = "{}\n";

/// Which agent a run targets. Selected by the `aibox claude` / `aibox codex`
/// subcommand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentKind {
    Claude,
    Codex,
}

/// Bump when a template in [`crate::template`] changes. Existing env files carry
/// the version they were written with (first line `# aibox-template: vN`); a run
/// whose file lags this prints a hint to `refresh`. Shared by both agents.
pub const TEMPLATE_VERSION: u32 = 6;

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
    /// struct, so [`crate::run`] rejects it per-agent by asking here.
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

    /// Translate a run's merged relay config into the agent-specific Docker
    /// args and command line. This is where the two agents genuinely diverge:
    ///
    /// - **Claude** gets the merged env through a staged `--env-file`; its command
    ///   line is only the permission toggle plus pass-through args.
    /// - **Codex** gets `-c key=value` overrides, a chosen auth mode, and optional
    ///   read-only mounts.
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
    // A relay without ANTHROPIC_BASE_URL means Claude talks to its default
    // endpoint. Legitimate for subscription logins (credentials live in the
    // profile home), so only warn — but say it, because a scaffolded-but-empty
    // relay would otherwise silently skip the relay the user thinks is active.
    if opts
        .env
        .get("ANTHROPIC_BASE_URL")
        .unwrap_or_default()
        .is_empty()
    {
        eprintln!(
            ">> note: ANTHROPIC_BASE_URL is not set in this relay — Claude will use its default endpoint"
        );
    }

    // Stage the merged env as a 0600 file Docker reads with --env-file. Held in
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

/// Codex provider settings are config values, not environment variables. We
/// never write the mounted `config.toml` (it holds the user's `codex login`,
/// trust levels, MCP servers); instead the provider is injected ephemerally via
/// Codex's own `-c key=value` overrides. The API key is delivered by one of two
/// mutually exclusive modes below.
fn build_codex(opts: &RunOpts) -> Result<Invocation> {
    // Read the keys we translate out of the merge. Unknown keys are ignored —
    // this understands exactly the set below.
    let env = opts.env;
    let base_url = env.get("CODEX_BASE_URL").unwrap_or_default();
    let api_key = env.get("CODEX_API_KEY").unwrap_or_default();
    let model = env.get("CODEX_MODEL").unwrap_or_default();
    let reasoning = env.get("CODEX_REASONING").unwrap_or_default();
    let plan_reasoning = env.get("CODEX_PLAN_REASONING").unwrap_or_default();
    let query_params = env.get("CODEX_QUERY_PARAMS").unwrap_or_default();
    let requires_openai_auth = env.get("CODEX_REQUIRES_OPENAI_AUTH").unwrap_or_default();
    let instructions_file = env.get("CODEX_INSTRUCTIONS_FILE").unwrap_or_default();

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

    // --- how the key reaches Codex: two mutually-exclusive modes ---------
    // env_key mode (default): key crosses as OPENAI_API_KEY via a 0600 --env-file.
    // auth.json mode (CODEX_REQUIRES_OPENAI_AUTH truthy): key delivered as a
    // throwaway {"OPENAI_API_KEY":"..."} mounted read-only at
    // CODEX_HOME/auth.json. Anything outside the two recognized sets is
    // rejected here: a typo (`ture`) silently landing in env_key mode would
    // only surface later as an opaque Codex-side auth failure.
    let use_auth_json = match requires_openai_auth.to_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => true,
        "" | "0" | "false" | "no" | "off" => false,
        _ => bail!(
            "CODEX_REQUIRES_OPENAI_AUTH={requires_openai_auth} is not recognized \
             (1/true/yes/on for auth.json mode; 0/false/no/off or unset for env_key mode)"
        ),
    };

    let auth_mount = opts.home_dir.join(".codex").join("auth.json");
    validate_auth_path(&auth_mount)?;
    if use_auth_json {
        // Pre-create the mount target so Docker over-mounts an existing file
        // (virtiofs can't create a target nested in the /home/codex mount). Only
        // a placeholder we create is removed later; a real login auth.json stays.
        guarded.push(GuardedPath::ensure(auth_mount.clone(), AUTH_PLACEHOLDER)?);

        let auth_json = StagedFile::create("aibox-codex-auth.", &codex_auth_json(&api_key))?;
        extra_run_args.push("-v".to_string());
        extra_run_args.push(read_only_bind(
            auth_json.path(),
            "/home/codex/.codex/auth.json",
        )?);
        staged.push(auth_json);
    } else {
        // A `{}` placeholder left by a SIGKILL'd auth.json-mode run would sit
        // unmounted in the profile home this run: Codex parses it as a ChatGPT
        // login with no tokens (auth.json presence = "logged in"), a phantom
        // auth state alongside the real env_key auth. It's ours
        // (content-checked), so clear it; a real login file stays.
        //
        // Caveat: this content check can't tell our own leftover from the `{}`
        // placeholder an auth.json-mode run is *actively* using on the same
        // profile — clearing it here would break that concurrent run before it
        // establishes its bind mount. Running both auth modes against one
        // profile at the same time is unsupported for this reason; use separate
        // profiles (`-p`) if you need them concurrently.
        crate::creds::remove_stale_placeholder(&auth_mount, AUTH_PLACEHOLDER)?;

        let key_env =
            StagedFile::create("aibox-codex-key.", &format!("OPENAI_API_KEY={api_key}\n"))?;
        extra_run_args.push("--env-file".to_string());
        extra_run_args.push(key_env.path().display().to_string());
        staged.push(key_env);
    }

    // --- model_instructions_file: a host path bind-mounted read-only -----
    if !instructions_file.is_empty() {
        let host = resolve_instructions_path(&instructions_file)?;
        if !host.is_file() {
            bail!("CODEX_INSTRUCTIONS_FILE not found: {}", host.display());
        }
        extra_run_args.push("-v".to_string());
        extra_run_args.push(read_only_bind(&host, CODEX_INSTRUCTIONS_CTR)?);
    }

    // --- build the Codex invocation --------------------------------------
    // The endpoint is injected ephemerally via Codex's own `-c` overrides;
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

    // Exactly one auth wiring — the two conflict in Codex's provider.validate(),
    // and Codex's built-in auth.json path only engages when env_key is unset.
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

    // query_params is a TOML inline table: split k=v[,k=v…] into per-key
    // overrides. Empty segments (a trailing comma, `a=b,,c=d`) are skipped; an
    // empty key is rejected here instead of producing a keyless override that
    // Codex rejects later with an opaque parse error.
    if !query_params.is_empty() {
        for pair in query_params
            .split(',')
            .map(str::trim)
            .filter(|p| !p.is_empty())
        {
            let (pk, pv) = pair.split_once('=').unwrap_or((pair, ""));
            let pk = pk.trim();
            let pv = pv.trim();
            if pk.is_empty() {
                bail!("CODEX_QUERY_PARAMS contains an empty key in segment {pair:?}");
            }
            push_c(
                &mut cmd,
                format!("model_providers.aibox.query_params.{pk}={pv}"),
            );
        }
    }

    // Wide-open by default: bypass BOTH Codex's approval prompts and its OS
    // sandbox. --safe puts the normal approvals + workspace-write sandbox back.
    // Prepended so an explicit flag in pass-through (e.g. -a/-s) still wins.
    if opts.safe {
        // The `exec` subcommand rejects a bare `-a` (approval flags are
        // root-command only in Codex's CLI); the `-c approval_policy=…`
        // override is the exec-safe spelling of the same setting.
        if opts.exec {
            push_c(&mut cmd, "approval_policy=on-request");
        } else {
            cmd.push("-a".into());
            cmd.push("on-request".into());
        }
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

/// Push a Codex `-c key=value` override as the two argv tokens it takes. Folds
/// the repeated `cmd.push("-c"); cmd.push(kv)` pair that wiring the ephemeral
/// provider needs into one call.
fn push_c(cmd: &mut Vec<String>, kv: impl Into<String>) {
    cmd.push("-c".to_string());
    cmd.push(kv.into());
}

/// The throwaway auth.json body for Codex's `requires_openai_auth` mode.
/// Serialized with serde_json so a key containing `"` or `\` still yields
/// valid JSON.
fn codex_auth_json(api_key: &str) -> String {
    format!("{}\n", serde_json::json!({ "OPENAI_API_KEY": api_key }))
}

/// Require Codex's fixed auth path to be absent or a regular file. Reading a
/// FIFO/socket can block forever, while writing through a dangling symlink can
/// create a target somewhere the user did not ask us to touch.
fn validate_auth_path(path: &Path) -> Result<()> {
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() => Ok(()),
        Ok(meta) if meta.is_dir() => bail!(
            "Codex auth path is a directory, expected a file: {}",
            path.display()
        ),
        Ok(_) => bail!("Codex auth path is not a regular file: {}", path.display()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            match std::fs::symlink_metadata(path) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    bail!("Codex auth path is a dangling symlink: {}", path.display())
                }
                Ok(_) => bail!("Codex auth path is not a regular file: {}", path.display()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e).with_context(|| format!("inspect {}", path.display())),
            }
        }
        Err(e) => Err(e).with_context(|| format!("inspect {}", path.display())),
    }
}

/// Render a read-only Docker bind after applying the same source-path check as
/// `/work`, profile home, and user-supplied mounts.
fn read_only_bind(source: &Path, target: &str) -> Result<String> {
    crate::runspec::reject_colon_in_bind_source("bind source", source)?;
    Ok(format!("{}:{target}:ro", source.display()))
}

/// Resolve a `CODEX_INSTRUCTIONS_FILE` value (a host path) to an absolute path.
/// A bare `~` or leading `~/` expands against `$HOME`; an absolute path is taken
/// as-is; a relative path is taken against the launch dir (`$PWD`). Note this is
/// the launch cwd, *not* the `-w` work dir — the two differ when `-w` is passed.
fn resolve_instructions_path(value: &str) -> Result<PathBuf> {
    if value == "~" {
        let home = crate::env_override("HOME")?.context("$HOME is not set for ~ expansion")?;
        Ok(PathBuf::from(home))
    } else if let Some(rest) = value.strip_prefix("~/") {
        let home = crate::env_override("HOME")?.context("$HOME is not set for ~/ expansion")?;
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

    #[test]
    fn instructions_path_tilde_expansion() {
        let home = std::env::var("HOME").expect("HOME set in test env");
        assert_eq!(
            resolve_instructions_path("~").unwrap(),
            PathBuf::from(&home)
        );
        assert_eq!(
            resolve_instructions_path("~/x.md").unwrap(),
            Path::new(&home).join("x.md")
        );
        assert_eq!(
            resolve_instructions_path("/abs/x.md").unwrap(),
            PathBuf::from("/abs/x.md")
        );
    }

    #[test]
    fn codex_auth_json_escapes_special_chars() {
        let key = r#"sk-we"ird\key"#;
        let body = codex_auth_json(key);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["OPENAI_API_KEY"], key);
    }

    // --- invocation building ----------------------------------------------
    //
    // These assert on the argv the agents actually receive. They deliberately
    // never assert that a staged file exists on disk: staged paths live in the
    // process-global pending set, and the creds test's signal-path check may
    // unlink them concurrently (see creds::tests).

    use crate::envfile::MergedEnv;

    /// Minimal relay config satisfying build_codex's required keys.
    const CODEX_MIN: &str =
        "CODEX_BASE_URL=https://relay.example/v1\nCODEX_API_KEY=sk-test\nCODEX_MODEL=gpt-test\n";

    fn env_of(src: &str) -> MergedEnv {
        MergedEnv::merge(&[src.to_string()])
    }

    fn opts<'a>(env: &'a MergedEnv, home: &'a Path) -> RunOpts<'a> {
        RunOpts {
            env,
            safe: false,
            exec: false,
            passthrough: &[],
            home_dir: home,
        }
    }

    /// The value token following each `-c` flag.
    fn c_overrides(cmd: &[String]) -> Vec<&str> {
        cmd.windows(2)
            .filter(|w| w[0] == "-c")
            .map(|w| w[1].as_str())
            .collect()
    }

    fn contains_pair(args: &[String], a: &str, b: &str) -> bool {
        args.windows(2).any(|w| w[0] == a && w[1] == b)
    }

    /// Build and expect failure, returning the error message (`Invocation`
    /// itself has no `Debug`, so `unwrap_err` needs the Ok side dropped first).
    fn build_err(agent: AgentKind, o: &RunOpts) -> String {
        agent
            .build_invocation(o)
            .map(|_| ())
            .unwrap_err()
            .to_string()
    }

    #[test]
    fn claude_default_prepends_skip_permissions_before_passthrough() {
        let env = env_of("ANTHROPIC_BASE_URL=https://relay.example\n");
        let home = tempfile::tempdir().unwrap();
        let passthrough = vec!["--permission-mode".to_string(), "plan".to_string()];
        let mut o = opts(&env, home.path());
        o.passthrough = &passthrough;

        let inv = AgentKind::Claude.build_invocation(&o).unwrap();

        assert_eq!(
            inv.agent_cmd,
            vec![
                "--dangerously-skip-permissions",
                "--permission-mode",
                "plan"
            ],
            "default flag is prepended so an explicit pass-through flag wins"
        );
        assert_eq!(inv.extra_run_args[0], "--env-file");
        assert!(inv.extra_run_args[1].contains("aibox-env."));
        assert_eq!(inv.staged.len(), 1, "merged env is staged for cleanup");
        assert!(inv.guarded.is_empty());
    }

    #[test]
    fn claude_safe_keeps_permission_prompts() {
        let env = env_of("ANTHROPIC_BASE_URL=https://relay.example\n");
        let home = tempfile::tempdir().unwrap();
        let mut o = opts(&env, home.path());
        o.safe = true;

        let inv = AgentKind::Claude.build_invocation(&o).unwrap();

        assert!(inv.agent_cmd.is_empty(), "--safe adds no bypass flag");
    }

    #[test]
    fn codex_missing_required_keys_are_all_listed() {
        let home = tempfile::tempdir().unwrap();

        let env = env_of("");
        assert_eq!(
            build_err(AgentKind::Codex, &opts(&env, home.path())),
            "relay is missing required keys: CODEX_BASE_URL, CODEX_API_KEY, CODEX_MODEL"
        );

        let env = env_of("CODEX_BASE_URL=https://x\nCODEX_MODEL=m\n");
        assert_eq!(
            build_err(AgentKind::Codex, &opts(&env, home.path())),
            "relay is missing required keys: CODEX_API_KEY"
        );
    }

    #[test]
    fn codex_env_key_mode_is_the_default() {
        let env = env_of(CODEX_MIN);
        let home = tempfile::tempdir().unwrap();

        let inv = AgentKind::Codex
            .build_invocation(&opts(&env, home.path()))
            .unwrap();

        assert_eq!(inv.extra_run_args[0], "--env-file");
        assert!(inv.extra_run_args[1].contains("aibox-codex-key."));
        assert_eq!(inv.staged.len(), 1);
        assert!(
            inv.guarded.is_empty(),
            "no auth.json mount target in env_key mode"
        );

        let c = c_overrides(&inv.agent_cmd);
        assert!(c.contains(&"model_provider=aibox"));
        assert!(c.contains(&"model_providers.aibox.base_url=https://relay.example/v1"));
        assert!(c.contains(&"model_providers.aibox.wire_api=responses"));
        assert!(c.contains(&"model=gpt-test"));
        assert!(c.contains(&"model_providers.aibox.env_key=OPENAI_API_KEY"));
        // The two auth wirings conflict in Codex's provider validation; env_key
        // mode must not also set requires_openai_auth.
        assert!(!c.iter().any(|v| v.contains("requires_openai_auth")));
    }

    #[test]
    fn codex_env_key_mode_clears_stale_placeholder_but_keeps_real_auth_json() {
        let env = env_of(CODEX_MIN);
        let home = tempfile::tempdir().unwrap();
        let auth = home.path().join(".codex").join("auth.json");
        std::fs::create_dir_all(auth.parent().unwrap()).unwrap();

        // A `{}` placeholder from a SIGKILL'd auth.json-mode run is ours: an
        // env_key run must clear it instead of leaving a phantom login.
        std::fs::write(&auth, AUTH_PLACEHOLDER).unwrap();
        let _inv = AgentKind::Codex
            .build_invocation(&opts(&env, home.path()))
            .unwrap();
        assert!(!auth.exists(), "stale placeholder is cleared");

        // A real `codex login` auth.json is never touched.
        std::fs::write(&auth, "{\"OPENAI_API_KEY\":\"sk-real\"}\n").unwrap();
        let _inv = AgentKind::Codex
            .build_invocation(&opts(&env, home.path()))
            .unwrap();
        assert!(auth.exists(), "real auth.json survives env_key mode");
    }

    #[test]
    fn codex_auth_json_mode_excludes_env_key() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join(".codex")).unwrap();

        for truthy in ["1", "true", "yes", "on", "YES"] {
            let env = env_of(&format!("{CODEX_MIN}CODEX_REQUIRES_OPENAI_AUTH={truthy}\n"));
            let inv = AgentKind::Codex
                .build_invocation(&opts(&env, home.path()))
                .unwrap();

            let mount = inv
                .extra_run_args
                .windows(2)
                .find(|w| w[0] == "-v")
                .map(|w| w[1].clone())
                .expect("auth.json bind mount present");
            assert!(mount.contains("aibox-codex-auth."));
            assert!(mount.ends_with(":/home/codex/.codex/auth.json:ro"));
            assert_eq!(inv.guarded.len(), 1, "mount target is guarded");
            assert!(
                !inv.extra_run_args.contains(&"--env-file".to_string()),
                "no env-file key delivery in auth.json mode"
            );

            let c = c_overrides(&inv.agent_cmd);
            assert!(c.contains(&"model_providers.aibox.requires_openai_auth=true"));
            assert!(
                !c.iter().any(|v| v.contains("env_key")),
                "auth modes are mutually exclusive"
            );
        }

        // An explicit falsy value stays in env_key mode.
        for falsy in ["0", "false", "no", "off", "OFF", ""] {
            let env = env_of(&format!("{CODEX_MIN}CODEX_REQUIRES_OPENAI_AUTH={falsy}\n"));
            let inv = AgentKind::Codex
                .build_invocation(&opts(&env, home.path()))
                .unwrap();
            let c = c_overrides(&inv.agent_cmd);
            assert!(c.contains(&"model_providers.aibox.env_key=OPENAI_API_KEY"));
            assert!(inv.guarded.is_empty());
        }
    }

    #[test]
    fn codex_requires_openai_auth_rejects_unrecognized_values() {
        // A typo must not silently pick an auth mode: it would surface much
        // later as an opaque Codex-side auth failure.
        let home = tempfile::tempdir().unwrap();
        let env = env_of(&format!("{CODEX_MIN}CODEX_REQUIRES_OPENAI_AUTH=ture\n"));
        let err = build_err(AgentKind::Codex, &opts(&env, home.path()));
        assert!(
            err.contains("CODEX_REQUIRES_OPENAI_AUTH=ture"),
            "error names the key and bad value: {err}"
        );
    }

    #[test]
    fn codex_auth_json_directory_errors_before_docker() {
        let home = tempfile::tempdir().unwrap();
        let auth = home.path().join(".codex").join("auth.json");
        std::fs::create_dir_all(&auth).unwrap();

        for mode in ["", "CODEX_REQUIRES_OPENAI_AUTH=1\n"] {
            let env = env_of(&format!("{CODEX_MIN}{mode}"));
            let err = build_err(AgentKind::Codex, &opts(&env, home.path()));
            assert!(
                err.contains("Codex auth path is a directory"),
                "invalid auth path should fail clearly: {err}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn codex_auth_json_special_file_errors_before_reading() {
        use std::os::unix::net::UnixListener;

        let home = tempfile::tempdir().unwrap();
        let auth = home.path().join(".codex").join("auth.json");
        std::fs::create_dir_all(auth.parent().unwrap()).unwrap();
        let _socket = UnixListener::bind(&auth).unwrap();

        for mode in ["", "CODEX_REQUIRES_OPENAI_AUTH=1\n"] {
            let env = env_of(&format!("{CODEX_MIN}{mode}"));
            let err = build_err(AgentKind::Codex, &opts(&env, home.path()));
            assert!(
                err.contains("Codex auth path is not a regular file"),
                "special auth paths should fail without being read: {err}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn codex_auth_json_dangling_symlink_errors_before_write() {
        use std::os::unix::fs::symlink;

        let home = tempfile::tempdir().unwrap();
        let auth = home.path().join(".codex").join("auth.json");
        let missing = home.path().join("missing-target");
        std::fs::create_dir_all(auth.parent().unwrap()).unwrap();
        symlink(&missing, &auth).unwrap();

        let env = env_of(&format!("{CODEX_MIN}CODEX_REQUIRES_OPENAI_AUTH=1\n"));
        let err = build_err(AgentKind::Codex, &opts(&env, home.path()));

        assert!(err.contains("Codex auth path is a dangling symlink"));
        assert!(!missing.exists());
    }

    #[test]
    fn codex_exec_subcommand_comes_first() {
        let env = env_of(CODEX_MIN);
        let home = tempfile::tempdir().unwrap();
        let mut o = opts(&env, home.path());
        o.exec = true;

        let inv = AgentKind::Codex.build_invocation(&o).unwrap();

        assert_eq!(
            inv.agent_cmd[0], "exec",
            "exec must precede the -c overrides"
        );
    }

    #[test]
    fn codex_exec_safe_uses_config_override_not_approval_flag() {
        // `codex exec` rejects a bare `-a` (approval flags are root-command
        // only), so --safe in exec mode must deliver the approval policy as a
        // `-c` override instead.
        let env = env_of(CODEX_MIN);
        let home = tempfile::tempdir().unwrap();
        let mut o = opts(&env, home.path());
        o.exec = true;
        o.safe = true;

        let cmd = AgentKind::Codex.build_invocation(&o).unwrap().agent_cmd;

        assert_eq!(cmd[0], "exec");
        assert!(
            !cmd.iter().any(|t| t == "-a"),
            "no bare -a after the exec subcommand"
        );
        assert!(c_overrides(&cmd).contains(&"approval_policy=on-request"));
        assert!(contains_pair(&cmd, "-s", "workspace-write"));

        // The TUI (non-exec) path keeps the flag form so an explicit `-a` in
        // pass-through still wins.
        o.exec = false;
        let cmd = AgentKind::Codex.build_invocation(&o).unwrap().agent_cmd;
        assert!(contains_pair(&cmd, "-a", "on-request"));
        assert!(!c_overrides(&cmd).contains(&"approval_policy=on-request"));
    }

    #[test]
    fn codex_permission_flags_precede_passthrough() {
        let env = env_of(CODEX_MIN);
        let home = tempfile::tempdir().unwrap();
        let passthrough = vec!["-a".to_string(), "never".to_string()];

        let mut o = opts(&env, home.path());
        o.passthrough = &passthrough;
        let cmd = AgentKind::Codex.build_invocation(&o).unwrap().agent_cmd;
        assert_eq!(
            &cmd[cmd.len() - 2..],
            passthrough.as_slice(),
            "passthrough is appended last"
        );
        let bypass = cmd
            .iter()
            .position(|t| t == "--dangerously-bypass-approvals-and-sandbox")
            .expect("default bypasses approvals and sandbox");
        assert!(
            bypass < cmd.len() - 2,
            "default flag precedes passthrough so an explicit flag wins"
        );

        o.safe = true;
        let cmd = AgentKind::Codex.build_invocation(&o).unwrap().agent_cmd;
        assert!(!cmd.contains(&"--dangerously-bypass-approvals-and-sandbox".to_string()));
        assert!(contains_pair(&cmd, "-a", "on-request"));
        assert!(contains_pair(&cmd, "-s", "workspace-write"));
        assert_eq!(&cmd[cmd.len() - 2..], passthrough.as_slice());
    }

    #[test]
    fn codex_optional_overrides_injected_only_when_set() {
        let home = tempfile::tempdir().unwrap();

        let env = env_of(CODEX_MIN);
        let inv = AgentKind::Codex
            .build_invocation(&opts(&env, home.path()))
            .unwrap();
        let c = c_overrides(&inv.agent_cmd);
        assert!(!c.iter().any(|v| v.starts_with("model_reasoning_effort=")));
        assert!(!c
            .iter()
            .any(|v| v.starts_with("plan_mode_reasoning_effort=")));
        assert!(!c.iter().any(|v| v.contains("query_params")));

        let env = env_of(&format!(
            "{CODEX_MIN}CODEX_REASONING=high\nCODEX_PLAN_REASONING=xhigh\n\
             CODEX_QUERY_PARAMS= api-version=2025-04-01-preview, foo=bar,compound=a=b, ,\n"
        ));
        let inv = AgentKind::Codex
            .build_invocation(&opts(&env, home.path()))
            .unwrap();
        let c = c_overrides(&inv.agent_cmd);
        assert!(c.contains(&"model_reasoning_effort=high"));
        assert!(c.contains(&"plan_mode_reasoning_effort=xhigh"));
        // query_params: comma-separated k=v pairs become per-key overrides.
        assert!(c.contains(&"model_providers.aibox.query_params.api-version=2025-04-01-preview"));
        assert!(c.contains(&"model_providers.aibox.query_params.foo=bar"));
        assert!(c.contains(&"model_providers.aibox.query_params.compound=a=b"));
        // The trailing comma above must not become a keyless `query_params.=`.
        assert!(!c.contains(&"model_providers.aibox.query_params.="));
    }

    #[test]
    fn codex_query_params_reject_empty_key() {
        let home = tempfile::tempdir().unwrap();
        let env = env_of(&format!(
            "{CODEX_MIN}CODEX_QUERY_PARAMS=api-version=1,=oops\n"
        ));

        let err = build_err(AgentKind::Codex, &opts(&env, home.path()));

        assert!(err.contains("CODEX_QUERY_PARAMS contains an empty key"));
    }

    #[test]
    fn codex_instructions_file_mounts_ro_or_errors_when_missing() {
        let home = tempfile::tempdir().unwrap();
        let file = tempfile::NamedTempFile::new().unwrap();

        let env = env_of(&format!(
            "{CODEX_MIN}CODEX_INSTRUCTIONS_FILE={}\n",
            file.path().display()
        ));
        let inv = AgentKind::Codex
            .build_invocation(&opts(&env, home.path()))
            .unwrap();
        let mount = format!("{}:{CODEX_INSTRUCTIONS_CTR}:ro", file.path().display());
        assert!(contains_pair(&inv.extra_run_args, "-v", &mount));
        let c = c_overrides(&inv.agent_cmd);
        let expected = format!("model_instructions_file={CODEX_INSTRUCTIONS_CTR}");
        assert!(c.contains(&expected.as_str()));

        let env = env_of(&format!(
            "{CODEX_MIN}CODEX_INSTRUCTIONS_FILE=/no/such/file.md\n"
        ));
        let err = build_err(AgentKind::Codex, &opts(&env, home.path()));
        assert!(err.contains("CODEX_INSTRUCTIONS_FILE not found"));
    }

    #[test]
    fn codex_read_only_binds_reject_colons_in_host_path() {
        let err = read_only_bind(Path::new("/tmp/with:colon"), CODEX_INSTRUCTIONS_CTR)
            .unwrap_err()
            .to_string();
        assert!(err.contains("contains ':'"), "{err}");
    }
}
