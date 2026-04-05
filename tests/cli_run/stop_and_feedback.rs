use super::*;

#[test]
fn structured_stdout_does_not_override_non_zero_exit() {
    let temp_result = tempfile::tempdir();
    assert!(temp_result.is_ok(), "tempdir creation should succeed");
    let Ok(temp) = temp_result else {
        return;
    };

    let write_result = fs::write(
        temp.path().join("hook-bridge.yaml"),
        r#"
version: 1
hooks:
  - id: r_fail_json
    event: before_command
    command: |
      cat <<'EOF'
      {"hook_bridge":{"kind":"additional_context","text":"ignore me"}}
      EOF
      exit 9
"#,
    );
    assert!(write_result.is_ok(), "config file should be written");

    let gen_result = Command::cargo_bin("hook_bridge");
    assert!(
        gen_result.is_ok(),
        "binary should build for integration tests"
    );
    let Ok(mut gen_command) = gen_result else {
        return;
    };
    gen_command
        .current_dir(temp.path())
        .arg("generate")
        .arg("--config")
        .arg("hook-bridge.yaml")
        .assert()
        .success();

    let payload =
        r#"{"hook_event_name":"before_command","session_id":"structured_fail","cwd":"."}"#;
    let run_result = Command::cargo_bin("hook_bridge");
    assert!(
        run_result.is_ok(),
        "binary should build for integration tests"
    );
    let Ok(mut run_command) = run_result else {
        return;
    };
    let assert = run_command
        .current_dir(temp.path())
        .arg("run")
        .arg("--platform")
        .arg("codex")
        .arg("--rule-id")
        .arg("r_fail_json")
        .write_stdin(payload)
        .assert()
        .success();
    let output = parse_stdout_json(&assert.get_output().stdout);
    assert_eq!(
        output.get("decision"),
        Some(&serde_json::Value::from("block"))
    );
    let reason = output.get("reason").and_then(serde_json::Value::as_str);
    assert!(
        reason.is_some_and(|value| value.contains("Command failed with exit code 9.")),
        "reason should include the failure header"
    );
    assert!(
        reason.is_some_and(|value| value.contains("Command:\ncat <<'EOF'")),
        "reason should include the command block"
    );
    assert!(
        reason.is_some_and(|value| {
            value.contains(
                "stdout (tail):\n{\"hook_bridge\":{\"kind\":\"additional_context\",\"text\":\"ignore me\"}}"
            )
        }),
        "reason should include the stdout tail contents"
    );
    assert!(
        reason.is_some_and(|value| !value.contains("stderr (tail):")),
        "reason should omit stderr when stderr is empty"
    );
}

#[test]
fn stop_non_zero_exit_returns_command_and_output_summary() {
    let temp_result = tempfile::tempdir();
    assert!(temp_result.is_ok(), "tempdir creation should succeed");
    let Ok(temp) = temp_result else {
        return;
    };

    let write_result = fs::write(
        temp.path().join("hook-bridge.yaml"),
        r"
version: 1
hooks:
  - id: stop_fail
    event: Stop
    command: |
      echo running make verify wrapper
      echo cargo clippy failed >&2
      exit 2
    platforms:
      claude:
        enabled: false
",
    );
    assert!(write_result.is_ok(), "config file should be written");

    let gen_result = Command::cargo_bin("hook_bridge");
    assert!(
        gen_result.is_ok(),
        "binary should build for integration tests"
    );
    let Ok(mut gen_command) = gen_result else {
        return;
    };
    gen_command
        .current_dir(temp.path())
        .arg("generate")
        .arg("--config")
        .arg("hook-bridge.yaml")
        .assert()
        .success();

    let payload = r#"{"hook_event_name":"Stop","session_id":"stop_fail","cwd":"."}"#;
    let run_result = Command::cargo_bin("hook_bridge");
    assert!(
        run_result.is_ok(),
        "binary should build for integration tests"
    );
    let Ok(mut run_command) = run_result else {
        return;
    };
    let assert = run_command
        .current_dir(temp.path())
        .arg("run")
        .arg("--platform")
        .arg("codex")
        .arg("--rule-id")
        .arg("stop_fail")
        .write_stdin(payload)
        .assert()
        .success();
    let output = parse_stdout_json(&assert.get_output().stdout);
    assert_eq!(
        output.get("decision"),
        Some(&serde_json::Value::from("block"))
    );
    let reason = output.get("reason").and_then(serde_json::Value::as_str);
    assert!(
        reason.is_some_and(|value| value.contains("Command failed with exit code 2.")),
        "reason should include the failure header"
    );
    assert!(
        reason.is_some_and(|value| {
            value.contains(
                "Command:\necho running make verify wrapper\necho cargo clippy failed >&2\nexit 2",
            )
        }),
        "reason should include the command block"
    );
    assert!(
        reason.is_some_and(|value| value.contains("stderr (tail):\ncargo clippy failed")),
        "reason should include the stderr tail contents"
    );
    assert!(
        reason.is_some_and(|value| value.contains("stdout (tail):\nrunning make verify wrapper")),
        "reason should include the stdout tail contents"
    );
    let stderr_index = reason.and_then(|value| value.find("stderr (tail):"));
    let stdout_index = reason.and_then(|value| value.find("stdout (tail):"));
    assert!(stderr_index.is_some(), "stderr tail should be present");
    assert!(stdout_index.is_some(), "stdout tail should be present");
    assert!(
        stderr_index < stdout_index,
        "stderr tail should appear before stdout tail"
    );
}

#[test]
fn plaintext_stdout_is_ignored_for_codex_stop() {
    let temp_result = tempfile::tempdir();
    assert!(temp_result.is_ok(), "tempdir creation should succeed");
    let Ok(temp) = temp_result else {
        return;
    };

    let write_result = fs::write(
        temp.path().join("hook-bridge.yaml"),
        r"
version: 1
hooks:
  - id: r_stop_plain
    event: Stop
    command: echo keep-going
    platforms:
      claude:
        enabled: false
",
    );
    assert!(write_result.is_ok(), "config file should be written");

    let gen_result = Command::cargo_bin("hook_bridge");
    assert!(
        gen_result.is_ok(),
        "binary should build for integration tests"
    );
    let Ok(mut gen_command) = gen_result else {
        return;
    };
    gen_command
        .current_dir(temp.path())
        .arg("generate")
        .arg("--config")
        .arg("hook-bridge.yaml")
        .assert()
        .success();

    let payload = r#"{"hook_event_name":"Stop","session_id":"stop_plain","cwd":"."}"#;
    let run_result = Command::cargo_bin("hook_bridge");
    assert!(
        run_result.is_ok(),
        "binary should build for integration tests"
    );
    let Ok(mut run_command) = run_result else {
        return;
    };
    run_command
        .current_dir(temp.path())
        .arg("run")
        .arg("--platform")
        .arg("codex")
        .arg("--rule-id")
        .arg("r_stop_plain")
        .write_stdin(payload)
        .assert()
        .success()
        .stderr(predicate::str::is_empty())
        .stdout(predicate::str::is_empty());
}

#[test]
fn claude_task_completed_exit_two_returns_feedback_without_protocol_error() {
    let temp_result = tempfile::tempdir();
    assert!(temp_result.is_ok(), "tempdir creation should succeed");
    let Ok(temp) = temp_result else {
        return;
    };

    let write_result = fs::write(
        temp.path().join("hook-bridge.yaml"),
        r"
version: 1
hooks:
  - id: teammate_feedback
    event: TaskCompleted
    command: |
      echo ask the user for clarification >&2
      exit 2
    platforms:
      codex:
        enabled: false
",
    );
    assert!(write_result.is_ok(), "config file should be written");

    let gen_result = Command::cargo_bin("hook_bridge");
    assert!(
        gen_result.is_ok(),
        "binary should build for integration tests"
    );
    let Ok(mut gen_command) = gen_result else {
        return;
    };
    gen_command
        .current_dir(temp.path())
        .arg("generate")
        .arg("--config")
        .arg("hook-bridge.yaml")
        .assert()
        .success();

    let payload = r#"{"hook_event_name":"TaskCompleted","session_id":"teammate_done","cwd":"."}"#;
    let run_result = Command::cargo_bin("hook_bridge");
    assert!(
        run_result.is_ok(),
        "binary should build for integration tests"
    );
    let Ok(mut run_command) = run_result else {
        return;
    };
    run_command
        .current_dir(temp.path())
        .arg("run")
        .arg("--platform")
        .arg("claude")
        .arg("--rule-id")
        .arg("teammate_feedback")
        .write_stdin(payload)
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("ask the user for clarification"))
        .stderr(predicate::str::contains("platform protocol error").not())
        .stdout(predicate::str::is_empty());
}

#[test]
fn claude_subagent_stop_exit_two_returns_block_protocol() {
    let temp_result = tempfile::tempdir();
    assert!(temp_result.is_ok(), "tempdir creation should succeed");
    let Ok(temp) = temp_result else {
        return;
    };

    let write_result = fs::write(
        temp.path().join("hook-bridge.yaml"),
        r"
version: 1
hooks:
  - id: subagent_stop_block
    event: SubagentStop
    command: |
      echo stop this subagent >&2
      exit 2
    platforms:
      codex:
        enabled: false
",
    );
    assert!(write_result.is_ok(), "config file should be written");

    let gen_result = Command::cargo_bin("hook_bridge");
    assert!(
        gen_result.is_ok(),
        "binary should build for integration tests"
    );
    let Ok(mut gen_command) = gen_result else {
        return;
    };
    gen_command
        .current_dir(temp.path())
        .arg("generate")
        .arg("--config")
        .arg("hook-bridge.yaml")
        .assert()
        .success();

    let payload = r#"{"hook_event_name":"SubagentStop","session_id":"subagent_done","cwd":"."}"#;
    let run_result = Command::cargo_bin("hook_bridge");
    assert!(
        run_result.is_ok(),
        "binary should build for integration tests"
    );
    let Ok(mut run_command) = run_result else {
        return;
    };
    run_command
        .current_dir(temp.path())
        .arg("run")
        .arg("--platform")
        .arg("claude")
        .arg("--rule-id")
        .arg("subagent_stop_block")
        .write_stdin(payload)
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""decision":"block""#))
        .stdout(predicate::str::contains("Command failed with exit code 2."))
        .stderr(predicate::str::is_empty());
}
