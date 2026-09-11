use super::*;
use crate::agent::AgentKind;
use crate::config::{apply_named_config, create_named_config};
use crate::tenant::ManagedTenant;
use serde_json::json;

#[test]
fn comparison_tracks_projection_and_isolates_invalid_files_without_writes() {
    let root = tempfile::tempdir().unwrap();
    let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();
    tenant.ensure_initialized().unwrap();
    let selected = tenant.for_agent(AgentKind::Codex);
    let name = NamedConfigName::parse("source").unwrap();
    create_named_config(&selected, &name).unwrap();
    apply_named_config(&selected, &name).unwrap();
    let target = ConfigTarget::Current;
    let initial = compare_configs(&selected, &target, vec![]).unwrap();
    assert!(initial.files.iter().all(|file| file.differences.is_empty()));
    let snapshot = read_config_file_target(&selected, &target, ConfigFile::Main).unwrap();
    let mut text = String::from_utf8(snapshot.content.clone()).unwrap();
    text.push_str("\n# unrelated\n[extra]\nvalue = 'unchanged'\n");
    let draft = |content: String| ConfigComparisonDraft {
        file: ConfigFile::Main,
        revision: snapshot.revision.clone(),
        original: snapshot.content.clone(),
        edit: ConfigEdit::Raw {
            content: content.into_bytes(),
            custom_provider: None,
        },
    };
    let result = compare_configs(&selected, &target, vec![draft(text.clone())]).unwrap();
    assert!(result.files[0].differences.is_empty());
    let modified = text
        .lines()
        .map(|line| {
            if line.starts_with("model =") {
                "model = 'different'"
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let result = compare_configs(&selected, &target, vec![draft(modified)]).unwrap();
    assert_eq!(result.files[0].differences.len(), 1);
    assert_eq!(result.files[0].differences[0].path, ["model"]);
    let result = compare_configs(&selected, &target, vec![draft("[invalid".into())]).unwrap();
    assert!(result.incomplete);
    assert!(result.files[0].error.is_some());
    assert!(result.files[1].error.is_none());
    assert_eq!(
        read_config_file_target(&selected, &target, ConfigFile::Main).unwrap(),
        snapshot
    );
}

#[test]
fn distinguishes_missing_and_null_and_auth_paths() {
    let mut differences = vec![];
    collect_differences(
        Some(&json!({"tokens":{"access_token":"a"},"nullable":null})),
        Some(&json!({"tokens":{"access_token":"b"}})),
        &mut vec![],
        &mut differences,
    );
    assert_eq!(differences.len(), 2);
    assert!(differences[0].named_present);
    assert!(!differences[0].current_present);
    assert_eq!(differences[0].named_value, Some(Value::Null));
    assert_eq!(differences[1].path, ["tokens", "access_token"]);
}

#[test]
fn toml_ranges_handle_dotted_quoted_keys_multiline_and_utf16() {
    let text = "# 😀\n\"model\" = '''\nhello\nworld'''\n[model_providers.custom]\nbase_url = 'https://example.com'\n";
    let range = toml_range(text, &["model".into()]).unwrap();
    let chars = text.encode_utf16().collect::<Vec<_>>();
    assert_eq!(
        String::from_utf16(&chars[range[0]..range[1]]).unwrap(),
        "'''\nhello\nworld'''"
    );
    assert!(
        toml_range(
            text,
            &["model_providers".into(), "custom".into(), "base_url".into()]
        )
        .is_some()
    );
    assert!(toml_range(text, &["missing".into()]).is_none());
    assert!(toml_range("parent = 1\n", &["parent".into(), "child".into()]).is_some());
}

#[test]
fn custom_provider_omissions_and_credentials_are_explained() {
    let root = tempfile::tempdir().unwrap();
    let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();
    tenant.ensure_initialized().unwrap();
    let selected = tenant.for_agent(AgentKind::Codex);
    let name = NamedConfigName::parse("source").unwrap();
    create_named_config(&selected, &name).unwrap();
    apply_named_config(&selected, &name).unwrap();
    let target = ConfigTarget::Current;
    let main = read_config_file_target(&selected, &target, ConfigFile::Main).unwrap();
    let auth = read_config_file_target(&selected, &target, ConfigFile::Auth).unwrap();
    let mut main_value = selected
        .agent()
        .parse_main_config(std::str::from_utf8(&main.content).unwrap())
        .unwrap();
    main_value.remove("model");
    main_value.insert(
        "model_providers".into(),
        json!({"custom":{"base_url":"different"},"unrelated":{"base_url":"keep"}}),
    );
    let content = selected
        .agent()
        .render_main_config(&Value::Object(main_value))
        .unwrap();
    let result = compare_configs(&selected, &target, vec![
        ConfigComparisonDraft { file: ConfigFile::Main, revision: main.revision, original: main.content, edit: ConfigEdit::Raw { content: content.into_bytes(), custom_provider: None } },
        ConfigComparisonDraft { file: ConfigFile::Auth, revision: auth.revision, original: auth.content, edit: ConfigEdit::Raw { content: br#"{"tokens":{"access_token":"new"},"last_refresh":"2026-09-11T00:00:00Z"}"#.to_vec(), custom_provider: None } },
    ]).unwrap();
    assert!(!result.incomplete);
    assert!(
        result.files[1]
            .differences
            .iter()
            .all(|difference| difference.sensitive)
    );
    assert!(
        result.files[0]
            .differences
            .iter()
            .any(|difference| difference.path == ["model"] && !difference.current_present)
    );
    assert!(
        !result.files[0]
            .differences
            .iter()
            .any(|difference| difference.path.iter().any(|part| part == "unrelated"))
    );
    assert!(
        result.files[1]
            .differences
            .iter()
            .any(|difference| difference.path == ["tokens", "access_token"])
    );
    assert!(
        result.files[1]
            .differences
            .iter()
            .any(|difference| difference.path == ["last_refresh"])
    );
}

#[test]
fn stale_drafts_are_unavailable_and_untracked_reads_create_nothing() {
    let root = tempfile::tempdir().unwrap();
    let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();
    let selected = tenant.for_agent(AgentKind::Codex);
    assert!(compare_configs(&selected, &ConfigTarget::Current, vec![]).is_err());
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    tenant.ensure_initialized().unwrap();
    let name = NamedConfigName::parse("source").unwrap();
    create_named_config(&selected, &name).unwrap();
    apply_named_config(&selected, &name).unwrap();
    let result = compare_configs(
        &selected,
        &ConfigTarget::Current,
        vec![ConfigComparisonDraft {
            file: ConfigFile::Main,
            revision: "stale".into(),
            original: vec![],
            edit: ConfigEdit::Raw {
                content: vec![],
                custom_provider: None,
            },
        }],
    )
    .unwrap();
    assert!(
        result.files[0]
            .error
            .as_deref()
            .unwrap()
            .contains("changed since")
    );
    assert!(result.files[1].error.is_none());
}

#[test]
fn visual_drafts_use_rendering_and_cross_file_validation_without_saving() {
    let root = tempfile::tempdir().unwrap();
    let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();
    tenant.ensure_initialized().unwrap();
    let selected = tenant.for_agent(AgentKind::Codex);
    let name = NamedConfigName::parse("source").unwrap();
    create_named_config(&selected, &name).unwrap();
    apply_named_config(&selected, &name).unwrap();
    let target = ConfigTarget::Named(name);
    let main = read_config_file_target(&selected, &target, ConfigFile::Main).unwrap();
    let auth = read_config_file_target(&selected, &target, ConfigFile::Auth).unwrap();
    let visual = super::super::visual::inspect_visual_config(
        AgentKind::Codex,
        std::str::from_utf8(&main.content).unwrap(),
    )
    .unwrap();
    let options = visual
        .options
        .into_iter()
        .filter(|field| !field.path.starts_with("model_provider"))
        .map(|field| super::super::VisualConfigOptionInput {
            path: field.path.clone(),
            included: field.included,
            value: if field.path == "model" {
                Some(json!("draft-model"))
            } else {
                field.value
            },
        })
        .collect();
    let result = compare_configs(
        &selected,
        &target,
        vec![
            ConfigComparisonDraft {
                file: ConfigFile::Main,
                revision: main.revision.clone(),
                original: main.content.clone(),
                edit: ConfigEdit::VisualMain {
                    options,
                    custom_provider: Some(super::super::CustomProviderInput {
                        included: true,
                        name: "draft".into(),
                        base_url: "https://draft.test".into(),
                        proxy_routed: false,
                    }),
                },
            },
            ConfigComparisonDraft {
                file: ConfigFile::Auth,
                revision: auth.revision.clone(),
                original: auth.content.clone(),
                edit: ConfigEdit::Raw {
                    content: b"{}".to_vec(),
                    custom_provider: None,
                },
            },
        ],
    )
    .unwrap();
    assert!(result.incomplete);
    assert!(
        result.files[0]
            .differences
            .iter()
            .any(|difference| difference.path == ["model"]
                && difference.named_value == Some(json!("draft-model")))
    );
    assert!(
        result.files[1]
            .error
            .as_deref()
            .unwrap()
            .contains("OPENAI_API_KEY is required")
    );
    assert_eq!(
        read_config_file_target(&selected, &target, ConfigFile::Main).unwrap(),
        main
    );
    assert_eq!(
        read_config_file_target(&selected, &target, ConfigFile::Auth).unwrap(),
        auth
    );
}
