use super::*;

#[test]
fn agent_kind_carries_agent_contracts() {
    for (
        agent,
        tag,
        state_dir,
        main,
        native_auth,
        config_files,
        empty_files,
        main_config_fields,
        auth,
    ) in [
        (
            AgentKind::Claude,
            "claude",
            ".claude",
            "settings.json",
            None,
            &["settings.json"][..],
            &[("settings.json", "{}\n")][..],
            &[
                (
                    &["env", "ANTHROPIC_BASE_URL"][..],
                    MainConfigValueKind::String,
                ),
                (
                    &["env", "ANTHROPIC_AUTH_TOKEN"][..],
                    MainConfigValueKind::String,
                ),
                (
                    &["env", "ANTHROPIC_DEFAULT_HAIKU_MODEL"][..],
                    MainConfigValueKind::String,
                ),
                (
                    &["env", "ANTHROPIC_DEFAULT_SONNET_MODEL"][..],
                    MainConfigValueKind::String,
                ),
                (
                    &["env", "ANTHROPIC_DEFAULT_OPUS_MODEL"][..],
                    MainConfigValueKind::String,
                ),
                (
                    &["env", "ANTHROPIC_DEFAULT_FABLE_MODEL"][..],
                    MainConfigValueKind::String,
                ),
                (
                    &["permissions", "defaultMode"][..],
                    MainConfigValueKind::String,
                ),
                (
                    &["skipDangerousModePermissionPrompt"][..],
                    MainConfigValueKind::Bool,
                ),
            ][..],
            None,
        ),
        (
            AgentKind::Codex,
            "codex",
            ".codex",
            "config.toml",
            Some("auth.json"),
            &["config.toml", "auth.json"][..],
            &[("config.toml", ""), ("auth.json", "{}\n")][..],
            &[
                (&["approval_policy"][..], MainConfigValueKind::String),
                (&["sandbox_mode"][..], MainConfigValueKind::String),
                (&["model_reasoning_effort"][..], MainConfigValueKind::String),
                (
                    &["plan_mode_reasoning_effort"][..],
                    MainConfigValueKind::String,
                ),
                (&["model"][..], MainConfigValueKind::String),
                (&["model_provider"][..], MainConfigValueKind::String),
                (
                    &["model_providers", "custom", "name"][..],
                    MainConfigValueKind::String,
                ),
                (
                    &["model_providers", "custom", "base_url"][..],
                    MainConfigValueKind::String,
                ),
                (
                    &["model_providers", "custom", "requires_openai_auth"][..],
                    MainConfigValueKind::Bool,
                ),
            ][..],
            Some("{\n  \"OPENAI_API_KEY\": \"sk-example\"\n}\n"),
        ),
    ] {
        assert_eq!(agent.tag(), tag, "{agent:?}");
        assert_eq!(agent.state_dir_name(), state_dir, "{agent:?}");
        assert_eq!(agent.main_config_file(), main, "{agent:?}");
        assert_eq!(agent.native_auth_file(), native_auth, "{agent:?}");
        assert_eq!(agent.config_files(), config_files, "{agent:?}");
        assert_eq!(agent.config_auth_template(), auth, "{agent:?}");
        for (file, expected) in empty_files {
            assert_eq!(
                agent.empty_config_file(file),
                Some(*expected),
                "{agent:?} {file}"
            );
        }
        assert_eq!(agent.empty_config_file("unknown"), None, "{agent:?}");
        let actual_fields: Vec<_> = agent
            .main_config_fields()
            .iter()
            .map(|field| (field.path, field.value_kind))
            .collect();
        assert_eq!(actual_fields, main_config_fields, "{agent:?}");
    }
}

#[test]
fn invocation_preserves_passthrough_without_injecting_named_config() {
    let pass = vec![OsString::from("--model"), OsString::from("opus")];
    let invocation = AgentKind::Claude.invocation(Path::new("/home/aibox"), &pass);
    assert_eq!(
        invocation.command(),
        ["/home/aibox/.local/bin/claude", "--model", "opus",]
    );

    let invocation = AgentKind::Codex.invocation(Path::new("/home/aibox"), &[]);
    assert_eq!(
        invocation.command().last(),
        Some(&OsString::from("/home/aibox/.local/bin/codex"))
    );
}

#[test]
fn claude_template_uses_fables_native_one_megacontext_model_id() {
    let template: Value = serde_json::from_str(AgentKind::Claude.config_template()).unwrap();
    assert_eq!(
        template["env"]["ANTHROPIC_DEFAULT_FABLE_MODEL"],
        "claude-fable-5"
    );
}

#[cfg(unix)]
#[test]
fn command_preserves_non_utf8_passthrough_arguments() {
    use std::os::unix::ffi::OsStringExt;

    let opaque = OsString::from_vec(vec![b'f', 0x80, b'o']);
    let pass = vec![opaque.clone()];

    let invocation = AgentKind::Codex.invocation(Path::new("/home/aibox"), &pass);

    assert_eq!(invocation.command().last(), Some(&opaque));
}
