# AGENTS.md

Guidance for AI agents working on this repo. Read this before editing the
scripts.

## What this is

Two Bash wrapper scripts (`aibox-claude`, `aibox-codex`) plus their Dockerfiles.
Each runs a coding agent inside a container that is the sandbox boundary. The
scripts are the whole product — there's no build system, no tests beyond running
them. See `README.md` for user-facing docs.

## Hard constraints

**bash 3.2 compatibility.** macOS ships bash 3.2 and users run these there. Do
NOT use bash 4+ features:
- no associative arrays (`declare -A`)
- no `${var,,}` / `${var^^}` case expansion — pipe through `tr` instead
- guard empty-array expansion for `set -u`: `${arr[@]+"${arr[@]}"}`, not `"${arr[@]}"`

When you need an associative array, push the logic into `awk` (its arrays are
associative and always available). The `sync` merge and the env-file read both
do this deliberately.

**Quoting style.** Strings that don't need quotes don't get them
(`profile=default`, `-c model_provider=aibox`). When quotes ARE needed, prefer
double `"`; single `'` only as a last resort (e.g. heredoc delimiters that must
not expand). Expansions and values with spaces are quoted (`"$model"`,
`"model_providers.aibox.name=aibox relay"`). Keep this consistent — it was a
deliberate cleanup pass.

**Credentials never persist.** The API key is staged in a `0600` temp file
(env-file, or a throwaway `auth.json` bind-mounted read-only for codex's
`requires_openai_auth` mode) and removed via an EXIT/INT/TERM trap. Because of
that trap we canNOT `exec docker` — docker must run as a child so cleanup fires.
Don't "optimize" that into an exec.

**config.toml stays the user's.** For codex, the endpoint is injected only via
runtime `-c key=value` overrides, never written to the mounted `config.toml`.
That file holds the user's `codex login`, trust levels, MCP servers. The env
file (SessionFlags, precedence 30) intentionally outranks config.toml (User,
precedence 20), which is what lets the relay's model/reasoning win on each run.

## Keep the two scripts in parallel

`aibox-claude` and `aibox-codex` share structure intentionally: same arg-parse
loop, same profile/`base`/`envs/` layout, same `# --- section ---` headers, same
`sync` implementation, same hardening flags. When you change shared behavior,
change BOTH unless there's a Codex/Claude-specific reason not to (and note it).

## Templates and `sync`

Env-file templates live in `emit_base_template` / `emit_relay_template` —
single source, shared by first-run scaffolding and `sync`. Rules:
- Every item is a `#` comment describing it, followed by a commented
  `#KEY=example`. Nothing active — users append real lines under the examples.
- The first line is a `# aibox-template: vN` stamp. **Bump `TEMPLATE_VERSION`
  whenever you change a template**, so stale files get flagged and `sync` can
  refresh them.
- The `sync` awk matches example lines by `^#[A-Za-z_][A-Za-z0-9_]*=` and
  re-places the user's real lines under them. If you change example formatting,
  keep it matching that pattern.

## Help text is generated

`usage()` prints the leading comment block (lines 3 to the first blank line) with
the `# ` stripped. So the header comment IS the `--help` output — edit the
header to change help, and keep it formatted as help.

## After editing

Run `bash -n <script>` and `bash <script> --help`. For `sync` changes, test the
merge against a mock old file (stub `docker` on `$PATH` if you need to reach a
run path). Confirm both scripts still parse.
