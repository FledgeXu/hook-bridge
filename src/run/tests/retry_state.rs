use super::*;

#[test]
fn retry_guard_helpers_preserve_stop_as_non_failure() {
    let rule = sample_rule();
    let state = RetryState {
        consecutive_failures: 2,
        last_error: "boom".to_string(),
        last_failure_epoch_sec: 1,
    };

    assert!(retry_guard_engaged(&rule, &state));
    assert_eq!(retry_guard_result().status, InternalStatus::Stop);
}

#[test]
fn update_retry_state_clears_success_persists_failures_and_ignores_stop() {
    let temp_result = tempfile::tempdir();
    assert!(temp_result.is_ok(), "tempdir creation should succeed");
    let Ok(temp) = temp_result else {
        return;
    };
    let runtime = ExecuteRuntime {
        fs: OsFileSystem,
        clock: FixedClock::new(std::time::UNIX_EPOCH + Duration::from_secs(77)),
        process: FakeProcessRunner::success(0),
        io: CapturingIo::default(),
        tmp: temp.path().to_path_buf(),
    };
    let path = temp.path().join("retry/state.json");
    let initial = RetryState {
        consecutive_failures: 1,
        last_error: "old".to_string(),
        last_failure_epoch_sec: 5,
    };

    assert_eq!(persist_retry_state(&runtime, &path, &initial), Ok(()));
    assert_eq!(
        update_retry_state(
            &runtime,
            &path,
            &initial,
            &ExecutionResult {
                status: InternalStatus::Success,
                message: None,
                system_message: None,
                exit_code: Some(0),
                raw_stdout: Vec::new(),
                raw_stderr: Vec::new(),
                bridge_output: None,
            }
        ),
        Ok(())
    );
    assert_eq!(runtime.fs().exists(&path), Ok(false));

    assert_eq!(
        update_retry_state(
            &runtime,
            &path,
            &initial,
            &ExecutionResult {
                status: InternalStatus::Block,
                message: Some("blocked".to_string()),
                system_message: None,
                exit_code: Some(2),
                raw_stdout: Vec::new(),
                raw_stderr: Vec::new(),
                bridge_output: None,
            }
        ),
        Ok(())
    );
    assert_eq!(
        load_retry_state(&runtime, &path),
        Ok(RetryState {
            consecutive_failures: 2,
            last_error: "blocked".to_string(),
            last_failure_epoch_sec: 77,
        })
    );

    assert_eq!(
        update_retry_state(&runtime, &path, &initial, &retry_guard_result()),
        Ok(())
    );
    assert_eq!(
        load_retry_state(&runtime, &path),
        Ok(RetryState {
            consecutive_failures: 2,
            last_error: "blocked".to_string(),
            last_failure_epoch_sec: 77,
        })
    );
}

#[test]
fn execute_rule_creates_retry_state_and_clears_it_after_later_success() {
    let temp_result = tempfile::tempdir();
    assert!(temp_result.is_ok(), "tempdir creation should succeed");
    let Ok(temp) = temp_result else {
        return;
    };
    let context = RuntimeContext {
        source_config_path: temp.path().join("hook-bridge.yaml"),
        ..sample_context()
    };
    let state_path = retry_state_path(
        &ExecuteRuntime {
            fs: OsFileSystem,
            clock: FixedClock::new(std::time::UNIX_EPOCH),
            process: FakeProcessRunner::success(0),
            io: CapturingIo::default(),
            tmp: temp.path().to_path_buf(),
        },
        &context,
    );

    let failing_runtime = RecordingRuntime {
        fs: OsFileSystem,
        clock: FixedClock::new(std::time::UNIX_EPOCH + Duration::from_secs(11)),
        process: RecordingProcessRunner::failure(HookBridgeError::Process {
            message: "failed to spawn process: boom".to_string(),
        }),
        io: CapturingIo::default(),
        tmp: temp.path().to_path_buf(),
    };
    let failure_result = execute_rule(&failing_runtime, &sample_rule(), &context);

    assert!(matches!(
        failure_result,
        Ok(ExecutionResult {
            status: InternalStatus::Error,
            ..
        })
    ));
    assert_eq!(
        load_retry_state(&failing_runtime, &state_path),
        Ok(RetryState {
            consecutive_failures: 1,
            last_error: "process error: failed to spawn process: boom".to_string(),
            last_failure_epoch_sec: 11,
        })
    );

    let success_runtime = RecordingRuntime {
        fs: OsFileSystem,
        clock: FixedClock::new(std::time::UNIX_EPOCH + Duration::from_secs(12)),
        process: RecordingProcessRunner::success(0, b"", b""),
        io: CapturingIo::default(),
        tmp: temp.path().to_path_buf(),
    };

    assert!(matches!(
        execute_rule(&success_runtime, &sample_rule(), &context),
        Ok(ExecutionResult {
            status: InternalStatus::Success,
            ..
        })
    ));
    assert_eq!(success_runtime.fs().exists(&state_path), Ok(false));
}

#[test]
fn execute_rule_persists_retry_state_for_translate_time_protocol_failure() {
    let temp_result = tempfile::tempdir();
    assert!(temp_result.is_ok(), "tempdir creation should succeed");
    let Ok(temp) = temp_result else {
        return;
    };
    let context = RuntimeContext {
        platform: Platform::Claude,
        raw_event: "Notification".to_string(),
        event: "Notification".to_string(),
        rule_id: "bad_structured".to_string(),
        source_config_path: temp.path().join("hook-bridge.yaml"),
        session_or_thread_id: "s1".to_string(),
        cwd: None,
        transcript_path: None,
        raw_payload: "{}".to_string(),
    };
    let rule = PlatformRule {
        event: "Notification".to_string(),
        command: "echo bad".to_string(),
        matcher: None,
        shell: "sh".to_string(),
        timeout_sec: 30,
        max_retries: 2,
        working_dir: None,
        env: BTreeMap::new(),
        extra: BTreeMap::new(),
    };
    let state_path = retry_state_path(
        &ExecuteRuntime {
            fs: OsFileSystem,
            clock: FixedClock::new(std::time::UNIX_EPOCH),
            process: FakeProcessRunner::success(0),
            io: CapturingIo::default(),
            tmp: temp.path().to_path_buf(),
        },
        &context,
    );

    let runtime = RecordingRuntime {
        fs: OsFileSystem,
        clock: FixedClock::new(std::time::UNIX_EPOCH + Duration::from_secs(13)),
        process: RecordingProcessRunner::success(
            0,
            br#"{"hook_bridge":{"kind":"stop","reason":"later"}}"#,
            b"",
        ),
        io: CapturingIo::default(),
        tmp: temp.path().to_path_buf(),
    };

    assert!(matches!(
        execute_rule(&runtime, &rule, &context),
        Err(HookBridgeError::PlatformProtocol { .. })
    ));
    assert_eq!(
        load_retry_state(&runtime, &state_path),
        Ok(RetryState {
            consecutive_failures: 1,
            last_error: "platform protocol error: claude event 'Notification' does not support bridge output 'Stop { reason: Some(\"later\"), system_message: None }'".to_string(),
            last_failure_epoch_sec: 13,
        })
    );
}
