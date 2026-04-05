use super::*;

#[test]
fn run_user_command_passes_stdin_cwd_timeout_and_env_to_process_runner() {
    let temp_result = tempfile::tempdir();
    assert!(temp_result.is_ok(), "tempdir creation should succeed");
    let Ok(temp) = temp_result else {
        return;
    };
    let runtime = RecordingRuntime {
        fs: OsFileSystem,
        clock: FixedClock::new(std::time::UNIX_EPOCH),
        process: RecordingProcessRunner::success(0, b"out", b"err"),
        io: CapturingIo::default(),
        tmp: temp.path().to_path_buf(),
    };

    let result = run_user_command(&runtime, &sample_rule(), &sample_context());
    let requests = runtime.process.requests.borrow();
    let request = requests.first();

    assert_eq!(
        result.as_ref().map(|value| value.status),
        Ok(InternalStatus::Success)
    );
    assert_eq!(
        result.as_ref().map(|value| value.raw_stdout.clone()),
        Ok(b"out".to_vec())
    );
    assert_eq!(
        result.as_ref().map(|value| value.raw_stderr.clone()),
        Ok(b"err".to_vec())
    );
    assert!(request.is_some(), "process request should be recorded");
    let Some(request) = request else {
        return;
    };
    assert_eq!(request.program, "sh");
    assert_eq!(request.args, vec!["-lc".to_string(), "echo ok".to_string()]);
    assert_eq!(
        request.stdin,
        br#"{"hook_event_name":"PreToolUse","session_id":"t1"}"#.to_vec()
    );
    assert_eq!(request.timeout, Duration::from_secs(30));
    assert_eq!(request.cwd, Some(PathBuf::from("/rule-cwd")));
    assert_eq!(
        request.env.get("HOOK_BRIDGE_PLATFORM"),
        Some(&"codex".to_string())
    );
    assert_eq!(request.env.get("USER_DEFINED"), Some(&"1".to_string()));
}

#[test]
fn run_user_command_maps_non_zero_exit_to_block_result() {
    let temp_result = tempfile::tempdir();
    assert!(temp_result.is_ok(), "tempdir creation should succeed");
    let Ok(temp) = temp_result else {
        return;
    };
    let runtime = RecordingRuntime {
        fs: OsFileSystem,
        clock: FixedClock::new(std::time::UNIX_EPOCH),
        process: RecordingProcessRunner::success(23, b"out", b"err"),
        io: CapturingIo::default(),
        tmp: temp.path().to_path_buf(),
    };

    let result = run_user_command(&runtime, &sample_rule(), &sample_context());

    assert_eq!(
        result.as_ref().map(|value| value.status),
        Ok(InternalStatus::Block)
    );
    assert_eq!(result.as_ref().map(|value| value.exit_code), Ok(Some(23)));
    assert_eq!(
        result.as_ref().map(|value| value.message.clone()),
        Ok(Some(
            "Command failed with exit code 23.\n\nCommand:\necho ok\n\nstderr (tail):\nerr\n\nstdout (tail):\nout".to_string()
        ))
    );
    assert_eq!(
        result.as_ref().map(|value| value.system_message.clone()),
        Ok(None)
    );
}

#[test]
fn format_non_zero_exit_summary_prefers_stderr_then_stdout_tail() {
    let summary = format_non_zero_exit_summary(
        "make verify",
        2,
        b"stdout line 1\nstdout line 2\n",
        b"stderr line 1\nstderr line 2\n",
    );

    assert!(summary.starts_with("Command failed with exit code 2."));
    assert!(summary.contains("\n\nCommand:\nmake verify"));
    let stderr_index = summary.find("stderr (tail):\nstderr line 1\nstderr line 2");
    let stdout_index = summary.find("stdout (tail):\nstdout line 1\nstdout line 2");
    assert!(stderr_index.is_some(), "stderr summary should be included");
    assert!(stdout_index.is_some(), "stdout summary should be included");
    assert!(
        stderr_index < stdout_index,
        "stderr summary should appear before stdout summary"
    );
}

#[test]
fn summarize_output_stream_truncates_to_tail_and_handles_non_utf8() {
    let long_stream = (0..20)
        .map(|index| format!("line {index}"))
        .collect::<Vec<_>>()
        .join("\n");
    let summary = summarize_output_stream("stderr", long_stream.as_bytes());

    assert!(
        summary.is_some(),
        "summary should be generated for text output"
    );
    let Some(summary) = summary else {
        return;
    };
    assert!(!summary.contains("line 0"), "older lines should be trimmed");
    assert!(
        summary.contains("line 8"),
        "tail should retain recent lines"
    );
    assert!(
        summary.contains("line 19"),
        "tail should include the latest line"
    );

    let binary = summarize_output_stream("stdout", &[0xff, 0x00, 0xfe]);
    assert_eq!(
        binary,
        Some("stdout (tail):\n<non-UTF-8 output: 3 bytes>".to_string())
    );
}

#[test]
fn summarize_output_stream_respects_exact_and_over_char_limits() {
    let exact = "a".repeat(600);
    assert_eq!(
        summarize_output_stream("stderr", exact.as_bytes()),
        Some(format!("stderr (tail):\n{exact}"))
    );

    let over = format!("{}{}", "0123456789".repeat(60), "tail");
    let summary = summarize_output_stream("stderr", over.as_bytes());
    assert!(
        summary.is_some(),
        "summary should be generated for long output"
    );
    let Some(summary) = summary else {
        return;
    };
    assert!(summary.starts_with("stderr (tail):\n..."));
    assert_eq!(summary, format!("stderr (tail):\n...{}", &over[4..]));
}

#[test]
fn run_user_command_maps_spawn_and_timeout_errors_to_error_result() {
    let temp_result = tempfile::tempdir();
    assert!(temp_result.is_ok(), "tempdir creation should succeed");
    let Ok(temp) = temp_result else {
        return;
    };
    let runtime = RecordingRuntime {
        fs: OsFileSystem,
        clock: FixedClock::new(std::time::UNIX_EPOCH),
        process: RecordingProcessRunner::failure(HookBridgeError::Timeout { timeout_sec: 3 }),
        io: CapturingIo::default(),
        tmp: temp.path().to_path_buf(),
    };

    let result = run_user_command(&runtime, &sample_rule(), &sample_context());

    assert_eq!(
        result.as_ref().map(|value| value.status),
        Ok(InternalStatus::Error)
    );
    assert_eq!(result.as_ref().map(|value| value.exit_code), Ok(Some(1)));
    assert_eq!(
        result.as_ref().map(|value| value.message.clone()),
        Ok(Some("timeout after 3s".to_string()))
    );
}

#[test]
fn run_user_command_parses_structured_bridge_output() {
    let temp_result = tempfile::tempdir();
    assert!(temp_result.is_ok(), "tempdir creation should succeed");
    let Ok(temp) = temp_result else {
        return;
    };
    let runtime = RecordingRuntime {
        fs: OsFileSystem,
        clock: FixedClock::new(std::time::UNIX_EPOCH),
        process: RecordingProcessRunner::success(
            0,
            br#"{"hook_bridge":{"kind":"additional_context","text":"read docs"}}"#,
            b"warn",
        ),
        io: CapturingIo::default(),
        tmp: temp.path().to_path_buf(),
    };

    let result = run_user_command(&runtime, &sample_rule(), &sample_context());

    assert_eq!(
        result.as_ref().map(|value| value.bridge_output.clone()),
        Ok(Some(BridgeOutput::AdditionalContext {
            text: "read docs".to_string()
        }))
    );
    assert_eq!(
        result.as_ref().map(|value| value.status),
        Ok(InternalStatus::Success)
    );
    assert_eq!(
        result.as_ref().map(|value| value.raw_stdout.clone()),
        Ok(br#"{"hook_bridge":{"kind":"additional_context","text":"read docs"}}"#.to_vec())
    );
    assert_eq!(
        result.as_ref().map(|value| value.raw_stderr.clone()),
        Ok(b"warn".to_vec())
    );
    assert_eq!(result.as_ref().map(|value| value.exit_code), Ok(Some(0)));
}

#[test]
fn run_user_command_ignores_structured_stdout_on_non_zero_exit() {
    let temp_result = tempfile::tempdir();
    assert!(temp_result.is_ok(), "tempdir creation should succeed");
    let Ok(temp) = temp_result else {
        return;
    };
    let runtime = RecordingRuntime {
        fs: OsFileSystem,
        clock: FixedClock::new(std::time::UNIX_EPOCH),
        process: RecordingProcessRunner::success(
            7,
            br#"{"hook_bridge":{"kind":"additional_context","text":"should not win"}}"#,
            b"err",
        ),
        io: CapturingIo::default(),
        tmp: temp.path().to_path_buf(),
    };

    let result = run_user_command(&runtime, &sample_rule(), &sample_context());

    assert_eq!(
        result.as_ref().map(|value| value.status),
        Ok(InternalStatus::Block)
    );
    assert_eq!(
        result.as_ref().map(|value| value.bridge_output.clone()),
        Ok(None)
    );
    assert_eq!(result.as_ref().map(|value| value.exit_code), Ok(Some(7)));
}

#[test]
fn run_user_command_rejects_invalid_bridge_kinds_and_missing_fields() {
    let temp_result = tempfile::tempdir();
    assert!(temp_result.is_ok(), "tempdir creation should succeed");
    let Ok(temp) = temp_result else {
        return;
    };

    let invalid_kind_runtime = RecordingRuntime {
        fs: OsFileSystem,
        clock: FixedClock::new(std::time::UNIX_EPOCH),
        process: RecordingProcessRunner::success(0, br#"{"hook_bridge":{"kind":"wat"}} "#, b""),
        io: CapturingIo::default(),
        tmp: temp.path().to_path_buf(),
    };
    assert!(matches!(
        run_user_command(&invalid_kind_runtime, &sample_rule(), &sample_context()),
        Err(HookBridgeError::PlatformProtocol { .. })
    ));

    let missing_field_runtime = RecordingRuntime {
        fs: OsFileSystem,
        clock: FixedClock::new(std::time::UNIX_EPOCH),
        process: RecordingProcessRunner::success(
            0,
            br#"{"hook_bridge":{"kind":"worktree_path"}} "#,
            b"",
        ),
        io: CapturingIo::default(),
        tmp: temp.path().to_path_buf(),
    };
    assert!(matches!(
        run_user_command(&missing_field_runtime, &sample_rule(), &sample_context()),
        Err(HookBridgeError::PlatformProtocol { .. })
    ));
}

#[test]
fn run_user_command_maps_structured_bridge_variants_to_internal_results() {
    let temp_result = tempfile::tempdir();
    assert!(temp_result.is_ok(), "tempdir creation should succeed");
    let Ok(temp) = temp_result else {
        return;
    };

    let cases = [
        (
            br#"{"hook_bridge":{"kind":"block","reason":"no"}} "#.as_slice(),
            InternalStatus::Block,
        ),
        (
            br#"{"hook_bridge":{"kind":"stop","reason":"later"}} "#.as_slice(),
            InternalStatus::Stop,
        ),
        (
            br#"{"hook_bridge":{"kind":"permission_decision","behavior":"deny","reason":"blocked","additional_context":"ctx","updated_input":{"cmd":"safe"}}}"#
                .as_slice(),
            InternalStatus::Success,
        ),
        (
            br#"{"hook_bridge":{"kind":"permission_retry","reason":"retry"}} "#.as_slice(),
            InternalStatus::Success,
        ),
        (
            br#"{"hook_bridge":{"kind":"worktree_path","path":"/tmp/wt"}} "#.as_slice(),
            InternalStatus::Success,
        ),
        (
            br#"{"hook_bridge":{"kind":"elicitation_response","action":"accept","content":{"v":"x"}}}"#
                .as_slice(),
            InternalStatus::Success,
        ),
        (
            br#"{"hook_bridge":{"kind":"error","message":"bad","system_message":"bridge"}} "#.as_slice(),
            InternalStatus::Error,
        ),
    ];

    for (stdout, expected_status) in cases {
        let runtime = RecordingRuntime {
            fs: OsFileSystem,
            clock: FixedClock::new(std::time::UNIX_EPOCH),
            process: RecordingProcessRunner::success(0, stdout, b""),
            io: CapturingIo::default(),
            tmp: temp.path().to_path_buf(),
        };

        let result = run_user_command(&runtime, &sample_rule(), &sample_context());
        assert_eq!(
            result.as_ref().map(|value| value.status),
            Ok(expected_status),
            "structured stdout should map to expected status"
        );
        assert!(
            result
                .as_ref()
                .ok()
                .and_then(|value| value.bridge_output.as_ref())
                .is_some(),
            "bridge output should be captured"
        );
    }
}

#[test]
fn run_user_command_ignores_non_bridge_json_stdout() {
    let temp_result = tempfile::tempdir();
    assert!(temp_result.is_ok(), "tempdir creation should succeed");
    let Ok(temp) = temp_result else {
        return;
    };
    let runtime = RecordingRuntime {
        fs: OsFileSystem,
        clock: FixedClock::new(std::time::UNIX_EPOCH),
        process: RecordingProcessRunner::success(0, br#"{"plain":"json"}"#, b""),
        io: CapturingIo::default(),
        tmp: temp.path().to_path_buf(),
    };

    let result = run_user_command(&runtime, &sample_rule(), &sample_context());
    assert_eq!(
        result.as_ref().map(|value| value.status),
        Ok(InternalStatus::Success)
    );
    assert_eq!(
        result.as_ref().map(|value| value.bridge_output.clone()),
        Ok(None)
    );
}

#[test]
fn run_user_command_promotes_plaintext_stdout_to_codex_additional_context() {
    let temp_result = tempfile::tempdir();
    assert!(temp_result.is_ok(), "tempdir creation should succeed");
    let Ok(temp) = temp_result else {
        return;
    };
    let runtime = RecordingRuntime {
        fs: OsFileSystem,
        clock: FixedClock::new(std::time::UNIX_EPOCH),
        process: RecordingProcessRunner::success(
            0,
            b"Load workspace conventions.\n",
            b"native stderr",
        ),
        io: CapturingIo::default(),
        tmp: temp.path().to_path_buf(),
    };
    let context = RuntimeContext {
        platform: Platform::Codex,
        raw_event: "SessionStart".to_string(),
        event: "SessionStart".to_string(),
        rule_id: "r1".to_string(),
        source_config_path: "/tmp/cfg.yaml".into(),
        session_or_thread_id: "t1".to_string(),
        cwd: None,
        transcript_path: None,
        raw_payload: "{}".to_string(),
    };
    let rule = PlatformRule {
        event: "SessionStart".to_string(),
        ..sample_rule()
    };

    let result = run_user_command(&runtime, &rule, &context);
    assert_eq!(
        result.as_ref().map(|value| value.status),
        Ok(InternalStatus::Success)
    );
    assert_eq!(
        result.as_ref().map(|value| value.bridge_output.clone()),
        Ok(Some(BridgeOutput::AdditionalContext {
            text: "Load workspace conventions.".to_string()
        }))
    );
    assert_eq!(
        result.as_ref().map(|value| value.raw_stdout.clone()),
        Ok(b"Load workspace conventions.\n".to_vec())
    );
    assert_eq!(
        result.as_ref().map(|value| value.raw_stderr.clone()),
        Ok(b"native stderr".to_vec())
    );
    assert_eq!(result.as_ref().map(|value| value.exit_code), Ok(Some(0)));
}

#[test]
fn run_user_command_ignores_plaintext_stdout_for_codex_stop() {
    let temp_result = tempfile::tempdir();
    assert!(temp_result.is_ok(), "tempdir creation should succeed");
    let Ok(temp) = temp_result else {
        return;
    };
    let runtime = RecordingRuntime {
        fs: OsFileSystem,
        clock: FixedClock::new(std::time::UNIX_EPOCH),
        process: RecordingProcessRunner::success(0, b"keep-going\n", b""),
        io: CapturingIo::default(),
        tmp: temp.path().to_path_buf(),
    };
    let context = RuntimeContext {
        platform: Platform::Codex,
        raw_event: "Stop".to_string(),
        event: "Stop".to_string(),
        rule_id: "r1".to_string(),
        source_config_path: "/tmp/cfg.yaml".into(),
        session_or_thread_id: "t1".to_string(),
        cwd: None,
        transcript_path: None,
        raw_payload: "{}".to_string(),
    };
    let rule = PlatformRule {
        event: "Stop".to_string(),
        ..sample_rule()
    };

    assert_eq!(
        run_user_command(&runtime, &rule, &context),
        Ok(ExecutionResult {
            status: InternalStatus::Success,
            message: None,
            system_message: None,
            exit_code: Some(0),
            raw_stdout: b"keep-going\n".to_vec(),
            raw_stderr: Vec::new(),
            bridge_output: None,
        })
    );
}
