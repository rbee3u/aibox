use super::*;

#[test]
fn schema_accepts_only_fixed_fields_and_types() {
    ProfileDefinition::parse(
        AgentKind::Claude,
        r#"{"env":{},"permissions":{},"skipDangerousModePermissionPrompt":true}"#,
        r#"{"ANTHROPIC_AUTH_TOKEN":"secret"}"#,
    )
    .unwrap();
    ProfileDefinition::parse(
        AgentKind::Codex,
        "model = \"gpt\"\n[model_providers.custom]\nrequires_openai_auth = true\n",
        r#"{"tokens":{"access":"secret"}}"#,
    )
    .unwrap();

    let unknown = ProfileDefinition::parse(AgentKind::Claude, r#"{"theme":"dark"}"#, "{}")
        .unwrap_err()
        .to_string();
    assert!(unknown.contains("/config/theme"), "{unknown}");
    let wrong_type = ProfileDefinition::parse(AgentKind::Codex, "model = true", "{}")
        .unwrap_err()
        .to_string();
    assert!(wrong_type.contains("must be a string"), "{wrong_type}");
    let unknown_provider = ProfileDefinition::parse(
        AgentKind::Codex,
        "[model_providers.other]\nname = \"other\"\n",
        "{}",
    )
    .unwrap_err()
    .to_string();
    assert!(
        unknown_provider.contains("/config/model_providers/other"),
        "{unknown_provider}"
    );
}

#[test]
fn claude_auth_accepts_only_an_optional_string_token() {
    ProfileDefinition::parse(AgentKind::Claude, "{}", "{}").unwrap();
    let unknown =
        ProfileDefinition::parse(AgentKind::Claude, "{}", r#"{"ANTHROPIC_API_KEY":"secret"}"#)
            .unwrap_err()
            .to_string();
    assert!(unknown.contains("/auth/ANTHROPIC_API_KEY"), "{unknown}");
    let wrong_type =
        ProfileDefinition::parse(AgentKind::Claude, "{}", r#"{"ANTHROPIC_AUTH_TOKEN":true}"#)
            .unwrap_err()
            .to_string();
    assert!(wrong_type.contains("must be a string"), "{wrong_type}");

    assert!(ProfileDefinition::parse(AgentKind::Claude, "{}", "").is_err());
    assert!(ProfileDefinition::parse(AgentKind::Claude, "", "{}").is_err());
}

#[test]
fn claude_application_sets_removes_and_preserves_fields() {
    let profile = ProfileDefinition::parse(
        AgentKind::Claude,
        r#"{
          "env": {"ANTHROPIC_BASE_URL": "https://new"},
          "permissions": {"defaultMode": "bypassPermissions"}
        }"#,
        r#"{"ANTHROPIC_AUTH_TOKEN":"new-token"}"#,
    )
    .unwrap();
    let result = profile
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
    let profile = ProfileDefinition::parse(
        AgentKind::Codex,
        "model = \"new\"\n[model_providers.custom]\nname = \"custom\"\n",
        r#"{"OPENAI_API_KEY":"new"}"#,
    )
    .unwrap();
    let result = profile
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
    let claude = ProfileDefinition::parse(AgentKind::Claude, "{}", "{}").unwrap();
    assert_eq!(
        claude.apply(None, None).unwrap(),
        ApplicationResult {
            main: None,
            auth: None
        }
    );
    let codex = ProfileDefinition::parse(AgentKind::Codex, "", "{}").unwrap();
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
    let claude = ProfileDefinition::parse(AgentKind::Claude, "{}", "{}").unwrap();
    assert!(claude.apply(Some(""), None).is_err());

    let codex = ProfileDefinition::parse(AgentKind::Codex, "", "{}").unwrap();
    assert!(codex.apply(None, Some("")).is_err());
}

#[test]
fn missing_fields_remove_conflicting_parent_structures() {
    let claude = ProfileDefinition::parse(AgentKind::Claude, "{}", "{}").unwrap();
    let result = claude
        .apply(
            Some(r#"{"env":"conflict","permissions":["conflict"],"keep":true}"#),
            None,
        )
        .unwrap();
    let main: Value = serde_json::from_str(result.main.as_deref().unwrap()).unwrap();
    assert_eq!(main, serde_json::json!({"keep": true}));

    let codex = ProfileDefinition::parse(AgentKind::Codex, "", "{}").unwrap();
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
