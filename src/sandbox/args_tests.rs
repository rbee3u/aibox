use super::*;

#[test]
fn assemble_run_args_keeps_sandbox_flags_and_mount_order() {
    let args = assemble_run_args(
        "/abs/workspace",
        Path::new("/abs/tenant"),
        &["/abs/cache:/cache:ro".to_string()],
    );
    let mut expected = vec![
        "--rm".to_string(),
        if platform::has_tty() { "-it" } else { "-i" }.to_string(),
        "--security-opt".to_string(),
        "no-new-privileges".to_string(),
        "--cap-drop".to_string(),
        "ALL".to_string(),
    ];
    if platform::is_linux() {
        let (uid, gid) = platform::uid_gid();
        expected.extend([
            "--user".to_string(),
            format!("{uid}:{gid}"),
            "--add-host".to_string(),
            "host.docker.internal:host-gateway".to_string(),
        ]);
    }
    expected.extend([
        "-v".to_string(),
        "/abs/tenant:/home/aibox".to_string(),
        "-v".to_string(),
        "/abs/workspace:/workspace".to_string(),
        "-w".to_string(),
        "/workspace".to_string(),
        "-v".to_string(),
        "/abs/cache:/cache:ro".to_string(),
    ]);

    assert_eq!(args, expected);
}

#[test]
fn component_run_args_mount_only_the_tenant_home() {
    let args = assemble_component_run_args(Path::new("/abs/tenant"));
    let mut expected = vec![
        "--rm".to_string(),
        "--security-opt".to_string(),
        "no-new-privileges".to_string(),
        "--cap-drop".to_string(),
        "ALL".to_string(),
    ];
    if platform::is_linux() {
        let (uid, gid) = platform::uid_gid();
        expected.extend([
            "--user".to_string(),
            format!("{uid}:{gid}"),
            "--add-host".to_string(),
            "host.docker.internal:host-gateway".to_string(),
        ]);
    }
    expected.extend([
        "-v".to_string(),
        "/abs/tenant:/home/aibox".to_string(),
        "-w".to_string(),
        "/home/aibox".to_string(),
    ]);

    assert_eq!(args, expected);
}

#[test]
fn debug_args_are_interactive_and_mount_only_the_tenant_home() {
    let args = assemble_debug_args(Path::new("/abs/tenant"));
    let mut expected = vec![
        "--rm".to_string(),
        if platform::has_tty() { "-it" } else { "-i" }.to_string(),
        "--security-opt".to_string(),
        "no-new-privileges".to_string(),
        "--cap-drop".to_string(),
        "ALL".to_string(),
    ];
    if platform::is_linux() {
        let (uid, gid) = platform::uid_gid();
        expected.extend([
            "--user".to_string(),
            format!("{uid}:{gid}"),
            "--add-host".to_string(),
            "host.docker.internal:host-gateway".to_string(),
        ]);
    }
    expected.extend([
        "-v".to_string(),
        "/abs/tenant:/home/aibox".to_string(),
        "-w".to_string(),
        "/home/aibox".to_string(),
    ]);

    assert_eq!(args, expected);
    assert!(!args.iter().any(|arg| arg == "/workspace"));
}

#[test]
fn debug_args_select_tty_or_stdin_mode_from_the_terminal_state() {
    for (has_tty, expected) in [(true, "-it"), (false, "-i")] {
        let args = assemble_debug_args_for_terminal(Path::new("/abs/tenant"), has_tty);
        assert_eq!(args.get(1).map(String::as_str), Some(expected));
    }
}
