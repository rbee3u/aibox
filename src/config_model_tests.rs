use super::*;

fn visual_inputs(agent: AgentKind, content: &str) -> Vec<VisualFieldInput> {
    visual_fields(agent, content)
        .unwrap()
        .into_iter()
        .map(|field| VisualFieldInput {
            path: field.path,
            included: field.included,
            value: field.value,
        })
        .collect()
}

fn visual_input_mut<'a>(
    inputs: &'a mut [VisualFieldInput],
    path: &str,
) -> &'a mut VisualFieldInput {
    inputs.iter_mut().find(|field| field.path == path).unwrap()
}

#[test]
fn visual_schema_comes_from_agent_fields() {
    let claude = visual_fields(
        AgentKind::Claude,
        r#"{"env":{"ANTHROPIC_AUTH_TOKEN":"secret"}}"#,
    )
    .unwrap();
    let token = claude
        .iter()
        .find(|field| field.path == "env.ANTHROPIC_AUTH_TOKEN")
        .unwrap();
    assert_eq!(token.label, "Anthropic auth token");
    assert_eq!(token.group, "Endpoint & credentials");
    assert!(token.sensitive);
    assert!(token.included);

    let codex = visual_fields(AgentKind::Codex, "approval_policy = \"never\"\n").unwrap();
    let approval = codex
        .iter()
        .find(|field| field.path == "approval_policy")
        .unwrap();
    assert_eq!(approval.value_kind, "string");
    assert_eq!(approval.suggestions, ["untrusted", "on-request", "never"]);
    assert_eq!(codex.len(), AgentKind::Codex.main_config_fields().len());
}

#[test]
fn visual_claude_rendering_supports_omit_empty_strings_and_booleans() {
    let original = r#"{
      "env": {
        "ANTHROPIC_BASE_URL": "https://example.com",
        "ANTHROPIC_AUTH_TOKEN": "secret"
      },
      "skipDangerousModePermissionPrompt": true
    }"#;
    let mut inputs = visual_inputs(AgentKind::Claude, original);
    visual_input_mut(&mut inputs, "env.ANTHROPIC_BASE_URL").included = false;
    visual_input_mut(&mut inputs, "env.ANTHROPIC_AUTH_TOKEN").value =
        Some(Value::String(String::new()));
    visual_input_mut(&mut inputs, "skipDangerousModePermissionPrompt").value =
        Some(Value::Bool(false));

    let rendered = render_visual_main(AgentKind::Claude, original, &inputs).unwrap();
    let value: Value = serde_json::from_str(&rendered).unwrap();
    assert!(value["env"].get("ANTHROPIC_BASE_URL").is_none());
    assert_eq!(value["env"]["ANTHROPIC_AUTH_TOKEN"], "");
    assert_eq!(value["skipDangerousModePermissionPrompt"], false);
}

#[test]
fn visual_codex_rendering_preserves_comments_and_accepts_custom_values() {
    let original = "# keep this comment\napproval_policy = \"never\"\nsandbox_mode = \"workspace-write\"\n\n[model_providers.custom]\nrequires_openai_auth = true\n";
    let mut inputs = visual_inputs(AgentKind::Codex, original);
    visual_input_mut(&mut inputs, "approval_policy").value =
        Some(Value::String("future-policy".to_string()));
    visual_input_mut(&mut inputs, "sandbox_mode").included = false;
    visual_input_mut(&mut inputs, "model_providers.custom.requires_openai_auth").value =
        Some(Value::Bool(false));

    let rendered = render_visual_main(AgentKind::Codex, original, &inputs).unwrap();
    assert!(rendered.starts_with("# keep this comment\n"), "{rendered}");
    let document = rendered.parse::<toml_edit::DocumentMut>().unwrap();
    assert_eq!(document["approval_policy"].as_str(), Some("future-policy"));
    assert!(document.get("sandbox_mode").is_none());
    assert_eq!(
        document["model_providers"]["custom"]["requires_openai_auth"].as_bool(),
        Some(false)
    );
}

#[test]
fn visual_rendering_requires_each_fixed_field_once() {
    let mut inputs = visual_inputs(AgentKind::Codex, "");
    inputs.pop();
    assert!(render_visual_main(AgentKind::Codex, "", &inputs).is_err());

    let mut inputs = visual_inputs(AgentKind::Codex, "");
    inputs.push(VisualFieldInput {
        path: inputs[0].path.clone(),
        included: false,
        value: None,
    });
    assert!(render_visual_main(AgentKind::Codex, "", &inputs).is_err());
}

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
