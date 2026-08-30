# AIBox

AIBox provides persistent identities and a Filesystem Sandbox for running
Coding Agents while keeping host access and native configuration explicit.

## Language

### Product identity

**AIBox**:
The product brand for the CLI, Service, Console, and their managed state.
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
The fixed Docker image that provides the shared OS substrate for Tenant-bound
container activity.
_Avoid_: Agent image, Tenant image, persistent container

**Extra Mount**:
An explicit grant that exposes an additional host path inside the Filesystem
Sandbox for a Run.
_Avoid_: Shared path, implicit mount

### Local management

**AIBox Service**:
The foreground process that hosts one AIBox Root's Console and Request Proxy.
_Avoid_: Daemon, Request server, backend

**Console**:
The browser management interface embedded in the AIBox Service.
_Avoid_: Requests Viewer, admin site, dashboard

**Control API**:
The Console-internal HTTP interface of the AIBox Service.
_Avoid_: Public API, SDK, remote API

**Service Lock**:
The Root-local identity of one active AIBox Service process.
_Avoid_: Global lock, Run lock, filesystem transaction

**Management Operation**:
A long-running Console action represented by transient progress and a terminal
result rather than persistent history.
_Avoid_: Job, Run, Operation History

### Tenant identity

**AIBox Root**:
The dedicated host storage boundary for AIBox-managed identities,
configuration, and Requests.
_Avoid_: Install prefix, Tenant Home

**Tenant**:
A persistent identity that scopes Coding Agent state, Named Configs, Tenant
Components, and Sessions.
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
The launch-time command environment belonging to one Managed Tenant.
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
The latest comparable stable version observed for a versioned Tenant
Component. It is evidence rather than desired state.
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
The optional provider URL prefix that routes Coding Agent traffic through the
Request Proxy.
_Avoid_: Proxy setting, endpoint override

**Config Application**:
An explicit one-time projection of a Named Config into Current Config.
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
An explicit one-time distribution of a Host Tenant Codex credential snapshot
to eligible Configs.
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
A user input or Coding Agent reply in a Session's readable primary
conversation. It is a view of a Transcript rather than a native record.
_Avoid_: Prompt, Transcript Entry, Run

**Tool Activity**:
A tool invocation or result observed in a Transcript and shown as supporting
evidence alongside Conversation Messages.
_Avoid_: Conversation Message, Request, Run

**Transcript Evidence**:
A diagnostic view of a Transcript Entry that is neither a Conversation Message
nor Tool Activity.
_Avoid_: Conversation Message, Tool Activity, raw Transcript

**Transcript Entry**:
One native record in a Coding Agent Transcript, including readable messages,
Tool Activity, injected context, internal reasoning, malformed records, and
other diagnostic evidence.
_Avoid_: Prompt, Request, log line

### Request diagnostics

**Request Proxy**:
The AIBox Service HTTP intermediary that forwards Incoming HTTP Requests and
captures application-visible evidence.
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
The HTTP response the Request Proxy receives from an upstream service.
_Avoid_: Downstream Response, Provider Response

**Downstream Response**:
The HTTP response the Request Proxy returns to its client, either by relaying
an Upstream Response or by producing one locally.
_Avoid_: Upstream Response, Client Response

**Request**:
The diagnostic lifecycle that begins when the Request Proxy receives one
Incoming HTTP Request and includes its captured evidence.
_Avoid_: Request Trace, Session, Run History

**Model Protocol Summary**:
A materialized provider-specific diagnostic projection associated with a
recognized model request in a Request.
_Avoid_: Parsed Body, Request Outcome, Request Assessment

**Request Outcome**:
The terminal lifecycle result of one Request, independent of any HTTP response
status.
_Avoid_: HTTP status, response code

**Request State**:
Whether a Request is Active, Completed, or Interrupted, independently of its
terminal Request Outcome and diagnostic Request Assessment.
_Avoid_: Request Outcome, Request Assessment

**Request Assessment**:
The diagnostic classification of one Request, derived from its independent
lifecycle, HTTP, provider, and integrity evidence.
_Avoid_: Request Outcome, HTTP status, Provider Error

**Coding Agent Session ID**:
An unverified Session identifier reported in Coding Agent request evidence. It
does not establish an identity relationship between a Request and an AIBox
Session.
_Avoid_: Request-to-Session mapping, Run Session ID
