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

#[derive(Default)]
struct CrateDependencies {
    modules: BTreeSet<String>,
}

impl CrateDependencies {
    fn record_use_tree(&mut self, tree: &UseTree, inside_crate: bool) {
        match tree {
            UseTree::Path(path) if inside_crate => {
                self.modules.insert(path.ident.to_string());
            }
            UseTree::Name(name) if inside_crate => {
                self.modules.insert(name.ident.to_string());
            }
            UseTree::Rename(rename) if inside_crate => {
                self.modules.insert(rename.ident.to_string());
            }
            UseTree::Path(path) => {
                self.record_use_tree(&path.tree, path.ident == "crate");
            }
            UseTree::Group(group) => {
                for item in &group.items {
                    self.record_use_tree(item, inside_crate);
                }
            }
            UseTree::Glob(_) | UseTree::Name(_) | UseTree::Rename(_) => {}
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
        self.record_use_tree(&item.tree, false);
        visit::visit_item_use(self, item);
    }

    fn visit_path(&mut self, path: &'ast syn::Path) {
        let mut segments = path.segments.iter();
        if segments
            .next()
            .is_some_and(|segment| segment.ident == "crate")
            && let Some(module) = segments.next()
        {
            self.modules.insert(module.ident.to_string());
        }
        visit::visit_path(self, path);
    }
}

fn source_module(src: &Path, path: &Path) -> String {
    let relative = path.strip_prefix(src).expect("Rust source below src/");
    relative
        .components()
        .next()
        .expect("Rust source has a module")
        .as_os_str()
        .to_string_lossy()
        .trim_end_matches(".rs")
        .to_string()
}

fn allowed_dependencies() -> BTreeMap<&'static str, BTreeSet<&'static str>> {
    BTreeMap::from([
        ("agent", BTreeSet::new()),
        ("application_error", BTreeSet::new()),
        ("cli", BTreeSet::from(["agent", "tenant"])),
        (
            "component",
            BTreeSet::from(["agent", "docker", "foundation", "sandbox", "tenant"]),
        ),
        (
            "config",
            BTreeSet::from([
                "agent",
                "application_error",
                "foundation",
                "metadata",
                "tenant",
            ]),
        ),
        ("docker", BTreeSet::from(["foundation"])),
        (
            "execution",
            BTreeSet::from(["agent", "component", "docker", "sandbox", "tenant"]),
        ),
        ("foundation", BTreeSet::new()),
        ("metadata", BTreeSet::from(["foundation", "tenant"])),
        (
            "request",
            BTreeSet::from(["application_error", "foundation"]),
        ),
        ("sandbox", BTreeSet::from(["agent", "foundation", "tenant"])),
        (
            "service",
            BTreeSet::from([
                "agent",
                "application_error",
                "component",
                "config",
                "docker",
                "foundation",
                "request",
                "session",
                "tenant",
            ]),
        ),
        (
            "session",
            BTreeSet::from(["agent", "application_error", "foundation"]),
        ),
        ("tenant", BTreeSet::from(["agent", "foundation"])),
    ])
}

/// Sources allowed to keep an inline `mod tests`.
///
/// `service/control/contract.rs` is test-only in its entirety: the module
/// declaration is the file's whole purpose, so externalizing it would leave a
/// one-line shell.
const INLINE_TEST_EXCEPTIONS: &[&str] = &["service/control/contract.rs"];

/// Keep test code in `<module>_tests.rs` beside the module it covers.
///
/// Mixed placement made large suites easy to hide: four modules had grown to
/// more test lines than production lines, which reading the file top to bottom
/// did not reveal.
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

#[test]
fn stable_domain_dependency_edges_remain_one_directional() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let allowed = allowed_dependencies();
    let mut violations = Vec::new();
    for path in rust_sources(&src) {
        if is_test_source(&path) {
            continue;
        }
        let module = source_module(&src, &path);
        let Some(module_allowed) = allowed.get(module.as_str()) else {
            continue;
        };
        let content = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read architecture source {}: {error}", path.display()));
        let syntax = syn::parse_file(&content).unwrap_or_else(|error| {
            panic!("parse architecture source {}: {error}", path.display())
        });
        let mut dependencies = CrateDependencies::default();
        dependencies.visit_file(&syntax);
        for dependency in dependencies.modules {
            if dependency != module && !module_allowed.contains(dependency.as_str()) {
                violations.push(format!(
                    "{} ({module}) must not depend on crate::{dependency}",
                    path.display()
                ));
            }
        }
    }
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}
