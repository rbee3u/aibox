use std::collections::{BTreeMap, BTreeSet};

/// Every non-structural module edge in the crate, declared once.
///
/// Nodes cover depth one and depth two so major domains are governed internally
/// as well as at their boundary. Naming `foundation::safe_fs` rather than
/// `foundation` states which mechanism an edge reaches.
///
/// The set is exact in both directions: an undeclared edge and a declared edge
/// nothing uses both fail.
pub(super) fn allowed_dependencies() -> BTreeMap<&'static str, BTreeSet<&'static str>> {
    BTreeMap::from([
        ("agent", BTreeSet::from([])),
        ("agent::claude", BTreeSet::from([])),
        ("agent::codex", BTreeSet::from([])),
        ("application_error", BTreeSet::from([])),
        ("cli", BTreeSet::from(["agent", "tenant"])),
        (
            "component",
            BTreeSet::from(["docker", "foundation", "tenant"]),
        ),
        (
            "component::catalog",
            BTreeSet::from([
                "agent",
                "component::node_agent",
                "component::python",
                "component::rust_go",
                "component::statusline",
                "foundation::safe_fs",
                "tenant",
            ]),
        ),
        ("component::links", BTreeSet::from(["sandbox"])),
        ("component::model", BTreeSet::from(["agent"])),
        ("component::native", BTreeSet::from(["foundation::safe_fs"])),
        (
            "component::node_agent",
            BTreeSet::from([
                "component::links",
                "component::native",
                "foundation::safe_fs",
            ]),
        ),
        (
            "component::python",
            BTreeSet::from([
                "component::links",
                "component::native",
                "foundation::safe_fs",
            ]),
        ),
        (
            "component::runtime",
            BTreeSet::from([
                "component::catalog",
                "component::native",
                "docker",
                "foundation::safe_fs",
                "sandbox",
                "tenant",
            ]),
        ),
        (
            "component::rust_go",
            BTreeSet::from(["component::native", "foundation::safe_fs"]),
        ),
        (
            "component::statusline",
            BTreeSet::from([
                "agent",
                "component::native",
                "foundation::safe_fs",
                "tenant",
            ]),
        ),
        ("component::updates", BTreeSet::from([])),
        ("config", BTreeSet::from(["foundation"])),
        (
            "config::application",
            BTreeSet::from([
                "config::metadata",
                "config::storage",
                "foundation::safe_fs",
                "tenant",
            ]),
        ),
        (
            "config::auth",
            BTreeSet::from(["agent", "config::storage", "foundation::safe_fs", "tenant"]),
        ),
        (
            "config::catalog",
            BTreeSet::from([
                "config::definition",
                "config::storage",
                "foundation::safe_fs",
                "tenant",
            ]),
        ),
        (
            "config::comparison",
            BTreeSet::from([
                "config::application",
                "config::definition",
                "config::editing",
                "config::visual",
                "tenant",
            ]),
        ),
        (
            "config::definition",
            BTreeSet::from(["agent", "config::native"]),
        ),
        (
            "config::editing",
            BTreeSet::from([
                "application_error",
                "config::definition",
                "config::storage",
                "config::visual",
                "foundation::safe_fs",
                "tenant",
            ]),
        ),
        (
            "config::metadata",
            BTreeSet::from(["foundation::safe_fs", "tenant"]),
        ),
        (
            "config::model",
            BTreeSet::from(["agent", "config::visual", "tenant"]),
        ),
        ("config::native", BTreeSet::from([])),
        (
            "config::visual",
            BTreeSet::from(["agent", "config::definition", "config::native"]),
        ),
        (
            "config::storage",
            BTreeSet::from(["config::definition", "foundation::safe_fs", "tenant"]),
        ),
        ("docker", BTreeSet::from([])),
        ("docker::image", BTreeSet::from([])),
        ("docker::run", BTreeSet::from(["docker::supervision"])),
        ("docker::supervision", BTreeSet::from([])),
        (
            "execution",
            BTreeSet::from(["component", "docker", "sandbox", "tenant"]),
        ),
        (
            "execution::debug",
            BTreeSet::from(["docker", "sandbox", "tenant"]),
        ),
        (
            "execution::run",
            BTreeSet::from(["agent", "component", "docker", "sandbox", "tenant"]),
        ),
        ("foundation", BTreeSet::from([])),
        ("foundation::platform", BTreeSet::from([])),
        ("foundation::safe_fs", BTreeSet::from([])),
        ("foundation::sync", BTreeSet::from([])),
        (
            "lib",
            BTreeSet::from(["agent", "cli", "execution", "service", "tenant"]),
        ),
        ("main", BTreeSet::from([])),
        (
            "management",
            BTreeSet::from([
                "application_error",
                "component",
                "foundation::safe_fs",
                "request",
                "tenant",
            ]),
        ),
        (
            "management::component",
            BTreeSet::from(["component", "docker", "tenant"]),
        ),
        (
            "management::config",
            BTreeSet::from(["agent", "application_error", "config", "tenant"]),
        ),
        ("management::operation", BTreeSet::from(["docker"])),
        (
            "management::operation_state",
            BTreeSet::from(["application_error"]),
        ),
        (
            "management::overview",
            BTreeSet::from([
                "agent",
                "component",
                "config",
                "docker",
                "foundation::safe_fs",
                "session",
                "tenant",
            ]),
        ),
        ("management::request", BTreeSet::from(["request"])),
        (
            "management::session",
            BTreeSet::from(["agent", "application_error", "session", "tenant"]),
        ),
        (
            "management::tenant",
            BTreeSet::from(["application_error", "tenant"]),
        ),
        ("request", BTreeSet::from([])),
        ("request::assessment", BTreeSet::from(["request::model"])),
        (
            "request::inspection",
            BTreeSet::from([
                "request::assessment",
                "request::interpretation",
                "request::model",
                "request::store",
            ]),
        ),
        (
            "request::interpretation",
            BTreeSet::from(["foundation::safe_fs", "request::model"]),
        ),
        ("request::model", BTreeSet::from([])),
        (
            "request::proxy",
            BTreeSet::from([
                "foundation::safe_fs",
                "foundation::sync",
                "request::interpretation",
                "request::model",
                "request::reporter",
                "request::response_observation",
                "request::sse",
                "request::store",
            ]),
        ),
        ("request::reporter", BTreeSet::from(["request::model"])),
        (
            "request::response_observation",
            BTreeSet::from(["request::interpretation", "request::model", "request::sse"]),
        ),
        ("request::sse", BTreeSet::from(["request::model"])),
        (
            "request::store",
            BTreeSet::from([
                "application_error",
                "foundation::safe_fs",
                "foundation::sync",
                "request::assessment",
                "request::interpretation",
                "request::model",
                "request::sse",
            ]),
        ),
        ("sandbox", BTreeSet::from([])),
        ("sandbox::args", BTreeSet::from(["foundation::platform"])),
        ("sandbox::mount", BTreeSet::from(["tenant"])),
        (
            "sandbox::spec",
            BTreeSet::from(["sandbox::args", "sandbox::mount"]),
        ),
        ("service", BTreeSet::from([])),
        (
            "service::control",
            BTreeSet::from([
                "agent",
                "application_error",
                "component",
                "config",
                "foundation",
                "management",
                "request",
                "service::state",
                "session",
                "tenant",
            ]),
        ),
        (
            "service::http",
            BTreeSet::from(["request", "service::state"]),
        ),
        (
            "service::runtime",
            BTreeSet::from([
                "component",
                "docker",
                "foundation::platform",
                "foundation::safe_fs",
                "management",
                "request",
                "service::state",
                "tenant",
            ]),
        ),
        ("service::state", BTreeSet::from(["management", "request"])),
        ("session", BTreeSet::from(["agent"])),
        (
            "session::backend",
            BTreeSet::from([
                "agent",
                "session::claude",
                "session::codex",
                "session::filesystem",
                "session::model",
            ]),
        ),
        (
            "session::catalog",
            BTreeSet::from([
                "session::backend",
                "session::filesystem",
                "session::model",
                "session::text",
            ]),
        ),
        ("session::claude", BTreeSet::from([])),
        ("session::codex", BTreeSet::from([])),
        (
            "session::detail",
            BTreeSet::from([
                "application_error",
                "session::backend",
                "session::catalog",
                "session::filesystem",
                "session::model",
                "session::text",
            ]),
        ),
        (
            "session::filesystem",
            BTreeSet::from(["foundation::safe_fs", "session::text"]),
        ),
        ("session::model", BTreeSet::from(["session::text"])),
        ("session::text", BTreeSet::from([])),
        ("tenant", BTreeSet::from([])),
        (
            "execution::environment",
            BTreeSet::from(["component", "sandbox"]),
        ),
        ("tenant::host", BTreeSet::from(["foundation::safe_fs"])),
        ("tenant::identity", BTreeSet::from([])),
        (
            "tenant::layout",
            BTreeSet::from([
                "agent",
                "foundation::safe_fs",
                "tenant::host",
                "tenant::identity",
            ]),
        ),
        (
            "tenant::lifecycle",
            BTreeSet::from([
                "agent",
                "foundation::safe_fs",
                "tenant::identity",
                "tenant::layout",
            ]),
        ),
    ])
}

/// Sources allowed to keep an inline `mod tests`.
///
/// `service/control/contract.rs` is test-only in its entirety: the module
/// declaration is the file's whole purpose, so externalizing it would leave a
/// one-line shell.
pub(super) const INLINE_TEST_EXCEPTIONS: &[&str] = &["service/control/contract.rs"];

/// Test suites that cover no single module and so name none.
///
/// `architecture_tests.rs` checks the crate rather than a module, which is why
/// it is the one suite whose name has nothing to point at.
pub(super) const UNATTACHED_TEST_SUITES: &[&str] = &["architecture_tests.rs"];

/// Every `#[cfg(test)]` item that widens a module's visible surface.
///
/// A test reaching past a facade defeats the invariant the facade exists to
/// hold, so each entry has to name a seam a test legitimately needs and cannot
/// get through the same door production code uses. The exact list makes every
/// addition or removal reviewable.
///
/// Keys are `path::item`, so a line moving does not churn the list.
pub(super) const TEST_ONLY_SURFACE: &[&str] = &[
    // Types explicitly constructed by contract samples and fixture providers.
    "component/mod.rs::LatestEntry",
    "component/mod.rs::LatestEntryState",
    "component/mod.rs::LatestResult",
    "request/mod.rs::RequestOutcome",
    "request/mod.rs::ProtocolDiagnostic",
    "service/control/requests.rs::BodyQuery",
    "service/control/requests.rs::EventTimingQuery",
    "service/control/requests.rs::EventTimingResponse",
    "service/control/requests.rs::ListQuery",
    "service/control/requests.rs::RequestDetail",
    "service/control/requests.rs::RequestList",
    "service/control/requests.rs::ResponseDetail",
    // The struct behind the test-facing route manifest `control_routes!` emits
    // beside the path constants, so a route declared once cannot desynchronize
    // from it. The `ENDPOINTS` const itself sits inside the macro body, which
    // this check cannot see: `syn` parses `macro_rules!` without expanding it.
    "service/control/routes.rs::EndpointDescription",
    // Docker injection: a suite substitutes a stub CLI instead of contacting a
    // daemon. `DockerCli::isolated` is the seam; the rest are its reach.
    "docker/image.rs::build_image_with",
    "docker/mod.rs::build_image_with",
    "docker/mod.rs::inspect_runtime_image_with",
    "docker/mod.rs::isolated",
    "docker/run.rs::new",
    // The process-wide run registry is a static, so suites that start a
    // container serialize on one lock.
    "docker/mod.rs::run_registry_test_lock",
    "docker/supervision.rs::RUN_REGISTRY_TEST_LOCK",
    "docker/supervision.rs::run_registry_test_lock",
    "docker/supervision.rs::command_quiet",
    "docker/supervision.rs::detached",
    "docker/supervision.rs::set_cidfile",
    "component/runtime.rs::install_runtime_component_with",
    // Recorded-Request seeding: a reader test needs stored Requests, and the
    // active-Request map is per-handle, so a separately opened store would not
    // see this state's in-flight Requests.
    "request/mod.rs::ObservedRequest",
    "request/mod.rs::RequestStore",
    "request/mod.rs::RuntimeMeasurements",
    "request/mod.rs::for_test",
    "request/mod.rs::new",
    "request/mod.rs::store",
    "request/proxy/attempt.rs::summary_handle",
    "request/proxy/request_stream.rs::recorded_request_stream",
    "request/proxy/response_stream.rs::record_response_stream",
    "request/store/mod.rs::update",
    "request/store/writing.rs::open",
    // Config fixtures written through the same validation production uses.
    "config/mod.rs::ConfigCatalogState",
    "config/mod.rs::PropagationOutcome",
    // Transcript fixtures and the parsed records a projection test asserts on.
    "session/detail.rs::detail_records_for_test",
    "session/filesystem.rs::test_transcript_home",
    "session/mod.rs::EvidenceEncoding",
    "session/model.rs::Prompt",
    // Environment-derived paths, exercised without mutating the real process.
    "tenant/host.rs::aibox_root_from",
    "tenant/host.rs::host_home_from",
    // Substitutes the Latest Release provider so an update check stays
    // socket-free.
    "management/component.rs::set_latest_provider",
    // Explicitly hold the process-local gate while exercising HTTP conflict responses.
    "management/tenant.rs::hold_mutation_for_test",
    // Seed deterministic operations for shutdown and HTTP lifecycle tests.
    "management/operation.rs::start_for_test",
    // Nested log type named only by the wire contract exporter.
    "management/mod.rs::OperationLog",
];
