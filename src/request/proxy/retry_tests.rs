use super::*;

#[test]
fn missing_file_disables_retries_and_rules_match_base_path_segments() {
    let root = tempfile::tempdir().unwrap();
    assert!(
        !RetryRules::load(root.path())
            .unwrap()
            .matches(&Url::parse("https://relay.example/v1/responses").unwrap())
    );
    std::fs::write(
        root.path().join("retry_urls.txt"),
        "# relay\n\nhttps://relay.example/v1\nhttp://localhost:8080/api/\n",
    )
    .unwrap();
    let rules = RetryRules::load(root.path()).unwrap();
    for target in [
        "https://relay.example/v1",
        "https://relay.example:443/v1/responses?stream=true",
        "http://localhost:8080/api/models",
    ] {
        assert!(rules.matches(&Url::parse(target).unwrap()), "{target}");
    }
    for target in [
        "http://relay.example/v1/responses",
        "https://relay.example:8443/v1/responses",
        "https://relay.example/v10/responses",
        "https://other.example/v1/responses",
    ] {
        assert!(!rules.matches(&Url::parse(target).unwrap()), "{target}");
    }
}

#[test]
fn malformed_rule_fails_with_line_number() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("retry_urls.txt"),
        "# relay\nhttps://relay.example/v1?key=secret\n",
    )
    .unwrap();
    let error = RetryRules::load(root.path()).err().unwrap().to_string();
    assert!(error.contains(":2"), "{error}");
    assert!(
        error.contains("without credentials, query, or fragment"),
        "{error}"
    );
}

#[cfg(unix)]
#[test]
fn symlinked_rule_file_is_rejected() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("target"), "https://relay.example/v1\n").unwrap();
    std::os::unix::fs::symlink("target", root.path().join("retry_urls.txt")).unwrap();
    assert!(RetryRules::load(root.path()).is_err());
}
