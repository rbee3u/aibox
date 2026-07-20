//! Env-file templates (single source, shared by first-run scaffolding and
//! `refresh`) plus the version-stamp reader.
//!
//! Every line is a comment: each key is documented, then shown as a commented
//! `#KEY=example`. The user sets one by adding a real line under its example. The
//! first line is a `# aibox-template: vN` stamp so `refresh` can tell a file's
//! vintage — bump [`crate::agent::TEMPLATE_VERSION`] whenever a template here
//! changes.

use crate::agent::AgentKind;

/// Base template — shared config inherited by every relay.
pub fn base_template(agent: AgentKind, ver: u32) -> String {
    let stamp = format!("# aibox-template: v{ver}\n");
    let body = match agent {
        AgentKind::Claude => CLAUDE_BASE,
        AgentKind::Codex => CODEX_BASE,
    };
    format!("{stamp}{body}")
}

/// Relay template — one endpoint, merged onto `base`. `name` fills the header
/// line.
pub fn relay_template(agent: AgentKind, name: &str, ver: u32) -> String {
    let stamp = format!("# aibox-template: v{ver}\n");
    let header = format!("# {name} — relay endpoint, merged onto ../base (this file wins).\n");
    let body = match agent {
        AgentKind::Claude => CLAUDE_RELAY,
        AgentKind::Codex => CODEX_RELAY,
    };
    format!("{stamp}{header}{body}")
}

/// Read a file's first-line `# aibox-template: vN` stamp, returning N (0 if the
/// file is unstamped / pre-versioning).
pub fn file_template_version(contents: &str) -> u32 {
    let first = contents.lines().next().unwrap_or("");
    let prefix = "# aibox-template: v";
    let Some(rest) = first.strip_prefix(prefix) else {
        return 0;
    };
    // Take the leading digit run; ignore any trailing text.
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().unwrap_or(0)
}

// --- Claude templates --------------------------------------------------------

const CLAUDE_BASE: &str = "\
# base — shared config inherited by every relay under envs/.
# A relay (-e <name>) is merged ON TOP of this file; a key set there wins.
# docker --env-file format: KEY=VALUE, one per line.

# Each item is documented, then shown as a commented #EXAMPLE. To set one, add
# a real line under its example — leave the example itself as living reference.

# Model tiers: Claude Code shells out to the haiku tier for background work and
# resolves sonnet/opus through subagents or /model. Map every tier to a model
# your relays serve; a relay that differs overrides just that line.
#ANTHROPIC_DEFAULT_HAIKU_MODEL=replace-with-small-fast-model
#ANTHROPIC_DEFAULT_SONNET_MODEL=replace-with-mid-model
#ANTHROPIC_DEFAULT_OPUS_MODEL=replace-with-main-model

# Claude Code also has a fable tier, used only if you /model to it; many relays
# don't serve it, so set it only if yours does.
#ANTHROPIC_DEFAULT_FABLE_MODEL=replace-with-fable-model
";

const CLAUDE_RELAY: &str = "\
# docker --env-file format: KEY=VALUE. Each item is documented, then shown as a
# commented #EXAMPLE — add a real line under the example to set it.

# REQUIRED. base_url of your provider (Claude's /v1/messages root).
#ANTHROPIC_BASE_URL=https://your-provider.example.com/anthropic

# REQUIRED. API key / auth token.
#ANTHROPIC_AUTH_TOKEN=sk-replace-me

# Override any base model tier here when this relay serves a different model.
# Set only the tiers that differ; unset ones fall through to base.
#ANTHROPIC_DEFAULT_HAIKU_MODEL=this-relays-small-model
#ANTHROPIC_DEFAULT_SONNET_MODEL=this-relays-mid-model
#ANTHROPIC_DEFAULT_OPUS_MODEL=this-relays-big-model
#ANTHROPIC_DEFAULT_FABLE_MODEL=this-relays-fable-model
";

// --- Codex templates ---------------------------------------------------------

const CODEX_BASE: &str = "\
# base — shared config inherited by every relay under envs/.
# A relay (-e <name>) is merged ON TOP of this file; a key set there wins.
# docker --env-file format: KEY=VALUE, one per line.

# Each item is documented, then shown as a commented #EXAMPLE. To set one, add
# a real line under its example — leave the example itself as living reference.

# Default model for every relay; a relay that sets CODEX_MODEL overrides it.
# Codex uses ONE model plus a reasoning knob (no haiku/sonnet/opus tiers).
#CODEX_MODEL=replace-with-model

# Reasoning effort: none|minimal|low|medium|high|xhigh|max|ultra.
# Omit to use the model default.
#CODEX_REASONING=medium

# Reasoning effort while in plan mode. Omit to fall back to CODEX_REASONING.
#CODEX_PLAN_REASONING=xhigh
";

const CODEX_RELAY: &str = "\
# docker --env-file format: KEY=VALUE. Each item is documented, then shown as a
# commented #EXAMPLE — add a real line under the example to set it.

# REQUIRED. base_url of your provider. Codex appends the Responses path itself,
# so give the Responses-compatible API root
# (the model_providers.<id>.base_url value).
#CODEX_BASE_URL=https://your-provider.example.com/v1

# REQUIRED. API key. Delivered ephemerally (see the auth mode below) and never
# written to the mounted config.toml.
#CODEX_API_KEY=sk-replace-me

# REQUIRED unless set in base. The model this relay serves.
#CODEX_MODEL=replace-with-model

# Override other base defaults here when this relay differs:
#CODEX_REASONING=xhigh
#CODEX_PLAN_REASONING=xhigh

# Auth mode. Default (unset): the key crosses in as an env var and Codex reads
# it via the provider's env_key; nothing is written to config.toml or the
# profile home. Set to 1 for auth.json mode: Codex reads the key from a
# {\"OPENAI_API_KEY\": \"...\"} file at CODEX_HOME/auth.json
# (requires_openai_auth=true, no env_key), the same path `codex login` uses.
# aibox generates a throwaway file, mounts it read-only for the run, and removes
# it on exit. Use 1 only if your relay's account/refresh flow expects it;
# env_key mode is otherwise simpler and identical on the wire.
#CODEX_REQUIRES_OPENAI_AUTH=1

# A model-instructions file (config.toml's model_instructions_file). Give the
# path ON YOUR HOST — aibox bind-mounts it read-only and points Codex at it.
# Absolute, or relative to where you launch `aibox codex`.
#CODEX_INSTRUCTIONS_FILE=~/prompts/codex-instructions.md

# Optional provider query params, as comma-separated k=v pairs. Azure-style
# deployments commonly need api-version:
#CODEX_QUERY_PARAMS=api-version=2025-04-01-preview
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamp_is_first_line() {
        let t = base_template(AgentKind::Claude, 3);
        assert!(t.starts_with("# aibox-template: v3\n"));
    }

    #[test]
    fn relay_header_names_the_relay() {
        let t = relay_template(AgentKind::Codex, "openrouter", 3);
        assert!(t.contains("# openrouter — relay endpoint"));
    }

    #[test]
    fn version_roundtrip() {
        let t = relay_template(AgentKind::Claude, "r", 3);
        assert_eq!(file_template_version(&t), 3);
    }

    #[test]
    fn unstamped_is_zero() {
        assert_eq!(file_template_version("KEY=value\n"), 0);
        assert_eq!(file_template_version(""), 0);
    }

    #[test]
    fn stamp_with_trailing_text() {
        assert_eq!(file_template_version("# aibox-template: v12 (old)\n"), 12);
    }
}
