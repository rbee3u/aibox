# AIBox

AIBox provides persistent identities and a Filesystem Sandbox for running
Coding Agents while keeping host access and native configuration explicit.

## Language

### Product identity

**AIBox**:
The product brand used in user-facing prose, the Console, clap help, and
documentation. The CLI command, filesystem paths, repository name, crate
name, container user, and other technical `aibox` identifiers remain
lowercase.
_Avoid_: Aibox, AI Box

### Execution boundary

**Coding Agent**:
An external coding assistant that AIBox can run or manage, currently OpenAI
Codex or Claude Code.
_Avoid_: Agent process, Agent Profile

**Run**:
A transient invocation of a Coding Agent using a Managed Tenant and a
Workspace. A Run is not persistent history and has no identity relationship to
a Session.
_Avoid_: Session, Run History, execution profile

**Debug Shell**:
A transient Bash session using one Managed Tenant without a Coding Agent or
Workspace. It exposes the Run-time Tenant Environment for direct diagnosis.
_Avoid_: Run, recovery shell, Host shell

**Workspace**:
The host directory selected as the Coding Agent's working area for a Run.
_Avoid_: Project directory, work directory, work tree

**Filesystem Sandbox**:
The boundary that limits local host-filesystem access during a Run or Debug
Shell. It is not a complete authority or network boundary.
_Avoid_: Complete isolation, authority boundary

**Runtime Image**:
The fixed local Docker image named `aibox:latest` that provides the stable OS
substrate for Runs, Debug Shells, and Managed Tenant Component installation. It
is independent of every Tenant and does not own application language runtimes,
toolchains, or Coding Agent executables.
_Avoid_: Agent image, Tenant image, persistent container

**Extra Mount**:
An explicit grant that exposes an additional host path inside the Filesystem
Sandbox for a Run.
_Avoid_: Shared path, implicit mount

### Local management

**AIBox Service**:
The foreground process started by `aibox console`. One Service exclusively
manages one AIBox Root for browser management while also running the global
Request Proxy.
_Avoid_: Daemon, Request server, backend

**Console**:
The exclusive management interface embedded in the AIBox Service, with
Overview, Tenants, Configs, Sessions, and Requests modules. Runtime Image builds
and persistent lifecycle actions enter through it.
_Avoid_: Requests Viewer, admin site, dashboard

**Control API**:
The single Console-internal HTTP interface shared by every Console module. It
is available only to loopback TCP peers and is not a public embedding API.
_Avoid_: Public API, SDK, remote API

**Service Lock**:
The advisory `$AIBOX_ROOT/.service.lock` held for the lifetime of one AIBox
Service. It prevents a second Service for the same Root but does not coordinate
Runs, Debug Shells, or Console operations in another process.
_Avoid_: Global lock, Run lock, filesystem transaction

**Management Operation**:
The single cancellable long-running Console image build or Component action
retained in Service memory. Only the latest Operation and its bounded log are
observable; it is not persistent history.
_Avoid_: Job, Run, Operation History

### Tenant identity

**AIBox Root**:
The dedicated host storage boundary for AIBox-managed identities,
configuration, and Requests.
_Avoid_: Install prefix, Tenant Home

**Tenant**:
A persistent identity that scopes Coding Agent state, Named Configs, Tenant
Components, and Sessions. Every Tenant is either a Managed Tenant or the Host
Tenant.
_Avoid_: Scope, Namespace, Target, profile, environment

**Tenant Selection**:
An explicit reference to exactly one Managed Tenant or the Host Tenant for a
Tenant-scoped view or action.
_Avoid_: Scope, Target, Tenant key

**Managed Tenant**:
An AIBox-managed, runnable Tenant with its own Tenant Home.
_Avoid_: Agent Namespace, managed Target, Linux namespace

**Default Managed Tenant**:
The protected Managed Tenant named `default`, used when an operation needs a
Managed Tenant and does not explicitly select another one.
_Avoid_: Default profile, implicit Tenant, Host Tenant

**Host Tenant**:
The management-only Tenant backed by the real Host Home.
_Avoid_: Host Target, Host Namespace, host profile

**Tenant Home**:
The AIBox-managed Home belonging to one Managed Tenant and containing its
persistent native Coding Agent and Component state.
_Avoid_: Host Home, profile home

**Tenant Environment**:
The command environment composed when a Run or Debug Shell starts from one
Managed Tenant's user initialization, healthy Tenant Component defaults, and
available Tenant-local tool paths.
_Avoid_: Runtime Image environment, Agent Profile

**Host Home**:
The real user Home that backs the Host Tenant's native Coding Agent state.
_Avoid_: Tenant Home, AIBox Root

**Tenant Component**:
An optional native capability independently installed and managed for one
Tenant, such as a Coding Agent executable, language runtime, toolchain, or
statusline.
_Avoid_: Plugin, package, add-on

**Latest Release**:
The latest comparable stable version observed from a versioned Tenant
Component's authoritative release source. It is transient evidence, not desired
state or an instruction to change a Tenant.
_Avoid_: Target Version, Desired Version, available update

**Component Definition**:
The statusline content and native settings built into the current AIBox
version. It has no independent package version.
_Avoid_: Statusline Version, Latest Release

**Component Update Check**:
An explicit Service-wide observation of Latest Releases and the selected
Tenant's current Component state.
_Avoid_: Component Sync, automatic update, reconciliation

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
value, or the complete Codex credential object. Unknown native fields are
observed as warnings and remain native data; they are not Config Fields.
_Avoid_: Owned path, managed slot

**Visual Config Option**:
A user-facing control in the Visual Editor that projects to one or more Config
Fields without exposing their native paths.
_Avoid_: Visual field, native setting

**Custom Provider**:
The optional fixed Codex provider aggregate in a Named Config. When present it
selects `custom` and contains a nonempty name, base URL, and OpenAI-auth
requirement; when absent the Coding Agent uses its official OpenAI default.
_Avoid_: Provider Field, arbitrary provider

**Request Proxy Route**:
The optional local URL prefix that sends a Coding Agent's provider traffic
through the global Request Proxy. Its hostname represents the destination
scope: loopback for the Host Tenant and Docker's host gateway for a Managed
Tenant.
_Avoid_: Proxy setting, endpoint override

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

**Session Source**:
One Tenant-and-Coding Agent combination used to discover Transcripts. A
Session Source identifies where discovery happens; it is not a Session or a
Run.
_Avoid_: Session scope, Session target, Run source

**Transcript**:
The Coding Agent's native persistent record of one Session.
_Avoid_: Session, prompt history

**Conversation Message**:
A user input or Coding Agent reply that is readable as part of a Session's
primary conversation. It is a view of a Transcript, not a replacement for the
native record. The historical typed Prompt projection is only one possible
source of a Conversation Message and is not the complete content of a Session.
_Avoid_: Prompt, Transcript Entry, Run

**Tool Activity**:
A tool invocation or result observed in a Transcript and shown as supporting
evidence alongside Conversation Messages.
_Avoid_: Conversation Message, Request, Run

**Transcript Evidence**:
A diagnostic view of a Transcript Entry that is neither a Conversation Message
nor Tool Activity. It preserves observable context without turning diagnostic
or internal records into conversation content.
_Avoid_: Conversation Message, Tool Activity, raw Transcript

**Transcript Entry**:
One native record in a Coding Agent Transcript, including readable messages,
Tool Activity, injected context, internal reasoning, malformed records, and
other diagnostic evidence.
_Avoid_: Prompt, Request, log line

### Request diagnostics

**Request Proxy**:
The always-on host-side HTTP intermediary inside a running AIBox Service that
forwards Incoming HTTP Requests as Upstream Requests and captures
application-visible evidence. It is global to AIBox rather than owned by a
Tenant or Coding Agent.
_Avoid_: Router, packet capture

**Requests module**:
The Console module for inspecting and deleting Requests.
_Avoid_: Standalone Requests Viewer, packet capture

**Incoming HTTP Request**:
The application-visible HTTP message received by the Request Proxy from a
client.
_Avoid_: Upstream Request

**Upstream Request**:
The HTTP message the Request Proxy sends toward the selected upstream service.
It may not exist when an Incoming HTTP Request is rejected first.
_Avoid_: Incoming HTTP Request, Provider Request

**Upstream Response**:
The HTTP response the Request Proxy receives from the selected upstream
service. It may not exist when forwarding fails or the Request Proxy produces
a response locally.
_Avoid_: Downstream Response, Provider Response

**Downstream Response**:
The HTTP response the Request Proxy returns to its client, either by relaying
an Upstream Response or by producing one locally.
_Avoid_: Upstream Response, Client Response

**Request**:
The diagnostic lifecycle that begins when the Request Proxy receives one
Incoming HTTP Request, including captured request evidence, an optional
response, timing, Request Outcome, and Request Assessment.
_Avoid_: Request Trace, Session, Run History

**Model Protocol Summary**:
A materialized provider-specific diagnostic projection associated with a
recognized model request in a Request.
_Avoid_: Parsed Body, Request Outcome, Request Assessment

**Request Outcome**:
The terminal lifecycle result of one Request, independent of any HTTP response
status.
_Avoid_: HTTP status, response code

**Request Assessment**:
The diagnostic classification of one Request, derived from its independent
lifecycle, HTTP, provider, and integrity evidence.
_Avoid_: Request Outcome, HTTP status, Provider Error
