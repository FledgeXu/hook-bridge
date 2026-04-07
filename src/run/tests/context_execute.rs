use super::*;

#[test]
fn parse_context_works_for_codex_shape() {
    let args = RunArgs {
        platform: Platform::Codex,
        rule_id: "r1".to_string(),
    };
    let payload = r#"{"hook_event_name":"before_command","session_id":"t1","cwd":"/tmp"}"#;
    let context = parse_runtime_context(&args, payload, Path::new("/tmp/cfg.yaml"));
    assert_eq!(
        context.as_ref().map(|value| value.event.as_str()),
        Ok("PreToolUse")
    );
    assert_eq!(
        context.as_ref().map(|value| value.raw_event.as_str()),
        Ok("before_command")
    );
    assert_eq!(
        context
            .as_ref()
            .map(|value| value.session_or_thread_id.as_str()),
        Ok("t1")
    );
}

#[test]
fn parse_context_preserves_raw_native_event_for_platform_output() {
    let args = RunArgs {
        platform: Platform::Codex,
        rule_id: "r1".to_string(),
    };
    let payload = r#"{"hook_event_name":"PreToolUse","session_id":"t1","cwd":"/tmp"}"#;
    let context = parse_runtime_context(&args, payload, Path::new("/tmp/cfg.yaml"));

    assert_eq!(
        context.as_ref().map(|value| value.event.as_str()),
        Ok("PreToolUse")
    );
    assert_eq!(
        context.as_ref().map(|value| value.raw_event.as_str()),
        Ok("PreToolUse")
    );
}

#[test]
fn parse_context_preserves_full_raw_payload() {
    let args = RunArgs {
        platform: Platform::Claude,
        rule_id: "r1".to_string(),
    };
    let payload = r#"{"hook_event_name":"before_command","session_id":"s1","cwd":"/tmp","extra":{"nested":true}}"#;
    let context = parse_runtime_context(&args, payload, Path::new("/tmp/cfg.yaml"));

    assert_eq!(
        context.as_ref().map(|value| value.raw_payload.as_str()),
        Ok(payload)
    );
}

#[test]
fn parse_context_rejects_payload_event_not_supported_by_selected_platform() {
    let args = RunArgs {
        platform: Platform::Codex,
        rule_id: "r1".to_string(),
    };

    assert!(matches!(
        parse_runtime_context(
            &args,
            r#"{"hook_event_name":"Notification","session_id":"t1"}"#,
            Path::new("/tmp/cfg.yaml"),
        ),
        Err(crate::error::HookBridgeError::PlatformProtocol { message })
            if message
                == "codex payload event 'Notification' is not supported for platform 'codex'"
    ));
}

#[test]
fn retry_key_is_stable_for_platform_session_and_rule() {
    let context = RuntimeContext {
        platform: Platform::Claude,
        raw_event: "PreToolUse".to_string(),
        event: "PreToolUse".to_string(),
        rule_id: "rule_1".to_string(),
        source_config_path: "/tmp/custom/cfg.yaml".into(),
        session_or_thread_id: "session_1".to_string(),
        cwd: None,
        transcript_path: None,
        raw_payload: "{}".to_string(),
    };
    let runtime = TestRuntime {
        fs: FakeFileSystem::default(),
        clock: FixedClock::new(std::time::SystemTime::UNIX_EPOCH),
        process: FakeProcessRunner::success(0),
        io: FakeIo::default(),
        tmp: "/tmp/custom".into(),
    };
    let path = retry_state_path(&runtime, &context);
    let as_string = path.display().to_string();
    assert!(as_string.contains("/tmp/custom/hook_bridge/retries/claude/"));
    assert!(as_string.ends_with("/rule_1.json"));
}

#[test]
fn retry_key_is_isolated_by_source_config_path() {
    let runtime = TestRuntime {
        fs: FakeFileSystem::default(),
        clock: FixedClock::new(std::time::SystemTime::UNIX_EPOCH),
        process: FakeProcessRunner::success(0),
        io: FakeIo::default(),
        tmp: "/tmp/custom".into(),
    };
    let context_a = RuntimeContext {
        platform: Platform::Codex,
        raw_event: "PreToolUse".to_string(),
        event: "PreToolUse".to_string(),
        rule_id: "rule_same".to_string(),
        source_config_path: "/repo_a/hook-bridge.yaml".into(),
        session_or_thread_id: "thread_same".to_string(),
        cwd: None,
        transcript_path: None,
        raw_payload: "{}".to_string(),
    };
    let context_b = RuntimeContext {
        platform: Platform::Codex,
        raw_event: "PreToolUse".to_string(),
        event: "PreToolUse".to_string(),
        rule_id: "rule_same".to_string(),
        source_config_path: "/repo_b/hook-bridge.yaml".into(),
        session_or_thread_id: "thread_same".to_string(),
        cwd: None,
        transcript_path: None,
        raw_payload: "{}".to_string(),
    };

    let path_a = retry_state_path(&runtime, &context_a);
    let path_b = retry_state_path(&runtime, &context_b);

    assert_ne!(path_a, path_b);
}

#[test]
fn parse_context_rejects_invalid_json() {
    let args = RunArgs {
        platform: Platform::Codex,
        rule_id: "r1".to_string(),
    };

    assert!(matches!(
        parse_runtime_context(&args, "{", Path::new("/tmp/cfg.yaml")),
        Err(crate::error::HookBridgeError::JsonParse { message })
            if message.contains("invalid runtime JSON input")
    ));
}

#[test]
fn test_runtime_exposes_all_dependencies() {
    let runtime = TestRuntime {
        fs: FakeFileSystem::default(),
        clock: FixedClock::new(std::time::SystemTime::UNIX_EPOCH),
        process: FakeProcessRunner::success(0),
        io: FakeIo::default(),
        tmp: "/tmp/run-tests".into(),
    };

    assert!(matches!(runtime.fs().exists(Path::new(".")), Ok(false)));
    assert_eq!(runtime.clock().now(), std::time::SystemTime::UNIX_EPOCH);
    assert_eq!(runtime.io().read_stdin(), Ok(Vec::new()));
    assert_eq!(runtime.temp_dir(), PathBuf::from("/tmp/run-tests"));
    assert!(matches!(
        runtime.process_runner().run(&crate::runtime::process::ProcessRequest {
            program: "echo".to_string(),
            args: vec!["ok".to_string()],
            stdin: Vec::new(),
            timeout: std::time::Duration::from_secs(1),
            cwd: None,
            env: std::collections::BTreeMap::new(),
        }),
        Ok(output) if output.status_code == 0
    ));
}

#[test]
fn execute_rejects_relative_managed_source_config() {
    let Ok(_lock) = crate::CWD_LOCK.lock() else {
        return;
    };
    let temp_result = tempfile::tempdir();
    assert!(temp_result.is_ok(), "tempdir creation should succeed");
    let Ok(temp) = temp_result else {
        return;
    };
    let guard_result = CurrentDirGuard::enter(temp.path());
    assert!(guard_result.is_ok(), "cwd switch should succeed");
    let Ok(_guard) = guard_result else {
        return;
    };
    write_managed_hooks_file(temp.path(), "hook-bridge.yaml");
    let runtime = ExecuteRuntime {
        fs: OsFileSystem,
        clock: FixedClock::new(std::time::SystemTime::UNIX_EPOCH),
        process: FakeProcessRunner::success(0),
        io: CapturingIo::default(),
        tmp: temp.path().to_path_buf(),
    };

    assert_eq!(
        execute(
            &RunArgs {
                platform: Platform::Codex,
                rule_id: "r1".to_string(),
            },
            &runtime,
        ),
        Err(crate::error::HookBridgeError::ConfigValidation {
            message: "managed source_config must be absolute, got 'hook-bridge.yaml'".to_string(),
        })
    );
}

#[test]
fn execute_rejects_non_utf8_stdin() {
    let Ok(_lock) = crate::CWD_LOCK.lock() else {
        return;
    };
    let temp_result = tempfile::tempdir();
    assert!(temp_result.is_ok(), "tempdir creation should succeed");
    let Ok(temp) = temp_result else {
        return;
    };
    let guard_result = CurrentDirGuard::enter(temp.path());
    assert!(guard_result.is_ok(), "cwd switch should succeed");
    let Ok(_guard) = guard_result else {
        return;
    };
    let config_path = write_config(temp.path());
    write_managed_hooks_file(temp.path(), &config_path.display().to_string());
    let runtime = ExecuteRuntime {
        fs: OsFileSystem,
        clock: FixedClock::new(std::time::SystemTime::UNIX_EPOCH),
        process: FakeProcessRunner::success(0),
        io: CapturingIo {
            stdin: vec![0xff],
            stdout: RefCell::new(Vec::new()),
        },
        tmp: temp.path().to_path_buf(),
    };

    assert!(matches!(
        execute(
            &RunArgs {
                platform: Platform::Codex,
                rule_id: "r1".to_string(),
            },
            &runtime,
        ),
        Err(crate::error::HookBridgeError::JsonParse { message })
            if message.contains("stdin payload is not valid UTF-8 JSON")
    ));
}

#[test]
fn execute_rejects_event_mismatch() {
    let Ok(_lock) = crate::CWD_LOCK.lock() else {
        return;
    };
    let temp_result = tempfile::tempdir();
    assert!(temp_result.is_ok(), "tempdir creation should succeed");
    let Ok(temp) = temp_result else {
        return;
    };
    let guard_result = CurrentDirGuard::enter(temp.path());
    assert!(guard_result.is_ok(), "cwd switch should succeed");
    let Ok(_guard) = guard_result else {
        return;
    };
    let config_path = write_config(temp.path());
    write_managed_hooks_file(temp.path(), &config_path.display().to_string());
    let runtime = ExecuteRuntime {
        fs: OsFileSystem,
        clock: FixedClock::new(std::time::SystemTime::UNIX_EPOCH),
        process: FakeProcessRunner::success(0),
        io: CapturingIo {
            stdin: br#"{"hook_event_name":"after_command","session_id":"t1"}"#.to_vec(),
            stdout: RefCell::new(Vec::new()),
        },
        tmp: temp.path().to_path_buf(),
    };

    assert_eq!(
        execute(
            &RunArgs {
                platform: Platform::Codex,
                rule_id: "r1".to_string(),
            },
            &runtime,
        ),
        Err(crate::error::HookBridgeError::PlatformProtocol {
            message: "event mismatch for rule 'r1': stdin event 'PostToolUse' but configured event 'PreToolUse'".to_string(),
        })
    );
}

#[test]
fn execute_short_circuits_when_retry_guard_is_engaged() {
    let Ok(_lock) = crate::CWD_LOCK.lock() else {
        return;
    };
    let temp_result = tempfile::tempdir();
    assert!(temp_result.is_ok(), "tempdir creation should succeed");
    let Ok(temp) = temp_result else {
        return;
    };
    let guard_result = CurrentDirGuard::enter(temp.path());
    assert!(guard_result.is_ok(), "cwd switch should succeed");
    let Ok(_guard) = guard_result else {
        return;
    };
    let config_path = write_config(temp.path());
    write_managed_hooks_file(temp.path(), &config_path.display().to_string());
    let runtime = ExecuteRuntime {
        fs: OsFileSystem,
        clock: FixedClock::new(std::time::UNIX_EPOCH + std::time::Duration::from_secs(10)),
        process: FakeProcessRunner::success(0),
        io: CapturingIo {
            stdin: br#"{"hook_event_name":"before_command","session_id":"t1"}"#.to_vec(),
            stdout: RefCell::new(Vec::new()),
        },
        tmp: temp.path().to_path_buf(),
    };
    let context = RuntimeContext {
        platform: Platform::Codex,
        raw_event: "PreToolUse".to_string(),
        event: "PreToolUse".to_string(),
        rule_id: "r1".to_string(),
        source_config_path: config_path,
        session_or_thread_id: "t1".to_string(),
        cwd: None,
        transcript_path: None,
        raw_payload: "{}".to_string(),
    };
    let state_path = retry_state_path(&runtime, &context);
    assert_eq!(
        persist_retry_state(
            &runtime,
            &state_path,
            &RetryState {
                consecutive_failures: 1,
                last_error: "boom".to_string(),
                last_failure_epoch_sec: 9,
            },
        ),
        Ok(())
    );

    assert_eq!(
        execute(
            &RunArgs {
                platform: Platform::Codex,
                rule_id: "r1".to_string(),
            },
            &runtime,
        ),
        Ok(0)
    );
    assert!(
        String::from_utf8(runtime.io.stdout.borrow().clone())
            .is_ok_and(|payload| payload.contains("max retries reached")),
        "retry guard should write protocol output"
    );
}

#[test]
fn execute_runtime_exposes_all_dependencies() {
    let runtime = ExecuteRuntime {
        fs: OsFileSystem,
        clock: FixedClock::new(std::time::UNIX_EPOCH),
        process: FakeProcessRunner::success(0),
        io: CapturingIo::default(),
        tmp: "/tmp/exec-tests".into(),
    };

    assert!(matches!(
        runtime.fs().exists(Path::new("/definitely/missing")),
        Ok(false)
    ));
    assert_eq!(runtime.clock().now(), std::time::UNIX_EPOCH);
    assert_eq!(
        runtime
            .process_runner()
            .run(&crate::runtime::process::ProcessRequest {
                program: "echo".to_string(),
                args: vec!["ok".to_string()],
                stdin: Vec::new(),
                timeout: std::time::Duration::from_secs(1),
                cwd: None,
                env: std::collections::BTreeMap::new(),
            }),
        Ok(crate::runtime::process::ProcessOutput {
            status_code: 0,
            stdout: Vec::new(),
            stderr: Vec::new(),
        })
    );
    assert_eq!(runtime.io().read_stdin(), Ok(Vec::new()));
    assert_eq!(runtime.temp_dir(), PathBuf::from("/tmp/exec-tests"));
}

#[test]
fn helper_functions_cover_error_and_output_paths() {
    let retry_path = PathBuf::from("/tmp/retry.json");
    let runtime = TestRuntime {
        fs: FakeFileSystem::with_existing(vec![retry_path.clone()]),
        clock: FixedClock::new(std::time::UNIX_EPOCH - std::time::Duration::from_secs(1)),
        process: FakeProcessRunner::success(0),
        io: FakeIo::default(),
        tmp: "/tmp/custom".into(),
    };
    let context = RuntimeContext {
        platform: Platform::Codex,
        raw_event: "PreToolUse".to_string(),
        event: "PreToolUse".to_string(),
        rule_id: "r1".to_string(),
        source_config_path: "/tmp/cfg.yaml".into(),
        session_or_thread_id: "t1".to_string(),
        cwd: None,
        transcript_path: None,
        raw_payload: "{}".to_string(),
    };

    assert!(matches!(
        now_epoch_sec(&runtime),
        Err(crate::error::HookBridgeError::Process { message })
            if message.contains("clock error")
    ));
    assert!(matches!(
        load_retry_state(&runtime, retry_path.as_path()),
        Err(crate::error::HookBridgeError::JsonParse { message })
            if message.contains("invalid retry state JSON")
    ));
    assert_eq!(
        translate_output(
            Platform::Codex,
            &context,
            &super::ExecutionResult {
                status: super::InternalStatus::Error,
                message: Some("boom".to_string()),
                system_message: Some("bridge failed".to_string()),
                exit_code: Some(1),
                raw_stdout: Vec::new(),
                raw_stderr: Vec::new(),
                bridge_output: None,
            },
        )
        .map(|output| {
            let value_result = serde_json::from_slice::<serde_json::Value>(&output.stdout);
            assert!(value_result.is_ok(), "protocol output should be valid json");
            let Ok(value) = value_result else {
                return serde_json::Value::Null;
            };
            value
        }),
        Ok(serde_json::json!({
            "decision": "block",
            "reason": "boom",
            "systemMessage": "bridge failed",
        }))
    );
}

#[test]
fn command_env_injects_bridge_metadata_over_user_values() {
    let env = command_env(&sample_rule(), &sample_context());

    assert_eq!(env.get("USER_DEFINED"), Some(&"1".to_string()));
    assert_eq!(env.get("HOOK_BRIDGE_PLATFORM"), Some(&"codex".to_string()));
    assert_eq!(env.get("HOOK_BRIDGE_RULE_ID"), Some(&"r1".to_string()));
    assert_eq!(
        env.get("HOOK_BRIDGE_EVENT"),
        Some(&"PreToolUse".to_string())
    );
}
