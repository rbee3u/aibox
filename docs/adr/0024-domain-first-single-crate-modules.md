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

The implemented direction is `foundation` -> `docker`/`execution` and domain
aggregates (`tenant`, `config`, `component`, `session`, `request`) -> `service`
control adapters -> `lib`/CLI composition. Axum handlers and embedded Console
assets live under `service/control`; Request forwarding and persistence do not
depend on those adapters. Concrete Tenant, Config, Component, Session, and
Management Operation coordinators own mutation gates, long operations,
cancellation, and worker execution, while `ServiceState` keeps its locks and
stores private.

Splitting into a Cargo workspace was rejected because AIBox has one executable
composition root and no supported embedding surface. Keeping the flat module
graph was rejected because shared filesystem mechanics, web handlers, and
domain rules had accumulated bidirectional dependencies. Files are split only
at cohesive policy or lifecycle boundaries; file count and DRY metrics do not
justify an abstraction by themselves.
