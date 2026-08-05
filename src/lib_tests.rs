use super::*;
use crate::testutil::EnvGuard;

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
    // Fields drop in declaration order. Restore env before deleting stub
    // dirs, and release the env lock last so parallel tests can't observe a
    // half-restored PATH.
    _guards: Vec<EnvGuard>,
    _docker_dir: tempfile::TempDir,
    _host_home: tempfile::TempDir,
    root: tempfile::TempDir,
    docker_log: std::path::PathBuf,
    _run_lock: std::sync::MutexGuard<'static, ()>,
    _env_lock: std::sync::MutexGuard<'static, ()>,
}

#[cfg(unix)]
impl RunFixture {
    fn new() -> Self {
        let env_lock = test_env_lock();
        let run_lock = crate::docker::run_registry_test_lock();
        let root = tempfile::tempdir().unwrap();
        let host_home = tempfile::tempdir().unwrap();
        let docker_dir = tempfile::tempdir().unwrap();
        let docker_log = docker_dir.path().join("docker.log");
        write_successful_run_docker(docker_dir.path());
        let guards = vec![
            EnvGuard::prepend_path(docker_dir.path()),
            EnvGuard::set("AIBOX_FAKE_DOCKER_LOG", docker_log.as_os_str()),
            EnvGuard::set("AIBOX_ROOT", root.path().as_os_str()),
            EnvGuard::set("HOME", host_home.path().as_os_str()),
        ];
        Self {
            _guards: guards,
            _docker_dir: docker_dir,
            _host_home: host_home,
            root,
            docker_log,
            _run_lock: run_lock,
            _env_lock: env_lock,
        }
    }

    fn run(&self, argv: &[&str], passthrough: Vec<String>) -> Result<i32> {
        let cli = Cli::try_parse_from(argv.iter().copied()).unwrap();
        run(cli, passthrough)
    }

    fn log(&self) -> String {
        std::fs::read_to_string(&self.docker_log).unwrap_or_default()
    }
}

#[test]
fn image_ref_validation_rejects_bad_refs() {
    validate_image_ref("aibox:latest").unwrap();
    assert!(validate_image_ref("")
        .unwrap_err()
        .to_string()
        .contains("empty"));
    assert!(validate_image_ref("--bad")
        .unwrap_err()
        .to_string()
        .contains("must not start"));
    assert!(validate_image_ref("bad image")
        .unwrap_err()
        .to_string()
        .contains("whitespace"));
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
        let _image = EnvGuard::set("AIBOX_IMAGE", image);

        let err = fx
            .run(&["aibox", "run"], Vec::new())
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
    let _image = EnvGuard::set("AIBOX_IMAGE", image);

    let err = fx
        .run(&["aibox", "run"], Vec::new())
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
    let _env_lock = test_env_lock();
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("docker-build.log");
    write_successful_build_docker(dir.path());
    let _path = EnvGuard::prepend_path(dir.path());
    let _log = EnvGuard::set("AIBOX_FAKE_DOCKER_LOG", log.as_os_str());
    let _image = EnvGuard::set("AIBOX_IMAGE", "local/aibox:dev");

    let cli = Cli::try_parse_from(["aibox", "build", "--force"]).unwrap();
    let code = run(cli, Vec::new()).unwrap();

    assert_eq!(code, 0);
    let log = std::fs::read_to_string(log).unwrap();
    assert!(log.contains("<--no-cache> <--pull>"), "{log}");
    assert!(log.contains("<-t> <local/aibox:dev>"), "{log}");
    assert!(log.contains("STDIN:# aibox.Dockerfile"), "{log}");
    assert_eq!(log.matches("ARGS: <build>").count(), 1, "{log}");
}

#[cfg(unix)]
#[test]
fn default_run_uses_codex_managed_tenant_home_without_profile_injection() {
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
    assert!(!fx
        .root
        .path()
        .join("tenants/default/.claude/statusline.sh")
        .exists());
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
    let _status = EnvGuard::set("AIBOX_FAKE_DOCKER_RUN_STATUS", "23");

    let code = fx
        .run(&["aibox", "run", "--tenant", "failed-run"], Vec::new())
        .unwrap();

    assert_eq!(code, 23);
    assert!(fx.root.path().join("tenants/failed-run/.codex").is_dir());
    assert!(fx.log().contains("ARGS: <run>"));
}

#[cfg(unix)]
#[test]
fn run_recovers_a_pending_profile_transaction_before_starting_the_agent() {
    let fx = RunFixture::new();
    let tenant = ManagedTenant::resolve(fx.root.path(), "default").unwrap();
    tenant.ensure_initialized().unwrap();
    let selected = tenant.for_agent(AgentKind::Codex);
    selected.ensure_for_management().unwrap();
    let stale = selected.profile_dir("stale");
    std::fs::create_dir(&stale).unwrap();
    std::fs::write(stale.join("keep"), b"partial transaction").unwrap();
    std::fs::write(
        selected.metadata_file(),
        r#"{
  "active_profile": null,
  "pending": {
    "changes": [{
      "kind": "profile-directory",
      "profile": "stale",
      "present": false
    }],
    "active_profile": null
  }
}
"#,
    )
    .unwrap();
    tenant::set_600(&selected.metadata_file()).unwrap();

    let code = fx.run(&["aibox", "run"], Vec::new()).unwrap();

    assert_eq!(code, 0);
    assert!(!stale.exists());
    let metadata = std::fs::read_to_string(selected.metadata_file()).unwrap();
    assert!(!metadata.contains("\"pending\""), "{metadata}");
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
    let _root = EnvGuard::set("AIBOX_ROOT", configured_root.as_os_str());

    let code = fx.run(&["aibox", "run"], Vec::new()).unwrap();

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
    let _image = EnvGuard::set("AIBOX_IMAGE", "registry.example/aibox:test");

    let code = fx.run(&["aibox", "run"], Vec::new()).unwrap();

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
fn run_preserves_working_config_without_remounting_or_reapplying_profile_data() {
    let fx = RunFixture::new();
    let tenant = ManagedTenant::resolve(fx.root.path(), "default").unwrap();
    let selected = tenant.for_agent(AgentKind::Codex);
    profile::create_profile(&selected, "openai").unwrap();
    std::fs::write(
        selected.profile_file("openai", "config.toml"),
        "model = \"profile\"\n",
    )
    .unwrap();
    std::fs::write(
        selected.profile_file("openai", "auth.json"),
        r#"{"token":"profile"}"#,
    )
    .unwrap();
    profile::activate_profile(&selected, "openai", false).unwrap();

    let working_config = "model = \"locally-adjusted\"\n";
    let working_auth = r#"{"token":"locally-adjusted"}"#;
    std::fs::write(selected.state_file("config.toml"), working_config).unwrap();
    std::fs::write(selected.state_file("auth.json"), working_auth).unwrap();
    let metadata_before = std::fs::read(selected.metadata_file()).unwrap();

    let code = fx.run(&["aibox", "run"], Vec::new()).unwrap();

    assert_eq!(code, 0);
    assert_eq!(
        std::fs::read_to_string(selected.state_file("config.toml")).unwrap(),
        working_config,
        "a Run must consume native Agent Configuration without injecting Profile source"
    );
    assert_eq!(
        std::fs::read_to_string(selected.state_file("auth.json")).unwrap(),
        working_auth,
        "a run must not replace persisted auth from profile metadata"
    );
    assert_eq!(
        std::fs::read(selected.metadata_file()).unwrap(),
        metadata_before,
        "a Run must not activate or reconcile Profile configuration"
    );
    let log = fx.log();
    assert!(
        !log.contains(&selected.metadata_dir().display().to_string()),
        "profile metadata must stay host-only and never enter docker arguments: {log}"
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
    let cli = Cli::try_parse_from(["aibox", "run"]).unwrap();

    let code = run_os(cli, std::slice::from_ref(&opaque)).unwrap();

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
    assert!(!fx
        .root
        .path()
        .join("tenants/default/.claude/statusline.sh")
        .exists());
    assert!(fx.root.path().join("tenants/default/.codex").is_dir());
    assert!(fx.root.path().join("tenants/default/.gitconfig").is_file());
    assert!(!fx.root.path().join("codex/default").exists());
    assert!(!fx.root.path().join("claude/default").exists());
    assert!(!fx
        .root
        .path()
        .join("tenants/default/.claude/settings.json")
        .exists());
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
        (&["aibox", "profile", "list"][..], "applies only to a run"),
        (&["aibox", "session", "list"][..], "applies only to a run"),
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
        &["aibox", "profile", "--tenant", "missing", "list"][..],
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
fn profile_command_agent_selects_the_agent_tenant_catalog() {
    let fx = RunFixture::new();

    let code = fx
        .run(
            &[
                "aibox",
                "profile",
                "--agent",
                "claude",
                "create",
                "anthropic",
            ],
            Vec::new(),
        )
        .unwrap();

    assert_eq!(code, 0);
    assert!(fx
        .root
        .path()
        .join("claude/default/anthropic/settings.json")
        .is_file());
    assert!(
        !fx.root.path().join("codex/default/anthropic").exists(),
        "a command-level --agent claude must not create a Codex profile"
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
fn profile_activate_and_delete_route_to_the_selected_tenant_without_docker() {
    let fx = RunFixture::new();

    fx.run(
        &["aibox", "profile", "--tenant", "work", "create", "openai"],
        Vec::new(),
    )
    .unwrap();
    let selected = ManagedTenant::resolve(fx.root.path(), "work")
        .unwrap()
        .for_agent(AgentKind::Codex);
    std::fs::write(
        selected.profile_file("openai", "config.toml"),
        "model = \"selected-tenant\"\n",
    )
    .unwrap();
    std::fs::write(
        selected.profile_file("openai", "auth.json"),
        r#"{"token":"selected-tenant"}"#,
    )
    .unwrap();

    let code = fx
        .run(
            &["aibox", "profile", "activate", "openai", "--tenant", "work"],
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
        "a scoped profile command must not fall back to the default tenant"
    );

    let error = fx
        .run(
            &[
                "aibox",
                "profile",
                "--tenant=work",
                "delete",
                "openai",
                "--yes",
            ],
            Vec::new(),
        )
        .unwrap_err();

    assert!(error.to_string().contains("is active"));
    fx.run(
        &["aibox", "profile", "--tenant=work", "deactivate"],
        Vec::new(),
    )
    .unwrap();
    let code = fx
        .run(
            &[
                "aibox",
                "profile",
                "--tenant=work",
                "delete",
                "openai",
                "--yes",
            ],
            Vec::new(),
        )
        .unwrap();

    assert_eq!(code, 0);
    assert!(!selected.profile_dir("openai").exists());
    assert!(
        !selected.state_file("config.toml").exists(),
        "deactivation restores the exact pre-activation Agent Configuration"
    );
    assert_eq!(
        fx.log(),
        "",
        "host-side profile management must never invoke Docker"
    );
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
            "aibox", "profile", "--agent", "claude", "list", "--agent", "codex",
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
    let _mode = EnvGuard::set("AIBOX_FAKE_DOCKER_IMAGE_MODE", "missing");

    let err = fx
        .run(&["aibox", "run"], Vec::new())
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
fn run_rejects_mount_that_would_expose_profile_data() {
    let fx = RunFixture::new();
    let metadata = fx.root.path().join("codex/default");
    std::fs::create_dir_all(&metadata).unwrap();
    let mount = format!("{}:/secrets:ro", metadata.display());

    let err = fx
        .run(&["aibox", "run", "-m", &mount], Vec::new())
        .unwrap_err()
        .to_string();

    assert!(err.contains("aibox internal data"), "{err}");
    assert!(!fx.root.path().join("tenants/default").exists());
    assert_eq!(
        fx.log(),
        "",
        "Profile metadata mount validation should fail before docker is consulted"
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
    assert!(
        !write_line(&mut BrokenOnNewline { writes: 0 }, "x").unwrap(),
        "a reader may hang up after the line body but before its delimiter"
    );
    let err = write_text(&mut PermissionDenied, "x")
        .unwrap_err()
        .to_string();
    assert!(err.contains("write to stdout"), "{err}");
}
