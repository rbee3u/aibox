use crate::agent::AgentKind;
use crate::config::definition::NamedConfigDefinition;
use serde_json::Value;

#[test]
fn schema_accepts_only_fixed_fields_and_types() {
    NamedConfigDefinition::parse(
        AgentKind::Claude,
        r#"{"env":{"ANTHROPIC_BASE_URL":"https://example.com","ANTHROPIC_AUTH_TOKEN":"secret"},"permissions":{"defaultMode":"bypassPermissions"},"skipDangerousModePermissionPrompt":true}"#,
        None,
    )
    .unwrap();
    NamedConfigDefinition::parse(
        AgentKind::Codex,
        "approval_policy = \"never\"\nsandbox_mode = \"danger-full-access\"\nmodel = \"gpt\"\nmodel_provider = \"custom\"\n[model_providers.custom]\nname = \"custom\"\nbase_url = \"https://example.com/v1\"\nrequires_openai_auth = true\n",
        Some(r#"{"OPENAI_API_KEY":"secret"}"#),
    )
    .unwrap();
    NamedConfigDefinition::parse(
        AgentKind::Codex,
        "approval_policy = \"never\"\nsandbox_mode = \"danger-full-access\"\nmodel = \"gpt\"\n",
        Some(r#"{"OPENAI_API_KEY":"secret"}"#),
    )
    .unwrap();

    let unknown = NamedConfigDefinition::parse_with_warnings(
        AgentKind::Claude,
        r#"{"env":{"ANTHROPIC_BASE_URL":"https://example.com","ANTHROPIC_AUTH_TOKEN":"secret"},"permissions":{"defaultMode":"bypassPermissions"},"theme":"dark"}"#,
        None,
    )
    .unwrap();
    assert!(
        unknown
            .warnings
            .iter()
            .any(|warning| warning.contains("/config/theme"))
    );
    let wrong_type = NamedConfigDefinition::parse(AgentKind::Codex, "model = true", Some("{}"))
        .unwrap_err()
        .to_string();
    assert!(wrong_type.contains("must be a string"), "{wrong_type}");
    let wrong_base_url_type =
        NamedConfigDefinition::parse(AgentKind::Codex, "model = true", Some("{}"))
            .unwrap_err()
            .to_string();
    assert!(
        wrong_base_url_type.contains("/config/model must be a string"),
        "{wrong_base_url_type}"
    );
    let unknown_provider = NamedConfigDefinition::parse(
        AgentKind::Codex,
        "approval_policy = \"never\"\nsandbox_mode = \"danger-full-access\"\nmodel = \"gpt\"\nmodel_provider = \"openai\"\n",
        Some("{}"),
    );
    assert!(unknown_provider.is_err());
}

#[test]
fn claude_token_is_a_string_field_in_settings() {
    NamedConfigDefinition::parse(
        AgentKind::Claude,
        r#"{"env":{"ANTHROPIC_BASE_URL":"https://example.com","ANTHROPIC_AUTH_TOKEN":"secret"},"permissions":{"defaultMode":"bypassPermissions"}}"#,
        None,
    )
    .unwrap();
    let wrong_type = NamedConfigDefinition::parse(
        AgentKind::Claude,
        r#"{"env":{"ANTHROPIC_BASE_URL":"https://example.com","ANTHROPIC_AUTH_TOKEN":true},"permissions":{"defaultMode":"bypassPermissions"}}"#,
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
    let unexpected_auth = NamedConfigDefinition::parse(
        AgentKind::Claude,
        r#"{"env":{"ANTHROPIC_BASE_URL":"https://example.com","ANTHROPIC_AUTH_TOKEN":"secret"},"permissions":{"defaultMode":"bypassPermissions"}}"#,
        Some("{}"),
    )
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
        "approval_policy = \"never\"\nsandbox_mode = \"danger-full-access\"\nmodel = \"new\"\nmodel_provider = \"custom\"\n[model_providers.custom]\nname = \"custom\"\nbase_url = \"https://example.com/v1\"\nrequires_openai_auth = true\n",
        Some(r#"{"OPENAI_API_KEY":"new"}"#),
    )
    .unwrap();
    let result = config
        .apply(
            Some(
                "# keep comment\napproval_policy = \"on-request\"\nsandbox_mode = \"workspace-write\"\nmodel = \"old\"\nmodel_provider = \"openai\"\n\n[tui]\nstatus_line = [\"model\"]\n",
            ),
            Some(r#"{"old":"value"}"#),
        )
        .unwrap();
    let main = result.main.unwrap();
    assert!(main.contains("# keep comment"), "{main}");
    let document = main.parse::<toml_edit::DocumentMut>().unwrap();
    assert_eq!(document["model"].as_str(), Some("new"));
    assert_eq!(
        document["sandbox_mode"].as_str(),
        Some("danger-full-access")
    );
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
fn codex_unknown_fields_are_preserved_but_not_applied() {
    let configured = NamedConfigDefinition::parse(
        AgentKind::Codex,
        "approval_policy = \"never\"\nsandbox_mode = \"danger-full-access\"\nmodel = \"gpt\"\n",
        Some(r#"{"OPENAI_API_KEY":"new"}"#),
    )
    .unwrap();
    let result = configured
        .apply(
            Some("# endpoint\napproval_policy = \"on-request\"\nsandbox_mode = \"workspace-write\"\nmodel = \"old\"\nmodel_provider = \"openai\"\nopenai_base_url = \"https://api.openai.com/v1\"\nkeep = true\n"),
            None,
        )
        .unwrap();
    let main = result.main.unwrap();
    assert!(main.contains("# endpoint"), "{main}");
    let document = main.parse::<toml_edit::DocumentMut>().unwrap();
    assert_eq!(
        document["openai_base_url"].as_str(),
        Some("https://api.openai.com/v1")
    );
    assert_eq!(document["keep"].as_bool(), Some(true));
}

#[test]
fn semantically_empty_missing_files_remain_absent() {
    assert!(NamedConfigDefinition::parse(AgentKind::Claude, "{}", None).is_err());
    assert!(NamedConfigDefinition::parse(AgentKind::Codex, "", Some("{}")).is_err());
}

#[test]
fn existing_blank_json_configuration_is_invalid() {
    let claude = NamedConfigDefinition::parse(
        AgentKind::Claude,
        r#"{"env":{"ANTHROPIC_BASE_URL":"https://example.com","ANTHROPIC_AUTH_TOKEN":"secret"},"permissions":{"defaultMode":"bypassPermissions"}}"#,
        None,
    )
    .unwrap();
    let claude_error = claude.apply(Some(""), None).unwrap_err().to_string();
    assert!(
        claude_error.contains("parse Current Config settings.json"),
        "{claude_error}"
    );

    let codex = NamedConfigDefinition::parse(
        AgentKind::Codex,
        "approval_policy = \"never\"\nsandbox_mode = \"danger-full-access\"\nmodel = \"gpt\"\n",
        Some("{}"),
    )
    .unwrap();
    let codex_error = codex.apply(None, Some("")).unwrap_err().to_string();
    assert!(
        codex_error.contains("parse Current Config auth.json"),
        "{codex_error}"
    );
}

#[test]
fn missing_fields_remove_conflicting_parent_structures() {
    let claude = NamedConfigDefinition::parse(
        AgentKind::Claude,
        r#"{"env":{"ANTHROPIC_BASE_URL":"https://example.com","ANTHROPIC_AUTH_TOKEN":"secret"},"permissions":{"defaultMode":"bypassPermissions"}}"#,
        None,
    )
    .unwrap();
    let result = claude
        .apply(
            Some(r#"{"env":"conflict","permissions":["conflict"],"keep":true}"#),
            None,
        )
        .unwrap();
    let main: Value = serde_json::from_str(result.main.as_deref().unwrap()).unwrap();
    assert_eq!(
        main,
        serde_json::json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://example.com",
                "ANTHROPIC_AUTH_TOKEN": "secret"
            },
            "permissions": {"defaultMode": "bypassPermissions"},
            "keep": true
        })
    );

    let codex = NamedConfigDefinition::parse(
        AgentKind::Codex,
        "approval_policy = \"never\"\nsandbox_mode = \"danger-full-access\"\nmodel = \"gpt\"\n",
        Some("{}"),
    )
    .unwrap();
    let result = codex
        .apply(Some("model_providers = \"conflict\"\nkeep = true\n"), None)
        .unwrap();
    assert_eq!(
        result.main.as_deref(),
        Some(
            "keep = true\napproval_policy = \"never\"\nsandbox_mode = \"danger-full-access\"\nmodel = \"gpt\"\n"
        )
    );

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
