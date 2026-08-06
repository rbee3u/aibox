# aibox

aibox provides persistent identities and a Filesystem Sandbox for running
Coding Agents while keeping host access and configuration ownership explicit.

## Language

### Execution and isolation

**Coding Agent**:
An external coding assistant that aibox can run or manage, currently OpenAI
Codex or Claude Code.
_Avoid_: Agent process, Agent Profile

**Run**:
A transient invocation of a Coding Agent using a Managed Tenant and a
Workspace. It is not persistent history and has no identity relationship to a
Session.
_Avoid_: Session, Run History, execution profile

**Workspace**:
The host directory selected as the Coding Agent's working area for a Run.
_Avoid_: Project directory, work directory, work tree

**Filesystem Sandbox**:
The boundary that limits a Coding Agent's local host-filesystem access to the
Workspace, Tenant Home, and explicitly granted Extra Mounts.
_Avoid_: Complete isolation, authority boundary

**Extra Mount**:
An explicit grant that exposes an additional host path inside the Filesystem
Sandbox for a Run.
_Avoid_: Shared path, implicit mount

**Traffic Proxy**:
A temporary host-side HTTP intermediary that records application-visible
request and response data while forwarding one upstream attempt. It is global
to aibox and independent of every Tenant and Coding Agent.
_Avoid_: Router, Routing service, packet capture

**Traffic Record**:
The raw observable request, response, and timing data from one Traffic Proxy
attempt. A Traffic Record may be incomplete after cancellation, recording
failure, or process interruption.
_Avoid_: Session, Transcript, Run History

### Persistent identity

**Tenant**:
A persistent identity that scopes Coding Agent state, Named Configs, Tenant
Components, and Sessions. Every Tenant is either a Managed Tenant or the Host
Tenant.
_Avoid_: Namespace, Target, profile, environment

**Managed Tenant**:
An aibox-managed, runnable Tenant with its own Tenant Home.
_Avoid_: Agent Namespace, managed Target, Linux namespace

**Host Tenant**:
The management-only Tenant backed by the real host Home and selected explicitly
rather than by a Managed Tenant name.
_Avoid_: Host Target, Host Namespace, host profile

**Tenant Home**:
The Home that contains one Tenant's native Coding Agent state. Only a Managed
Tenant's Home can be mounted into a Run.
_Avoid_: Namespace Home, profile home

**Tenant Component**:
An optional capability installed into one Managed Tenant's Tenant Home, such
as a Coding Agent status line or a Tenant-local toolchain. Status-line
Components directly modify native Current Config.
_Avoid_: Plugin, package, add-on

### Configuration and history

**Current Config**:
The current native configuration files consumed by a Coding Agent and directly
modified by aibox, the Coding Agent, its TUI, or the user.
_Avoid_: Agent Configuration, active config, effective config, working config

**Named Config**:
A named, Agent-specific set of Config Field values belonging to exactly one
Tenant and one Coding Agent.
_Avoid_: Agent Profile, Saved Config, Config Template, preset

**Config Field**:
One fixed logical location in a Named Config schema. A Config Field may be a
native setting, a credential value, or the complete Codex credential object.
_Avoid_: Owned path, managed slot

**Config Application**:
A one-time operation that sets every present Config Field and removes every
absent Config Field from Current Config without retaining a relationship.
_Avoid_: Activation, materialization, reconciliation

**Session**:
One interaction record created by a Coding Agent and discovered independently
of Runs. Its typed-prompt view is best-effort, and a Session may exist without
a recognized typed prompt.
_Avoid_: Run, transcript file

**Transcript**:
The Coding Agent's persistent record of a Session.
_Avoid_: Session, prompt history
