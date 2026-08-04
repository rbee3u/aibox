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

### Persistent identity

**Tenant**:
A persistent identity that scopes Coding Agent state, Agent Profiles, Tenant
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
as a Coding Agent status line or a Tenant-local toolchain. A Tenant Component
owns its native state independently of any Agent Profile.
_Avoid_: Plugin, package, add-on

### Configuration and history

**Agent Configuration**:
The native configuration files read and directly modified by a Coding Agent,
its TUI, or the user.
_Avoid_: Active config, generated config

**Agent Profile**:
A named set of owned native Agent Configuration and credentials belonging to
exactly one Tenant and one Coding Agent.
_Avoid_: Global Agent Profile, permission profile, preset

**Active Agent Profile**:
The zero or one Agent Profile currently materialized in one Tenant and Coding
Agent scope and tracked for later comparison or deactivation.
_Avoid_: Last-applied Agent Profile, selected Agent Profile

**Reconciliation**:
The three-way resolution of changes to Agent Profile source and working Agent
Configuration since activation. Working-only changes become Agent Profile
source ownership unless they belong to a Tenant Component.
_Avoid_: Reapply, reset, sync

**Session**:
One interaction record created by a Coding Agent and discovered independently
of Runs. Its typed-prompt view is best-effort, and a Session may exist without
a recognized typed prompt.
_Avoid_: Run, transcript file

**Transcript**:
The Coding Agent's persistent record of a Session.
_Avoid_: Session, prompt history
