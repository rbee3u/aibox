# Use Named-only Visual Config Editing

The Configs detail view provides a Visual Editor for safe Named Config main
files with valid native content. Claude has one native file; Codex Raw mode
presents `config.toml` above `auth.json`, with independent scrolling,
diagnostics, revision checks, dirty state, and saves. Visual mode is a single
column of compact Visual Config Options, and Codex credentials appear as a
separate Visual section with their own save path. An Option presents a
user-facing label and may project to one or more fixed Config Fields without
exposing native paths. Current Config remains Raw-only.

Visual enum Options are closed to the values declared by the Agent contract.
Optional enum and boolean Options use Default to omit the Config Field. An
existing unknown string enum remains selectable only so Visual can preserve it;
Visual cannot create a new unknown value. Required Options have no omission
control. Descriptions use accessible hover-and-focus help tooltips rather than
permanently consuming a row below each label.

Unknown native fields are warnings rather than fixed schema fields: they remain
in the source, are not projected by Config Application, and do not by themselves
make a catalog invalid. Codex Visual uses one Custom provider aggregate. Named
Configs either omit the provider entirely or contain only the complete fixed
Custom provider with `requires_openai_auth = true`; unsupported provider shapes
remain Raw-repairable.

Provider base URL fields expose a Request Proxy Route toggle in Visual mode. The
input shows only the upstream URL; enabling the toggle prepends the current
Service route, using loopback for Host Tenant and Docker's host gateway for a
Managed Tenant. The Codex `openai_base_url` field is intentionally outside the
fixed schema; old raw occurrences remain native and are warned about.

The main Codex Visual editor is independent from auth validity. When Custom is
enabled, saving the main file creates the `sk-example` auth placeholder only
when `auth.json` is missing or an empty object; existing credentials and
malformed non-empty content are preserved.
