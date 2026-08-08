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
A temporary host-side HTTP intermediary that semantically forwards HTTP and
records the application-visible data from one upstream request attempt. It is
global to aibox and independent of every Tenant and Coding Agent.
_Avoid_: Router, Routing service, packet capture

**Traffic Record**:
The raw observable request, upstream response, and timing data from one Traffic
Proxy upstream request attempt, together with its checkpointed Model Protocol
Summary. It can exist without an upstream response and may be incomplete after
cancellation, recording failure, or process interruption.
_Avoid_: Session, Transcript, Run History

**SSE Event**:
A dispatchable event in an event-stream response: a blank-line-terminated
block containing at least one `data` field. Its bytes remain part of the raw
response body.
_Avoid_: Chunk, token, model delta

**Traffic Outcome**:
The terminal lifecycle result of one Traffic Proxy attempt, independent of any
HTTP response status. An HTTP 500 can be a completed outcome, while a 200 can
still end with a stream or client-disconnect failure.
_Avoid_: HTTP status, response code

**Traffic Phase**:
The observable phase of an active Traffic Record: Waiting before response
metadata exists, or Streaming after response metadata arrives.
_Avoid_: Traffic Outcome, HTTP status

**Traffic Duration**:
The elapsed time from the start of a Traffic Record until its terminal outcome.
For an active record it is the current elapsed time; an interrupted record has
no known terminal duration.
_Avoid_: TTFB, response-header time

**First Token**:
The elapsed time from Traffic Record start until a protocol parser observes the
first output-bearing model delta. For SSE, the delta is observed when its
complete dispatchable event is available; a delta may contain multiple
tokenizer tokens.
_Avoid_: TTFB, response-header time

**Timing Stage**:
One observable interval within a Traffic Record's timing breakdown, such as
request upload, response wait, or model output.
_Avoid_: Traffic Phase, network phase

**Requested Model Response Mode**:
Whether a model API request asks for incremental event-stream output or one
complete response.
_Avoid_: Observed Model Response Mode, request-body streaming, Traffic Phase

**Observed Model Response Mode**:
Whether the upstream model API responds with an event stream or one normal
response, as observed from its response metadata.
_Avoid_: Requested Model Response Mode, Traffic Phase

**Model Response Terminality**:
Whether a recognized model protocol has reported its terminal response. It is
independent of Traffic Outcome and may remain unobserved after a disconnect,
shutdown, or missing terminal event.
_Avoid_: Traffic Outcome, HTTP status

**Effective Model**:
The model identity reported by an upstream response as having produced it.
_Avoid_: Requested Model, configured model

**Requested Model**:
The model identity supplied in a model API request.
_Avoid_: Effective Model, configured model

**Requested Reasoning Effort**:
The provider-native reasoning-effort value explicitly supplied in a model API
request. No provider default is inferred when it is absent.
_Avoid_: Effective Reasoning Effort, Reasoning Output Tokens

**Effective Reasoning Effort**:
The reasoning-effort value explicitly reported by an upstream model API as
having been applied. No provider default is inferred when it is absent.
_Avoid_: Requested Reasoning Effort, Reasoning Output Tokens

**Model Protocol Summary**:
The best-effort, checkpointed stable model-protocol facts materialized in a
Traffic Record Summary. Its family remains Unknown when the request is not a
recognized model API; raw request and response data remain the diagnostic
evidence and are not reconstructed from this summary.
_Avoid_: Interpretation cache, parsed body copy

**Coding Agent Session ID**:
An opaque identifier reported by a recognized model API request for the Coding
Agent Session that issued it. It does not associate a Run, Session, and Traffic
Record or resolve a Transcript.
_Avoid_: Traffic Record ID, provider conversation ID, prompt cache key

**Token Usage**:
Provider-reported token counters associated with one model API response. It is
not a local tokenizer estimate.
_Avoid_: Token estimate, Traffic size

**Provider Error**:
A structured failure reported by a recognized model API response, distinct
from the Traffic Outcome and HTTP response status.
_Avoid_: Traffic Outcome, HTTP error status

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
An optional capability installed into one Tenant's Tenant Home, such as a
Coding Agent status line or a Managed Tenant-local toolchain. Status-line
Components directly modify native Current Config; Host Tenant Components are
limited to status lines.
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

**ChatGPT Credentials**:
Codex credentials issued and refreshable through ChatGPT sign-in for one
ChatGPT account.
_Avoid_: API key credentials, OpenAI credentials

**Credential Propagation**:
An explicit one-time distribution of one newer ChatGPT Credentials snapshot to
older Configs for the same ChatGPT account without retaining a relationship.
_Avoid_: Credential Sync, activation, reconciliation

**Session**:
One interaction record created by a Coding Agent and discovered independently
of Runs. Its typed-prompt view is best-effort, and a Session may exist without
a recognized typed prompt.
_Avoid_: Run, transcript file

**Transcript**:
The Coding Agent's persistent record of a Session.
_Avoid_: Session, prompt history
