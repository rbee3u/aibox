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

**Runtime Image**:
The shared local Docker image built by aibox and used for Runs and Managed
Tenant toolchain installation. It is inspected and built independently of any
Tenant.
_Avoid_: Agent image, Tenant image, persistent container

**Extra Mount**:
An explicit grant that exposes an additional host path inside the Filesystem
Sandbox for a Run.
_Avoid_: Shared path, implicit mount

### Local management

**aibox Service**:
The foreground process started by `aibox serve`. One Service exclusively
manages one aibox Root for browser management while also running the global
Request Proxy.
_Avoid_: Daemon, Request server, backend

**Console**:
The browser interface embedded in the aibox Service. Its modules are Overview,
Tenants, Configs, Sessions, and Requests.
_Avoid_: Requests Viewer, admin site, dashboard

**Control API**:
The Console-internal HTTP interface under `/_aibox/api/`. It is available only
to loopback TCP peers and is not a public embedding API.
_Avoid_: Public API, SDK, remote API

**Service Lock**:
The advisory `$AIBOX_ROOT/.service.lock` held for the lifetime of one aibox
Service. It prevents a second Service for the same Root but does not coordinate
`aibox run` or deprecated CLI management commands.
_Avoid_: Global lock, Run lock, filesystem transaction

**Management Operation**:
The single cancellable long-running build or toolchain action retained in
Service memory. Only the latest Operation and its bounded log are observable;
it is not persistent history.
_Avoid_: Job, Run, Operation History

### Tenant identity

**aibox Root**:
The dedicated host storage boundary for aibox-managed identities,
configuration, and Request Records.
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
An explicit one-time projection of a Named Config into Current Config. A
successful Application updates Last Application for observation only; it never
causes automatic reapplication.
_Avoid_: Activation, materialization, reconciliation

**Last Application**:
The most recent successfully applied Named Config name and timestamp for one
Tenant and Coding Agent. It is diagnostic provenance, not an active binding.
_Avoid_: Active Config, desired state, synchronization state

**Config Drift**:
A live Console comparison between Last Application's Named Config and Current
Config: Untracked, Clean, Dirty, Source missing, or Comparison error.
_Avoid_: Reconciliation status, sync status, automatic repair

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

### Request diagnostics

**Request Proxy**:
The always-on host-side HTTP intermediary inside a running aibox Service that
forwards requests and records application-visible evidence. It is global to
aibox rather than owned by a Tenant or Coding Agent.
_Avoid_: Router, packet capture

**Requests module**:
The Console module for inspecting and deleting Request Records.
_Avoid_: Standalone Requests Viewer, packet capture

**Request Record**:
The diagnostic evidence from one request attempt received by the Request Proxy.
It may exist without an upstream request or response.
_Avoid_: Session, Transcript, Run History

**Model Protocol Summary**:
A materialized provider-specific diagnostic projection associated with a
recognized model request in a Request Record.
_Avoid_: Parsed Body, Request Outcome, Record Assessment

**Request Outcome**:
The terminal lifecycle result of one Request Record, independent of any HTTP
response status.
_Avoid_: HTTP status, response code

**Record Assessment**:
The diagnostic classification of one Request Record, derived from its
independent lifecycle, HTTP, provider, and integrity evidence.
_Avoid_: Request Outcome, HTTP status, Provider Error
