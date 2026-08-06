use super::*;

#[test]
fn schema_accepts_only_fixed_fields_and_types() {
    NamedConfigDefinition::parse(
        AgentKind::Claude,
        r#"{"env":{"ANTHROPIC_AUTH_TOKEN":"secret"},"permissions":{},"skipDangerousModePermissionPrompt":true}"#,
        None,
    )
    .unwrap();
    NamedConfigDefinition::parse(
        AgentKind::Codex,
        "model = \"gpt\"\n[model_providers.custom]\nrequires_openai_auth = true\n",
        Some(r#"{"tokens":{"access":"secret"}}"#),
    )
    .unwrap();

    let unknown = NamedConfigDefinition::parse(AgentKind::Claude, r#"{"theme":"dark"}"#, None)
        .unwrap_err()
        .to_string();
    assert!(unknown.contains("/config/theme"), "{unknown}");
    let wrong_type = NamedConfigDefinition::parse(AgentKind::Codex, "model = true", Some("{}"))
        .unwrap_err()
        .to_string();
    assert!(wrong_type.contains("must be a string"), "{wrong_type}");
    let unknown_provider = NamedConfigDefinition::parse(
        AgentKind::Codex,
        "[model_providers.other]\nname = \"other\"\n",
        Some("{}"),
    )
    .unwrap_err()
    .to_string();
    assert!(
        unknown_provider.contains("/config/model_providers/other"),
        "{unknown_provider}"
    );
}

#[test]
fn claude_token_is_a_string_field_in_settings() {
    NamedConfigDefinition::parse(
        AgentKind::Claude,
        r#"{"env":{"ANTHROPIC_AUTH_TOKEN":"secret"}}"#,
        None,
    )
    .unwrap();
    let wrong_type = NamedConfigDefinition::parse(
        AgentKind::Claude,
        r#"{"env":{"ANTHROPIC_AUTH_TOKEN":true}}"#,
        None,
    )
    .unwrap_err()
    .to_string();
    assert!(wrong_type.contains("must be a string"), "{wrong_type}");

    assert!(NamedConfigDefinition::parse(AgentKind::Claude, "", None).is_err());
    assert!(NamedConfigDefinition::parse(AgentKind::Claude, "{}", Some("{}")).is_err());
}

#[test]
fn claude_application_sets_removes_and_preserves_fields() {
    let config = NamedConfigDefinition::parse(
        AgentKind::Claude,
        r#"{
          "env": {
            "ANTHROPIC_BASE_URL": "https://new",
            "ANTHROPIC_AUTH_TOKEN": "new-token"
          },
          "permissions": {"defaultMode": "bypassPermissions"}
        }"#,
        None,
    )
    .unwrap();
    let result = config
        .apply(
            Some(
                r#"{
                  "env": {
                    "ANTHROPIC_BASE_URL": "https://old",
                    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "old-haiku",
                    "KEEP": "yes",
                    "ANTHROPIC_AUTH_TOKEN": "old-token"
                  },
                  "permissions": "conflict",
                  "statusLine": {"type":"command"}
                }"#,
            ),
            None,
        )
        .unwrap();
    let value: Value = serde_json::from_str(result.main.as_deref().unwrap()).unwrap();
    assert_eq!(value["env"]["ANTHROPIC_BASE_URL"], "https://new");
    assert_eq!(value["env"]["ANTHROPIC_AUTH_TOKEN"], "new-token");
    assert_eq!(value["env"]["KEEP"], "yes");
    assert!(value["env"].get("ANTHROPIC_DEFAULT_HAIKU_MODEL").is_none());
    assert_eq!(value["permissions"]["defaultMode"], "bypassPermissions");
    assert_eq!(value["statusLine"]["type"], "command");
}

#[test]
fn codex_application_preserves_comments_and_replaces_whole_auth() {
    let config = NamedConfigDefinition::parse(
        AgentKind::Codex,
        "model = \"new\"\n[model_providers.custom]\nname = \"custom\"\n",
        Some(r#"{"OPENAI_API_KEY":"new"}"#),
    )
    .unwrap();
    let result = config
        .apply(
            Some(
                "# keep comment\nmodel = \"old\"\nsandbox_mode = \"workspace-write\"\n\n[tui]\nstatus_line = [\"model\"]\n",
            ),
            Some(r#"{"old":"value"}"#),
        )
        .unwrap();
    let main = result.main.unwrap();
    assert!(main.contains("# keep comment"), "{main}");
    assert!(main.contains("model = \"new\""), "{main}");
    assert!(!main.contains("sandbox_mode"), "{main}");
    assert!(main.contains("status_line"), "{main}");
    let auth: Value = serde_json::from_str(result.auth.as_deref().unwrap()).unwrap();
    assert_eq!(auth, serde_json::json!({"OPENAI_API_KEY": "new"}));
}

#[test]
fn semantically_empty_missing_files_remain_absent() {
    let claude = NamedConfigDefinition::parse(AgentKind::Claude, "{}", None).unwrap();
    assert_eq!(
        claude.apply(None, None).unwrap(),
        ApplicationResult {
            main: None,
            auth: None
        }
    );
    let codex = NamedConfigDefinition::parse(AgentKind::Codex, "", Some("{}")).unwrap();
    assert_eq!(
        codex.apply(None, None).unwrap(),
        ApplicationResult {
            main: None,
            auth: None
        }
    );
}

#[test]
fn existing_blank_json_configuration_is_invalid() {
    let claude = NamedConfigDefinition::parse(AgentKind::Claude, "{}", None).unwrap();
    assert!(claude.apply(Some(""), None).is_err());

    let codex = NamedConfigDefinition::parse(AgentKind::Codex, "", Some("{}")).unwrap();
    assert!(codex.apply(None, Some("")).is_err());
}

#[test]
fn missing_fields_remove_conflicting_parent_structures() {
    let claude = NamedConfigDefinition::parse(AgentKind::Claude, "{}", None).unwrap();
    let result = claude
        .apply(
            Some(r#"{"env":"conflict","permissions":["conflict"],"keep":true}"#),
            None,
        )
        .unwrap();
    let main: Value = serde_json::from_str(result.main.as_deref().unwrap()).unwrap();
    assert_eq!(main, serde_json::json!({"keep": true}));

    let codex = NamedConfigDefinition::parse(AgentKind::Codex, "", Some("{}")).unwrap();
    let result = codex
        .apply(Some("model_providers = \"conflict\"\nkeep = true\n"), None)
        .unwrap();
    assert_eq!(result.main.as_deref(), Some("keep = true\n"));

    let result = codex
        .apply(
            Some("[model_providers]\ncustom = \"conflict\"\nother = true\n"),
            None,
        )
        .unwrap();
    let main = result.main.unwrap();
    assert!(!main.contains("custom"), "{main}");
    assert!(main.contains("other = true"), "{main}");
}
