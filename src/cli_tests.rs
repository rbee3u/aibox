use super::*;
use clap::error::ErrorKind;

#[track_caller]
fn assert_parse_error(args: &[&str], expected: ErrorKind) {
    let error = Cli::try_parse_from(args).unwrap_err();
    assert_eq!(error.kind(), expected, "{args:?}: {error}");
}

#[test]
fn passthrough_uses_the_first_boundary() {
    let args = ["aibox", "run", "--tenant", "work", "--", "exec", "--"]
        .map(String::from)
        .to_vec();
    let (left, right) = split_passthrough(args);
    assert_eq!(left, ["aibox", "run", "--tenant", "work"]);
    assert_eq!(right, ["exec", "--"]);
}

#[test]
fn help_exposes_only_supported_commands() {
    let help = Cli::try_parse_from(["aibox", "--help"]).unwrap_err();
    assert_eq!(help.kind(), ErrorKind::DisplayHelp);
    let help = help.to_string();
    for command in ["console", "debug", "run"] {
        assert!(help.contains(command), "{command}: {help}");
    }
    for command in [
        "build",
        "completion",
        "component",
        "config",
        "serve",
        "session",
        "tenant",
    ] {
        assert!(!help.contains(command), "{command}: {help}");
    }
}

#[test]
fn removed_commands_are_unknown() {
    for command in [
        "build",
        "completion",
        "component",
        "config",
        "serve",
        "session",
        "tenant",
    ] {
        assert_parse_error(&["aibox", command], ErrorKind::InvalidSubcommand);
    }
}

#[test]
fn combined_short_options_are_rejected_without_blocking_attached_values() {
    assert_parse_error(&["aibox", "run", "-xy"], ErrorKind::UnknownArgument);

    let cli = Cli::try_parse_from(["aibox", "run", "-w.", "-msrc:/src:ro"]).unwrap();
    let Command::Run(args) = cli.command else {
        panic!("expected run command");
    };
    assert_eq!(args.workspace.as_deref(), Some("."));
    assert_eq!(args.mount, ["src:/src:ro"]);
}

#[test]
fn selection_and_options_stay_in_their_command_scopes() {
    for args in [
        &["aibox", "console", "--tenant", "work"][..],
        &["aibox", "run", "--host"][..],
        &["aibox", "debug", "--agent", "codex"][..],
        &["aibox", "debug", "--workspace", "."][..],
        &["aibox", "debug", "--mount", "src:/src"][..],
        &["aibox", "debug", "work"][..],
    ] {
        assert_parse_error(args, ErrorKind::UnknownArgument);
    }

    Cli::try_parse_from(["aibox", "run", "--agent", "claude", "--tenant", "work"]).unwrap();
    Cli::try_parse_from(["aibox", "console", "--listen", "0.0.0.0:8080"]).unwrap();
}

#[test]
fn duplicate_selection_options_are_rejected_before_passthrough() {
    for args in [
        &["aibox", "run", "--tenant", "one", "--tenant=two"][..],
        &["aibox", "run", "--agent=claude", "--agent", "codex"][..],
        &["aibox", "debug", "--tenant", "one", "--tenant=two"][..],
    ] {
        let error = Cli::try_parse_from(args).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::ArgumentConflict, "{args:?}");
    }

    let args = [
        "aibox",
        "run",
        "--tenant",
        "work",
        "--",
        "--tenant",
        "agent-value",
    ];
    let (aibox_args, passthrough) = split_passthrough(args.to_vec());
    Cli::try_parse_from(aibox_args).unwrap();
    assert_eq!(passthrough, ["--tenant", "agent-value"]);
}

#[test]
fn managed_tenant_named_host_remains_runnable() {
    let cli = Cli::try_parse_from(["aibox", "run", "--tenant", "host"]).unwrap();
    let Command::Run(args) = cli.command else {
        panic!("expected run command");
    };
    assert_eq!(args.tenant.as_deref(), Some("host"));
}

#[test]
fn debug_selects_a_managed_tenant_and_defaults_to_default() {
    let cli = Cli::try_parse_from(["aibox", "debug"]).unwrap();
    let Command::Debug(args) = cli.command else {
        panic!("expected debug command");
    };
    assert_eq!(args.tenant, None);

    let cli = Cli::try_parse_from(["aibox", "debug", "--tenant", "host"]).unwrap();
    let Command::Debug(args) = cli.command else {
        panic!("expected debug command");
    };
    assert_eq!(args.tenant.as_deref(), Some("host"));

    assert_parse_error(
        &["aibox", "debug", "--tenant", "Invalid"],
        ErrorKind::ValueValidation,
    );
}

#[test]
fn console_requires_a_nonzero_ip_socket() {
    let cli = Cli::try_parse_from(["aibox", "console"]).unwrap();
    let Command::Console(args) = cli.command else {
        panic!("expected console command");
    };
    assert_eq!(args.listen, "127.0.0.1:9923".parse().unwrap());

    for value in ["localhost:9923", "127.0.0.1:0"] {
        assert_parse_error(
            &["aibox", "console", "--listen", value],
            ErrorKind::ValueValidation,
        );
    }
    let cli = Cli::try_parse_from(["aibox", "console", "--listen", "0.0.0.0:8080"]).unwrap();
    let Command::Console(args) = cli.command else {
        panic!("expected console command");
    };
    assert_eq!(args.listen, "0.0.0.0:8080".parse().unwrap());
}
