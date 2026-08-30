# Organize the application-only crate as domain-first modules

AIBox remains one application-only Rust crate, but its private implementation
is organized from mechanical foundations through domain modules to the Service
and CLI composition roots. `safe_fs`, `platform`, and synchronization code own
mechanics without Tenant or Config policy; Agent and Tenant identity support
Config, Components, Sessions, and the Request model; I/O adapters and
lifecycle orchestration depend on those domains rather than the reverse.
Modules expose narrow facades and concrete closed-set types instead of generic
repository, service, registry, or plugin abstractions. This makes dependency
direction visible while preserving the CLI, disk layout, Control API, Docker
protocol, and single-crate deployment.

The implemented direction is mechanical `foundation` and `docker`, shared
`sandbox` argument validation, domain aggregates (`tenant`, `config`,
`component`, `session`, `request`), `service` coordination and Control
adapters, then `lib`/CLI composition. `lib` converts Clap DTOs into
execution-owned Run and Debug commands; execution never imports CLI types.
`sandbox` owns RunSpec and the mount/container argument invariants shared by
Run, Debug Shell, and Component installers, split so that `spec` owns RunSpec,
`mount` owns parsing and boundary checks, and `args` owns pure `docker run`
builders. Tenant Environment composition depends on a Tenant-owned capability
snapshot populated by Component inspection, so Tenant does not import Component
types.

A value or behavior with one meaning has one definition. The container Home
belongs to `tenant`, and the untrusted-file size bound to `foundation`, because
a second copy of either cannot fail to compile when it drifts — it can only
mount the wrong path or admit the wrong file. For the same reason a struct with
several same-typed fields is constructed by name rather than by position, and a
Coding Agent's contracts are reached only through `AgentKind`, whose matches all
sit in `agent/mod.rs` so a new Agent cannot compile until each is supplied.

Modules do not widen their public surface for tests. `sandbox`'s mount
resolution and validation are private because only `RunSpec::resolve` calls
them, and resolving in the wrong order is therefore unrepresentable rather than
a rule a caller must remember. Tests enter through the same facade production
code uses.

Config and Component roots are narrow facades over concrete ownership modules,
not generic repository or service traits. Named Config names are validated once
at the Control/coordinator boundary and path construction stays inside Config.
Component families keep inspection, owned paths, installation, and removal
adjacent. Axum handlers, narrow response helpers, embedded Console assets, and
the test-only Rust-owned contract exporter live under `service/control`.

Request exposes a concrete inspection facade to Control while persistence,
assessment, interpretation, and the v4 layout remain private. Store modules
separate writing, reading/deletion, and safe layout; proxy modules separate
target/transport, headers, request stream, response stream, and attempt
orchestration. A Request attempt owns the closed `Active`, `Finalizing`, and
`Finished` lifecycle so normal completion, persistence retry, reporting, and
Drop fallback share one terminal decision. Concrete Tenant, Config, Component,
Session, and Management Operation coordinators own mutation gates, long
operations, cancellation, and worker execution, while `ServiceState` keeps its
locks and stores private.

The Docker facade delegates child spawning and output capture to
`docker/run.rs` and cidfile, child, signal, and lingering-container supervision
to `docker/supervision.rs`. Session Transcript reads and deletes use the common
anchored `foundation::safe_fs` primitives; the Session facade retains discovery,
projection, and backend selection policy.

Control handlers return a `Result` whose error converts to a response, so wire
decoding, selector parsing, and coordinator calls use `?`. Each Control route is
declared once and expands to both its path constant and the test-facing manifest
that generates the Console route bindings, because a route written twice can
desynchronize the manifest meant to detect contract drift.

Test suites live in `<module>_tests.rs` beside the module they cover, enforced
by an architecture test. Mixed inline and external placement was rejected after
four modules had quietly grown more test lines than production lines, which
reading a file from the top did not reveal.

Splitting into a Cargo workspace was rejected because AIBox has one executable
composition root and no supported embedding surface. Keeping the flat module
graph was rejected because shared filesystem mechanics, web handlers, and
domain rules had accumulated bidirectional dependencies. Files are split only
at cohesive policy or lifecycle boundaries; file count and DRY metrics do not
justify an abstraction by themselves. Consolidation is likewise justified by a
concrete failure mode — a second definition that drifts silently, or a machine
whose four copies disagree — not by a duplication count.
