use crate::agent::AgentKind;
use crate::config::visual::{
    CustomProviderInput, VisualAuthInput, VisualConfigOptionInput, inspect_codex_auth,
    inspect_visual_config, render_visual_auth, render_visual_main,
};
use serde_json::Value;

fn visual_inputs(agent: AgentKind, content: &str) -> Vec<VisualConfigOptionInput> {
    inspect_visual_config(agent, content)
        .unwrap()
        .options
        .into_iter()
        .map(|field| VisualConfigOptionInput {
            path: field.path,
            included: field.included,
            value: field.value,
        })
        .collect()
}

fn custom_provider(included: bool) -> CustomProviderInput {
    CustomProviderInput {
        included,
        name: "custom".to_string(),
        base_url: "https://example.com/v1".to_string(),
        proxy_routed: false,
    }
}

fn visual_input_mut<'a>(
    inputs: &'a mut [VisualConfigOptionInput],
    path: &str,
) -> &'a mut VisualConfigOptionInput {
    inputs.iter_mut().find(|field| field.path == path).unwrap()
}

#[test]
fn visual_schema_comes_from_agent_fields() {
    let claude = inspect_visual_config(
        AgentKind::Claude,
        r#"{"env":{"ANTHROPIC_BASE_URL":"https://example.com","ANTHROPIC_AUTH_TOKEN":"secret"},"permissions":{"defaultMode":"bypassPermissions"}}"#,
    )
    .unwrap();
    let token = claude
        .options
        .iter()
        .find(|field| field.path == "env.ANTHROPIC_AUTH_TOKEN")
        .unwrap();
    assert_eq!(token.label, "Anthropic auth token");
    assert_eq!(token.group, "Endpoint & credentials");
    assert!(token.sensitive);
    assert!(token.included);

    let codex = inspect_visual_config(
        AgentKind::Codex,
        "approval_policy = \"never\"\nsandbox_mode = \"danger-full-access\"\nmodel = \"gpt\"\n",
    )
    .unwrap();
    let approval = codex
        .options
        .iter()
        .find(|field| field.path == "approval_policy")
        .unwrap();
    assert_eq!(approval.value_kind, "string");
    assert_eq!(approval.enum_values, ["untrusted", "on-request", "never"]);
    assert_eq!(
        codex.options.len(),
        AgentKind::Codex.main_config_fields().len() - 4
    );
}

#[test]
fn visual_claude_rendering_supports_omit_empty_strings_and_booleans() {
    let original = r#"{
      "env": {
        "ANTHROPIC_BASE_URL": "https://example.com",
        "ANTHROPIC_AUTH_TOKEN": "secret"
      },
      "permissions": {"defaultMode": "bypassPermissions"},
      "skipDangerousModePermissionPrompt": true
    }"#;
    let mut inputs = visual_inputs(AgentKind::Claude, original);
    visual_input_mut(&mut inputs, "env.ANTHROPIC_DEFAULT_HAIKU_MODEL").included = false;
    visual_input_mut(&mut inputs, "env.ANTHROPIC_DEFAULT_SONNET_MODEL").included = true;
    visual_input_mut(&mut inputs, "env.ANTHROPIC_DEFAULT_SONNET_MODEL").value =
        Some(Value::String(String::new()));
    visual_input_mut(&mut inputs, "skipDangerousModePermissionPrompt").value =
        Some(Value::Bool(false));

    let rendered = render_visual_main(AgentKind::Claude, original, &inputs, None).unwrap();
    let value: Value = serde_json::from_str(&rendered).unwrap();
    assert!(value["env"].get("ANTHROPIC_DEFAULT_HAIKU_MODEL").is_none());
    assert_eq!(value["env"]["ANTHROPIC_DEFAULT_SONNET_MODEL"], "");
    assert_eq!(value["skipDangerousModePermissionPrompt"], false);
}

#[test]
fn visual_codex_rendering_preserves_comments_and_existing_unknown_enum_values() {
    let original = "# keep this comment\napproval_policy = \"future-policy\"\nsandbox_mode = \"workspace-write\"\nmodel = \"gpt\"\nmodel_provider = \"custom\"\n\n[model_providers.custom]\nname = \"custom\"\nbase_url = \"https://example.com/v1\"\nrequires_openai_auth = true\n";
    let mut inputs = visual_inputs(AgentKind::Codex, original);
    visual_input_mut(&mut inputs, "model_reasoning_effort").included = false;

    let provider = custom_provider(true);
    let rendered =
        render_visual_main(AgentKind::Codex, original, &inputs, Some(&provider)).unwrap();
    assert!(rendered.starts_with("# keep this comment\n"), "{rendered}");
    let document = rendered.parse::<toml_edit::DocumentMut>().unwrap();
    assert_eq!(document["approval_policy"].as_str(), Some("future-policy"));
    assert!(document.get("model_reasoning_effort").is_none());
    assert_eq!(
        document["model_providers"]["custom"]["requires_openai_auth"].as_bool(),
        Some(true)
    );
}

#[test]
fn visual_codex_rendering_rejects_new_unknown_enum_values() {
    let original =
        "approval_policy = \"never\"\nsandbox_mode = \"workspace-write\"\nmodel = \"gpt\"\n";
    let mut inputs = visual_inputs(AgentKind::Codex, original);
    visual_input_mut(&mut inputs, "approval_policy").value =
        Some(Value::String("future-policy".to_string()));

    let error = render_visual_main(
        AgentKind::Codex,
        original,
        &inputs,
        Some(&custom_provider(false)),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("must use a supported enum value"), "{error}");
}

#[test]
fn visual_rendering_requires_each_fixed_field_once() {
    let original =
        "approval_policy = \"never\"\nsandbox_mode = \"danger-full-access\"\nmodel = \"gpt\"\n";
    let mut inputs = visual_inputs(AgentKind::Codex, original);
    inputs.pop();
    assert!(
        render_visual_main(
            AgentKind::Codex,
            original,
            &inputs,
            Some(&custom_provider(false))
        )
        .is_err()
    );

    let mut inputs = visual_inputs(AgentKind::Codex, original);
    inputs.push(VisualConfigOptionInput {
        path: inputs[0].path.clone(),
        included: false,
        value: None,
    });
    assert!(
        render_visual_main(
            AgentKind::Codex,
            original,
            &inputs,
            Some(&custom_provider(false))
        )
        .is_err()
    );
}

#[test]
fn required_and_custom_provider_fields_cannot_be_omitted() {
    let mut claude = visual_inputs(
        AgentKind::Claude,
        r#"{"env":{"ANTHROPIC_BASE_URL":"https://example.com","ANTHROPIC_AUTH_TOKEN":"secret"},"permissions":{"defaultMode":"bypassPermissions"}}"#,
    );
    visual_input_mut(&mut claude, "env.ANTHROPIC_BASE_URL").included = false;
    assert!(
        render_visual_main(AgentKind::Claude, "{}", &claude, None)
            .unwrap_err()
            .to_string()
            .contains("required Config Field env.ANTHROPIC_BASE_URL is missing")
    );

    let original = "approval_policy = \"never\"\nsandbox_mode = \"danger-full-access\"\nmodel = \"gpt\"\nmodel_provider = \"custom\"\n[model_providers.custom]\nname = \"custom\"\nbase_url = \"https://example.com/v1\"\nrequires_openai_auth = true\n";
    let codex = visual_inputs(AgentKind::Codex, original);
    let rendered = render_visual_main(
        AgentKind::Codex,
        original,
        &codex,
        Some(&custom_provider(false)),
    )
    .unwrap();
    assert!(!rendered.contains("model_provider"));
}

#[test]
fn codex_auth_supports_api_key_and_valid_chatgpt_credentials() {
    let api_key = inspect_codex_auth(r#"{"OPENAI_API_KEY":"secret"}"#, Some(true)).unwrap();
    assert_eq!(api_key.mode, "api-key");
    assert_eq!(api_key.api_key.as_deref(), Some("secret"));
    assert!(!api_key.extra_fields);

    let chatgpt = inspect_codex_auth(
        r#"{"auth_mode":"chatgpt","OPENAI_API_KEY":"saved","tokens":{"account_id":"acct"},"last_refresh":"2026-08-21T00:00:00Z"}"#,
        Some(true),
    )
    .unwrap();
    assert_eq!(chatgpt.mode, "chatgpt");
    assert_eq!(chatgpt.api_key.as_deref(), Some("saved"));

    let invalid = inspect_codex_auth(
        r#"{"auth_mode":"chatgpt","tokens":{"account_id":""},"last_refresh":"not-a-time"}"#,
        None,
    )
    .unwrap_err()
    .to_string();
    assert!(invalid.contains("tokens.account_id"), "{invalid}");
}

#[test]
fn visual_auth_replaces_the_native_object_with_api_key_credentials() {
    assert_eq!(
        render_visual_auth(&VisualAuthInput {
            included: true,
            value: Some("secret".to_string()),
        })
        .unwrap(),
        "{\n  \"OPENAI_API_KEY\": \"secret\"\n}\n"
    );
    assert_eq!(
        render_visual_auth(&VisualAuthInput {
            included: false,
            value: None,
        })
        .unwrap(),
        "{}\n"
    );
}
