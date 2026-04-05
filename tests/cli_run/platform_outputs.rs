use super::*;

#[test]
fn generate_to_run_round_trip_works_for_claude_native_protocol() {
    let temp_result = tempfile::tempdir();
    assert!(temp_result.is_ok(), "tempdir creation should succeed");
    let Ok(temp) = temp_result else {
        return;
    };

    let config_path = temp.path().join("hook-bridge.yaml");
    let write_result = fs::write(
        &config_path,
        r"
version: 1
defaults:
  timeout_sec: 10
hooks:
  - id: r_claude_round_trip
    event: before_command
    command: cat > claude-payload.json
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

    let settings_result = fs::read_to_string(temp.path().join(".claude/settings.json"));
    assert!(
        settings_result.is_ok(),
        "generated claude settings should be readable"
    );
    let Ok(settings) = settings_result else {
        return;
    };
    assert!(
        settings.contains("hook_bridge run --platform claude --rule-id r_claude_round_trip"),
        "generated claude hook should point at the run command"
    );

    let payload = r#"{"hook_event_name":"PreToolUse","session_id":"claude_round_trip","cwd":"."}"#;
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
        .arg("r_claude_round_trip")
        .write_stdin(payload)
        .assert()
        .success()
        .stdout(predicate::str::is_empty());

    let payload_result = fs::read_to_string(temp.path().join("claude-payload.json"));
    assert!(
        payload_result.is_ok(),
        "claude command should receive the raw payload over stdin"
    );
    let Ok(persisted_payload) = payload_result else {
        return;
    };
    assert_eq!(persisted_payload, payload);
}

#[test]
fn claude_payload_uses_hook_event_name_and_block_decision_on_failure() {
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
  - id: r_claude
    event: UserPromptSubmit
    command: exit 1
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

    let payload = r#"{"hook_event_name":"UserPromptSubmit","session_id":"claude_s","cwd":"."}"#;
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
        .arg("r_claude")
        .write_stdin(payload)
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""decision":"block""#));
}

#[test]
fn run_command_maps_invalid_json_payload_to_json_parse_exit_code() {
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
  - id: r1
    event: before_command
    command: echo ok
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
        .arg("r1")
        .write_stdin("{")
        .assert()
        .failure()
        .code(5)
        .stderr(predicate::str::contains("json parse error"));
}

#[test]
fn run_command_translates_structured_codex_session_start_output() {
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
  - id: r_session
    event: session_start
    command: |
      cat <<'EOF'
      {"hook_bridge":{"kind":"additional_context","text":"Load the workspace conventions before editing."}}
      EOF
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
        r#"{"hook_event_name":"SessionStart","session_id":"session_structured","cwd":"."}"#;
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
        .arg("r_session")
        .write_stdin(payload)
        .assert()
        .success()
        .stdout(predicate::str::contains(
            r#""hookEventName":"SessionStart""#,
        ))
        .stdout(predicate::str::contains(
            r#""additionalContext":"Load the workspace conventions before editing.""#,
        ));
}

#[test]
fn run_command_translates_plaintext_codex_session_start_output() {
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
  - id: r_session_plain
    event: session_start
    command: echo Load the workspace conventions before editing.
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

    let payload = r#"{"hook_event_name":"SessionStart","session_id":"session_plain","cwd":"."}"#;
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
        .arg("r_session_plain")
        .write_stdin(payload)
        .assert()
        .success()
        .stdout(predicate::str::contains(
            r#""hookEventName":"SessionStart""#,
        ))
        .stdout(predicate::str::contains(
            r#""additionalContext":"Load the workspace conventions before editing.""#,
        ));
}

#[test]
fn run_command_translates_structured_claude_worktree_path_output() {
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
  - id: r_worktree
    event: WorktreeCreate
    command: |
      cat <<'EOF'
      {"hook_bridge":{"kind":"worktree_path","path":"/tmp/hook-bridge-worktree"}}
      EOF
    platforms:
      codex:
        enabled: false
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
        r#"{"hook_event_name":"WorktreeCreate","session_id":"claude_worktree","cwd":"."}"#;
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
        .arg("r_worktree")
        .write_stdin(payload)
        .assert()
        .success()
        .stdout("/tmp/hook-bridge-worktree\n");
}
