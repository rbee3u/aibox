use super::*;
use serde_json::Value;
use std::io::Cursor;

struct TestBackend;

impl SessionBackend for TestBackend {
    fn session_dir_components(&self) -> &'static [&'static str] {
        &["sessions"]
    }

    fn keep_transcript_name(&self, _name: &str) -> bool {
        true
    }

    fn id_of(&self, path: &Path) -> String {
        path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string()
    }

    fn prompt_record(&self, v: &Value) -> PromptRecord {
        if v.get("unsupported").and_then(Value::as_bool) == Some(true) {
            return PromptRecord::UnsupportedUserLike;
        }
        match v.get("typed").and_then(Value::as_str) {
            Some(text) if v.get("partial").and_then(Value::as_bool) == Some(true) => {
                PromptRecord::TypedWithUnsupported(text.to_string())
            }
            Some(text) => PromptRecord::Typed(text.to_string()),
            None => PromptRecord::NotTyped,
        }
    }

    fn start_ts_of(&self, v: &Value) -> Option<String> {
        v.get("ts").and_then(Value::as_str).map(str::to_string)
    }
}

fn write_session(home: &Path, id: &str) -> PathBuf {
    let path = home.join("sessions").join(format!("{id}.jsonl"));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "{}\n").unwrap();
    path
}

#[test]
fn transcript_line_reader_rejects_oversized_lines_without_reading_the_whole_file() {
    let home = tempfile::tempdir().unwrap();
    let path = write_session(home.path(), "oversized");
    std::fs::write(&path, vec![b'x'; 33]).unwrap();

    let error = for_each_json_line_with_limit(home.path(), &path, 32, |_| {})
        .unwrap_err()
        .to_string();

    assert!(
        error.contains("line 1 exceeds the 32 byte limit"),
        "{error}"
    );
}

#[test]
fn transcript_line_limit_accepts_exact_size_records_with_jsonl_delimiters() {
    let home = tempfile::tempdir().unwrap();
    let path = write_session(home.path(), "exact");
    let record = r#"{"typed":"ok"}"#;

    for delimiter in ["", "\n", "\r\n"] {
        std::fs::write(&path, format!("{record}{delimiter}")).unwrap();
        let mut visits = 0;

        for_each_json_line_with_limit(home.path(), &path, record.len() as u64, |_| visits += 1)
            .unwrap();

        assert_eq!(visits, 1, "delimiter {delimiter:?}");
    }
}

#[test]
fn diagnostic_records_keep_readable_output_and_report_their_exact_kind() {
    for (case, body, expected_diagnostics) in [
        (
            "malformed JSON",
            "not-json\n{\"typed\":\"visible\",\"ts\":\"2026-01-01T00:00:00Z\",\"timestamp\":\"2026-01-01T00:00:00Z\"}\n",
            TranscriptDiagnostics {
                malformed_lines: 1,
                unsupported_user_records: 0,
            },
        ),
        (
            "unsupported user record",
            "{\"unsupported\":true}\n{\"typed\":\"visible\",\"ts\":\"2026-01-01T00:00:00Z\",\"timestamp\":\"2026-01-01T00:00:00Z\"}\n",
            TranscriptDiagnostics {
                malformed_lines: 0,
                unsupported_user_records: 1,
            },
        ),
        (
            "partially supported prompt",
            "{\"typed\":\"visible\",\"partial\":true,\"ts\":\"2026-01-01T00:00:00Z\",\"timestamp\":\"2026-01-01T00:00:00Z\"}\n",
            TranscriptDiagnostics {
                malformed_lines: 0,
                unsupported_user_records: 1,
            },
        ),
    ] {
        let home = tempfile::tempdir().unwrap();
        let path = write_session(home.path(), "diagnostic");
        std::fs::write(&path, body).unwrap();

        let summary = TestBackend.summarize(&path).unwrap();
        assert_eq!(summary.diagnostics, expected_diagnostics, "{case}");

        let mut rows = Vec::new();
        let list_code = list_with_printer(&TestBackend, home.path(), |line| {
            rows.push(line.to_string());
            Ok(true)
        })
        .unwrap();
        assert_eq!(list_code, 1, "{case}");
        assert_eq!(rows, ["diagnostic    2026-01-01 00:00  visible"], "{case}");

        let mut prompts = Vec::new();
        let get_code = get_with_printer(&TestBackend, home.path(), "diagnostic", |line| {
            prompts.push(line.to_string());
            Ok(true)
        })
        .unwrap();
        assert_eq!(get_code, 1, "{case}");
        assert_eq!(prompts, ["\n[1] 2026-01-01 00:00\nvisible"], "{case}");
    }
}

#[test]
fn recognized_non_prompt_records_are_a_clean_empty_prompt_view() {
    let home = tempfile::tempdir().unwrap();
    write_session(home.path(), "empty");
    let mut rows = Vec::new();
    assert_eq!(
        list_with_printer(&TestBackend, home.path(), |line| {
            rows.push(line.to_string());
            Ok(true)
        })
        .unwrap(),
        0
    );
    assert_eq!(rows.len(), 1, "the empty prompt view must remain listable");
    assert!(rows[0].starts_with("empty"), "{rows:?}");
    let mut output = Vec::new();
    assert_eq!(
        get_with_printer(&TestBackend, home.path(), "empty", |line| {
            output.push(line.to_string());
            Ok(true)
        })
        .unwrap(),
        0
    );
    assert_eq!(output, ["(no typed prompts in this session)"]);
}

#[test]
fn session_display_escapes_terminal_controls_from_container_owned_data() {
    assert_eq!(terminal_safe("普通\x1b[2J\n"), "普通\\u{1b}[2J\\n");
    assert_eq!(
        terminal_safe_prompt("first\n\tsecond\x1b[2J"),
        "first\n\tsecond\\u{1b}[2J"
    );

    let home = tempfile::tempdir().unwrap();
    write_session(home.path(), "\x1b[2Jmalicious");
    let mut lines = Vec::new();
    let code = list_with_printer(&TestBackend, home.path(), |line| {
        lines.push(line.to_string());
        Ok(true)
    })
    .unwrap();

    assert_eq!(code, 0);
    assert_eq!(lines.len(), 1);
    assert!(!lines[0].contains('\x1b'), "{:?}", lines[0]);
    assert!(lines[0].contains("\\u{1b}"), "{:?}", lines[0]);
}

struct ExplicitFilesBackend {
    files: Vec<PathBuf>,
    list_errors: Vec<String>,
    files_error: Option<String>,
}

impl ExplicitFilesBackend {
    fn new(files: Vec<PathBuf>) -> Self {
        ExplicitFilesBackend {
            files,
            list_errors: Vec::new(),
            files_error: None,
        }
    }

    fn with_list_errors(files: Vec<PathBuf>, list_errors: Vec<String>) -> Self {
        ExplicitFilesBackend {
            files,
            list_errors,
            files_error: None,
        }
    }

    fn with_files_error(message: &str) -> Self {
        ExplicitFilesBackend {
            files: Vec::new(),
            list_errors: Vec::new(),
            files_error: Some(message.to_string()),
        }
    }
}

impl SessionBackend for ExplicitFilesBackend {
    // Never reached: this backend overrides both discovery walks with its
    // explicit lists, which is the point — the shared list/get/delete
    // logic under test takes whatever discovery hands it.
    fn session_dir_components(&self) -> &'static [&'static str] {
        &[]
    }

    fn keep_transcript_name(&self, _name: &str) -> bool {
        true
    }

    fn files(&self, _home: &Path) -> Result<Vec<PathBuf>> {
        if let Some(message) = &self.files_error {
            bail!("{message}");
        }
        Ok(self.files.clone())
    }

    fn list_files(&self, _home: &Path) -> Result<SessionDiscovery> {
        Ok(SessionDiscovery {
            files: self.files.clone(),
            errors: self.list_errors.clone(),
        })
    }

    fn id_of(&self, path: &Path) -> String {
        path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string()
    }

    fn prompt_record(&self, v: &Value) -> PromptRecord {
        v.get("typed")
            .and_then(Value::as_str)
            .map_or(PromptRecord::NotTyped, |text| {
                PromptRecord::Typed(text.to_string())
            })
    }

    fn start_ts_of(&self, v: &Value) -> Option<String> {
        v.get("ts").and_then(Value::as_str).map(str::to_string)
    }
}

#[test]
fn timestamps_are_formatted_to_minutes_and_missing_values_stay_empty() {
    assert_eq!(fmt_ts("2026-07-14T02:16:33.123Z"), "2026-07-14 02:16");
    assert_eq!(fmt_ts(""), "");
}

#[test]
fn list_whitespace_normalization_removes_control_runs_but_preserves_spaces() {
    assert_eq!(collapse_ws("a\n\nb\tc"), "a b c");
    assert_eq!(collapse_ws("a\rb\u{7f}c\u{00a0}d"), "a b c d");
    assert_eq!(collapse_ws("a  b"), "a  b");
    assert_eq!(collapse_ws("plain"), "plain");
}

#[test]
fn list_title_collapses_and_truncates_to_64_chars() {
    assert_eq!(list_title("a\n\nb\tc"), "a b c");
    let long: String = "0123456789".repeat(7); // 70 chars
    assert_eq!(list_title(&long), long[..64].to_string());
    assert_eq!(list_title(&long).chars().count(), 64);

    let multibyte = "é".repeat(70);
    assert_eq!(list_title(&multibyte), "é".repeat(64));
    assert_eq!(list_title(&multibyte).chars().count(), 64);
}

#[test]
fn list_shows_non_uuid_session_ids_in_full() {
    let dir = tempfile::tempdir().unwrap();
    let id = "é".repeat(14);
    let path = dir.path().join(format!("{id}.jsonl"));
    std::fs::write(&path, "{\"typed\":\"bonjour\"}\n").unwrap();
    let backend = ExplicitFilesBackend::new(vec![path]);
    let mut lines = Vec::new();

    let code = list_with_printer(&backend, dir.path(), |line| {
        lines.push(line.to_string());
        Ok(true)
    })
    .unwrap();

    assert_eq!(code, 0);
    assert_eq!(lines.len(), 1);
    assert!(
        lines[0].starts_with(&format!("{id}  ")),
        "non-UUID ids must be shown in full: {lines:?}"
    );
}

#[test]
fn list_uses_the_final_group_of_canonical_uuid_ids() {
    for (id, expected) in [
        ("019fded0-6b15-7163-8881-458cbf92d123", "458cbf92d123"),
        ("019fded0-e5a1-74c1-a7dd-7cc52d16f280", "7cc52d16f280"),
        ("019FDED0-F91B-7893-A4C9-91B8FFE164EA", "91B8FFE164EA"),
    ] {
        assert_eq!(list_id(id), expected);
    }

    let compact = "019fded06b1571638881458cbf92d123";
    assert_eq!(list_id(compact), compact);
    let malformed = "019fded0-6b15-7163-8881-458cbf92d12z";
    assert_eq!(list_id(malformed), malformed);
    assert_eq!(list_id("claude-session"), "claude-session");
}

#[test]
fn list_keeps_duplicate_uuid_suffixes_fixed_at_twelve_characters() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir
        .path()
        .join("aaaaaaaa-aaaa-4aaa-8aaa-000000000001.jsonl");
    let second = dir
        .path()
        .join("bbbbbbbb-bbbb-7bbb-8bbb-000000000001.jsonl");
    std::fs::write(&first, "{\"typed\":\"first\"}\n").unwrap();
    std::fs::write(&second, "{\"typed\":\"second\"}\n").unwrap();
    let backend = ExplicitFilesBackend::new(vec![first, second]);
    let mut lines = Vec::new();

    list_with_printer(&backend, dir.path(), |line| {
        lines.push(line.to_string());
        Ok(true)
    })
    .unwrap();

    assert_eq!(lines.len(), 2);
    assert!(
        lines.iter().all(|line| line.starts_with("000000000001  ")),
        "colliding UUID suffixes must remain fixed-width duplicate tokens: {lines:?}"
    );

    let error = resolve(&backend, dir.path(), "000000000001")
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("ambiguous id '000000000001' matches 2 sessions"),
        "{error}"
    );
}

#[test]
fn non_utf8_transcript_is_reported_by_list_and_fails_the_read_paths() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("33333333.jsonl");
    // Valid line, then a lone continuation byte: read_line errors on it.
    std::fs::write(&path, b"{\"typed\":\"ok\"}\n\xff\xfe").unwrap();
    let backend = ExplicitFilesBackend::new(vec![path.clone()]);

    let err = backend
        .prompts(&path)
        .err()
        .expect("invalid UTF-8 must not read as an empty prompt list")
        .to_string();
    assert!(err.contains("read session transcript"), "{err}");
    assert!(err.contains("33333333.jsonl"), "{err}");

    let err = get_with_printer(&backend, dir.path(), "3333", |_| Ok(true))
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("read session transcript"),
        "get must surface the read failure: {err}"
    );

    let mut lines = Vec::new();
    let code = list_with_printer(&backend, dir.path(), |line| {
        lines.push(line.to_string());
        Ok(true)
    })
    .unwrap();
    assert_eq!(code, 1, "an unreadable transcript makes list non-zero");
    assert!(
        lines.is_empty(),
        "no row for a transcript that failed to read"
    );
}

#[test]
fn list_skips_bad_transcripts_but_returns_nonzero() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing.jsonl");
    let good = dir.path().join("good.jsonl");
    std::fs::write(&good, "{\"typed\":\"hello\"}\n").unwrap();
    let backend = ExplicitFilesBackend::new(vec![missing, good]);
    let mut lines = Vec::new();

    let code = list_with_printer(&backend, dir.path(), |line| {
        lines.push(line.to_string());
        Ok(true)
    })
    .unwrap();

    assert_eq!(code, 1, "one skipped transcript makes list non-zero");
    assert_eq!(lines.len(), 1, "the readable session still lists");
    assert!(lines[0].contains("good"));
    assert!(lines[0].contains("hello"));
}

#[test]
fn get_prints_numbered_timestamped_prompts_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("11111111.jsonl");
    std::fs::write(
        &path,
        "\
{\"timestamp\":\"2026-07-14T02:16:33.123Z\",\"typed\":\"first ask\"}
{\"timestamp\":\"2026-07-14T02:18:00Z\",\"typed\":\"second ask\"}
",
    )
    .unwrap();
    let backend = ExplicitFilesBackend::new(vec![path]);
    let mut printed = Vec::new();

    let code = get_with_printer(&backend, dir.path(), "1111", |line| {
        printed.push(line.to_string());
        Ok(true)
    })
    .unwrap();

    assert_eq!(code, 0);
    assert_eq!(
        printed,
        vec![
            "\n[1] 2026-07-14 02:16\nfirst ask".to_string(),
            "\n[2] 2026-07-14 02:18\nsecond ask".to_string(),
        ],
        "get numbers prompts from 1 and shows each turn's minute-precision timestamp"
    );
}

#[test]
fn get_stops_cleanly_when_printer_hangs_up() {
    // `session get | head` closes the pipe; the Rust runtime ignores
    // SIGPIPE, so this must stop reading and writing instead of reaching
    // malformed data later in a large transcript.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("33333333.jsonl");
    std::fs::write(&path, b"{\"typed\":\"first\"}\n\xff\xfe").unwrap();
    let backend = ExplicitFilesBackend::new(vec![path]);
    let mut printed = Vec::new();

    let code = get_with_printer(&backend, dir.path(), "3333", |line| {
        printed.push(line.to_string());
        Ok(false)
    })
    .unwrap();

    assert_eq!(code, 0);
    assert_eq!(
        printed.len(),
        1,
        "get stops after a broken-pipe-style false"
    );
}

#[test]
fn get_still_fails_fast_on_bad_transcript() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing.jsonl");
    let backend = ExplicitFilesBackend::new(vec![missing]);

    let err = get(&backend, dir.path(), "missing")
        .unwrap_err()
        .to_string();

    assert!(err.contains("open session transcript"), "{err}");
}

#[test]
fn list_reports_discovery_errors_but_keeps_readable_transcripts() {
    let dir = tempfile::tempdir().unwrap();
    let good = dir.path().join("good.jsonl");
    std::fs::write(&good, "{\"typed\":\"hello\"}\n").unwrap();
    let backend = ExplicitFilesBackend::with_list_errors(
        vec![good],
        vec!["walk session directory /sessions: permission denied".to_string()],
    );
    let mut lines = Vec::new();

    let code = list_with_printer(&backend, dir.path(), |line| {
        lines.push(line.to_string());
        Ok(true)
    })
    .unwrap();

    assert_eq!(code, 1, "discovery errors make list non-zero");
    assert_eq!(lines.len(), 1, "readable sessions still list");
    assert!(lines[0].contains("hello"));
}

#[test]
fn list_orders_sessions_newest_first() {
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join("old.jsonl");
    let new = dir.path().join("new.jsonl");
    std::fs::write(
        &old,
        "{\"ts\":\"2026-07-14T02:16:00Z\",\"typed\":\"old prompt\"}\n",
    )
    .unwrap();
    std::fs::write(
        &new,
        "{\"ts\":\"2026-07-14T02:17:00Z\",\"typed\":\"new prompt\"}\n",
    )
    .unwrap();
    let backend = ExplicitFilesBackend::new(vec![old, new]);
    let mut lines = Vec::new();

    let code = list_with_printer(&backend, dir.path(), |line| {
        lines.push(line.to_string());
        Ok(true)
    })
    .unwrap();

    assert_eq!(code, 0);
    assert_eq!(lines.len(), 2);
    assert!(
        lines[0].contains("new prompt") && lines[1].contains("old prompt"),
        "list rows should be newest first: {lines:?}"
    );
}

#[test]
fn list_places_sessions_without_timestamps_after_timestamped_rows() {
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join("old.jsonl");
    let new = dir.path().join("new.jsonl");
    let no_ts = dir.path().join("no-ts.jsonl");
    std::fs::write(
        &old,
        "{\"ts\":\"2026-07-14T02:16:00Z\",\"typed\":\"old prompt\"}\n",
    )
    .unwrap();
    std::fs::write(
        &new,
        "{\"ts\":\"2026-07-14T02:17:00Z\",\"typed\":\"new prompt\"}\n",
    )
    .unwrap();
    std::fs::write(&no_ts, "{\"typed\":\"no timestamp\"}\n").unwrap();
    let backend = ExplicitFilesBackend::new(vec![no_ts, new, old]);
    let mut lines = Vec::new();

    let code = list_with_printer(&backend, dir.path(), |line| {
        lines.push(line.to_string());
        Ok(true)
    })
    .unwrap();

    assert_eq!(code, 0);
    assert_eq!(lines.len(), 3);
    assert!(
        lines[0].contains("new prompt")
            && lines[1].contains("old prompt")
            && lines[2].contains("no timestamp"),
        "timestamp-less sessions should not sort above real timestamps: {lines:?}"
    );
}

#[test]
fn list_stops_cleanly_when_printer_hangs_up() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("first.jsonl");
    let second = dir.path().join("second.jsonl");
    std::fs::write(
        &first,
        "{\"ts\":\"2026-07-14T02:17:00Z\",\"typed\":\"first\"}\n",
    )
    .unwrap();
    std::fs::write(
        &second,
        "{\"ts\":\"2026-07-14T02:16:00Z\",\"typed\":\"second\"}\n",
    )
    .unwrap();
    let backend = ExplicitFilesBackend::new(vec![first, second]);
    let mut printed = Vec::new();

    let code = list_with_printer(&backend, dir.path(), |line| {
        printed.push(line.to_string());
        Ok(false)
    })
    .unwrap();

    assert_eq!(code, 0);
    assert_eq!(
        printed.len(),
        1,
        "list should stop writing after a broken-pipe-style false"
    );
    assert!(printed[0].contains("first"));
}

#[test]
fn get_and_delete_still_fail_fast_on_discovery_errors() {
    let dir = tempfile::tempdir().unwrap();
    let backend = ExplicitFilesBackend::with_files_error("discovery failed");

    let err = get(&backend, dir.path(), "anything")
        .unwrap_err()
        .to_string();
    assert!(err.contains("discovery failed"), "{err}");

    let err = delete(&backend, dir.path(), &[], true, true)
        .unwrap_err()
        .to_string();
    assert!(err.contains("discovery failed"), "{err}");
}

#[test]
fn resolved_snapshots_cannot_read_or_delete_outside_or_non_normalized_paths() {
    let home = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let outside_path = outside.path().join("outside.jsonl");
    let inside_path = home.path().join("inside.jsonl");
    std::fs::write(&outside_path, "{\"typed\":\"outside\"}\n").unwrap();
    std::fs::write(&inside_path, "{\"typed\":\"inside\"}\n").unwrap();

    for (candidate, expected) in [
        (outside_path.clone(), "outside tenant home"),
        (
            home.path().join("sessions/../inside.jsonl"),
            "not a normalized child",
        ),
    ] {
        let backend = ExplicitFilesBackend::new(vec![candidate.clone()]);
        let id = backend.id_of(&candidate);
        let err = get_with_printer(&backend, home.path(), &id, |_| Ok(true))
            .unwrap_err()
            .to_string();
        assert!(err.contains(expected), "{candidate:?}: {err}");

        let mut input = Cursor::new(Vec::<u8>::new());
        let err = delete_targets_with_input(
            &backend,
            home.path(),
            vec![candidate.clone()],
            true,
            &mut input,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains(expected), "{candidate:?}: {err}");
    }

    assert!(outside_path.exists());
    assert!(inside_path.exists());
}

#[cfg(unix)]
#[test]
fn session_discovery_does_not_follow_transcript_symlinks() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let outside_file = dir.path().join("outside.jsonl");
    std::fs::write(&outside_file, "{}\n").unwrap();
    let outside_dir = dir.path().join("outside-dir");
    std::fs::create_dir(&outside_dir).unwrap();
    std::fs::write(outside_dir.join("nested.jsonl"), "{}\n").unwrap();
    let sessions = dir.path().join("sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    symlink(&outside_file, sessions.join("linked.jsonl")).unwrap();
    symlink(&outside_dir, sessions.join("linked-dir")).unwrap();

    let files = TestBackend.files(dir.path()).unwrap();

    assert!(
        files.is_empty(),
        "host-side browsing must not follow transcript or directory symlinks"
    );
}

#[cfg(unix)]
#[test]
fn get_rejects_symlinked_transcript_even_from_a_resolved_snapshot() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let outside = dir.path().join("outside.jsonl");
    let link = dir.path().join("11111111.jsonl");
    std::fs::write(&outside, "{\"typed\":\"outside\"}\n").unwrap();
    symlink(&outside, &link).unwrap();
    let backend = ExplicitFilesBackend::new(vec![link]);

    let err = get_with_printer(&backend, dir.path(), "1111", |_| Ok(true))
        .unwrap_err()
        .to_string();

    assert!(
        err.contains("session transcript is not a regular file"),
        "{err}"
    );
}

#[cfg(unix)]
#[test]
fn delete_rejects_a_transcript_replaced_by_a_symlink_after_discovery() {
    use std::os::unix::fs::symlink;

    let home = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let transcript = write_session(home.path(), "11111111");
    let targets = delete_targets(&TestBackend, home.path(), &["1111".to_string()], false).unwrap();
    assert_eq!(crate::testutil::only(&targets), &transcript);

    std::fs::remove_file(&transcript).unwrap();
    let outside_transcript = outside.path().join("outside.jsonl");
    std::fs::write(&outside_transcript, "{\"typed\":\"outside\"}\n").unwrap();
    symlink(&outside_transcript, &transcript).unwrap();

    let mut input = Cursor::new(Vec::<u8>::new());
    let err = delete_targets_with_input(&TestBackend, home.path(), targets, true, &mut input)
        .unwrap_err()
        .to_string();

    assert!(
        err.contains("session transcript is not a regular file"),
        "{err}"
    );
    assert_eq!(
        std::fs::read_to_string(&outside_transcript).unwrap(),
        "{\"typed\":\"outside\"}\n"
    );
    assert!(
        transcript
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink(),
        "a failed delete must leave the replacement symlink itself untouched"
    );
}

#[cfg(unix)]
#[test]
fn get_rejects_fifo_replacement_without_blocking() {
    use std::os::unix::ffi::OsStrExt;

    let dir = tempfile::tempdir().unwrap();
    let fifo = dir.path().join("11111111.jsonl");
    let fifo_path = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
    // SAFETY: `fifo_path` is a live NUL-terminated path and the mode contains
    // only permission bits; the return value is checked before using the path.
    let result = unsafe { libc::mkfifo(fifo_path.as_ptr(), 0o600) };
    assert_eq!(result, 0, "create FIFO: {}", io::Error::last_os_error());
    let backend = ExplicitFilesBackend::new(vec![fifo]);

    let err = get_with_printer(&backend, dir.path(), "1111", |_| Ok(true))
        .unwrap_err()
        .to_string();

    assert!(
        err.contains("session transcript is not a regular file"),
        "{err}"
    );
}

#[cfg(unix)]
#[test]
fn reads_do_not_follow_an_ancestor_replaced_after_discovery() {
    use std::os::unix::fs::symlink;

    let home = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let transcript = write_session(home.path(), "11111111");
    let snapshot = TestBackend.files(home.path()).unwrap();
    assert_eq!(snapshot, [transcript]);

    std::fs::remove_file(&snapshot[0]).unwrap();
    std::fs::remove_dir(home.path().join("sessions")).unwrap();
    std::fs::write(
        outside.path().join("11111111.jsonl"),
        "{\"typed\":\"outside\"}\n",
    )
    .unwrap();
    symlink(outside.path(), home.path().join("sessions")).unwrap();

    let err = TestBackend
        .prompts_in(home.path(), &snapshot[0])
        .err()
        .expect("a replaced ancestor must be rejected")
        .to_string();

    assert!(err.contains("open session path"), "{err}");
    assert!(
        outside.path().join("11111111.jsonl").exists(),
        "reading a resolved snapshot must not follow a replaced ancestor"
    );
}

#[cfg(unix)]
#[test]
fn delete_does_not_follow_an_ancestor_replaced_after_discovery() {
    use std::os::unix::fs::symlink;

    let home = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let transcript = write_session(home.path(), "11111111");
    let targets = delete_targets(&TestBackend, home.path(), &["1111".to_string()], false).unwrap();
    assert_eq!(targets, [transcript]);

    std::fs::remove_file(&targets[0]).unwrap();
    std::fs::remove_dir(home.path().join("sessions")).unwrap();
    let outside_transcript = outside.path().join("11111111.jsonl");
    std::fs::write(&outside_transcript, "{}\n").unwrap();
    symlink(outside.path(), home.path().join("sessions")).unwrap();

    let mut input = Cursor::new(Vec::<u8>::new());
    let err = delete_targets_with_input(&TestBackend, home.path(), targets, true, &mut input)
        .unwrap_err()
        .to_string();

    assert!(err.contains("open session path"), "{err}");
    assert!(
        outside_transcript.exists(),
        "deleting a resolved snapshot must not follow a replaced ancestor"
    );
}

#[cfg(unix)]
#[test]
fn tolerant_session_discovery_does_not_follow_transcript_symlinks() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let outside = dir.path().join("outside.jsonl");
    std::fs::write(&outside, "{}\n").unwrap();
    let sessions = dir.path().join("sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    symlink(&outside, sessions.join("linked.jsonl")).unwrap();

    let discovery = walk_jsonl_tolerant(&sessions, |_| true).unwrap();

    assert!(
        discovery.files.is_empty(),
        "list's tolerant walk must not follow transcript-shaped symlinks"
    );
    assert!(
        discovery.errors.is_empty(),
        "skipped transcript symlinks should not be reported as walk failures"
    );
}

#[test]
fn session_discovery_rejects_a_non_directory_session_path() {
    // A file where the transcript tree should be is a broken tenant, not an
    // empty one: reporting "no sessions" would hide it.
    let dir = tempfile::tempdir().unwrap();
    let sessions = dir.path().join("sessions");
    std::fs::write(&sessions, "not a directory\n").unwrap();

    let err = walk_jsonl(&sessions, |_| true).unwrap_err().to_string();
    assert!(err.contains("session path is not a directory"), "{err}");

    let err = walk_jsonl_tolerant(&sessions, |_| true)
        .map(|_| ())
        .unwrap_err()
        .to_string();
    assert!(err.contains("session path is not a directory"), "{err}");
}

#[test]
fn session_discovery_reports_no_files_for_a_missing_tree() {
    // A tenant that has never run an agent has no transcript dir at all;
    // that is empty, not an error.
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("never-used");

    assert!(walk_jsonl(&missing, |_| true).unwrap().is_empty());
    let discovery = walk_jsonl_tolerant(&missing, |_| true).unwrap();
    assert!(discovery.files.is_empty());
    assert!(discovery.errors.is_empty());
}

#[cfg(unix)]
#[test]
fn tolerant_walk_reports_unreadable_subdirectories_without_hiding_readable_ones() {
    // `list`'s walk is tolerant on purpose: one unreadable child dir must be
    // reported while every readable transcript still lists.
    let dir = tempfile::tempdir().unwrap();
    let sessions = dir.path().join("sessions");
    let readable = sessions.join("readable");
    let locked = sessions.join("locked");
    std::fs::create_dir_all(&readable).unwrap();
    std::fs::create_dir_all(&locked).unwrap();
    let good = readable.join("11111111.jsonl");
    std::fs::write(&good, "{\"typed\":\"hello\"}\n").unwrap();
    std::fs::write(locked.join("22222222.jsonl"), "{}\n").unwrap();
    let lock = crate::testutil::UnreadableDir::new(&locked);

    let discovery = walk_jsonl_tolerant(&sessions, |_| true).unwrap();
    lock.restore();

    assert_eq!(
        discovery.files,
        vec![good],
        "the readable transcript must still be discovered"
    );
    assert_eq!(
        discovery.errors.len(),
        1,
        "the unreadable subdirectory is reported: {:?}",
        discovery.errors
    );
    assert!(
        discovery.errors[0].contains("walk session directory"),
        "{:?}",
        discovery.errors
    );

    // The strict walk `get`/`delete` use instead fails fast: a destructive
    // or single-target action must not act on a partial view of the tree.
    let lock = crate::testutil::UnreadableDir::new(&locked);
    let strict = walk_jsonl(&sessions, |_| true);
    lock.restore();
    let err = strict.unwrap_err().to_string();
    assert!(err.contains("walk session directory"), "{err}");
}

#[cfg(unix)]
#[test]
fn session_discovery_rejects_a_symlinked_tenant_home() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let actual_home = root.path().join("actual-home");
    let linked_home = root.path().join("linked-home");
    write_session(&actual_home, "11111111");
    symlink(&actual_home, &linked_home).unwrap();

    let err = TestBackend.files(&linked_home).unwrap_err().to_string();

    assert!(err.contains("tenant home is not a real directory"), "{err}");
}

#[cfg(unix)]
#[test]
fn session_discovery_rejects_a_symlinked_agent_state_directory() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let outside = root.path().join("outside-claude");
    let transcript = outside.join("projects/p/11111111.jsonl");
    std::fs::create_dir_all(transcript.parent().unwrap()).unwrap();
    std::fs::write(&transcript, "{}\n").unwrap();
    std::fs::create_dir(&home).unwrap();
    symlink(&outside, home.join(".claude")).unwrap();

    let err = crate::session_claude::Claude
        .files(&home)
        .unwrap_err()
        .to_string();

    assert!(
        err.contains("session directory is not a real directory"),
        "{err}"
    );
    assert!(
        transcript.exists(),
        "outside transcript must remain untouched"
    );
}

#[test]
fn list_and_delete_all_report_an_empty_tenant_without_failing() {
    let dir = tempfile::tempdir().unwrap();
    let missing_home = dir.path().join("missing-home");

    for home in [dir.path(), missing_home.as_path()] {
        let mut printed = Vec::new();
        let code = list_with_printer(&TestBackend, home, |line| {
            printed.push(line.to_string());
            Ok(true)
        })
        .unwrap();

        assert_eq!(code, 0, "an empty tenant is not a list failure");
        assert!(printed.is_empty(), "no rows to print: {printed:?}");

        let code = delete(&TestBackend, home, &[], true, true).unwrap();
        assert_eq!(code, 0, "deleting nothing is not a failure");
    }
    assert!(
        !missing_home.exists(),
        "session discovery must not initialize a tenant home"
    );
}

#[test]
fn delete_empty_selection_is_rejected_without_deleting_sessions() {
    let dir = tempfile::tempdir().unwrap();
    let one = write_session(dir.path(), "11111111");
    let two = write_session(dir.path(), "22222222");

    let error = delete(&TestBackend, dir.path(), &[], false, true)
        .unwrap_err()
        .to_string();

    assert!(error.contains("at least one session id"), "{error}");
    assert!(one.exists());
    assert!(two.exists());
}

#[test]
fn delete_all_flag_selects_all_sessions_with_yes() {
    let dir = tempfile::tempdir().unwrap();
    let one = write_session(dir.path(), "11111111");
    let two = write_session(dir.path(), "22222222");

    delete(&TestBackend, dir.path(), &[], true, true).unwrap();

    assert!(!one.exists());
    assert!(!two.exists());
}

#[test]
fn delete_all_flag_cannot_be_mixed_with_ids() {
    let dir = tempfile::tempdir().unwrap();
    let one = write_session(dir.path(), "11111111");

    let err = delete(
        &TestBackend,
        dir.path(),
        &["11111111".to_string()],
        true,
        true,
    )
    .unwrap_err()
    .to_string();

    assert!(err.contains("--all cannot be combined"), "{err}");
    assert!(one.exists());
}

#[test]
fn delete_treats_all_as_a_session_id_without_all_flag() {
    let dir = tempfile::tempdir().unwrap();
    let all = write_session(dir.path(), "all");
    let other = write_session(dir.path(), "11111111");

    delete(&TestBackend, dir.path(), &["all".to_string()], false, true).unwrap();

    assert!(!all.exists());
    assert!(other.exists());
}

#[test]
fn delete_all_includes_sessions_without_typed_prompts() {
    // `--all` clears the whole tenant — including tool/injected-only
    // shells that carry no typed prompt. `list` shows those same shells
    // (empty title), so the two stay consistent and all rows are removable.
    let dir = tempfile::tempdir().unwrap();
    let a = write_session(dir.path(), "11111111");
    let shell = dir.path().join("sessions").join("22222222.jsonl");
    std::fs::write(&shell, "{}\n").unwrap();

    let targets = delete_targets(&TestBackend, dir.path(), &[], true).unwrap();

    assert_eq!(targets, vec![a, shell]);
}

#[test]
fn deletion_is_format_independent_for_malformed_transcripts() {
    let dir = tempfile::tempdir().unwrap();
    let malformed = write_session(dir.path(), "11111111");
    std::fs::write(&malformed, b"not-json\n{still-not-json\n").unwrap();

    delete(
        &TestBackend,
        dir.path(),
        &["11111111".to_string()],
        false,
        true,
    )
    .unwrap();

    assert!(!malformed.exists());
}

#[cfg(unix)]
#[test]
fn non_utf8_transcript_names_still_pass_discovery_filters() {
    use std::os::unix::ffi::OsStringExt;

    let transcript = PathBuf::from(std::ffi::OsString::from_vec(b"invalid-\xff.jsonl".to_vec()));

    assert!(
        has_wanted_transcript_name(&transcript, &|name| name == "invalid-\u{fffd}.jsonl"),
        "discovery must not silently omit a transcript solely because its name is not UTF-8"
    );
}

#[test]
fn summarize_empty_shell_has_empty_title() {
    let dir = tempfile::tempdir().unwrap();
    let shell = dir.path().join("sessions").join("33333333.jsonl");
    std::fs::create_dir_all(shell.parent().unwrap()).unwrap();
    std::fs::write(&shell, "{}\n").unwrap();

    let s = TestBackend.summarize(&shell).unwrap();
    assert_eq!(s.title, "");
    assert!(s.id.starts_with("33333333"));
}

#[test]
fn delete_multiple_ids_confirms_each_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let keep = write_session(dir.path(), "11111111");
    let remove = write_session(dir.path(), "22222222");
    let targets = delete_targets(
        &TestBackend,
        dir.path(),
        &["2222".to_string(), "1111".to_string()],
        false,
    )
    .unwrap();
    let mut input = Cursor::new(b"y\nn\n");

    delete_targets_with_input(&TestBackend, dir.path(), targets, false, &mut input).unwrap();

    assert!(keep.exists());
    assert!(!remove.exists());
}

#[test]
fn delete_refuses_noninteractive_confirmation_without_yes() {
    if io::stdin().is_terminal() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let target = write_session(dir.path(), "11111111");

    let err = delete(
        &TestBackend,
        dir.path(),
        &["1111".to_string()],
        false,
        false,
    )
    .unwrap_err()
    .to_string();

    assert!(
        err.contains("without --yes in a non-interactive shell"),
        "{err}"
    );
    assert!(target.exists());
}

#[test]
fn confirm_delete_accepts_only_explicit_yes_answers() {
    for yes in ["y\n", "Y\n", "yes\n", " YES \n"] {
        let mut input = Cursor::new(yes.as_bytes());
        assert!(confirm_delete("11111111", &mut input).unwrap(), "{yes:?}");
    }

    for no in ["", "\n", "n\n", "yeah\n", "yep\n", " yes please\n"] {
        let mut input = Cursor::new(no.as_bytes());
        assert!(!confirm_delete("11111111", &mut input).unwrap(), "{no:?}");
    }
}

#[test]
fn delete_confirmation_read_errors_are_not_reported_as_successful_keeps() {
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

    let dir = tempfile::tempdir().unwrap();
    let target = write_session(dir.path(), "11111111");
    let targets = delete_targets(&TestBackend, dir.path(), &["1111".to_string()], false).unwrap();
    let error =
        delete_targets_with_input(&TestBackend, dir.path(), targets, false, &mut FailingInput)
            .unwrap_err()
            .to_string();

    assert!(
        error.contains("read session delete confirmation"),
        "{error}"
    );
    assert!(target.exists(), "an unread confirmation must not delete");
}

#[test]
fn delete_targets_dedupes_repeated_ids() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_session(dir.path(), "11111111");

    let targets = delete_targets(
        &TestBackend,
        dir.path(),
        &["1111".to_string(), "11111111".to_string()],
        false,
    )
    .unwrap();

    assert_eq!(targets, vec![path]);
}

#[test]
fn delete_all_orders_targets_by_session_id() {
    let dir = tempfile::tempdir().unwrap();
    let z = dir.path().join("z-session.jsonl");
    let a = dir.path().join("a-session.jsonl");
    std::fs::write(&z, "{}\n").unwrap();
    std::fs::write(&a, "{}\n").unwrap();
    let backend = ExplicitFilesBackend::new(vec![z.clone(), a.clone()]);

    let targets = delete_targets(&backend, dir.path(), &[], true).unwrap();

    assert_eq!(
        targets,
        vec![a, z],
        "delete --all should prompt in deterministic session-id order"
    );
}

#[test]
fn resolve_exact_id_wins_over_suffix_ambiguity() {
    // An id that is also another id's suffix must still be addressable: the
    // exact match wins instead of reading as an ambiguous suffix.
    let dir = tempfile::tempdir().unwrap();
    let exact = write_session(dir.path(), "1111");
    write_session(dir.path(), "22221111");

    let got = resolve(&TestBackend, dir.path(), "1111").unwrap();

    assert_eq!(got, exact);
}

#[test]
fn resolve_duplicate_exact_ids_is_ambiguous() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("sessions/a/11111111.jsonl");
    let second = dir.path().join("sessions/b/11111111.jsonl");
    std::fs::create_dir_all(first.parent().unwrap()).unwrap();
    std::fs::create_dir_all(second.parent().unwrap()).unwrap();
    std::fs::write(&first, "{}\n").unwrap();
    std::fs::write(&second, "{}\n").unwrap();

    let err = resolve(&TestBackend, dir.path(), "11111111")
        .unwrap_err()
        .to_string();

    assert!(err.contains("ambiguous id '11111111' matches 2 sessions"));
    assert!(err.contains(&first.display().to_string()));
    assert!(err.contains(&second.display().to_string()));
}

#[test]
fn resolve_ambiguous_suffix_lists_all_candidates() {
    let dir = tempfile::tempdir().unwrap();
    write_session(dir.path(), "22221111");
    write_session(dir.path(), "33331111");

    let err = resolve(&TestBackend, dir.path(), "1111")
        .unwrap_err()
        .to_string();

    assert!(err.contains("ambiguous id '1111' matches 2 sessions"));
    assert!(err.contains("22221111"));
    assert!(err.contains("33331111"));
}

#[test]
fn resolve_accepts_any_nonempty_unique_suffix_but_not_an_old_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let target = write_session(dir.path(), "1111222a");

    assert_eq!(resolve(&TestBackend, dir.path(), "a").unwrap(), target);

    let err = resolve(&TestBackend, dir.path(), "1111")
        .unwrap_err()
        .to_string();
    assert!(err.contains("no session matches: 1111"), "{err}");

    let err = resolve(&TestBackend, dir.path(), "")
        .unwrap_err()
        .to_string();
    assert!(err.contains("unique suffix"), "{err}");
}

#[test]
fn delete_resolves_all_ids_before_removing_anything() {
    let dir = tempfile::tempdir().unwrap();
    let keep = write_session(dir.path(), "11111111");

    let err = delete(
        &TestBackend,
        dir.path(),
        &["1111".to_string(), "missing".to_string()],
        false,
        true,
    )
    .unwrap_err();

    assert!(err.to_string().contains("no session matches: missing"));
    assert!(keep.exists());
}
