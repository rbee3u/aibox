use super::*;

#[cfg(unix)]
fn write_successful_run_docker(dir: &std::path::Path) {
    crate::testutil::write_stub_script(
        dir,
        "docker",
        r#"#!/bin/sh
log="$AIBOX_FAKE_DOCKER_LOG"
printf 'ARGS:' >> "$log"
for arg in "$@"; do
    printf ' <%s>' "$arg" >> "$log"
done
printf '\n' >> "$log"

if [ "$1" = "image" ] && [ "$2" = "inspect" ]; then
    if [ "$AIBOX_FAKE_DOCKER_IMAGE_MODE" = "missing" ]; then
        exit 1
    fi
    printf 'sha256:fake-image\n'
    exit 0
fi

if [ "$1" = "image" ] && [ "$2" = "ls" ]; then
    if [ "$AIBOX_FAKE_DOCKER_IMAGE_MODE" = "missing" ]; then
        exit 0
    fi
    printf 'sha256:fake-image\n'
    exit 0
fi

if [ "$1" = "inspect" ]; then
    printf 'false\n'
    exit 0
fi

if [ "$1" = "run" ]; then
    shift
    cid=
    while [ "$#" -gt 0 ]; do
        if [ "$1" = "--cidfile" ]; then
            cid="$2"
            shift 2
        else
            shift
        fi
    done
    printf 'fake-container\n' > "$cid"
    exit "${AIBOX_FAKE_DOCKER_RUN_STATUS:-0}"
fi

exit 99
"#,
    );
}

#[cfg(unix)]
fn write_successful_build_docker(dir: &std::path::Path) {
    crate::testutil::write_stub_script(
        dir,
        "docker",
        r#"#!/bin/sh
if [ "$1" != "build" ]; then
    exit 99
fi
log="$AIBOX_FAKE_DOCKER_LOG"
printf 'ARGS:' >> "$log"
for arg in "$@"; do
    printf ' <%s>' "$arg" >> "$log"
done
printf '\nSTDIN:' >> "$log"
cat >> "$log"
printf '\nEND\n' >> "$log"
"#,
    );
}

#[cfg(unix)]
struct RunFixture {
    docker_dir: tempfile::TempDir,
    host_home: tempfile::TempDir,
    root: tempfile::TempDir,
    docker_log: std::path::PathBuf,
    _run_lock: std::sync::MutexGuard<'static, ()>,
}

#[cfg(unix)]
impl RunFixture {
    fn new() -> Self {
        let run_lock = crate::docker::run_registry_test_lock();
        let root = tempfile::tempdir().unwrap();
        let host_home = tempfile::tempdir().unwrap();
        let docker_dir = tempfile::tempdir().unwrap();
        let docker_log = docker_dir.path().join("docker.log");
        write_successful_run_docker(docker_dir.path());
        Self {
            docker_dir,
            host_home,
            root,
            docker_log,
            _run_lock: run_lock,
        }
    }

    fn run(&self, argv: &[&str], passthrough: Vec<String>) -> Result<i32> {
        self.run_with(argv, passthrough, None, &[])
    }

    fn run_with(
        &self,
        argv: &[&str],
        passthrough: Vec<String>,
        image_override: Option<&std::ffi::OsStr>,
        docker_env: &[(&str, &str)],
    ) -> Result<i32> {
        self.run_at(
            self.root.path(),
            argv,
            passthrough,
            image_override,
            docker_env,
        )
    }

    fn run_at(
        &self,
        root: &std::path::Path,
        argv: &[&str],
        passthrough: Vec<String>,
        image_override: Option<&std::ffi::OsStr>,
        docker_env: &[(&str, &str)],
    ) -> Result<i32> {
        let passthrough: Vec<_> = passthrough.into_iter().map(OsString::from).collect();
        self.run_os_at(root, argv, &passthrough, image_override, docker_env)
    }

    fn run_os_at(
        &self,
        root: &std::path::Path,
        argv: &[&str],
        passthrough: &[OsString],
        image_override: Option<&std::ffi::OsStr>,
        docker_env: &[(&str, &str)],
    ) -> Result<i32> {
        let cli = Cli::try_parse_from(argv.iter().copied()).unwrap();
        let mut env = vec![
            (OsString::from("PATH"), OsString::from("/usr/bin:/bin")),
            (
                OsString::from("AIBOX_FAKE_DOCKER_LOG"),
                self.docker_log.as_os_str().to_owned(),
            ),
        ];
        env.extend(
            docker_env
                .iter()
                .map(|(name, value)| (OsString::from(name), OsString::from(value))),
        );
        let docker = crate::docker::DockerCli::isolated(self.docker_dir.path().join("docker"), env);
        run_with_context(
            cli,
            passthrough,
            TestCommandContext {
                root,
                host_home: self.host_home.path(),
                image_override,
                docker: &docker,
            },
        )
    }

    fn log(&self) -> String {
        std::fs::read_to_string(&self.docker_log).unwrap_or_default()
    }
}

#[test]
fn image_ref_validation_rejects_bad_refs() {
    validate_image_ref("aibox:latest").unwrap();
    assert!(
        validate_image_ref("")
            .unwrap_err()
            .to_string()
            .contains("empty")
    );
    assert!(
        validate_image_ref("--bad")
            .unwrap_err()
            .to_string()
            .contains("must not start")
    );
    assert!(
        validate_image_ref("bad image")
            .unwrap_err()
            .to_string()
            .contains("whitespace")
    );
}

#[cfg(unix)]
#[test]
fn invalid_image_override_is_rejected_before_docker_lookup() {
    for (image, expected) in [
        ("", "AIBOX_IMAGE is set but empty"),
        ("bad image", "whitespace/control"),
        ("--bad", "must not start"),
    ] {
        let fx = RunFixture::new();

        let err = fx
            .run_with(
                &["aibox", "run"],
                Vec::new(),
                Some(std::ffi::OsStr::new(image)),
                &[],
            )
            .unwrap_err()
            .to_string();

        assert!(err.contains(expected), "{image:?}: {err}");
        assert_eq!(
            fx.log(),
            "",
            "{image:?}: an invalid image override should fail before docker is consulted"
        );
        assert!(
            !fx.root.path().join("tenants/default").exists(),
            "{image:?}: a bad environment override must not initialize a tenant"
        );
    }
}

#[cfg(unix)]
#[test]
fn non_utf8_image_override_is_rejected_before_docker_lookup() {
    use std::os::unix::ffi::OsStringExt;

    let fx = RunFixture::new();
    let image = OsString::from_vec(vec![b'a', b'i', b'b', b'o', b'x', 0xff]);

    let err = fx
        .run_with(&["aibox", "run"], Vec::new(), Some(image.as_os_str()), &[])
        .unwrap_err()
        .to_string();

    assert!(err.contains("AIBOX_IMAGE is not valid UTF-8"), "{err}");
    assert_eq!(
        fx.log(),
        "",
        "an unrepresentable image name must fail before docker is consulted"
    );
    assert!(!fx.root.path().join("tenants/default").exists());
}

#[cfg(unix)]
#[test]
fn build_uses_single_image_and_aibox_image_override() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("docker-build.log");
    write_successful_build_docker(dir.path());
    let docker = crate::docker::DockerCli::isolated(
        dir.path().join("docker"),
        [
            (OsString::from("PATH"), OsString::from("/usr/bin:/bin")),
            (
                OsString::from("AIBOX_FAKE_DOCKER_LOG"),
                log.as_os_str().to_owned(),
            ),
        ],
    );

    let cli = Cli::try_parse_from(["aibox", "build", "--force"]).unwrap();
    let passthrough = Vec::new();
    let root = tempfile::tempdir().unwrap();
    let host_home = tempfile::tempdir().unwrap();
    let code = run_with_context(
        cli,
        &passthrough,
        TestCommandContext {
            root: root.path(),
            host_home: host_home.path(),
            image_override: Some(std::ffi::OsStr::new("local/aibox:dev")),
            docker: &docker,
        },
    )
    .unwrap();

    assert_eq!(code, 0);
    let log = std::fs::read_to_string(log).unwrap();
    assert!(log.contains("<--no-cache> <--pull>"), "{log}");
    assert!(log.contains("<-t> <local/aibox:dev>"), "{log}");
    assert!(log.contains("STDIN:# aibox.Dockerfile"), "{log}");
    assert_eq!(log.matches("ARGS: <build>").count(), 1, "{log}");
}

#[cfg(unix)]
#[test]
fn default_run_uses_codex_managed_tenant_home_without_config_injection() {
    let fx = RunFixture::new();
    let code = fx.run(&["aibox", "run"], Vec::new()).unwrap();
    assert_eq!(code, 0);

    let log = fx.log();
    let expected_home = std::fs::canonicalize(fx.root.path().join("tenants/default")).unwrap();
    assert!(log.contains(&format!("<{}:/home/aibox>", expected_home.display())));
    assert!(log.contains("<aibox:latest> <codex>"), "{log}");
    assert!(
        !log.contains("<--dangerously-bypass-approvals-and-sandbox>"),
        "{log}"
    );
    assert!(fx.root.path().join("tenants/default/.codex").is_dir());
    assert!(
        !fx.root
            .path()
            .join("tenants/default/.claude/statusline.sh")
            .exists()
    );
    assert!(fx.root.path().join("tenants/default/.gitconfig").is_file());
    assert!(!fx.root.path().join("codex/default").exists());
    assert!(!fx.root.path().join("claude/default").exists());
    assert!(!log.contains("<--env-file>"), "{log}");
    assert!(!log.contains("<-c>"), "{log}");
}

#[cfg(unix)]
#[test]
fn nonzero_agent_exit_still_leaves_the_validated_tenant_initialized() {
    let fx = RunFixture::new();

    let code = fx
        .run_with(
            &["aibox", "run", "--tenant", "failed-run"],
            Vec::new(),
            None,
            &[("AIBOX_FAKE_DOCKER_RUN_STATUS", "23")],
        )
        .unwrap();

    assert_eq!(code, 23);
    assert!(fx.root.path().join("tenants/failed-run/.codex").is_dir());
    assert!(fx.log().contains("ARGS: <run>"));
}

#[cfg(unix)]
#[test]
fn run_resolves_a_symlinked_aibox_root_before_mounting_tenant_home() {
    use std::os::unix::fs::symlink;

    let fx = RunFixture::new();
    let parent_link = fx.root.path().join("parent-link");
    let real_parent = fx.root.path().join("real-parent");
    let real_root = real_parent.join("aibox-root");
    std::fs::create_dir(&real_parent).unwrap();
    std::fs::create_dir(&real_root).unwrap();
    symlink(&real_parent, &parent_link).unwrap();
    let configured_root = parent_link.join("aibox-root");

    let code = fx
        .run_at(&configured_root, &["aibox", "run"], Vec::new(), None, &[])
        .unwrap();

    assert_eq!(code, 0);
    let log = fx.log();
    let expected_home = std::fs::canonicalize(real_root.join("tenants/default")).unwrap();
    assert!(
        log.contains(&format!("<{}:/home/aibox>", expected_home.display())),
        "Docker must receive a resolved bind source: {log}"
    );
    assert!(
        !log.contains(&configured_root.display().to_string()),
        "the symlinked bind source must not reach Docker: {log}"
    );
}

#[cfg(unix)]
#[test]
fn run_uses_a_valid_image_override_for_lookup_and_launch() {
    let fx = RunFixture::new();

    let code = fx
        .run_with(
            &["aibox", "run"],
            Vec::new(),
            Some(std::ffi::OsStr::new("registry.example/aibox:test")),
            &[],
        )
        .unwrap();

    assert_eq!(code, 0);
    let log = fx.log();
    assert!(
        log.contains("<image> <inspect> <--format> <{{.Id}}> <registry.example/aibox:test>"),
        "{log}"
    );
    assert!(
        log.contains("<registry.example/aibox:test> <codex>"),
        "the validated override must also be the launched image: {log}"
    );
    assert!(!log.contains("<aibox:latest> <codex>"), "{log}");
}

#[cfg(unix)]
#[test]
fn run_preserves_current_config_without_reading_or_reapplying_named_configs() {
    let fx = RunFixture::new();
    let tenant = ManagedTenant::resolve(fx.root.path(), "default").unwrap();
    let selected = tenant.for_agent(AgentKind::Codex);
    config::create_named_config(&selected, "openai").unwrap();
    std::fs::write(
        selected.named_config_file("openai", "config.toml"),
        "model = \"config\"\n",
    )
    .unwrap();
    std::fs::write(
        selected.named_config_file("openai", "auth.json"),
        r#"{"token":"config"}"#,
    )
    .unwrap();
    config::apply_named_config(&selected, "openai").unwrap();

    let native_config = "model = \"locally-adjusted\"\n";
    let native_auth = r#"{"token":"locally-adjusted"}"#;
    std::fs::write(selected.state_file("config.toml"), native_config).unwrap();
    std::fs::write(selected.state_file("auth.json"), native_auth).unwrap();
    let code = fx.run(&["aibox", "run"], Vec::new()).unwrap();

    assert_eq!(code, 0);
    assert_eq!(
        std::fs::read_to_string(selected.state_file("config.toml")).unwrap(),
        native_config,
        "a Run must consume Current Config without injecting Named Config data"
    );
    assert_eq!(
        std::fs::read_to_string(selected.state_file("auth.json")).unwrap(),
        native_auth,
        "a Run must not replace persisted auth from a Config"
    );
    let log = fx.log();
    assert!(
        !log.contains(&selected.named_config_catalog_dir().display().to_string()),
        "the Named Config catalog must stay host-only and never enter docker arguments: {log}"
    );
}

#[cfg(unix)]
#[test]
fn codex_exec_subcommand_can_be_passed_through() {
    let fx = RunFixture::new();

    let code = fx
        .run(
            &["aibox", "run"],
            vec![
                "exec".to_string(),
                "fix tests".to_string(),
                "--json".to_string(),
            ],
        )
        .unwrap();

    assert_eq!(code, 0);
    let log = fx.log();
    assert!(
        log.contains("<aibox:latest> <codex> <exec> <fix tests> <--json>"),
        "{log}"
    );
    assert!(
        !log.contains("<--dangerously-bypass-approvals-and-sandbox>"),
        "{log}"
    );
}

#[cfg(unix)]
#[test]
fn run_os_preserves_non_utf8_agent_arguments_through_docker_spawn() {
    use std::os::unix::ffi::OsStringExt;

    let fx = RunFixture::new();
    let opaque = OsString::from_vec(vec![b'f', 0x80, b'o']);
    let code = fx
        .run_os_at(
            fx.root.path(),
            &["aibox", "run"],
            std::slice::from_ref(&opaque),
            None,
            &[],
        )
        .unwrap();

    assert_eq!(code, 0);
    let log = std::fs::read(&fx.docker_log).unwrap();
    assert!(
        log.windows(opaque.as_encoded_bytes().len())
            .any(|window| window == opaque.as_encoded_bytes()),
        "the opaque pass-through argument must reach the docker child unchanged"
    );
}

#[cfg(unix)]
#[test]
fn claude_run_does_not_install_the_optional_statusline_component() {
    let fx = RunFixture::new();
    fx.run(&["aibox", "run", "--agent", "claude"], Vec::new())
        .unwrap();

    let log = fx.log();
    let expected_home = std::fs::canonicalize(fx.root.path().join("tenants/default")).unwrap();
    assert!(log.contains(&format!("<{}:/home/aibox>", expected_home.display())));
    assert!(log.contains("<aibox:latest> <claude>"), "{log}");
    assert!(!log.contains("<--dangerously-skip-permissions>"), "{log}");
    assert!(
        !fx.root
            .path()
            .join("tenants/default/.claude/statusline.sh")
            .exists()
    );
    assert!(fx.root.path().join("tenants/default/.codex").is_dir());
    assert!(fx.root.path().join("tenants/default/.gitconfig").is_file());
    assert!(!fx.root.path().join("codex/default").exists());
    assert!(!fx.root.path().join("claude/default").exists());
    assert!(
        !fx.root
            .path()
            .join("tenants/default/.claude/settings.json")
            .exists()
    );
}

#[cfg(unix)]
#[test]
fn managed_tenant_named_host_runs_while_host_tenant_is_session_only() {
    let fx = RunFixture::new();
    let code = fx
        .run(&["aibox", "run", "--tenant", "host"], Vec::new())
        .unwrap();
    assert_eq!(code, 0);
    assert!(fx.root.path().join("tenants/host").is_dir());

    let code = fx.run(&["aibox", "session", "--host"], Vec::new()).unwrap();
    assert_eq!(code, 0);
}

#[cfg(unix)]
#[test]
fn non_run_commands_reject_passthrough_before_docker() {
    let fx = RunFixture::new();
    for (argv, expected) in [
        (&["aibox", "component", "list"][..], "applies only to a run"),
        (&["aibox", "config", "list"][..], "applies only to a run"),
        (&["aibox", "session", "list"][..], "applies only to a run"),
        (
            &["aibox", "traffic"][..],
            "traffic takes no pass-through args",
        ),
        (&["aibox", "build"][..], "build takes no pass-through args"),
        (
            &["aibox", "tenant", "list"][..],
            "tenant takes no pass-through args",
        ),
        (
            &["aibox", "completion", "zsh"][..],
            "completion takes no pass-through args",
        ),
    ] {
        let err = fx
            .run(argv, vec!["ignored".to_string()])
            .unwrap_err()
            .to_string();
        assert!(err.contains(expected), "{argv:?}: {err}");
    }
    assert_eq!(
        fx.log(),
        "",
        "rejected non-Run commands must not consult Docker"
    );
}

#[cfg(unix)]
#[test]
fn read_only_commands_keep_a_missing_managed_tenant_quiet_and_absent() {
    let fx = RunFixture::new();

    for argv in [
        &["aibox", "config", "--tenant", "missing", "list"][..],
        &["aibox", "session", "--tenant", "missing", "list"][..],
        &["aibox", "component", "--tenant", "missing", "list"][..],
    ] {
        assert_eq!(fx.run(argv, Vec::new()).unwrap(), 0, "{argv:?}");
    }

    assert!(!fx.root.path().join("tenants/missing").exists());
    assert!(!fx.root.path().join("codex/missing").exists());
    assert_eq!(fx.log(), "");
}

#[cfg(unix)]
#[test]
fn config_command_agent_selects_the_agent_tenant_catalog() {
    let fx = RunFixture::new();

    let code = fx
        .run(
            &[
                "aibox",
                "config",
                "--agent",
                "claude",
                "create",
                "anthropic",
            ],
            Vec::new(),
        )
        .unwrap();

    assert_eq!(code, 0);
    assert!(
        fx.root
            .path()
            .join("claude/default/anthropic/settings.json")
            .is_file()
    );
    assert!(
        !fx.root
            .path()
            .join("claude/default/anthropic/auth.json")
            .exists()
    );
    assert!(
        !fx.root.path().join("codex/default/anthropic").exists(),
        "a command-level --agent claude must not create a Codex config"
    );
}

#[cfg(unix)]
#[test]
fn tenant_commands_create_and_delete_without_starting_docker() {
    let fx = RunFixture::new();

    let code = fx
        .run(&["aibox", "tenant", "create", "work"], Vec::new())
        .unwrap();

    assert_eq!(code, 0);
    assert!(fx.root.path().join("tenants/work/.codex").is_dir());
    assert!(fx.root.path().join("tenants/work/.claude").is_dir());

    let code = fx
        .run(&["aibox", "tenant", "delete", "work", "--yes"], Vec::new())
        .unwrap();

    assert_eq!(code, 0);
    assert!(!fx.root.path().join("tenants/work").exists());
    assert_eq!(
        fx.log(),
        "",
        "host-side tenant management must never invoke Docker"
    );
}

#[cfg(unix)]
#[test]
fn config_apply_and_delete_route_to_the_selected_tenant_without_docker() {
    let fx = RunFixture::new();

    fx.run(
        &["aibox", "config", "--tenant", "work", "create", "openai"],
        Vec::new(),
    )
    .unwrap();
    let selected = ManagedTenant::resolve(fx.root.path(), "work")
        .unwrap()
        .for_agent(AgentKind::Codex);
    std::fs::write(
        selected.named_config_file("openai", "config.toml"),
        "model = \"selected-tenant\"\n",
    )
    .unwrap();
    std::fs::write(
        selected.named_config_file("openai", "auth.json"),
        r#"{"token":"selected-tenant"}"#,
    )
    .unwrap();

    let code = fx
        .run(
            &["aibox", "config", "apply", "openai", "--tenant", "work"],
            Vec::new(),
        )
        .unwrap();

    assert_eq!(code, 0);
    assert_eq!(
        std::fs::read_to_string(selected.state_file("config.toml")).unwrap(),
        "model = \"selected-tenant\"\n"
    );
    assert!(
        !fx.root
            .path()
            .join("tenants/default/.codex/config.toml")
            .exists(),
        "a scoped config command must not fall back to the default tenant"
    );

    let code = fx
        .run(
            &[
                "aibox",
                "config",
                "--tenant=work",
                "delete",
                "openai",
                "--yes",
            ],
            Vec::new(),
        )
        .unwrap();

    assert_eq!(code, 0);
    assert!(!selected.named_config_dir("openai").exists());
    assert_eq!(
        std::fs::read_to_string(selected.state_file("config.toml")).unwrap(),
        "model = \"selected-tenant\"\n",
        "deleting a Config must not change Current Config"
    );
    assert_eq!(
        fx.log(),
        "",
        "host-side config management must never invoke Docker"
    );
}

#[cfg(unix)]
#[test]
fn config_propagate_auth_is_global_codex_current_only_and_never_starts_docker() {
    let fx = RunFixture::new();
    let source_dir = fx.host_home.path().join(".codex");
    std::fs::create_dir(&source_dir).unwrap();
    let source = br#"{
  "auth_mode": "chatgpt",
  "tokens": {"account_id": "account-a", "refresh_token": "new"},
  "last_refresh": "2026-08-08T04:22:23Z"
}
"#;
    std::fs::write(source_dir.join("auth.json"), source).unwrap();
    let tenant = ManagedTenant::resolve(fx.root.path(), "work").unwrap();
    tenant.ensure_initialized().unwrap();
    let target = tenant.for_agent(AgentKind::Codex).state_file("auth.json");
    std::fs::write(
        &target,
        br#"{
  "auth_mode": "chatgpt",
  "tokens": {"account_id": "account-a", "refresh_token": "old"},
  "last_refresh": "2026-08-07T04:22:23Z"
}
"#,
    )
    .unwrap();

    assert_eq!(
        fx.run(&["aibox", "config", "propagate-auth"], Vec::new())
            .unwrap(),
        0
    );
    assert_eq!(std::fs::read(&target).unwrap(), source);
    assert_eq!(
        fx.run(
            &[
                "aibox",
                "config",
                "--host",
                "--agent",
                "codex",
                "propagate-auth",
                "--current",
            ],
            Vec::new(),
        )
        .unwrap(),
        0
    );

    let error = fx
        .run(
            &["aibox", "config", "--tenant", "work", "propagate-auth"],
            Vec::new(),
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("does not accept --tenant"), "{error}");
    let error = fx
        .run(
            &["aibox", "config", "--agent", "claude", "propagate-auth"],
            Vec::new(),
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("supports only --agent codex"), "{error}");
    assert_eq!(fx.log(), "");
}

#[cfg(unix)]
#[test]
fn session_delete_routes_to_the_selected_tenant_without_docker() {
    let fx = RunFixture::new();
    ManagedTenant::resolve(fx.root.path(), "work")
        .unwrap()
        .ensure_initialized()
        .unwrap();
    let id = "11111111-2222-3333-4444-555555555555";
    let transcript = crate::testutil::write_jsonl(
        fx.root.path(),
        &format!("tenants/work/.codex/sessions/2026/07/30/rollout-test-{id}.jsonl"),
        &[r#"{"timestamp":"2026-07-30T10:00:00Z","type":"session_meta"}"#],
    );

    let code = fx
        .run(
            &[
                "aibox", "session", "delete", id, "--yes", "--tenant", "work",
            ],
            Vec::new(),
        )
        .unwrap();

    assert_eq!(code, 0);
    assert!(
        !transcript.exists(),
        "the selected tenant's transcript should be deleted"
    );
    assert_eq!(
        fx.log(),
        "",
        "host-side session management must never invoke Docker"
    );
}

#[cfg(unix)]
#[test]
fn duplicate_agent_flags_are_rejected_before_docker_is_consulted() {
    let fx = RunFixture::new();

    for argv in [
        &["aibox", "run", "--agent", "claude", "--agent", "codex"][..],
        &[
            "aibox", "config", "--agent", "claude", "list", "--agent", "codex",
        ][..],
        &[
            "aibox", "session", "--agent", "claude", "list", "--agent", "codex",
        ][..],
    ] {
        assert!(
            Cli::try_parse_from(argv).is_err(),
            "{argv:?} should reject conflicting agent selectors"
        );
    }
    assert_eq!(
        fx.log(),
        "",
        "agent-selector errors should be resolved before docker is consulted"
    );
}

#[cfg(unix)]
#[test]
fn invalid_run_mount_does_not_create_tenant_home() {
    let fx = RunFixture::new();
    let err = fx
        .run(&["aibox", "run", "-m", "/no/such/dir:/cache"], Vec::new())
        .unwrap_err()
        .to_string();

    assert!(err.contains("mount host path does not exist"), "{err}");
    assert!(!fx.root.path().join("tenants/default").exists());
}

#[cfg(unix)]
#[test]
fn missing_image_does_not_initialize_tenant_or_run_container() {
    let fx = RunFixture::new();

    let err = fx
        .run_with(
            &["aibox", "run"],
            Vec::new(),
            None,
            &[("AIBOX_FAKE_DOCKER_IMAGE_MODE", "missing")],
        )
        .unwrap_err()
        .to_string();

    assert!(err.contains("not present locally"), "{err}");
    assert!(
        !fx.root.path().join("tenants/default").exists(),
        "a missing image must fail before tenant initialization"
    );
    let log = fx.log();
    assert!(!log.contains("ARGS: <run>"), "{log}");
}

#[cfg(unix)]
#[test]
fn run_rejects_workspace_that_would_expose_aibox_internal_tree() {
    let fx = RunFixture::new();
    let work = fx.root.path().to_str().unwrap();
    let err = fx
        .run(&["aibox", "run", "-w", work], Vec::new())
        .unwrap_err()
        .to_string();

    assert!(err.contains("aibox internal data"), "{err}");
    assert!(!fx.root.path().join("tenants/default").exists());
}

#[cfg(unix)]
#[test]
fn run_rejects_mount_that_would_expose_config_data() {
    let fx = RunFixture::new();
    let catalog = fx.root.path().join("codex/default");
    std::fs::create_dir_all(&catalog).unwrap();
    let mount = format!("{}:/secrets:ro", catalog.display());

    let err = fx
        .run(&["aibox", "run", "-m", &mount], Vec::new())
        .unwrap_err()
        .to_string();

    assert!(err.contains("aibox internal data"), "{err}");
    assert!(!fx.root.path().join("tenants/default").exists());
    assert_eq!(
        fx.log(),
        "",
        "Named Config catalog mount validation should fail before docker is consulted"
    );
}

#[test]
fn output_writes_treat_broken_pipes_as_clean_stops_but_report_other_errors() {
    struct AlwaysBroken;
    impl std::io::Write for AlwaysBroken {
        fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    struct BrokenOnNewline {
        writes: usize,
    }
    impl std::io::Write for BrokenOnNewline {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.writes += 1;
            if self.writes == 1 {
                Ok(buf.len())
            } else {
                Err(std::io::ErrorKind::BrokenPipe.into())
            }
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    struct PermissionDenied;
    impl std::io::Write for PermissionDenied {
        fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::PermissionDenied.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    assert!(!write_line(&mut AlwaysBroken, "x").unwrap());
    assert!(!write_text(&mut AlwaysBroken, "x").unwrap());
    assert!(!write_bytes(&mut AlwaysBroken, b"x").unwrap());
    assert!(
        !write_line(&mut BrokenOnNewline { writes: 0 }, "x").unwrap(),
        "a reader may hang up after the line body but before its delimiter"
    );
    let err = write_text(&mut PermissionDenied, "x")
        .unwrap_err()
        .to_string();
    assert!(err.contains("write to stdout"), "{err}");
}
