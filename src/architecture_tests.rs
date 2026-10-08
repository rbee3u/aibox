//! Crate-wide architecture policy and its source-based enforcement.

mod inspection;
mod policy;

use inspection::*;
use policy::*;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use syn::Item;
use syn::visit::Visit;

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

#[test]
fn module_dependency_edges_match_the_declared_graph() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let allowed = allowed_dependencies();
    let nodes: BTreeSet<&'static str> = allowed.keys().copied().collect();
    assert_eq!(
        source_nodes(&src),
        nodes.iter().map(|node| (*node).to_string()).collect(),
        "every source owner must appear in allowed_dependencies, including owners without edges"
    );
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

/// These ownership boundaries stay forbidden even if an edge is added to the table.
#[test]
fn management_and_sse_keep_their_transport_and_storage_boundaries() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut violations = Vec::new();
    for path in rust_sources(&src) {
        if is_test_source(&path) {
            continue;
        }
        let module = module_path(&src, &path);
        let management = module.first().is_some_and(|name| name == "management");
        let sse = module == ["request", "sse"];
        if !management && !sse {
            continue;
        }
        let syntax = syn::parse_file(&fs::read_to_string(&path).unwrap()).unwrap();
        let mut dependencies = CrateDependencies::new(ModuleScope {
            path: module,
            children: declared_children(&syntax.items),
        });
        dependencies.visit_file(&syntax);
        for reached in &dependencies.modules {
            if (management && reached.first().is_some_and(|name| name == "service"))
                || (sse && reached.starts_with(&["request".to_string(), "store".to_string()]))
            {
                violations.push(format!("{} reaches {}", path.display(), reached.join("::")));
            }
        }
        for written in &dependencies.written_paths {
            let first = written.first().map(String::as_str);
            if (management && matches!(first, Some("axum" | "clap")))
                || (sse
                    && written.first().is_some_and(|name| name == "std")
                    && written
                        .get(1)
                        .is_some_and(|name| matches!(name.as_str(), "fs" | "io")))
            {
                violations.push(format!("{} imports {}", path.display(), written.join("::")));
            }
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
