use super::*;

#[test]
fn stable_versions_require_three_numeric_parts() {
    assert_eq!(validate_stable_version("24.19.0").unwrap(), "24.19.0");
    assert!(validate_stable_version("24.19").is_err());
    assert!(validate_stable_version("24.19.0-rc1").is_err());
}

#[test]
fn numeric_version_ordering_does_not_use_lexical_order() {
    let parse = |value: &str| {
        value
            .split('.')
            .map(|part| part.parse::<u32>().unwrap())
            .collect::<Vec<_>>()
    };
    assert!(parse("1.10.0") > parse("1.9.0"));
}

#[test]
fn official_source_fixtures_reject_prereleases_and_normalize_prefixes() {
    let node = serde_json::json!([
        {"version": "v25.0.0-rc.1"},
        {"version": "v24.19.0"}
    ]);
    assert_eq!(parse_node_releases(&node).unwrap(), "24.19.0");

    let go = serde_json::json!([
        {"version": "go1.27rc1", "stable": false},
        {"version": "go1.26.1", "stable": true}
    ]);
    assert_eq!(parse_go_releases(go).unwrap(), "1.26.1");

    let rust = r#"
        [pkg.rust]
        version = "1.97.0 (abcdef 2026-08-01)"
    "#;
    assert_eq!(parse_rust_channel(rust).unwrap(), "1.97.0");

    assert_eq!(
        parse_codex_release(&serde_json::json!({"tag_name": "rust-v0.149.1"})).unwrap(),
        "0.149.1"
    );
    assert_eq!(
        parse_claude_release(&serde_json::json!({"version": "2.1.245"})).unwrap(),
        "2.1.245"
    );
    let python = serde_json::json!({
        "assets": [
            {"name": "cpython-3.14.7+20260814-aarch64-unknown-linux-gnu-install_only.tar.gz"},
            {"name": "cpython-3.13.15+20260814-x86_64-unknown-linux-gnu-install_only.tar.gz"},
            {"name": "cpython-3.15.0rc1+20260814-x86_64-unknown-linux-gnu-install_only.tar.gz"}
        ]
    });
    assert_eq!(parse_python_release(&python).unwrap(), "3.14.7");
}

#[test]
fn malformed_official_source_fixtures_are_unavailable() {
    assert!(parse_node_releases(&serde_json::json!({})).is_err());
    assert!(parse_go_releases(serde_json::json!([])).is_err());
    assert!(parse_rust_channel("[pkg]").is_err());
    assert!(parse_codex_release(&serde_json::json!({"tag_name": "v1.2.3"})).is_err());
    assert!(parse_claude_release(&serde_json::json!({"version": "next"})).is_err());
    assert!(parse_python_release(&serde_json::json!({"assets": []})).is_err());
}

struct PendingProvider;

impl LatestProvider for PendingProvider {
    fn fetch(&self, _kind: ComponentKind) -> BoxFuture<'static, LatestResult> {
        Box::pin(std::future::pending())
    }
}

#[tokio::test]
async fn timed_out_sources_become_independent_unavailable_entries() {
    let snapshot =
        check_snapshot_with_timeout(Arc::new(PendingProvider), Duration::from_millis(1)).await;
    assert_eq!(snapshot.entries.len(), VERSIONED_COMPONENTS.len());
    assert!(snapshot.entries.iter().all(|entry| {
        entry.state == LatestEntryState::Unavailable
            && entry.error.as_deref() == Some("release source timed out")
    }));
}
