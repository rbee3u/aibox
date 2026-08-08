use super::*;
use crate::agent::AgentKind;
use crate::tenant::{ManagedTenant, Tenant};
use crate::testutil::EnvGuard;
use serde_json::Value;
use std::cell::Cell;
use std::io::{self, BufRead, Cursor, IsTerminal};
use std::path::Path;

fn selected(root: &Path, agent: AgentKind) -> TenantAgent {
    let tenant = ManagedTenant::resolve(root, "work").unwrap();
    tenant.ensure_initialized().unwrap();
    tenant.for_agent(agent)
}

fn replace_config_files(selected: &TenantAgent, config: &str, main: &str, auth: &str) {
    fs::write(
        selected.named_config_file(config, selected.agent.main_config_file()),
        main,
    )
    .unwrap();
    if let Some(file) = selected.agent.native_auth_file() {
        fs::write(selected.named_config_file(config, file), auth).unwrap();
    }
}

fn chatgpt_auth(account_id: &str, last_refresh: &str, marker: &str) -> Vec<u8> {
    let mut content = serde_json::to_vec_pretty(&serde_json::json!({
        "auth_mode": "chatgpt",
        "OPENAI_API_KEY": null,
        "tokens": {
            "id_token": format!("id-{marker}"),
            "access_token": format!("access-{marker}"),
            "refresh_token": format!("refresh-{marker}"),
            "account_id": account_id,
        },
        "last_refresh": last_refresh,
    }))
    .unwrap();
    content.push(b'\n');
    content
}

fn install_host_source(root: &Path, home: &Path, content: &[u8]) -> TenantAgent {
    let host = Tenant::Host {
        home_dir: home.to_path_buf(),
        root_dir: root.to_path_buf(),
    }
    .for_agent(AgentKind::Codex);
    tenant::ensure_real_dir(&host.agent_state_dir, "Agent state directory").unwrap();
    fs::write(host.state_file("auth.json"), content).unwrap();
    host
}

fn create_named_auth(selected: &TenantAgent, name: &str, content: &[u8]) -> std::path::PathBuf {
    create_named_config(selected, name).unwrap();
    let path = selected.named_config_file(name, "auth.json");
    fs::write(&path, content).unwrap();
    path
}

fn report_outcome<'a>(report: &'a AuthPropagationReport, label: &str) -> &'a PropagationOutcome {
    &report
        .entries
        .iter()
        .find(|entry| entry.label == label)
        .unwrap_or_else(|| panic!("missing propagation result for {label}"))
        .outcome
}

#[test]
fn credential_propagation_updates_every_matching_existing_scope() {
    let _env_lock = crate::test_env_lock();
    let root = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let _home = EnvGuard::set("HOME", home.path().as_os_str());
    let source = chatgpt_auth("account-a", "2026-08-08T04:22:23.476121Z", "source");
    let host = install_host_source(root.path(), home.path(), &source);

    let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();
    tenant.ensure_initialized().unwrap();
    let selected = tenant.for_agent(AgentKind::Codex);
    let older = chatgpt_auth("account-a", "2026-08-07T04:22:23Z", "older");
    fs::write(selected.state_file("auth.json"), &older).unwrap();
    let current_path = selected.state_file("auth.json");
    let named_old = create_named_auth(&selected, "old", &older);
    create_named_auth(&selected, "same", &source);
    create_named_auth(
        &selected,
        "same-reordered",
        br#"{"tokens":{"refresh_token":"refresh-source","account_id":"account-a","access_token":"access-source","id_token":"id-source"},"last_refresh":"2026-08-08T04:22:23.476121Z","OPENAI_API_KEY":null,"auth_mode":"chatgpt"}"#,
    );
    create_named_auth(
        &selected,
        "conflict",
        &chatgpt_auth("account-a", "2026-08-08T04:22:23.476121Z", "different"),
    );
    create_named_auth(
        &selected,
        "newer",
        &chatgpt_auth("account-a", "2026-08-09T04:22:23Z", "newer"),
    );
    create_named_auth(
        &selected,
        "other-account",
        &chatgpt_auth("account-b", "2026-08-07T04:22:23Z", "other"),
    );
    create_named_auth(
        &selected,
        "other-broken",
        br#"{"auth_mode":"chatgpt","tokens":{"account_id":"account-b"},"last_refresh":"bad"}"#,
    );
    create_named_auth(&selected, "api-key", br#"{"OPENAI_API_KEY":"sk-test"}"#);
    create_named_auth(&selected, "invalid", b"not-json\n");
    let host_old = create_named_auth(&host, "old", &older);

    let report = execute_auth_propagation(plan_auth_propagation(root.path()).unwrap());

    assert_eq!(fs::read(&current_path).unwrap(), source);
    assert_eq!(fs::read(&named_old).unwrap(), source);
    assert_eq!(fs::read(&host_old).unwrap(), source);
    assert_eq!(
        report_outcome(&report, "tenant/work/current"),
        &PropagationOutcome::Updated
    );
    assert_eq!(
        report_outcome(&report, "tenant/work/config/old"),
        &PropagationOutcome::Updated
    );
    assert_eq!(
        report_outcome(&report, "host/config/old"),
        &PropagationOutcome::Updated
    );
    assert_eq!(
        report_outcome(&report, "tenant/work/config/same"),
        &PropagationOutcome::Unchanged
    );
    assert_eq!(
        report_outcome(&report, "tenant/work/config/same-reordered"),
        &PropagationOutcome::Unchanged
    );
    assert!(matches!(
        report_outcome(&report, "tenant/work/config/conflict"),
        PropagationOutcome::Conflict { .. }
    ));
    assert!(matches!(
        report_outcome(&report, "tenant/work/config/newer"),
        PropagationOutcome::Newer { .. }
    ));
    assert!(matches!(
        report_outcome(&report, "tenant/work/config/invalid"),
        PropagationOutcome::Invalid { .. }
    ));
    assert!(report.entries.iter().all(|entry| {
        !entry.label.ends_with("other-account")
            && !entry.label.ends_with("other-broken")
            && !entry.label.ends_with("api-key")
    }));
    assert!(report
        .entries
        .windows(2)
        .all(|entries| entries[0].label < entries[1].label));
    assert_eq!(
        report.counts(),
        PropagationCounts {
            updated: 3,
            unchanged: 2,
            conflicts: 1,
            newer: 1,
            invalid: 1,
            failed: 0,
        }
    );
}

#[test]
fn credential_propagation_requires_a_valid_host_chatgpt_source() {
    let _env_lock = crate::test_env_lock();
    let root = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let _home = EnvGuard::set("HOME", home.path().as_os_str());
    let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();
    tenant.ensure_initialized().unwrap();
    let target = tenant.for_agent(AgentKind::Codex).state_file("auth.json");
    let old = chatgpt_auth("account-a", "2026-08-07T04:22:23Z", "old");
    fs::write(&target, &old).unwrap();

    let error = plan_auth_propagation(root.path()).unwrap_err().to_string();
    assert!(error.contains("does not exist"), "{error}");

    install_host_source(root.path(), home.path(), br#"{"OPENAI_API_KEY":"sk-test"}"#);
    let error = plan_auth_propagation(root.path()).unwrap_err().to_string();
    assert!(error.contains("not ChatGPT Credentials"), "{error}");

    install_host_source(
        root.path(),
        home.path(),
        br#"{"auth_mode":"chatgpt","tokens":{"account_id":"account-a"},"last_refresh":"bad"}"#,
    );
    let error = plan_auth_propagation(root.path()).unwrap_err().to_string();
    assert!(error.contains("invalid last_refresh"), "{error}");
    assert_eq!(fs::read(&target).unwrap(), old);
}

#[test]
fn credential_propagation_does_not_create_missing_or_scan_orphaned_configs() {
    let _env_lock = crate::test_env_lock();
    let root = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let _home = EnvGuard::set("HOME", home.path().as_os_str());
    let source = chatgpt_auth("account-a", "2026-08-08T04:22:23Z", "source");
    install_host_source(root.path(), home.path(), &source);

    let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();
    tenant.ensure_initialized().unwrap();
    let selected = tenant.for_agent(AgentKind::Codex);
    selected.ensure_named_config_catalog().unwrap();
    let incomplete = selected.named_config_dir("incomplete");
    tenant::ensure_real_dir(&incomplete, "Named Config directory").unwrap();
    let main = incomplete.join("config.toml");
    fs::write(&main, "model = \"kept\"\n").unwrap();
    tenant::set_600(&main).unwrap();

    let orphan = root.path().join("codex/orphan/old");
    tenant::ensure_real_dir(&orphan, "orphaned Named Config directory").unwrap();
    let orphan_main = orphan.join("config.toml");
    let orphan_auth = orphan.join("auth.json");
    fs::write(&orphan_main, "model = \"orphan\"\n").unwrap();
    fs::write(
        &orphan_auth,
        chatgpt_auth("account-a", "2026-08-07T04:22:23Z", "orphan"),
    )
    .unwrap();
    tenant::set_600(&orphan_main).unwrap();
    tenant::set_600(&orphan_auth).unwrap();
    let orphan_before = fs::read(&orphan_auth).unwrap();

    let report = execute_auth_propagation(plan_auth_propagation(root.path()).unwrap());

    assert!(report.entries.is_empty());
    assert!(!selected.state_file("auth.json").exists());
    assert!(!incomplete.join("auth.json").exists());
    assert_eq!(fs::read(orphan_auth).unwrap(), orphan_before);
}

#[cfg(unix)]
#[test]
fn credential_propagation_preflight_is_structurally_strict_and_preserves_modes() {
    use std::os::unix::fs::{symlink, PermissionsExt};

    let _env_lock = crate::test_env_lock();
    let root = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let _home = EnvGuard::set("HOME", home.path().as_os_str());
    let source = chatgpt_auth("account-a", "2026-08-08T04:22:23Z", "source");
    install_host_source(root.path(), home.path(), &source);
    let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();
    tenant.ensure_initialized().unwrap();
    let selected = tenant.for_agent(AgentKind::Codex);
    let older = chatgpt_auth("account-a", "2026-08-07T04:22:23Z", "older");
    let current = selected.state_file("auth.json");
    fs::write(&current, &older).unwrap();
    fs::set_permissions(&current, fs::Permissions::from_mode(0o640)).unwrap();
    let linked = create_named_auth(&selected, "linked", &older);
    fs::remove_file(&linked).unwrap();
    symlink(&current, &linked).unwrap();

    let error = plan_auth_propagation(root.path()).unwrap_err().to_string();
    assert!(error.contains("non-regular file"), "{error}");
    assert_eq!(fs::read(&current).unwrap(), older);

    fs::remove_file(&linked).unwrap();
    fs::write(&linked, &older).unwrap();
    tenant::set_600(&linked).unwrap();

    let linked_tenant = root.path().join("tenants/linked");
    symlink(&tenant.home_dir, &linked_tenant).unwrap();
    let error = plan_auth_propagation(root.path()).unwrap_err().to_string();
    assert!(error.contains("not a real directory"), "{error}");
    fs::remove_file(linked_tenant).unwrap();

    let report = execute_auth_propagation(plan_auth_propagation(root.path()).unwrap());
    assert_eq!(report.counts().updated, 2);
    assert_eq!(fs::read(&current).unwrap(), source);
    assert_eq!(
        fs::metadata(&current).unwrap().permissions().mode() & 0o777,
        0o640
    );
    assert_eq!(
        fs::metadata(&linked).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn credential_propagation_continues_after_write_failure_and_uses_the_plan_snapshot() {
    let _env_lock = crate::test_env_lock();
    let root = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let _home = EnvGuard::set("HOME", home.path().as_os_str());
    let source = chatgpt_auth("account-a", "2026-08-08T04:22:23Z", "source");
    install_host_source(root.path(), home.path(), &source);
    let older = chatgpt_auth("account-a", "2026-08-07T04:22:23Z", "older");

    let first = ManagedTenant::resolve(root.path(), "alpha").unwrap();
    let second = ManagedTenant::resolve(root.path(), "beta").unwrap();
    first.ensure_initialized().unwrap();
    second.ensure_initialized().unwrap();
    let first_auth = first.for_agent(AgentKind::Codex).state_file("auth.json");
    let second_auth = second.for_agent(AgentKind::Codex).state_file("auth.json");
    fs::write(&first_auth, &older).unwrap();
    fs::write(&second_auth, &older).unwrap();

    let plan = plan_auth_propagation(root.path()).unwrap();
    fs::remove_file(&first_auth).unwrap();
    fs::create_dir(&first_auth).unwrap();
    fs::write(
        &second_auth,
        chatgpt_auth("account-a", "2026-08-09T04:22:23Z", "concurrent-newer"),
    )
    .unwrap();

    let report = execute_auth_propagation(plan);

    assert!(matches!(
        report_outcome(&report, "tenant/alpha/current"),
        PropagationOutcome::Failed { .. }
    ));
    assert_eq!(
        report_outcome(&report, "tenant/beta/current"),
        &PropagationOutcome::Updated
    );
    assert_eq!(fs::read(&second_auth).unwrap(), source);
    assert_eq!(report.counts().failed, 1);
    assert_eq!(report.counts().updated, 1);
}

#[test]
fn configs_are_tenant_and_agent_local() {
    let root = tempfile::tempdir().unwrap();
    let codex = selected(root.path(), AgentKind::Codex);
    let claude = selected(root.path(), AgentKind::Claude);
    create_named_config(&codex, "custom").unwrap();
    assert_eq!(list_named_configs(&codex).unwrap(), ["custom"]);
    assert!(list_named_configs(&claude).unwrap().is_empty());
}

#[test]
fn create_uses_templates_and_rejects_a_complete_existing_config() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_named_config(&selected, "custom").unwrap();

    let main = fs::read_to_string(selected.named_config_file("custom", "config.toml")).unwrap();
    assert!(main.contains("name = \"custom\""), "{main}");
    assert_eq!(
        fs::read_to_string(selected.named_config_file("custom", "auth.json")).unwrap(),
        "{\n  \"OPENAI_API_KEY\": \"sk-example\"\n}\n"
    );
    assert!(!selected.state_file("config.toml").exists());
    assert!(!selected.state_file("auth.json").exists());
    let error = create_named_config(&selected, "custom")
        .unwrap_err()
        .to_string();
    assert!(error.contains("already exists"), "{error}");

    fs::write(
        selected.named_config_file("custom", "config.toml"),
        "invalid = true\n",
    )
    .unwrap();
    let error = create_named_config(&selected, "custom")
        .unwrap_err()
        .to_string();
    assert!(error.contains("already exists"), "{error}");
}

#[test]
fn claude_named_config_uses_one_native_file_with_the_token_in_env() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Claude);

    create_named_config(&selected, "custom").unwrap();

    assert_eq!(selected.agent.config_files(), ["settings.json"]);
    assert!(!selected.named_config_file("custom", "auth.json").exists());
    let settings: Value = serde_json::from_str(
        &fs::read_to_string(selected.named_config_file("custom", "settings.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(settings["env"]["ANTHROPIC_AUTH_TOKEN"], "sk-example");
}

#[test]
fn get_prints_native_files_in_order_with_unredacted_content_and_missing_markers() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_named_config(&selected, "custom").unwrap();
    replace_config_files(
        &selected,
        "custom",
        "model = \"custom\"\n",
        "{\"OPENAI_API_KEY\":\"secret\"}\n",
    );

    assert_eq!(
        get_named_config(&selected, "custom").unwrap(),
        b"==> config.toml <==\nmodel = \"custom\"\n\n==> auth.json <==\n{\"OPENAI_API_KEY\":\"secret\"}\n"
    );

    fs::write(selected.state_file("config.toml"), b"not valid toml\n").unwrap();
    assert_eq!(
        get_current_config(&selected).unwrap(),
        b"==> config.toml <==\nnot valid toml\n\n==> auth.json (missing) <==\n"
    );
}

#[test]
fn get_current_rejects_a_missing_managed_tenant_without_creating_it() {
    let root = tempfile::tempdir().unwrap();
    let tenant = ManagedTenant::resolve(root.path(), "missing").unwrap();
    let selected = tenant.for_agent(AgentKind::Codex);

    let error = get_current_config(&selected).unwrap_err().to_string();

    assert!(error.contains("Tenant Home does not exist"), "{error}");
    assert!(!tenant.home_dir.exists());
}

#[cfg(unix)]
#[test]
fn create_repairs_only_safe_valid_incomplete_configs() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    selected.ensure_named_config_catalog().unwrap();
    let directory = selected.named_config_dir("partial");
    tenant::ensure_real_dir(&directory, "Named Config directory").unwrap();
    let main = selected.named_config_file("partial", "config.toml");
    fs::write(&main, "model = \"kept\"\n").unwrap();
    tenant::set_600(&main).unwrap();

    create_named_config(&selected, "partial").unwrap();

    assert_eq!(fs::read_to_string(&main).unwrap(), "model = \"kept\"\n");
    assert!(selected.named_config_file("partial", "auth.json").is_file());

    let broken = selected.named_config_dir("broken");
    tenant::ensure_real_dir(&broken, "Named Config directory").unwrap();
    let broken_main = selected.named_config_file("broken", "config.toml");
    fs::write(&broken_main, "unknown = true\n").unwrap();
    tenant::set_600(&broken_main).unwrap();
    let error = format!(
        "{:#}",
        create_named_config(&selected, "broken").unwrap_err()
    );
    assert!(error.contains("unsupported Config Field"), "{error}");
    assert!(!selected.named_config_file("broken", "auth.json").exists());

    let unknown = selected.named_config_dir("unknown");
    tenant::ensure_real_dir(&unknown, "Named Config directory").unwrap();
    fs::write(unknown.join("extra"), "unexpected").unwrap();
    let error = create_named_config(&selected, "unknown")
        .unwrap_err()
        .to_string();
    assert!(error.contains("unknown entry"), "{error}");

    use std::os::unix::fs::symlink;
    let linked = selected.named_config_dir("linked");
    tenant::ensure_real_dir(&linked, "Named Config directory").unwrap();
    symlink(&main, linked.join("config.toml")).unwrap();
    let error = create_named_config(&selected, "linked")
        .unwrap_err()
        .to_string();
    assert!(error.contains("non-regular file"), "{error}");

    assert_eq!(list_named_configs(&selected).unwrap(), ["partial"]);
}

#[test]
fn invalid_complete_configs_are_visible_readable_and_rejected_by_apply() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Claude);
    create_named_config(&selected, "broken").unwrap();
    fs::write(
        selected.named_config_file("broken", "settings.json"),
        "{\"theme\":\"dark\"}\n",
    )
    .unwrap();

    assert_eq!(list_named_configs(&selected).unwrap(), ["broken"]);
    assert!(
        String::from_utf8(get_named_config(&selected, "broken").unwrap())
            .unwrap()
            .contains("theme")
    );
    let error = format!("{:#}", apply_named_config(&selected, "broken").unwrap_err());
    assert!(error.contains("/config/theme"), "{error}");
    assert!(!selected.state_file("settings.json").exists());
}

#[cfg(unix)]
#[test]
fn edit_can_repair_an_invalid_complete_config() {
    let _env_lock = crate::test_env_lock();
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_named_config(&selected, "broken").unwrap();
    fs::write(
        selected.named_config_file("broken", "config.toml"),
        "unknown = true\n",
    )
    .unwrap();
    fs::write(
        selected.named_config_file("broken", "auth.json"),
        "not-json\n",
    )
    .unwrap();
    let editor_dir = tempfile::tempdir().unwrap();
    let editor = crate::testutil::write_stub_script(
        editor_dir.path(),
        "editor",
        "#!/bin/sh\ncase \"$1\" in\n  *config.toml*) printf 'model = \"repaired\"\\n' > \"$1\" ;;\n  *auth.json*) printf '{}\\n' > \"$1\" ;;\nesac\n",
    );
    let _visual = EnvGuard::set("VISUAL", editor.as_os_str());

    edit_named_config(&selected, "broken").unwrap();
    apply_named_config(&selected, "broken").unwrap();

    assert!(fs::read_to_string(selected.state_file("config.toml"))
        .unwrap()
        .contains("model = \"repaired\""));
}

#[test]
fn apply_after_edit_confirmation_names_the_full_target_and_requires_explicit_yes() {
    let root = tempfile::tempdir().unwrap();
    let managed = selected(root.path(), AgentKind::Claude);
    let managed_prompt =
        "Apply Named Config 'custom' to Claude Current Config for Managed Tenant 'work' now? [y/N] ";

    for yes in ["y\n", "Y\n", "yes\n", " YES \n"] {
        let mut input = Cursor::new(yes.as_bytes());
        let mut output = Vec::new();
        assert!(
            read_apply_confirmation(&managed, "custom", &mut input, &mut output).unwrap(),
            "{yes:?}"
        );
        assert_eq!(String::from_utf8(output).unwrap(), managed_prompt);
    }

    for no in ["", "\n", "n\n", "yeah\n", "yes please\n"] {
        let mut input = Cursor::new(no.as_bytes());
        let mut output = Vec::new();
        assert!(
            !read_apply_confirmation(&managed, "custom", &mut input, &mut output).unwrap(),
            "{no:?}"
        );
        assert_eq!(String::from_utf8(output).unwrap(), managed_prompt);
    }

    let host_home = tempfile::tempdir().unwrap();
    let host = Tenant::Host {
        home_dir: host_home.path().to_path_buf(),
        root_dir: root.path().to_path_buf(),
    }
    .for_agent(AgentKind::Codex);
    let mut input = Cursor::new(b"\n");
    let mut output = Vec::new();
    assert!(!read_apply_confirmation(&host, "custom", &mut input, &mut output).unwrap());
    assert_eq!(
        String::from_utf8(output).unwrap(),
        "Apply Named Config 'custom' to Codex Current Config for Host Tenant now? [y/N] "
    );
}

#[test]
fn apply_after_edit_confirmation_propagates_read_errors() {
    struct FailingInput;

    impl io::Read for FailingInput {
        fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("input failed"))
        }
    }

    impl BufRead for FailingInput {
        fn fill_buf(&mut self) -> io::Result<&[u8]> {
            Err(io::Error::other("input failed"))
        }

        fn consume(&mut self, _amount: usize) {}
    }

    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    let error = read_apply_confirmation(&selected, "custom", &mut FailingInput, &mut Vec::new())
        .unwrap_err()
        .to_string();

    assert!(
        error.contains("read Config Application confirmation"),
        "{error}"
    );
}

#[test]
fn noninteractive_edit_confirmation_skips_application() {
    if io::stdin().is_terminal() {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);

    assert!(!confirm_apply_after_edit(&selected, "custom").unwrap());
}

#[cfg(unix)]
#[test]
fn successful_named_edit_applies_only_after_confirmation() {
    let _env_lock = crate::test_env_lock();
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_named_config(&selected, "custom").unwrap();
    let editor_dir = tempfile::tempdir().unwrap();
    let editor =
        crate::testutil::write_stub_script(editor_dir.path(), "editor", "#!/bin/sh\nexit 0\n");
    let _visual = EnvGuard::set("VISUAL", editor.as_os_str());
    let prompted = Cell::new(false);

    edit_named_config_with_apply_prompt(&selected, "custom", |_, _| {
        prompted.set(true);
        Ok(false)
    })
    .unwrap();

    assert!(prompted.get());
    assert!(!selected.state_file("config.toml").exists());
    assert!(!selected.state_file("auth.json").exists());

    edit_named_config_with_apply_prompt(&selected, "custom", |_, _| Ok(true)).unwrap();

    assert!(selected.state_file("config.toml").exists());
    assert!(selected.state_file("auth.json").exists());
}

#[cfg(unix)]
#[test]
fn failed_named_edit_does_not_ask_to_apply() {
    let _env_lock = crate::test_env_lock();
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_named_config(&selected, "custom").unwrap();
    let editor_dir = tempfile::tempdir().unwrap();
    let editor = crate::testutil::write_stub_script(
        editor_dir.path(),
        "editor",
        "#!/bin/sh\ncase \"$1\" in\n  *config.toml*) printf 'model = \"edited\"\\n' > \"$1\" ;;\n  *auth.json*) exit 7 ;;\nesac\n",
    );
    let _visual = EnvGuard::set("VISUAL", editor.as_os_str());
    let prompted = Cell::new(false);

    let error = edit_named_config_with_apply_prompt(&selected, "custom", |_, _| {
        prompted.set(true);
        Ok(true)
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("editor exited"), "{error}");
    assert!(!prompted.get());
    assert!(
        fs::read_to_string(selected.named_config_file("custom", "config.toml"))
            .unwrap()
            .contains("model = \"edited\"")
    );
    assert!(!selected.state_file("config.toml").exists());
}

#[cfg(unix)]
#[test]
fn apply_failure_after_edit_keeps_the_edit_and_reports_context() {
    let _env_lock = crate::test_env_lock();
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Claude);
    create_named_config(&selected, "custom").unwrap();
    let named = selected.named_config_file("custom", "settings.json");
    let edited = "{\"env\":{\"ANTHROPIC_AUTH_TOKEN\":\"edited\"}}\n";
    let editor_dir = tempfile::tempdir().unwrap();
    let editor = crate::testutil::write_stub_script(
        editor_dir.path(),
        "editor",
        "#!/bin/sh\nprintf '{\"env\":{\"ANTHROPIC_AUTH_TOKEN\":\"edited\"}}\\n' > \"$1\"\n",
    );
    let _visual = EnvGuard::set("VISUAL", editor.as_os_str());
    fs::write(selected.state_file("settings.json"), "not-json\n").unwrap();

    let error = format!(
        "{:#}",
        edit_named_config_with_apply_prompt(&selected, "custom", |_, _| Ok(true)).unwrap_err()
    );

    assert!(error.contains("was edited successfully"), "{error}");
    assert!(
        error.contains("Claude Current Config for Managed Tenant 'work'"),
        "{error}"
    );
    assert_eq!(fs::read_to_string(named).unwrap(), edited);
    assert_eq!(
        fs::read_to_string(selected.state_file("settings.json")).unwrap(),
        "not-json\n"
    );
}

#[cfg(unix)]
#[test]
fn edit_current_initializes_missing_state_and_preserves_raw_invalid_content() {
    use std::os::unix::fs::PermissionsExt;

    let _env_lock = crate::test_env_lock();
    let root = tempfile::tempdir().unwrap();
    let tenant = ManagedTenant::resolve(root.path(), "new").unwrap();
    let selected = tenant.for_agent(AgentKind::Codex);
    let editor_dir = tempfile::tempdir().unwrap();
    let editor = crate::testutil::write_stub_script(
        editor_dir.path(),
        "editor",
        "#!/bin/sh\ncase \"$1\" in\n  *config.toml*) printf 'not toml' > \"$1\" ;;\n  *auth.json*) printf 'not json' > \"$1\" ;;\nesac\n",
    );
    let _visual = EnvGuard::set("VISUAL", editor.as_os_str());

    edit_current_config(&selected).unwrap();

    assert_eq!(
        fs::read(selected.state_file("config.toml")).unwrap(),
        b"not toml"
    );
    assert_eq!(
        fs::read(selected.state_file("auth.json")).unwrap(),
        b"not json"
    );
    for file in selected.agent.config_files() {
        assert_eq!(
            fs::metadata(selected.state_file(file))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[cfg(unix)]
#[test]
fn edit_current_keeps_an_earlier_commit_when_the_next_editor_fails() {
    let _env_lock = crate::test_env_lock();
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    fs::write(selected.state_file("config.toml"), "old-main\n").unwrap();
    fs::write(selected.state_file("auth.json"), "old-auth\n").unwrap();
    let editor_dir = tempfile::tempdir().unwrap();
    let editor = crate::testutil::write_stub_script(
        editor_dir.path(),
        "editor",
        "#!/bin/sh\ncase \"$1\" in\n  *config.toml*) printf 'new-main' > \"$1\" ;;\n  *auth.json*) exit 7 ;;\nesac\n",
    );
    let _visual = EnvGuard::set("VISUAL", editor.as_os_str());

    let error = edit_current_config(&selected).unwrap_err().to_string();

    assert!(error.contains("editor exited"), "{error}");
    assert_eq!(
        fs::read(selected.state_file("config.toml")).unwrap(),
        b"new-main"
    );
    assert_eq!(
        fs::read(selected.state_file("auth.json")).unwrap(),
        b"old-auth\n"
    );
}

#[test]
fn claude_apply_sets_and_removes_fixed_fields_without_touching_statusline() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Claude);
    create_named_config(&selected, "partial").unwrap();
    replace_config_files(
        &selected,
        "partial",
        r#"{
          "env": {"ANTHROPIC_BASE_URL": "https://new"},
          "permissions": {"defaultMode": "bypassPermissions"}
        }
"#,
        "{}\n",
    );
    fs::write(
        selected.state_file("settings.json"),
        r#"{
          "env": {
            "ANTHROPIC_BASE_URL": "https://old",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": "old",
            "ANTHROPIC_AUTH_TOKEN": "old-token",
            "KEEP": "yes"
          },
          "permissions": "conflict",
          "statusLine": {"type":"command","command":"keep"}
        }
"#,
    )
    .unwrap();

    apply_named_config(&selected, "partial").unwrap();

    let settings: Value =
        serde_json::from_str(&fs::read_to_string(selected.state_file("settings.json")).unwrap())
            .unwrap();
    assert_eq!(settings["env"]["ANTHROPIC_BASE_URL"], "https://new");
    assert_eq!(settings["env"]["KEEP"], "yes");
    assert!(settings["env"].get("ANTHROPIC_AUTH_TOKEN").is_none());
    assert!(settings["env"]
        .get("ANTHROPIC_DEFAULT_HAIKU_MODEL")
        .is_none());
    assert_eq!(settings["permissions"]["defaultMode"], "bypassPermissions");
    assert_eq!(settings["statusLine"]["command"], "keep");
}

#[test]
fn codex_apply_preserves_toml_comments_unrelated_values_and_statusline() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_named_config(&selected, "partial").unwrap();
    replace_config_files(
        &selected,
        "partial",
        "model = \"new\"\n\n[model_providers.custom]\nname = \"custom\"\n",
        "{\"OPENAI_API_KEY\":\"new\"}\n",
    );
    fs::write(
        selected.state_file("config.toml"),
        "# keep comment\nmodel = \"old\"\nsandbox_mode = \"workspace-write\"\nkeep = true\n\n[tui]\nstatus_line = [\"model\"]\nstatus_line_use_colors = true\n",
    )
    .unwrap();
    fs::write(
        selected.state_file("auth.json"),
        "{\"old\":\"credential\"}\n",
    )
    .unwrap();

    apply_named_config(&selected, "partial").unwrap();

    let config = fs::read_to_string(selected.state_file("config.toml")).unwrap();
    assert!(config.contains("# keep comment"), "{config}");
    assert!(config.contains("model = \"new\""), "{config}");
    assert!(!config.contains("sandbox_mode"), "{config}");
    assert!(config.contains("keep = true"), "{config}");
    assert!(config.contains("status_line ="), "{config}");
    assert!(config.contains("status_line_use_colors = true"), "{config}");
    let auth: Value =
        serde_json::from_str(&fs::read_to_string(selected.state_file("auth.json")).unwrap())
            .unwrap();
    assert_eq!(auth, serde_json::json!({"OPENAI_API_KEY": "new"}));
}

#[test]
fn empty_config_keeps_missing_agent_files_absent() {
    let root = tempfile::tempdir().unwrap();
    for agent in AgentKind::ALL {
        let selected = selected(root.path(), agent);
        create_named_config(&selected, agent.tag()).unwrap();
        replace_config_files(
            &selected,
            agent.tag(),
            if agent == AgentKind::Claude {
                "{}\n"
            } else {
                ""
            },
            "{}\n",
        );

        apply_named_config(&selected, agent.tag()).unwrap();

        assert!(!selected
            .state_file(selected.agent.main_config_file())
            .exists());
        if let Some(auth) = selected.agent.native_auth_file() {
            assert!(!selected.state_file(auth).exists());
        }
    }
}

#[test]
fn apply_materializes_missing_codex_auth_file() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_named_config(&selected, "custom").unwrap();

    apply_named_config(&selected, "custom").unwrap();

    assert!(selected.state_file("config.toml").is_file());
    assert_eq!(
        fs::read_to_string(selected.state_file("auth.json")).unwrap(),
        "{\n  \"OPENAI_API_KEY\": \"sk-example\"\n}\n"
    );
}

#[cfg(unix)]
#[test]
fn apply_preserves_existing_modes_and_uses_0600_for_new_files() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let codex = selected(root.path(), AgentKind::Codex);
    create_named_config(&codex, "empty").unwrap();
    replace_config_files(&codex, "empty", "", "{}\n");
    fs::write(codex.state_file("config.toml"), "model = \"old\"\n").unwrap();
    fs::write(codex.state_file("auth.json"), "{\"old\":true}\n").unwrap();
    fs::set_permissions(
        codex.state_file("config.toml"),
        fs::Permissions::from_mode(0o640),
    )
    .unwrap();
    fs::set_permissions(
        codex.state_file("auth.json"),
        fs::Permissions::from_mode(0o400),
    )
    .unwrap();

    apply_named_config(&codex, "empty").unwrap();

    assert!(codex.state_file("config.toml").is_file());
    assert!(fs::read_to_string(codex.state_file("config.toml"))
        .unwrap()
        .trim()
        .is_empty());
    assert_eq!(
        fs::read_to_string(codex.state_file("auth.json")).unwrap(),
        "{}\n"
    );
    assert_eq!(
        fs::metadata(codex.state_file("config.toml"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o640
    );
    assert_eq!(
        fs::metadata(codex.state_file("auth.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o400
    );

    let claude = selected(root.path(), AgentKind::Claude);
    create_named_config(&claude, "new").unwrap();
    apply_named_config(&claude, "new").unwrap();
    assert_eq!(
        fs::metadata(claude.state_file("settings.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[test]
fn malformed_current_auth_prevents_all_codex_writes() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_named_config(&selected, "custom").unwrap();
    let original = "# untouched\nmodel = \"old\"\n";
    fs::write(selected.state_file("config.toml"), original).unwrap();
    fs::write(selected.state_file("auth.json"), "not-json\n").unwrap();

    let error = apply_named_config(&selected, "custom")
        .unwrap_err()
        .to_string();

    assert!(error.contains("Current Config auth.json"), "{error}");
    assert_eq!(
        fs::read_to_string(selected.state_file("config.toml")).unwrap(),
        original
    );
    assert_eq!(
        fs::read_to_string(selected.state_file("auth.json")).unwrap(),
        "not-json\n"
    );
}

#[test]
fn repeated_apply_is_idempotent() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_named_config(&selected, "custom").unwrap();
    apply_named_config(&selected, "custom").unwrap();
    let main = fs::read(selected.state_file("config.toml")).unwrap();
    let auth = fs::read(selected.state_file("auth.json")).unwrap();

    apply_named_config(&selected, "custom").unwrap();

    assert_eq!(fs::read(selected.state_file("config.toml")).unwrap(), main);
    assert_eq!(fs::read(selected.state_file("auth.json")).unwrap(), auth);
}

#[test]
fn delete_all_includes_invalid_and_incomplete_configs() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_named_config(&selected, "valid").unwrap();
    create_named_config(&selected, "invalid").unwrap();
    fs::write(
        selected.named_config_file("invalid", "config.toml"),
        "unknown = true\n",
    )
    .unwrap();
    tenant::ensure_real_dir(
        &selected.named_config_dir("partial"),
        "Named Config directory",
    )
    .unwrap();

    delete_named_configs(
        &selected,
        &["invalid".to_string(), "partial".to_string()],
        false,
        true,
    )
    .unwrap();
    assert!(!selected.named_config_dir("invalid").exists());
    assert!(!selected.named_config_dir("partial").exists());

    create_named_config(&selected, "invalid").unwrap();
    fs::write(
        selected.named_config_file("invalid", "config.toml"),
        "unknown = true\n",
    )
    .unwrap();
    tenant::ensure_real_dir(
        &selected.named_config_dir("partial"),
        "Named Config directory",
    )
    .unwrap();

    delete_named_configs(&selected, &[], true, true).unwrap();

    assert!(list_named_configs(&selected).unwrap().is_empty());
    assert!(!selected.named_config_dir("valid").exists());
    assert!(!selected.named_config_dir("invalid").exists());
    assert!(!selected.named_config_dir("partial").exists());
}

#[cfg(unix)]
#[test]
fn deletion_prevalidates_unsafe_targets() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_named_config(&selected, "valid").unwrap();
    fs::write(outside.path().join("keep"), "outside").unwrap();
    symlink(outside.path(), selected.named_config_dir("linked")).unwrap();

    let error = delete_named_configs(
        &selected,
        &["valid".to_string(), "linked".to_string()],
        false,
        true,
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("not a real directory"), "{error}");
    assert!(selected.named_config_dir("valid").is_dir());
    assert_eq!(
        fs::read_to_string(outside.path().join("keep")).unwrap(),
        "outside"
    );
}

#[cfg(unix)]
#[test]
fn config_catalog_permissions_are_private() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_named_config(&selected, "custom").unwrap();

    for directory in [
        selected.named_config_catalog_dir(),
        selected.named_config_dir("custom").as_path(),
    ] {
        assert_eq!(
            fs::metadata(directory).unwrap().permissions().mode() & 0o777,
            0o700,
            "{}",
            directory.display()
        );
    }
    for file in selected.agent.config_files() {
        assert_eq!(
            fs::metadata(selected.named_config_file("custom", file))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[cfg(unix)]
#[test]
fn host_apply_preserves_existing_home_and_file_modes() {
    use std::os::unix::fs::PermissionsExt;

    let _env_lock = crate::test_env_lock();
    let root = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let agent_dir = home.path().join(".claude");
    fs::create_dir(&agent_dir).unwrap();
    fs::set_permissions(&agent_dir, fs::Permissions::from_mode(0o711)).unwrap();
    let settings = agent_dir.join("settings.json");
    fs::write(&settings, "{\"theme\":\"dark\"}\n").unwrap();
    fs::set_permissions(&settings, fs::Permissions::from_mode(0o640)).unwrap();
    let _home = EnvGuard::set("HOME", home.path());
    let selected = Tenant::resolve(root.path(), true, "default")
        .unwrap()
        .for_agent(AgentKind::Claude);

    create_named_config(&selected, "custom").unwrap();
    apply_named_config(&selected, "custom").unwrap();

    assert_eq!(
        fs::metadata(agent_dir).unwrap().permissions().mode() & 0o777,
        0o711
    );
    assert_eq!(
        fs::metadata(settings).unwrap().permissions().mode() & 0o777,
        0o640
    );
    assert!(!home.path().join(".gitconfig").exists());
}

#[cfg(unix)]
#[test]
fn host_current_edit_preserves_existing_home_and_file_modes() {
    use std::os::unix::fs::PermissionsExt;

    let _env_lock = crate::test_env_lock();
    let root = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let agent_dir = home.path().join(".claude");
    fs::create_dir(&agent_dir).unwrap();
    fs::set_permissions(&agent_dir, fs::Permissions::from_mode(0o711)).unwrap();
    let settings = agent_dir.join("settings.json");
    fs::write(&settings, "old\n").unwrap();
    fs::set_permissions(&settings, fs::Permissions::from_mode(0o640)).unwrap();
    let editor_dir = tempfile::tempdir().unwrap();
    let editor = crate::testutil::write_stub_script(
        editor_dir.path(),
        "editor",
        "#!/bin/sh\nprintf 'new raw content' > \"$1\"\n",
    );
    let _home = EnvGuard::set("HOME", home.path());
    let _visual = EnvGuard::set("VISUAL", editor.as_os_str());
    let selected = Tenant::resolve(root.path(), true, "default")
        .unwrap()
        .for_agent(AgentKind::Claude);

    edit_current_config(&selected).unwrap();

    assert_eq!(fs::read(&settings).unwrap(), b"new raw content");
    assert_eq!(
        fs::metadata(agent_dir).unwrap().permissions().mode() & 0o777,
        0o711
    );
    assert_eq!(
        fs::metadata(settings).unwrap().permissions().mode() & 0o777,
        0o640
    );
    assert!(!home.path().join(".gitconfig").exists());
}

#[test]
fn editor_command_parsing_preserves_quoted_escaped_and_empty_arguments() {
    let parts = split_editor_command(OsStr::new(
        r#"code --wait "two words" 'literal $value' escaped\ space """#,
    ))
    .unwrap();
    assert_eq!(
        parts,
        [
            "code",
            "--wait",
            "two words",
            "literal $value",
            "escaped space",
            "",
        ]
        .map(OsString::from)
    );

    for invalid in ["   ", "code 'unterminated", "code trailing\\"] {
        assert!(split_editor_command(OsStr::new(invalid)).is_err());
    }
}

#[test]
fn config_deletion_requires_explicit_selection() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    let error = delete_named_configs(&selected, &[], false, true)
        .unwrap_err()
        .to_string();
    assert!(error.contains("at least one"), "{error}");

    create_named_config(&selected, "custom").unwrap();
    if !io::stdin().is_terminal() {
        let error = delete_named_configs(&selected, &["custom".to_string()], false, false)
            .unwrap_err()
            .to_string();
        assert!(error.contains("without --yes"), "{error}");
    }
}
