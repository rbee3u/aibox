use super::*;

#[cfg(unix)]
fn write_run_docker(dir: &std::path::Path) {
    crate::testutil::write_stub_script(
        dir,
        "docker",
        r#"#!/bin/sh
log="$AIBOX_FAKE_DOCKER_LOG"
printf 'ARGS:' >> "$log"
for arg in "$@"; do printf ' <%s>' "$arg" >> "$log"; done
printf '\n' >> "$log"
if [ "$1" = "image" ] && [ "$2" = "inspect" ]; then
  if [ "$AIBOX_FAKE_DOCKER_IMAGE_MODE" = "missing" ]; then exit 1; fi
  printf 'sha256:fake\n'; exit 0
fi
if [ "$1" = "image" ] && [ "$2" = "ls" ]; then
  if [ "$AIBOX_FAKE_DOCKER_IMAGE_MODE" = "missing" ]; then exit 0; fi
  printf 'sha256:fake\n'; exit 0
fi
if [ "$1" = "inspect" ]; then printf 'false\n'; exit 0; fi
if [ "$1" = "run" ]; then
  shift
  while [ "$#" -gt 0 ]; do
    if [ "$1" = "--cidfile" ]; then printf 'fake-container\n' > "$2"; break; fi
    shift
  done
  exit "${AIBOX_FAKE_DOCKER_RUN_STATUS:-0}"
fi
exit 99
"#,
    );
}

#[cfg(unix)]
struct RunFixture {
    docker_dir: tempfile::TempDir,
    root: tempfile::TempDir,
    docker_log: std::path::PathBuf,
    _run_lock: std::sync::MutexGuard<'static, ()>,
}

#[cfg(unix)]
impl RunFixture {
    fn new() -> Self {
        let _run_lock = crate::docker::run_registry_test_lock();
        let root = tempfile::tempdir().unwrap();
        let docker_dir = tempfile::tempdir().unwrap();
        let docker_log = docker_dir.path().join("docker.log");
        write_run_docker(docker_dir.path());
        Self {
            docker_dir,
            root,
            docker_log,
            _run_lock,
        }
    }

    fn execute(&self, argv: &[&str], env: &[(&str, &str)]) -> Result<i32> {
        let cli = Cli::try_parse_from(argv.iter().copied())?;
        let mut docker_env = vec![
            ("PATH".into(), "/usr/bin:/bin".into()),
            (
                "AIBOX_FAKE_DOCKER_LOG".into(),
                self.docker_log.clone().into_os_string(),
            ),
        ];
        docker_env.extend(
            env.iter()
                .map(|(key, value)| ((*key).into(), (*value).into())),
        );
        run_with_context(
            cli,
            &[],
            TestCommandContext {
                root: self.root.path().to_path_buf(),
                docker: docker::DockerCli::isolated(
                    self.docker_dir.path().join("docker"),
                    docker_env,
                ),
            },
        )
    }

    fn log(&self) -> String {
        std::fs::read_to_string(&self.docker_log).unwrap_or_default()
    }

    fn seed_codex_component(&self, name: &str) -> ManagedTenant {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let tenant = ManagedTenant::resolve(self.root.path(), name).unwrap();
        tenant.ensure_initialized().unwrap();
        let standalone = tenant.home_dir.join(".codex/packages/standalone");
        let release = standalone.join("releases/1.2.3-x86_64-unknown-linux-musl");
        std::fs::create_dir_all(release.join("bin")).unwrap();
        std::fs::write(release.join("bin/codex"), b"fake codex\n").unwrap();
        std::fs::set_permissions(
            release.join("bin/codex"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        std::fs::create_dir_all(tenant.home_dir.join(".local/bin")).unwrap();
        symlink(
            "/home/aibox/.codex/packages/standalone/releases/1.2.3-x86_64-unknown-linux-musl",
            standalone.join("current"),
        )
        .unwrap();
        symlink(
            "/home/aibox/.codex/packages/standalone/current/bin/codex",
            tenant.home_dir.join(".local/bin/codex"),
        )
        .unwrap();
        tenant
    }
}

#[cfg(unix)]
#[test]
fn run_uses_the_tenant_agent_component_and_forwards_opaque_args() {
    let fx = RunFixture::new();
    fx.seed_codex_component("work");
    let cli = Cli::try_parse_from(["aibox", "run", "--tenant", "work"]).unwrap();
    let passthrough = vec!["exec".into(), "fix tests".into(), "--json".into()];
    let code = run_with_context(
        cli,
        &passthrough,
        TestCommandContext {
            root: fx.root.path().to_path_buf(),
            docker: docker::DockerCli::isolated(
                fx.docker_dir.path().join("docker"),
                [
                    ("PATH".into(), "/usr/bin:/bin".into()),
                    (
                        "AIBOX_FAKE_DOCKER_LOG".into(),
                        fx.docker_log.clone().into_os_string(),
                    ),
                ],
            ),
        },
    )
    .unwrap();
    assert_eq!(code, 0);
    assert!(
        fx.log()
            .contains("<aibox:latest> </bin/bash> <--login> <-c>")
    );
    assert!(
        fx.log()
            .contains("<aibox-codex> <exec> <fix tests> <--json>")
    );
}

#[cfg(unix)]
#[test]
fn run_keeps_current_config_and_does_not_read_named_configs() {
    let fx = RunFixture::new();
    let tenant = fx.seed_codex_component("default");
    let selected = tenant.for_agent(AgentKind::Codex);
    selected.ensure_agent_state_dir().unwrap();
    std::fs::write(selected.state_file("config.toml"), b"model = \"local\"\n").unwrap();
    config::create_named_config(&selected, "saved").unwrap();
    let code = fx.execute(&["aibox", "run"], &[]).unwrap();
    assert_eq!(code, 0);
    assert_eq!(
        std::fs::read(selected.state_file("config.toml")).unwrap(),
        b"model = \"local\"\n"
    );
    assert!(!fx.log().contains("saved"));
}

#[cfg(unix)]
#[test]
fn run_initializes_a_missing_tenant_then_requires_an_explicit_agent_component() {
    let fx = RunFixture::new();

    let error = fx.execute(&["aibox", "run"], &[]).unwrap_err().to_string();

    assert!(
        error.contains("codex Component is not installed"),
        "{error}"
    );
    assert!(
        fx.root
            .path()
            .join("tenants/default/.config/aibox/env.sh")
            .is_file()
    );
    assert!(!fx.log().contains("<run>"), "{}", fx.log());
}

#[cfg(unix)]
#[test]
fn run_reports_missing_image_before_initializing_tenant() {
    let fx = RunFixture::new();
    let error = fx
        .execute(
            &["aibox", "run"],
            &[("AIBOX_FAKE_DOCKER_IMAGE_MODE", "missing")],
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("not present locally"), "{error}");
    assert!(error.contains("Console Overview"), "{error}");
    assert!(error.contains("aibox console"), "{error}");
    assert!(!error.contains("aibox build"), "{error}");
    assert!(!error.contains("aibox serve"), "{error}");
    assert!(!fx.root.path().join("tenants/default").exists());
}

#[cfg(unix)]
#[test]
fn run_rejects_invalid_mount_before_initialization() {
    let fx = RunFixture::new();
    let error = fx
        .execute(&["aibox", "run", "-m", "/no/such/dir:/cache"], &[])
        .unwrap_err()
        .to_string();
    assert!(error.contains("mount host path does not exist"), "{error}");
    assert!(!fx.root.path().join("tenants/default").exists());
}

#[test]
fn removed_commands_are_rejected_by_clap() {
    for command in ["completion", "tenant", "component", "config", "session"] {
        let error = Cli::try_parse_from(["aibox", command]).unwrap_err();
        assert_eq!(error.kind(), clap::error::ErrorKind::InvalidSubcommand);
    }
}
