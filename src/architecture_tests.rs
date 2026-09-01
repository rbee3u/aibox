use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use syn::visit::{self, Visit};
use syn::{Attribute, Item, ItemUse, UseTree};

fn rust_sources(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        let Ok(entries) = fs::read_dir(path) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                files.push(path);
            }
        }
    }
    files
}

fn is_test_source(path: &Path) -> bool {
    path.components().any(|part| part.as_os_str() == "tests")
        || path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                name == "tests.rs"
                    || name.ends_with("_tests.rs")
                    || matches!(
                        name,
                        "architecture_tests.rs" | "lib_tests.rs" | "testutil.rs"
                    )
            })
}

fn is_test_only(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attribute| {
        attribute.path().is_ident("cfg")
            && attribute.meta.require_list().is_ok_and(|list| {
                list.tokens
                    .to_string()
                    .split_whitespace()
                    .any(|part| part == "test")
            })
    })
}

fn item_attrs(item: &Item) -> Option<&[Attribute]> {
    Some(match item {
        Item::Const(item) => &item.attrs,
        Item::Enum(item) => &item.attrs,
        Item::ExternCrate(item) => &item.attrs,
        Item::Fn(item) => &item.attrs,
        Item::ForeignMod(item) => &item.attrs,
        Item::Impl(item) => &item.attrs,
        Item::Macro(item) => &item.attrs,
        Item::Mod(item) => &item.attrs,
        Item::Static(item) => &item.attrs,
        Item::Struct(item) => &item.attrs,
        Item::Trait(item) => &item.attrs,
        Item::TraitAlias(item) => &item.attrs,
        Item::Type(item) => &item.attrs,
        Item::Union(item) => &item.attrs,
        Item::Use(item) => &item.attrs,
        Item::Verbatim(_) => return None,
        _ => return None,
    })
}

/// The absolute module path of one source file, plus the child modules it
/// declares.
///
/// Declared children let resolution cover Rust 2018 uniform paths alongside
/// explicit `crate::`, `self::`, and `super::` paths.
struct ModuleScope {
    path: Vec<String>,
    children: BTreeSet<String>,
}

impl ModuleScope {
    /// Resolve one written path to an absolute crate module path.
    ///
    /// Returns `None` for anything that does not name this crate: an external
    /// crate, a primitive, or an item already in scope.
    fn resolve(&self, segments: &[String]) -> Option<Vec<String>> {
        let mut rest = segments;
        let mut base = match rest.first()?.as_str() {
            "crate" => {
                rest = &rest[1..];
                Vec::new()
            }
            "self" => {
                rest = &rest[1..];
                self.path.clone()
            }
            "super" => {
                let mut base = self.path.clone();
                while rest.first().is_some_and(|segment| segment == "super") {
                    base.pop()?;
                    rest = &rest[1..];
                }
                base
            }
            first if self.children.contains(first) => self.path.clone(),
            _ => return None,
        };
        base.extend(rest.iter().cloned());
        (!base.is_empty()).then_some(base)
    }
}

struct CrateDependencies {
    scope: ModuleScope,
    modules: BTreeSet<Vec<String>>,
}

impl CrateDependencies {
    fn new(scope: ModuleScope) -> Self {
        Self {
            scope,
            modules: BTreeSet::new(),
        }
    }

    fn record(&mut self, segments: &[String]) {
        if let Some(resolved) = self.scope.resolve(segments) {
            self.modules.insert(resolved);
        }
    }

    /// Record a path whose last segment names an item rather than a module.
    ///
    /// A type or expression path always ends at a function, type, or constant, so
    /// keeping that segment invents a module. `lib.rs` calling `execution::run`
    /// reaches the function `run` in `execution/mod.rs`, not the module
    /// `execution/run.rs` that happens to share its name. A `use` leaf is
    /// genuinely ambiguous and keeps its final segment; both forms then resolve
    /// to the governing node, so the distinction only matters for a leaf that
    /// collides with a sibling module's name.
    fn record_item_path(&mut self, segments: &[String]) {
        if let Some((_, prefix)) = segments.split_last() {
            self.record(prefix);
        }
    }

    fn record_use_tree(&mut self, tree: &UseTree, prefix: &mut Vec<String>) {
        match tree {
            UseTree::Path(path) => {
                prefix.push(path.ident.to_string());
                self.record_use_tree(&path.tree, prefix);
                prefix.pop();
            }
            UseTree::Name(name) => {
                prefix.push(name.ident.to_string());
                self.record(prefix);
                prefix.pop();
            }
            UseTree::Rename(rename) => {
                prefix.push(rename.ident.to_string());
                self.record(prefix);
                prefix.pop();
            }
            UseTree::Group(group) => {
                for item in &group.items {
                    self.record_use_tree(item, prefix);
                }
            }
            UseTree::Glob(_) => self.record(&prefix.clone()),
        }
    }
}

impl<'ast> Visit<'ast> for CrateDependencies {
    fn visit_item(&mut self, item: &'ast Item) {
        if item_attrs(item).is_some_and(is_test_only) {
            return;
        }
        visit::visit_item(self, item);
    }

    fn visit_item_use(&mut self, item: &'ast ItemUse) {
        self.record_use_tree(&item.tree, &mut Vec::new());
        visit::visit_item_use(self, item);
    }

    fn visit_path(&mut self, path: &'ast syn::Path) {
        let segments: Vec<String> = path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect();
        self.record_item_path(&segments);
        visit::visit_path(self, path);
    }
}

/// Child modules one file declares outside `cfg(test)`.
fn declared_children(items: &[Item]) -> BTreeSet<String> {
    items
        .iter()
        .filter_map(|item| match item {
            Item::Mod(module) if !is_test_only(&module.attrs) => Some(module.ident.to_string()),
            _ => None,
        })
        .collect()
}

/// The absolute module path of one source file.
///
/// `src/lib.rs` is the crate root, so its path is empty and the modules it
/// declares sit at the top level. `src/main.rs` is a second root with its own
/// tree.
fn module_path(src: &Path, path: &Path) -> Vec<String> {
    let relative = path.strip_prefix(src).expect("Rust source below src/");
    let mut segments: Vec<String> = relative
        .components()
        .map(|part| part.as_os_str().to_string_lossy().to_string())
        .collect();
    let file = segments.pop().expect("Rust source has a file name");
    let stem = file.trim_end_matches(".rs");
    if !matches!(stem, "mod" | "lib" | "main") {
        segments.push(stem.to_string());
    }
    segments
}

/// The table node that governs one absolute module path.
///
/// Nodes stop at depth two, so `request::proxy::attempt` answers for
/// `request::proxy`. Depth three and beyond is file cohesion inside one owner
/// rather than a dependency direction between owners.
fn governing_node<'a>(nodes: &BTreeSet<&'a str>, path: &[String]) -> Option<&'a str> {
    (1..=path.len().min(2))
        .rev()
        .find_map(|depth| nodes.get(path[..depth].join("::").as_str()).copied())
}

fn source_node<'a>(nodes: &BTreeSet<&'a str>, src: &Path, path: &Path) -> Option<&'a str> {
    match path.file_stem().and_then(|stem| stem.to_str()) {
        Some(root @ ("lib" | "main")) => nodes.get(root).copied(),
        _ => governing_node(nodes, &module_path(src, path)),
    }
}

/// True when one of the two nodes contains the other.
///
/// A facade re-exporting its own children, and those children using the types
/// the facade defines, are structural rather than architectural. Excluding both
/// directions leaves the edges that decide whether a change ripples.
fn is_structural(from: &str, to: &str) -> bool {
    from == to
        || to.strip_prefix(from).is_some_and(|r| r.starts_with("::"))
        || from.strip_prefix(to).is_some_and(|r| r.starts_with("::"))
}

/// Every non-structural module edge in the crate, declared once.
///
/// Nodes cover depth one and depth two so major domains are governed internally
/// as well as at their boundary. Naming `foundation::safe_fs` rather than
/// `foundation` states which mechanism an edge reaches.
///
/// The set is exact in both directions: an undeclared edge and a declared edge
/// nothing uses both fail.
fn allowed_dependencies() -> BTreeMap<&'static str, BTreeSet<&'static str>> {
    BTreeMap::from([
        ("agent", BTreeSet::new()),
        ("agent::claude", BTreeSet::new()),
        ("agent::codex", BTreeSet::new()),
        ("application_error", BTreeSet::new()),
        ("cli", BTreeSet::from(["agent", "tenant"])),
        (
            "component",
            BTreeSet::from(["agent", "docker", "foundation", "tenant"]),
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
        ("component::native", BTreeSet::from(["foundation::safe_fs"])),
        (
            "component::node_agent",
            BTreeSet::from(["component::native", "foundation::safe_fs", "tenant"]),
        ),
        (
            "component::python",
            BTreeSet::from([
                "component::native",
                "component::node_agent",
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
        ("component::updates", BTreeSet::new()),
        (
            "config",
            BTreeSet::from(["agent", "application_error", "foundation", "tenant"]),
        ),
        (
            "config::application",
            BTreeSet::from([
                "config::catalog",
                "config::files",
                "foundation::safe_fs",
                "metadata",
                "tenant",
            ]),
        ),
        (
            "config::auth",
            BTreeSet::from([
                "agent",
                "config::catalog",
                "config::files",
                "config::layout",
                "foundation::safe_fs",
                "tenant",
            ]),
        ),
        (
            "config::catalog",
            BTreeSet::from([
                "config::definition",
                "config::files",
                "config::layout",
                "foundation::safe_fs",
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
                "config::catalog",
                "config::definition",
                "config::files",
                "config::layout",
                "config::visual",
                "foundation::safe_fs",
                "tenant",
            ]),
        ),
        (
            "config::files",
            BTreeSet::from(["config::layout", "foundation::safe_fs", "tenant"]),
        ),
        ("config::layout", BTreeSet::from(["tenant"])),
        ("config::native", BTreeSet::new()),
        (
            "config::visual",
            BTreeSet::from(["agent", "config::definition", "config::native"]),
        ),
        ("docker", BTreeSet::new()),
        ("docker::docker_image", BTreeSet::new()),
        ("docker::run", BTreeSet::from(["docker::supervision"])),
        ("docker::supervision", BTreeSet::new()),
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
        ("foundation", BTreeSet::new()),
        ("foundation::platform", BTreeSet::new()),
        ("foundation::safe_fs", BTreeSet::new()),
        ("foundation::sync", BTreeSet::new()),
        (
            "lib",
            BTreeSet::from(["agent", "cli", "execution", "service", "tenant"]),
        ),
        ("main", BTreeSet::new()),
        (
            "metadata",
            BTreeSet::from(["foundation::safe_fs", "tenant"]),
        ),
        ("request", BTreeSet::new()),
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
        ("request::model", BTreeSet::new()),
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
        (
            "request::sse",
            BTreeSet::from(["request::interpretation", "request::store"]),
        ),
        (
            "request::store",
            BTreeSet::from([
                "application_error",
                "foundation::safe_fs",
                "foundation::sync",
                "request::assessment",
                "request::interpretation",
                "request::model",
            ]),
        ),
        ("sandbox", BTreeSet::new()),
        (
            "sandbox::args",
            BTreeSet::from(["foundation::platform", "tenant"]),
        ),
        ("sandbox::mount", BTreeSet::from(["tenant"])),
        (
            "sandbox::spec",
            BTreeSet::from(["sandbox::args", "sandbox::mount"]),
        ),
        (
            "service",
            BTreeSet::from([
                "component",
                "docker",
                "foundation::safe_fs",
                "request",
                "tenant",
            ]),
        ),
        (
            "service::control",
            BTreeSet::from([
                "agent",
                "application_error",
                "component",
                "config",
                "request",
                "service::coordination",
                "service::operation",
                "service::state",
                "session",
                "tenant",
            ]),
        ),
        (
            "service::coordination",
            BTreeSet::from([
                "agent",
                "application_error",
                "component",
                "config",
                "docker",
                "foundation::safe_fs",
                "service::operation",
                "service::state",
                "session",
                "tenant",
            ]),
        ),
        ("service::operation", BTreeSet::from(["application_error"])),
        (
            "service::state",
            BTreeSet::from([
                "application_error",
                "component",
                "config",
                "docker",
                "request",
                "service::operation",
            ]),
        ),
        ("session", BTreeSet::new()),
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
            BTreeSet::from(["session::backend", "session::filesystem", "session::model"]),
        ),
        ("session::claude", BTreeSet::new()),
        ("session::codex", BTreeSet::new()),
        (
            "session::detail",
            BTreeSet::from([
                "application_error",
                "session::backend",
                "session::catalog",
                "session::filesystem",
                "session::model",
            ]),
        ),
        (
            "session::filesystem",
            BTreeSet::from(["foundation::safe_fs"]),
        ),
        ("session::model", BTreeSet::from(["session::filesystem"])),
        ("tenant", BTreeSet::new()),
        ("tenant::environment", BTreeSet::from(["agent"])),
        ("tenant::host", BTreeSet::from(["foundation::safe_fs"])),
        ("tenant::identity", BTreeSet::new()),
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
const INLINE_TEST_EXCEPTIONS: &[&str] = &["service/control/contract.rs"];

/// Test suites that cover no single module and so name none.
///
/// `architecture_tests.rs` checks the crate rather than a module, which is why
/// it is the one suite whose name has nothing to point at.
const UNATTACHED_TEST_SUITES: &[&str] = &["architecture_tests.rs"];

/// Every `#[cfg(test)]` item that widens a module's visible surface.
///
/// A test reaching past a facade defeats the invariant the facade exists to
/// hold, so each entry has to name a seam a test legitimately needs and cannot
/// get through the same door production code uses. The exact list makes every
/// addition or removal reviewable.
///
/// Keys are `path::item`, so a line moving does not churn the list.
const TEST_ONLY_SURFACE: &[&str] = &[
    // Nested wire types the Rust-owned contract exporter must name. `ts_rs`
    // will not export a nested type on its own.
    "component/mod.rs::LatestEntry",
    "component/mod.rs::LatestEntryState",
    "component/mod.rs::LatestResult",
    "request/mod.rs::AssessmentPrimary",
    "request/mod.rs::DiagnosticMetadata",
    "request/mod.rs::ErrorKind",
    "request/mod.rs::ErrorMetadata",
    "request/mod.rs::Outcome",
    "request/mod.rs::ProtocolDiagnostic",
    "request/mod.rs::ProtocolFamily",
    "request/mod.rs::RequestedEffective",
    "request/mod.rs::RequestedObserved",
    "request/mod.rs::ResponseModeValue",
    "request/mod.rs::SummaryRequestMetadata",
    "request/mod.rs::SummaryResponseMetadata",
    "request/mod.rs::TimingMetadata",
    "request/mod.rs::TokenUsage",
    "service/control/requests.rs::BodyQuery",
    "service/control/requests.rs::DiagnosticGroups",
    "service/control/requests.rs::EventTimingEntry",
    "service/control/requests.rs::EventTimingQuery",
    "service/control/requests.rs::EventTimingResponse",
    "service/control/requests.rs::EventTimingState",
    "service/control/requests.rs::ListQuery",
    "service/control/requests.rs::RequestDetail",
    "service/control/requests.rs::RequestList",
    "service/control/requests.rs::RequestState",
    "service/control/requests.rs::RequestSummary",
    "service/control/requests.rs::ResponseDetail",
    // The struct behind the test-facing route manifest `control_routes!` emits
    // beside the path constants, so a route declared once cannot desynchronize
    // from it. The `ENDPOINTS` const itself sits inside the macro body, which
    // this check cannot see: `syn` parses `macro_rules!` without expanding it.
    "service/control/routes.rs::EndpointDescription",
    // Docker injection: a suite substitutes a stub CLI instead of contacting a
    // daemon. `DockerCli::isolated` is the seam; the rest are its reach.
    "docker/docker_image.rs::build_image_with",
    "docker/mod.rs::build_image_with",
    "docker/mod.rs::inspect_runtime_image_with",
    "docker/mod.rs::isolated",
    "docker/run.rs::new",
    "execution/mod.rs::injected_docker",
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
    "config/editing.rs::save_config_file",
    "config/mod.rs::ConfigCatalogState",
    "config/mod.rs::PropagationEntry",
    "config/mod.rs::PropagationOutcome",
    "config/mod.rs::PropagationPreviewEntry",
    "config/mod.rs::ensure_named_config_directory",
    "config/mod.rs::read_config_file",
    "config/mod.rs::save_config_file",
    "config/mod.rs::save_config_file_with_linked",
    // Transcript fixtures and the parsed records a projection test asserts on.
    "session/detail.rs::detail_records_for_test",
    "session/filesystem.rs::test_transcript_home",
    "session/mod.rs::EvidenceEncoding",
    "session/mod.rs::SessionListRow",
    "session/mod.rs::detail_records_for_test",
    "session/model.rs::Prompt",
    // Environment-derived paths, exercised without mutating the real process.
    "tenant/host.rs::aibox_root_from",
    "tenant/host.rs::host_home_from",
    // Substitutes the Latest Release provider so an update check stays
    // socket-free.
    "service/state.rs::set_latest_provider",
];

/// The names one `#[cfg(test)]` item adds to its module's surface.
fn exported_names(item: &Item) -> Vec<String> {
    fn use_names(tree: &UseTree, names: &mut Vec<String>) {
        match tree {
            UseTree::Path(path) => use_names(&path.tree, names),
            UseTree::Name(name) => names.push(name.ident.to_string()),
            UseTree::Rename(rename) => names.push(rename.rename.to_string()),
            UseTree::Group(group) => {
                for item in &group.items {
                    use_names(item, names);
                }
            }
            UseTree::Glob(_) => names.push("*".to_string()),
        }
    }

    let mut names = Vec::new();
    match item {
        Item::Use(item) if !matches!(item.vis, syn::Visibility::Inherited) => {
            use_names(&item.tree, &mut names);
        }
        Item::Fn(item) if !matches!(item.vis, syn::Visibility::Inherited) => {
            names.push(item.sig.ident.to_string());
        }
        Item::Struct(item) if !matches!(item.vis, syn::Visibility::Inherited) => {
            names.push(item.ident.to_string());
        }
        Item::Enum(item) if !matches!(item.vis, syn::Visibility::Inherited) => {
            names.push(item.ident.to_string());
        }
        Item::Const(item) if !matches!(item.vis, syn::Visibility::Inherited) => {
            names.push(item.ident.to_string());
        }
        Item::Static(item) if !matches!(item.vis, syn::Visibility::Inherited) => {
            names.push(item.ident.to_string());
        }
        Item::Type(item) if !matches!(item.vis, syn::Visibility::Inherited) => {
            names.push(item.ident.to_string());
        }
        _ => {}
    }
    names
}

/// Collect `path::item` for every test-only item that widens a surface.
fn test_only_surface(src: &Path) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for path in rust_sources(src) {
        if is_test_source(&path) {
            continue;
        }
        let relative = path
            .strip_prefix(src)
            .expect("Rust source below src/")
            .to_string_lossy()
            .replace('\\', "/");
        let content = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read architecture source {}: {error}", path.display()));
        let syntax = syn::parse_file(&content).unwrap_or_else(|error| {
            panic!("parse architecture source {}: {error}", path.display())
        });
        let mut items: Vec<&Item> = syntax.items.iter().collect();
        while let Some(item) = items.pop() {
            // An inherent `impl` is not itself test-only, but its methods can be.
            if let Item::Impl(block) = item {
                for method in &block.items {
                    if let syn::ImplItem::Fn(method) = method
                        && is_test_only(&method.attrs)
                        && !matches!(method.vis, syn::Visibility::Inherited)
                    {
                        found.insert(format!("{relative}::{}", method.sig.ident));
                    }
                }
                continue;
            }
            if !is_test_only(item_attrs(item).unwrap_or(&[])) {
                continue;
            }
            for name in exported_names(item) {
                found.insert(format!("{relative}::{name}"));
            }
        }
    }
    found
}

/// Keep tests out of a module's public surface, one reviewable exception list.
///
/// The list is exact in both directions, so unreviewed additions and entries
/// whose seam disappeared both fail.
#[test]
fn test_only_items_stay_on_the_reviewed_surface() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let observed = test_only_surface(&src);
    let allowed: BTreeSet<String> = TEST_ONLY_SURFACE
        .iter()
        .map(|key| key.to_string())
        .collect();
    let mut violations = Vec::new();
    for added in observed.difference(&allowed) {
        violations.push(format!(
            "{added} is a #[cfg(test)] item widening its module's surface; reach it through the \
             facade production code uses, or add it to TEST_ONLY_SURFACE with the seam it serves"
        ));
    }
    for stale in allowed.difference(&observed) {
        violations.push(format!(
            "{stale} is listed in TEST_ONLY_SURFACE but no longer exists; drop the stale entry"
        ));
    }
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

/// Keep test code in `<module>_tests.rs` beside the module it covers.
#[test]
fn test_code_lives_in_external_module_test_files() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut violations = Vec::new();
    for path in rust_sources(&src) {
        if is_test_source(&path) {
            continue;
        }
        let relative = path
            .strip_prefix(&src)
            .expect("Rust source below src/")
            .to_string_lossy()
            .replace('\\', "/");
        if INLINE_TEST_EXCEPTIONS.contains(&relative.as_str()) {
            continue;
        }
        let content = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read architecture source {}: {error}", path.display()));
        let syntax = syn::parse_file(&content).unwrap_or_else(|error| {
            panic!("parse architecture source {}: {error}", path.display())
        });
        for item in &syntax.items {
            let Item::Mod(module) = item else { continue };
            if module.ident == "tests" && module.content.is_some() {
                violations.push(format!(
                    "{relative} declares an inline `mod tests`; move it to a sibling <module>_tests.rs"
                ));
            }
        }
    }
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

/// Every `<module>_tests.rs` names a module that exists.
///
/// A suite is either `<dir>/<module>_tests.rs` beside `<dir>/<module>.rs`, or
/// `<dir>/<dir>_tests.rs` for the facade in `<dir>/mod.rs`.
#[test]
fn every_test_suite_names_an_existing_module() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut violations = Vec::new();
    for path in rust_sources(&src) {
        let Some(stem) = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_suffix("_tests.rs"))
        else {
            continue;
        };
        let relative = path
            .strip_prefix(&src)
            .expect("Rust source below src/")
            .to_string_lossy()
            .replace('\\', "/");
        if UNATTACHED_TEST_SUITES.contains(&relative.as_str()) {
            continue;
        }
        let directory = path.parent().expect("test suite inside a directory");
        let names_sibling = directory.join(format!("{stem}.rs")).is_file();
        let names_own_facade = directory.file_name().is_some_and(|name| name == stem)
            && directory.join("mod.rs").is_file();
        if !names_sibling && !names_own_facade {
            violations.push(format!(
                "{relative} names no module; cover one module per suite as <module>_tests.rs or <dir>/<dir>_tests.rs"
            ));
        }
    }
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

/// Collect the crate's observed non-structural module edges.
fn observed_dependencies(
    src: &Path,
    nodes: &BTreeSet<&'static str>,
) -> BTreeMap<&'static str, BTreeSet<&'static str>> {
    let mut observed: BTreeMap<&'static str, BTreeSet<&'static str>> =
        nodes.iter().map(|node| (*node, BTreeSet::new())).collect();
    for path in rust_sources(src) {
        if is_test_source(&path) {
            continue;
        }
        let Some(node) = source_node(nodes, src, &path) else {
            continue;
        };
        let content = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read architecture source {}: {error}", path.display()));
        let syntax = syn::parse_file(&content).unwrap_or_else(|error| {
            panic!("parse architecture source {}: {error}", path.display())
        });
        let mut dependencies = CrateDependencies::new(ModuleScope {
            path: module_path(src, &path),
            children: declared_children(&syntax.items),
        });
        dependencies.visit_file(&syntax);
        for module in &dependencies.modules {
            if let Some(dependency) = governing_node(nodes, module)
                && !is_structural(node, dependency)
            {
                observed
                    .get_mut(node)
                    .expect("observed entry for every node")
                    .insert(dependency);
            }
        }
    }
    observed
}

#[test]
fn module_dependency_edges_match_the_declared_graph() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let allowed = allowed_dependencies();
    let nodes: BTreeSet<&'static str> = allowed.keys().copied().collect();
    let observed = observed_dependencies(&src, &nodes);
    let mut violations = Vec::new();
    for (node, dependencies) in &observed {
        let declared = allowed.get(node).expect("declared entry for every node");
        for added in dependencies.difference(declared) {
            violations.push(format!(
                "{node} depends on {added}; declare the edge in allowed_dependencies or remove it"
            ));
        }
        for stale in declared.difference(dependencies) {
            violations.push(format!(
                "{node} no longer depends on {stale}; drop the stale edge from allowed_dependencies"
            ));
        }
    }
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

#[test]
fn declared_module_graph_stays_acyclic() {
    let allowed = allowed_dependencies();
    let mut visiting = BTreeSet::new();
    let mut settled = BTreeSet::new();
    let mut stack = Vec::new();
    let mut cycles = Vec::new();
    for node in allowed.keys() {
        walk_for_cycles(
            node,
            &allowed,
            &mut visiting,
            &mut settled,
            &mut stack,
            &mut cycles,
        );
    }
    assert!(cycles.is_empty(), "{}", cycles.join("\n"));
}

/// Every module in the crate as its own node, with the edges it reaches.
///
/// The declared table stops at depth two because that is where an edge states a
/// dependency direction between owners. Acyclicity is different: it holds at
/// every depth or not at all, and it needs no table, so this graph is derived
/// rather than declared. `nearest_node` is the full-depth analogue of
/// [`governing_node`] — the longest existing module prefix instead of the first
/// one at depth two.
fn module_graph(src: &Path) -> BTreeMap<String, BTreeSet<String>> {
    fn nearest_node<'a>(nodes: &'a BTreeSet<String>, path: &[String]) -> Option<&'a String> {
        (1..=path.len())
            .rev()
            .find_map(|depth| nodes.get(&path[..depth].join("::")))
    }

    let sources: Vec<PathBuf> = rust_sources(src)
        .into_iter()
        .filter(|path| !is_test_source(path))
        .collect();
    let nodes: BTreeSet<String> = sources
        .iter()
        .map(|path| module_path(src, path).join("::"))
        .filter(|node| !node.is_empty())
        .collect();
    let mut graph: BTreeMap<String, BTreeSet<String>> = nodes
        .iter()
        .map(|node| (node.clone(), BTreeSet::new()))
        .collect();
    for path in &sources {
        let module = module_path(src, path);
        // `lib.rs` and `main.rs` are crate roots, not modules: nothing can reach
        // back into them, so they cannot sit on a cycle.
        let Some(node) = nearest_node(&nodes, &module).cloned() else {
            continue;
        };
        let content = fs::read_to_string(path)
            .unwrap_or_else(|error| panic!("read architecture source {}: {error}", path.display()));
        let syntax = syn::parse_file(&content).unwrap_or_else(|error| {
            panic!("parse architecture source {}: {error}", path.display())
        });
        let mut dependencies = CrateDependencies::new(ModuleScope {
            path: module,
            children: declared_children(&syntax.items),
        });
        dependencies.visit_file(&syntax);
        for reached in &dependencies.modules {
            if let Some(dependency) = nearest_node(&nodes, reached)
                && !is_structural(&node, dependency)
            {
                graph
                    .get_mut(&node)
                    .expect("graph entry for every node")
                    .insert(dependency.clone());
            }
        }
    }
    graph
}

/// No module cycle anywhere in the crate, at any depth.
///
/// The declared table collapses `request::proxy::attempt` and its siblings into
/// one node, so a derived full-depth graph checks cycles below that horizon.
/// Shared sibling dependencies belong in a module that owns the shared concept.
///
/// This check derives its own graph instead of extending
/// [`allowed_dependencies`] to every file: acyclicity is a property, and a
/// hand-maintained table at file level would cost a reviewed edit per module
/// split without saying anything the property does not already say.
#[test]
fn module_graph_stays_acyclic_at_every_depth() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let graph = module_graph(&src);
    let borrowed: BTreeMap<&str, BTreeSet<&str>> = graph
        .iter()
        .map(|(node, edges)| (node.as_str(), edges.iter().map(String::as_str).collect()))
        .collect();
    let mut visiting = BTreeSet::new();
    let mut settled = BTreeSet::new();
    let mut stack = Vec::new();
    let mut cycles = Vec::new();
    for node in borrowed.keys() {
        walk_for_cycles(
            node,
            &borrowed,
            &mut visiting,
            &mut settled,
            &mut stack,
            &mut cycles,
        );
    }
    assert!(cycles.is_empty(), "{}", cycles.join("\n"));
}

/// Depth-first search reporting the first cycle reached through each node.
fn walk_for_cycles<'a>(
    node: &'a str,
    allowed: &BTreeMap<&'a str, BTreeSet<&'a str>>,
    visiting: &mut BTreeSet<&'a str>,
    settled: &mut BTreeSet<&'a str>,
    stack: &mut Vec<&'a str>,
    cycles: &mut Vec<String>,
) {
    if settled.contains(node) {
        return;
    }
    if !visiting.insert(node) {
        let start = stack.iter().position(|entry| *entry == node).unwrap_or(0);
        let mut path: Vec<&str> = stack[start..].to_vec();
        path.push(node);
        cycles.push(format!("module cycle: {}", path.join(" -> ")));
        return;
    }
    stack.push(node);
    for dependency in allowed.get(node).into_iter().flatten() {
        walk_for_cycles(dependency, allowed, visiting, settled, stack, cycles);
    }
    stack.pop();
    visiting.remove(node);
    settled.insert(node);
}
