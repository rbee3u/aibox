use super::*;

#[test]
fn discovers_unregistered_owners_without_including_test_infrastructure() {
    let root = tempfile::tempdir().unwrap();
    for (file, source) in [
        ("lib.rs", "mod old; mod new;"),
        ("old.rs", ""),
        ("new/mod.rs", "mod child;"),
        ("new/child.rs", ""),
        ("new/child/deep.rs", ""),
        ("new/child_tests.rs", ""),
        ("architecture_tests/inspection.rs", ""),
    ] {
        let path = root.path().join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }
    assert_eq!(
        source_nodes(root.path()),
        ["lib", "old", "new", "new::child"]
            .map(str::to_string)
            .into()
    );
}

#[test]
fn detects_actual_edges_and_cycles_between_policy_owners() {
    let root = tempfile::tempdir().unwrap();
    for (file, source) in [
        ("lib.rs", "mod feature;"),
        ("feature/mod.rs", "mod left; mod right;"),
        ("feature/left.rs", "use super::right::Value;"),
        ("feature/right.rs", "use crate::feature::left::Value;"),
    ] {
        let path = root.path().join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }
    let nodes = ["lib", "feature", "feature::left", "feature::right"].into();
    let edges = observed_dependencies(root.path(), &nodes);
    assert_eq!(edges["feature::left"], ["feature::right"].into());
    assert_eq!(edges["feature::right"], ["feature::left"].into());
    let mut cycles = Vec::new();
    walk_for_cycles(
        "feature::left",
        &edges,
        &mut BTreeSet::new(),
        &mut BTreeSet::new(),
        &mut Vec::new(),
        &mut cycles,
    );
    assert_eq!(
        cycles,
        ["module cycle: feature::left -> feature::right -> feature::left"]
    );
    assert_eq!(
        module_graph(root.path())["feature::left"],
        ["feature::right".to_string()].into()
    );
}

#[test]
fn detects_cycles_beyond_the_declared_policy_depth() {
    let root = tempfile::tempdir().unwrap();
    for (file, source) in [
        ("lib.rs", "mod feature;"),
        ("feature/mod.rs", "mod worker;"),
        ("feature/worker/mod.rs", "mod left; mod right;"),
        ("feature/worker/left.rs", "use super::right::Value;"),
        ("feature/worker/right.rs", "use super::left::Value;"),
    ] {
        let path = root.path().join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }
    let graph = module_graph(root.path());
    let borrowed = graph
        .iter()
        .map(|(node, edges)| (node.as_str(), edges.iter().map(String::as_str).collect()))
        .collect();
    let mut cycles = Vec::new();
    walk_for_cycles(
        "feature::worker::left",
        &borrowed,
        &mut BTreeSet::new(),
        &mut BTreeSet::new(),
        &mut Vec::new(),
        &mut cycles,
    );
    assert_eq!(
        cycles,
        ["module cycle: feature::worker::left -> feature::worker::right -> feature::worker::left"]
    );
}
