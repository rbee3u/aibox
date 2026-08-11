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
        "model = \"gpt\"\nopenai_base_url = \"https://api.openai.com/v1\"\n[model_providers.custom]\nrequires_openai_auth = true\n",
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
    let wrong_base_url_type =
        NamedConfigDefinition::parse(AgentKind::Codex, "openai_base_url = true", Some("{}"))
            .unwrap_err()
            .to_string();
    assert!(
        wrong_base_url_type.contains("/config/openai_base_url must be a string"),
        "{wrong_base_url_type}"
    );
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

    let empty_main = NamedConfigDefinition::parse(AgentKind::Claude, "", None)
        .unwrap_err()
        .to_string();
    assert!(
        empty_main.contains("parse Named Config main configuration"),
        "{empty_main}"
    );
    let unexpected_auth = NamedConfigDefinition::parse(AgentKind::Claude, "{}", Some("{}"))
        .unwrap_err()
        .to_string();
    assert_eq!(
        unexpected_auth,
        "Claude Named Config does not use auth.json"
    );
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
    let document = main.parse::<toml_edit::DocumentMut>().unwrap();
    assert_eq!(document["model"].as_str(), Some("new"));
    assert!(document.get("sandbox_mode").is_none());
    assert_eq!(
        document["model_providers"]["custom"]["name"].as_str(),
        Some("custom")
    );
    assert_eq!(
        document["tui"]["status_line"]
            .as_array()
            .and_then(|values| values.get(0))
            .and_then(toml_edit::Value::as_str),
        Some("model")
    );
    let auth: Value = serde_json::from_str(result.auth.as_deref().unwrap()).unwrap();
    assert_eq!(auth, serde_json::json!({"OPENAI_API_KEY": "new"}));
}

#[test]
fn codex_openai_base_url_sets_replaces_and_removes() {
    let configured = NamedConfigDefinition::parse(
        AgentKind::Codex,
        "openai_base_url = \"http://host.docker.internal:9923/https://api.openai.com/v1\"\n",
        Some("{}"),
    )
    .unwrap();
    let result = configured
        .apply(
            Some("# endpoint\nopenai_base_url = \"https://api.openai.com/v1\"\nkeep = true\n"),
            None,
        )
        .unwrap();
    let main = result.main.unwrap();
    assert!(main.contains("# endpoint"), "{main}");
    let document = main.parse::<toml_edit::DocumentMut>().unwrap();
    assert_eq!(
        document["openai_base_url"].as_str(),
        Some("http://host.docker.internal:9923/https://api.openai.com/v1")
    );
    assert_eq!(document["keep"].as_bool(), Some(true));

    let omitted = NamedConfigDefinition::parse(AgentKind::Codex, "", Some("{}")).unwrap();
    let result = omitted.apply(Some(&main), None).unwrap();
    let main = result.main.unwrap();
    let document = main.parse::<toml_edit::DocumentMut>().unwrap();
    assert!(document.get("openai_base_url").is_none());
    assert_eq!(document["keep"].as_bool(), Some(true));
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
    let claude_error = claude.apply(Some(""), None).unwrap_err().to_string();
    assert!(
        claude_error.contains("parse Current Config settings.json"),
        "{claude_error}"
    );

    let codex = NamedConfigDefinition::parse(AgentKind::Codex, "", Some("{}")).unwrap();
    let codex_error = codex.apply(None, Some("")).unwrap_err().to_string();
    assert!(
        codex_error.contains("parse Current Config auth.json"),
        "{codex_error}"
    );
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
    let document = main.parse::<toml_edit::DocumentMut>().unwrap();
    let providers = document["model_providers"].as_table_like().unwrap();
    assert!(providers.get("custom").is_none());
    assert_eq!(
        providers.get("other").and_then(toml_edit::Item::as_bool),
        Some(true)
    );
}
