# AIBox

AIBox runs Coding Agents inside explicit filesystem boundaries and manages
Tenant-scoped native state.

## Language

### Product Surface

**Service**:
The foreground local process that hosts the Request Proxy and the embedded Console.

**Console**:
The embedded management interface for AIBox state and operations.

### Execution and Isolation

**Coding Agent**:
A supported external coding assistant that AIBox can invoke.

**Run**:
A temporary Coding Agent execution within one Managed Tenant and Workspace,
independent of any Session.

**Debug Shell**:
A temporary interactive shell within one Managed Tenant, without a Coding Agent
or Workspace.

**Workspace**:
The host directory used as a Run's primary working area.

**Filesystem Sandbox**:
The boundary of host filesystem access granted to a Run, Debug Shell, or
Component installation.

**Runtime Image**:
The fixed base environment shared by sandboxed AIBox operations.

**Extra Mount**:
An explicit mapping from a host source into a container target for a Run.

### Tenancy

**AIBox Root**:
The dedicated host storage boundary for AIBox-managed state and Request evidence.

**Tenant**:
A scope for Coding Agent state and capabilities. A Tenant is either a Managed
Tenant or the Host Tenant.

**Managed Tenant**:
An AIBox-owned, runnable Tenant with its own Tenant Home. The instance named
`default` is the protected default Managed Tenant.

**Host Tenant**:
The management-only Tenant backed by the user's Host Home, distinct from every
Managed Tenant, including one named `host`.

**Tenant Home**:
The persistent home belonging to one Managed Tenant.

**Host Home**:
The user's real home that backs the Host Tenant's native Coding Agent state.

**Tenant Environment**:
The environment composed for one Managed Tenant when a Run or Debug Shell starts.

**Component**:
An optional native capability belonging to a Tenant, such as a Coding Agent,
language runtime, toolchain, or statusline.
_Avoid_: Plugin

### Configuration

**Current Config**:
The native configuration currently consumed by one Coding Agent in one Tenant.
_Avoid_: Active Config

**Named Config**:
A reusable named definition of Config Fields for one Tenant and one Coding Agent.

**Config Field**:
One logical setting or credential unit in a Coding Agent's Named Config model.

**Config Application**:
The one-time projection of a Named Config's Config Fields into Current Config.
_Avoid_: Activation

**Last Application**:
The record of the most recent Config Application and when it occurred.

**Config Drift**:
The observed relationship between Current Config and the Named Config recorded by
Last Application.

**Credential Propagation**:
A one-time distribution of a Host Tenant Codex credential snapshot to eligible
existing Codex Named Configs and Managed Tenant Current Configs.

### Sessions

**Session**:
A Coding Agent interaction identity within one Tenant and Coding Agent,
independent of Runs and represented by one Transcript.

**Transcript**:
The Coding Agent's native persistent record of one Session, including its native
records and diagnostic content.

**Transcript Evidence**:
A diagnostic view of Transcript content not represented as readable conversation
or tool activity.

### Request Diagnostics

**Request Proxy**:
The global host-side intermediary that forwards client traffic and captures
application-visible evidence. It is not owned by a Tenant.

**Request**:
One inbound HTTP request together with its Request Proxy lifecycle and recorded
evidence.

**Request State**:
The lifecycle phase of a Request, distinct from its Request Outcome and Request
Assessment.

**Request Outcome**:
The terminal result of a Request, independent of HTTP status.

**Request Assessment**:
A classification of a Request derived from its state, outcome, HTTP and provider
results, and diagnostic evidence.

**Coding Agent Session ID**:
An unverified session identifier found in Request evidence. It does not link a
Request to an AIBox Session.

**Request Group**:
A collection-root directory that holds older recorded Requests as a count and
pagination index. It is not Request evidence and is not a retention policy.
