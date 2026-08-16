# aibox

aibox provides persistent identities and a Filesystem Sandbox for running
Coding Agents while keeping host access and native configuration explicit.

## Language

### Execution boundary

**Coding Agent**:
An external coding assistant that aibox can run or manage, currently OpenAI
Codex or Claude Code.
_Avoid_: Agent process, Agent Profile

**Run**:
A transient invocation of a Coding Agent using a Managed Tenant and a
Workspace. A Run is not persistent history and has no identity relationship to
a Session.
_Avoid_: Session, Run History, execution profile

**Workspace**:
The host directory selected as the Coding Agent's working area for a Run.
_Avoid_: Project directory, work directory, work tree

**Filesystem Sandbox**:
The boundary that limits a Coding Agent's local host-filesystem access during a
Run. It is not a complete authority or network boundary.
_Avoid_: Complete isolation, authority boundary

**Extra Mount**:
An explicit grant that exposes an additional host path inside the Filesystem
Sandbox for a Run.
_Avoid_: Shared path, implicit mount

### Tenant identity

**aibox Root**:
The dedicated host storage boundary for aibox-managed identities,
configuration, and Traffic Records.
_Avoid_: Install prefix, Tenant Home

**Tenant**:
A persistent identity that scopes Coding Agent state, Named Configs, Tenant
Components, and Sessions. Every Tenant is either a Managed Tenant or the Host
Tenant.
_Avoid_: Namespace, Target, profile, environment

**Managed Tenant**:
An aibox-managed, runnable Tenant with its own Tenant Home.
_Avoid_: Agent Namespace, managed Target, Linux namespace

**Host Tenant**:
The management-only Tenant backed by the real Host Home.
_Avoid_: Host Target, Host Namespace, host profile

**Tenant Home**:
The aibox-managed Home belonging to one Managed Tenant and containing its
persistent native Coding Agent state.
_Avoid_: Host Home, profile home

**Host Home**:
The real user Home that backs the Host Tenant's native Coding Agent state.
_Avoid_: Tenant Home, aibox Root

**Tenant Component**:
An optional native capability installed for one Tenant, such as a Coding Agent
status line or a Managed Tenant-local toolchain.
_Avoid_: Plugin, package, add-on

### Configuration

**Current Config**:
The current native configuration consumed by a Coding Agent in one Tenant.
_Avoid_: Active Config, effective config, working config

**Named Config**:
A reusable named set of Config Fields belonging to exactly one Tenant and one
Coding Agent.
_Avoid_: Agent Profile, Saved Config, Config Template, preset

**Config Field**:
One fixed logical unit in a Named Config schema: a native setting, a credential
value, or the complete Codex credential object.
_Avoid_: Owned path, managed slot

**Config Application**:
An explicit one-time projection of a Named Config into Current Config without a
retained relationship between them.
_Avoid_: Activation, materialization, reconciliation

**Credential Propagation**:
An explicit one-time distribution of a newer Host Tenant Codex ChatGPT
credential snapshot to older same-account Configs without a retained
relationship.
_Avoid_: Credential Sync, activation, reconciliation

### Sessions

**Session**:
One interaction identity created by a Coding Agent and discovered from its
Transcript independently of Runs.
_Avoid_: Run, Transcript

**Transcript**:
The Coding Agent's native persistent record of one Session.
_Avoid_: Session, prompt history

### Traffic diagnostics

**Traffic Proxy**:
A temporary host-side HTTP intermediary that forwards requests and records
application-visible evidence. It is global to aibox rather than owned by a
Tenant or Coding Agent.
_Avoid_: Router, packet capture

**Traffic Viewer**:
The browser interface provided by a running Traffic Proxy for inspecting and
deleting Traffic Records.
_Avoid_: Management page, Traffic Console

**Traffic Record**:
The diagnostic evidence from one request attempt received by the Traffic Proxy.
It may exist without an upstream request or response.
_Avoid_: Session, Transcript, Run History

**Model Protocol Summary**:
A materialized provider-specific diagnostic projection associated with a
recognized model request in a Traffic Record.
_Avoid_: Parsed Body, Traffic Outcome, Record Assessment

**Traffic Outcome**:
The terminal lifecycle result of one Traffic Record, independent of any HTTP
response status.
_Avoid_: HTTP status, response code

**Record Assessment**:
The diagnostic classification of one Traffic Record, derived from its
independent lifecycle, HTTP, provider, and integrity evidence.
_Avoid_: Traffic Outcome, HTTP status, Provider Error
