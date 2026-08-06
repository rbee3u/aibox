# Manage Named and Current Config without retained state

Status: accepted

The `config` command manages two distinct objects in one Tenant and Coding
Agent scope. A Named Config is a reusable named set of fixed Config Fields;
Current Config is the complete native configuration consumed by the Coding
Agent. `config apply` projects a Named Config into Current Config once without
retaining an active association, while `config get/edit NAME|--current` makes
both objects directly inspectable and editable.

Named and Current Config use the Coding Agent's native file set: Claude has
only `settings.json`, including `env.ANTHROPIC_AUTH_TOKEN`; Codex has
`config.toml` and `auth.json`. Named Config edits validate each file against
the fixed schema. Current Config edits deliberately preserve arbitrary content
without syntax validation. Multi-file reads show every file in native order;
multi-file edits run and commit one editor at a time without rollback.

## Considered Options

Keeping Profile as a separate command hid that both objects are native Coding
Agent configuration. Treating Current Config as a special Named Config would
hide their different field scope and lifecycle. A synthetic Claude
`auth.json` made the two Agents look uniform but duplicated a value that is
native to `settings.json`. Validating Current Config would prevent direct
repair or deliberate experimentation with arbitrary native content. Staging
both Codex files before committing would avoid ordinary partial edits but
would conflict with the selected sequential editor workflow and still could
not provide cross-file atomicity after process interruption.

## Consequences

Runs consume Current Config without consulting Named Configs. Config
Application sets present Config Fields, removes missing Config Fields, and
preserves unrelated native settings such as status-line configuration. Codex
auth replaces native `auth.json` as a whole. Each file replacement is atomic,
but Config Application and sequential edit are not atomic across files. There
is no activation, drift tracking, rollback, migration reader, transaction
journal, backup, or cross-process lock. Host Tenant commands can print or
modify real credentials without redaction.
