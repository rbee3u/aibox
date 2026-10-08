use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use syn::visit::{self, Visit};
use syn::{Attribute, Item, ItemUse, UseTree};

pub(super) fn rust_sources(root: &Path) -> Vec<PathBuf> {
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

pub(super) fn is_test_source(path: &Path) -> bool {
    path.components().any(|part| {
        matches!(
            part.as_os_str().to_str(),
            Some("tests" | "architecture_tests")
        )
    }) || path
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
pub(super) struct ModuleScope {
    pub(super) path: Vec<String>,
    pub(super) children: BTreeSet<String>,
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

pub(super) struct CrateDependencies {
    scope: ModuleScope,
    pub(super) modules: BTreeSet<Vec<String>>,
    pub(super) written_paths: BTreeSet<Vec<String>>,
}

impl CrateDependencies {
    pub(super) fn new(scope: ModuleScope) -> Self {
        Self {
            scope,
            modules: BTreeSet::new(),
            written_paths: BTreeSet::new(),
        }
    }

    fn record(&mut self, segments: &[String]) {
        self.written_paths.insert(segments.to_vec());
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
pub(super) fn declared_children(items: &[Item]) -> BTreeSet<String> {
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
pub(super) fn module_path(src: &Path, path: &Path) -> Vec<String> {
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
pub(super) fn test_only_surface(src: &Path) -> BTreeSet<String> {
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

/// Collect the crate's observed non-structural module edges.
pub(super) fn observed_dependencies(
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

/// Every source owner that must participate in the depth-two policy.
pub(super) fn source_nodes(src: &Path) -> BTreeSet<String> {
    rust_sources(src)
        .into_iter()
        .filter(|path| !is_test_source(path))
        .map(|path| {
            let module = module_path(src, &path);
            if module.is_empty() {
                path.file_stem().unwrap().to_string_lossy().into_owned()
            } else {
                module[..module.len().min(2)].join("::")
            }
        })
        .collect()
}

#[cfg(test)]
#[path = "inspection_tests.rs"]
mod tests;

/// Every module in the crate as its own node, with the edges it reaches.
///
/// The declared table stops at depth two because that is where an edge states a
/// dependency direction between owners. Acyclicity is different: it holds at
/// every depth or not at all, and it needs no table, so this graph is derived
/// rather than declared. `nearest_node` is the full-depth analogue of
/// [`governing_node`] — the longest existing module prefix instead of the first
/// one at depth two.
pub(super) fn module_graph(src: &Path) -> BTreeMap<String, BTreeSet<String>> {
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

/// Depth-first search reporting the first cycle reached through each node.
pub(super) fn walk_for_cycles<'a>(
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
