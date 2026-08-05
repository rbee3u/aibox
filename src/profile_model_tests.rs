use super::*;
use crate::testutil::only;
use serde_json::json;

fn profile(agent: AgentKind, main: &str, auth: &str, tombstones: &[&str]) -> ProfileDefinition {
    let metadata = serde_json::json!({"tombstones": tombstones}).to_string();
    ProfileDefinition::parse(agent, main, auth, Some(&metadata)).unwrap()
}

#[test]
fn pointers_round_trip_escaped_segments() {
    let path = Pointer::parse("/config/a~1b/~0value").unwrap();
    assert_eq!(path.segments(), &["config", "a/b", "~value"]);
    assert_eq!(path.to_string(), "/config/a~1b/~0value");
}

#[test]
fn pointer_validation_rejects_malformed_or_out_of_domain_paths() {
    for path in [
        "",
        "config/model",
        "/",
        "/other/model",
        "/config/~",
        "/config/~2",
    ] {
        assert!(Pointer::parse(path).is_err(), "{path:?} should be rejected");
    }
}

#[test]
fn terminal_pointer_display_escapes_control_characters() {
    let path = Pointer::from_segments(vec!["config".to_string(), "line\n\u{1b}[31m".to_string()]);

    assert_eq!(path.to_string(), "/config/line\n\u{1b}[31m");
    assert_eq!(path.display_for_terminal(), "/config/line\\n\\u{1b}[31m");
}

#[test]
fn claude_auth_is_separate_and_materializes_into_env() {
    let definition = profile(
        AgentKind::Claude,
        r#"{"env":{"ANTHROPIC_BASE_URL":"https://api.example"}}"#,
        r#"{"ANTHROPIC_AUTH_TOKEN":"secret"}"#,
        &[],
    );
    let base = AgentKind::Claude
        .normalize_config_files("{}", None, &definition.auth_keys())
        .unwrap();
    let effective = materialize(&base, &definition).unwrap();
    let (settings, auth) = AgentKind::Claude.render_config_files(&effective).unwrap();
    assert!(auth.is_none());
    let settings: Value = serde_json::from_str(&settings).unwrap();
    assert_eq!(settings["env"]["ANTHROPIC_AUTH_TOKEN"], "secret");
    assert_eq!(settings["env"]["ANTHROPIC_BASE_URL"], "https://api.example");
}

#[test]
fn profile_tombstones_reject_ambiguous_or_partial_ownership() {
    let cases = [
        (
            AgentKind::Claude,
            r#"{"model":"profile"}"#,
            "{}",
            r#"{"tombstones":["/config/model"]}"#,
            "overlaps a declared value",
        ),
        (
            AgentKind::Claude,
            "{}",
            "{}",
            r#"{"tombstones":["/config/model","/config/model"]}"#,
            "duplicate Agent Profile tombstone",
        ),
        (
            AgentKind::Codex,
            "",
            "{}",
            r#"{"tombstones":["/auth/token"]}"#,
            "whole-file at /auth",
        ),
    ];

    for (agent, main, auth, metadata, expected) in cases {
        let error = ProfileDefinition::parse(agent, main, auth, Some(metadata))
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn claude_credentials_must_be_strings_and_disjoint_from_settings_env() {
    let non_string = ProfileDefinition::parse(
        AgentKind::Claude,
        "{}",
        r#"{"ANTHROPIC_AUTH_TOKEN":42}"#,
        None,
    )
    .unwrap_err()
    .to_string();
    assert!(
        non_string.contains("object of string values"),
        "{non_string}"
    );

    let duplicate = ProfileDefinition::parse(
        AgentKind::Claude,
        r#"{"env":{"ANTHROPIC_AUTH_TOKEN":"inline"}}"#,
        r#"{"ANTHROPIC_AUTH_TOKEN":"secret"}"#,
        None,
    )
    .unwrap_err()
    .to_string();
    assert!(duplicate.contains("declared in both"), "{duplicate}");
}

#[test]
fn claude_effective_round_trip_extracts_only_profile_owned_credentials() {
    let keys = BTreeSet::from(["ANTHROPIC_AUTH_TOKEN".to_string()]);
    let effective = AgentKind::Claude
        .normalize_config_files(
            r#"{"env":{"ANTHROPIC_AUTH_TOKEN":"secret","KEEP":"value"},"theme":"dark"}"#,
            None,
            &keys,
        )
        .unwrap();

    assert_eq!(effective["auth"], json!({"ANTHROPIC_AUTH_TOKEN": "secret"}));
    assert_eq!(effective["config"]["env"], json!({"KEEP": "value"}));
    let (settings, auth) = AgentKind::Claude.render_config_files(&effective).unwrap();
    assert!(auth.is_none());
    assert_eq!(
        serde_json::from_str::<Value>(&settings).unwrap(),
        json!({
            "env": {
                "ANTHROPIC_AUTH_TOKEN": "secret",
                "KEEP": "value"
            },
            "theme": "dark"
        })
    );
}

#[test]
fn copying_effective_paths_preserves_siblings_and_prunes_absent_ancestors() {
    let status_line = Pointer::parse("/config/tui/status_line").unwrap();
    let source = json!({
        "config": {"tui": {"status_line": ["model"], "use_colors": true}},
        "auth": {}
    });
    let mut target = json!({
        "config": {"tui": {"status_line": ["old"], "use_colors": false}, "keep": 1},
        "auth": {}
    });

    copy_effective_paths(&source, &mut target, std::slice::from_ref(&status_line)).unwrap();
    assert_eq!(target["config"]["tui"]["status_line"], json!(["model"]));
    assert_eq!(target["config"]["tui"]["use_colors"], false);
    assert_eq!(target["config"]["keep"], 1);

    let source = json!({"config": {"keep": true}, "auth": {}});
    let mut target = json!({
        "config": {"tui": {"status_line": ["old"]}, "keep": false},
        "auth": {}
    });
    copy_effective_paths(&source, &mut target, &[status_line]).unwrap();
    assert!(target["config"].get("tui").is_none());
    assert_eq!(target["config"]["keep"], false);
}

#[test]
fn materialization_rejects_overwriting_an_unowned_scalar_ancestor() {
    let definition = profile(
        AgentKind::Codex,
        "[tui]\nstatus_line = [\"model\"]\n",
        "{}",
        &[],
    );
    let base = AgentKind::Codex
        .normalize_config_files("tui = \"keep\"\n", Some(""), &BTreeSet::new())
        .unwrap();

    let error = materialize(&base, &definition).unwrap_err().to_string();
    assert!(error.contains("/config/tui"), "{error}");
    assert!(error.contains("unowned non-object"), "{error}");
}

#[test]
fn working_deletion_becomes_a_tombstone() {
    let applied = profile(AgentKind::Claude, r#"{"model":"old"}"#, "{}", &[]);
    let keys = BTreeSet::new();
    let base = AgentKind::Claude
        .normalize_config_files(r#"{"model":"base","keep":true}"#, None, &keys)
        .unwrap();
    let expected = materialize(&base, &applied).unwrap();
    let working = AgentKind::Claude
        .normalize_config_files(r#"{"keep":true}"#, None, &keys)
        .unwrap();
    let working = working_definition(AgentKind::Claude, &applied, &expected, &working).unwrap();
    let (_, _, metadata) = working.render(AgentKind::Claude).unwrap();
    assert!(metadata.contains("/config/model"));
}

#[test]
fn codex_working_auth_changes_keep_whole_file_ownership() {
    let applied = profile(AgentKind::Codex, "", r#"{"token":"applied"}"#, &[]);
    let base = AgentKind::Codex
        .normalize_config_files("", Some(""), &BTreeSet::new())
        .unwrap();
    let expected = materialize(&base, &applied).unwrap();
    let working_tree = AgentKind::Codex
        .normalize_config_files(
            "",
            Some(r#"{"token":"working","account":"new"}"#),
            &BTreeSet::new(),
        )
        .unwrap();

    let working = working_definition(AgentKind::Codex, &applied, &expected, &working_tree).unwrap();
    let entries = diff(&applied, &working);
    assert_eq!(only(&entries).path.to_string(), "/auth");
    let (_, auth, metadata) = working.render(AgentKind::Codex).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&auth).unwrap(),
        json!({"account": "new", "token": "working"})
    );
    assert!(!metadata.contains("/auth"));
}

#[test]
fn codex_empty_working_auth_becomes_a_whole_file_tombstone() {
    let applied = profile(AgentKind::Codex, "", r#"{"token":"applied"}"#, &[]);
    let base = AgentKind::Codex
        .normalize_config_files("", Some(""), &BTreeSet::new())
        .unwrap();
    let expected = materialize(&base, &applied).unwrap();
    let working_tree = AgentKind::Codex
        .normalize_config_files("", Some(""), &BTreeSet::new())
        .unwrap();

    let working = working_definition(AgentKind::Codex, &applied, &expected, &working_tree).unwrap();
    let entries = diff(&applied, &working);
    assert_eq!(only(&entries).path.to_string(), "/auth");
    let (_, auth, metadata) = working.render(AgentKind::Codex).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&auth).unwrap(), json!({}));
    assert!(metadata.contains("/auth"));

    let effective = materialize(&base, &working).unwrap();
    assert_eq!(effective["auth"], json!({}));
}

#[test]
fn growing_an_owned_empty_object_uses_the_canonical_structural_form() {
    let applied = profile(AgentKind::Claude, r#"{"service":{}}"#, "{}", &[]);
    let base = AgentKind::Claude
        .normalize_config_files("{}", None, &BTreeSet::new())
        .unwrap();
    let expected = materialize(&base, &applied).unwrap();
    let working_tree = AgentKind::Claude
        .normalize_config_files(
            r#"{"service":{"url":"https://example.com"}}"#,
            None,
            &BTreeSet::new(),
        )
        .unwrap();

    let working =
        working_definition(AgentKind::Claude, &applied, &expected, &working_tree).unwrap();
    let (main, auth, metadata) = working.render(AgentKind::Claude).unwrap();
    let reparsed =
        ProfileDefinition::parse(AgentKind::Claude, &main, &auth, Some(&metadata)).unwrap();
    assert_eq!(working, reparsed);
}

#[test]
fn adopting_an_object_replacement_uses_the_canonical_structural_form() {
    let applied = ProfileDefinition::empty();
    let base = AgentKind::Claude
        .normalize_config_files(r#"{"service":"base"}"#, None, &BTreeSet::new())
        .unwrap();
    let working_tree = AgentKind::Claude
        .normalize_config_files(
            r#"{"service":{"url":"https://example.com"}}"#,
            None,
            &BTreeSet::new(),
        )
        .unwrap();

    let working = working_definition(AgentKind::Claude, &applied, &base, &working_tree).unwrap();
    let (main, auth, metadata) = working.render(AgentKind::Claude).unwrap();
    let reparsed =
        ProfileDefinition::parse(AgentKind::Claude, &main, &auth, Some(&metadata)).unwrap();
    assert_eq!(working, reparsed);
}

#[test]
fn three_way_merge_auto_merges_non_overlapping_changes() {
    let applied = profile(AgentKind::Claude, r#"{"a":1,"b":1}"#, "{}", &[]);
    let working = profile(AgentKind::Claude, r#"{"a":2,"b":1}"#, "{}", &[]);
    let source = profile(AgentKind::Claude, r#"{"a":1,"b":2}"#, "{}", &[]);
    let result = reconcile(&applied, &working, &source, &BTreeMap::new()).unwrap();
    assert_eq!(result.changes.len(), 2);
    assert_eq!(result.changes[0].class, ChangeClass::WorkingOnly);
    assert_eq!(result.changes[1].class, ChangeClass::SourceOnly);
    let (main, _, _) = result.merged.render(AgentKind::Claude).unwrap();
    let main: Value = serde_json::from_str(&main).unwrap();
    assert_eq!(main, json!({"a": 2, "b": 2}));
}

#[test]
fn identical_changes_on_both_sides_are_classified_and_applied_once() {
    let applied = profile(AgentKind::Claude, r#"{"model":"old"}"#, "{}", &[]);
    let changed = profile(AgentKind::Claude, r#"{"model":"new"}"#, "{}", &[]);

    let result = reconcile(&applied, &changed, &changed, &BTreeMap::new()).unwrap();

    let change = only(&result.changes);
    assert_eq!(change.path.to_string(), "/config/model");
    assert_eq!(change.class, ChangeClass::BothSame);
    let (main, _, _) = result.merged.render(AgentKind::Claude).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&main).unwrap(),
        json!({"model": "new"})
    );
}

#[test]
fn removing_the_last_source_value_drops_structural_ownership() {
    let applied = profile(AgentKind::Claude, r#"{"model":"profile"}"#, "{}", &[]);
    let source = ProfileDefinition::empty();

    let result = reconcile(&applied, &applied, &source, &BTreeMap::new()).unwrap();

    let change = only(&result.changes);
    assert_eq!(change.class, ChangeClass::SourceOnly);
    assert_eq!(change.path.to_string(), "/config/model");
    assert!(!result.merged.owns_domain("config"));
    let (main, auth, metadata) = result.merged.render(AgentKind::Claude).unwrap();
    let reparsed =
        ProfileDefinition::parse(AgentKind::Claude, &main, &auth, Some(&metadata)).unwrap();
    assert_eq!(result.merged, reparsed);
}

#[test]
fn one_sided_top_level_addition_is_classified_at_its_real_path() {
    let applied = profile(AgentKind::Claude, r#"{"model":"a"}"#, "{}", &[]);
    let working = profile(
        AgentKind::Claude,
        r#"{"model":"a","working":true}"#,
        "{}",
        &[],
    );
    let result = reconcile(&applied, &working, &applied, &BTreeMap::new()).unwrap();

    let change = only(&result.changes);
    assert_eq!(change.class, ChangeClass::WorkingOnly);
    assert_eq!(change.path.to_string(), "/config/working");
}

#[test]
fn divergent_scalar_change_is_an_explicit_conflict() {
    let applied = profile(AgentKind::Claude, r#"{"model":"a"}"#, "{}", &[]);
    let working = profile(AgentKind::Claude, r#"{"model":"working"}"#, "{}", &[]);
    let source = profile(AgentKind::Claude, r#"{"model":"source"}"#, "{}", &[]);
    let unresolved = reconcile(&applied, &working, &source, &BTreeMap::new()).unwrap();
    let change = only(&unresolved.changes);
    assert_eq!(change.class, ChangeClass::Conflict);
    assert_eq!(change.path.to_string(), "/config/model");

    let mut choices = BTreeMap::new();
    choices.insert(
        Pointer::parse("/config/model").unwrap(),
        ConflictChoice::Config,
    );
    let resolved = reconcile(&applied, &working, &source, &choices).unwrap();
    let (main, _, _) = resolved.merged.render(AgentKind::Claude).unwrap();
    assert!(main.contains("working"));
}

#[test]
fn deletion_and_modification_conflict_preserves_the_selected_semantics() {
    let applied = profile(AgentKind::Claude, r#"{"model":"applied"}"#, "{}", &[]);
    let working = profile(AgentKind::Claude, "{}", "{}", &["/config/model"]);
    let source = profile(AgentKind::Claude, r#"{"model":"source"}"#, "{}", &[]);

    let unresolved = reconcile(&applied, &working, &source, &BTreeMap::new()).unwrap();
    let change = only(&unresolved.changes);
    assert_eq!(change.path.to_string(), "/config/model");
    assert_eq!(change.class, ChangeClass::Conflict);

    let path = Pointer::parse("/config/model").unwrap();
    let base = AgentKind::Claude
        .normalize_config_files(r#"{"model":"base","keep":true}"#, None, &BTreeSet::new())
        .unwrap();
    for (choice, expected_model) in [
        (ConflictChoice::Config, None),
        (ConflictChoice::Profile, Some("source")),
    ] {
        let resolutions = BTreeMap::from([(path.clone(), choice)]);
        let resolved = reconcile(&applied, &working, &source, &resolutions).unwrap();
        let effective = materialize(&base, &resolved.merged).unwrap();
        assert_eq!(
            effective["config"].get("model").and_then(Value::as_str),
            expected_model
        );
        assert_eq!(effective["config"]["keep"], true);
    }
}

#[test]
fn structural_replacement_conflicts_at_the_parent() {
    let applied = profile(AgentKind::Claude, r#"{"service":{"url":"a"}}"#, "{}", &[]);
    let working = ProfileDefinition {
        root: OverlayNode::Object(BTreeMap::from([(
            "config".to_string(),
            OverlayNode::Object(BTreeMap::from([(
                "service".to_string(),
                OverlayNode::Value(json!(["working"])),
            )])),
        )])),
    };
    let source = profile(AgentKind::Claude, r#"{"service":"source"}"#, "{}", &[]);
    let result = reconcile(&applied, &working, &source, &BTreeMap::new()).unwrap();
    let change = only(&result.changes);
    assert_eq!(change.path.to_string(), "/config/service");
    assert_eq!(change.class, ChangeClass::Conflict);
}
