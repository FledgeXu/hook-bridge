use super::*;

#[test]
fn retry_state_is_isolated_between_projects_with_same_thread_and_rule() {
    let alpha_dir_result = tempfile::tempdir();
    assert!(alpha_dir_result.is_ok(), "project A tempdir should succeed");
    let Ok(alpha_dir) = alpha_dir_result else {
        return;
    };
    let beta_dir_result = tempfile::tempdir();
    assert!(beta_dir_result.is_ok(), "project B tempdir should succeed");
    let Ok(beta_dir) = beta_dir_result else {
        return;
    };

    for root in [alpha_dir.path(), beta_dir.path()] {
        let write_result = fs::write(
            root.join("hook-bridge.yaml"),
            r"
version: 1
defaults:
  max_retries: 1
hooks:
  - id: same_rule
    event: before_command
    command: echo fail >> attempts.log; exit 1
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
            .current_dir(root)
            .arg("generate")
            .arg("--config")
            .arg("hook-bridge.yaml")
            .assert()
            .success();
    }

    let payload = r#"{"hook_event_name":"before_command","session_id":"shared_thread","cwd":"."}"#;

    let first_project_run_result = Command::cargo_bin("hook_bridge");
    assert!(
        first_project_run_result.is_ok(),
        "binary should build for integration tests"
    );
    let Ok(mut run_first_project) = first_project_run_result else {
        return;
    };
    run_first_project
        .current_dir(alpha_dir.path())
        .arg("run")
        .arg("--platform")
        .arg("codex")
        .arg("--rule-id")
        .arg("same_rule")
        .write_stdin(payload)
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""decision":"block""#));

    let second_project_run_result = Command::cargo_bin("hook_bridge");
    assert!(
        second_project_run_result.is_ok(),
        "binary should build for integration tests"
    );
    let Ok(mut run_second_project) = second_project_run_result else {
        return;
    };
    run_second_project
        .current_dir(beta_dir.path())
        .arg("run")
        .arg("--platform")
        .arg("codex")
        .arg("--rule-id")
        .arg("same_rule")
        .write_stdin(payload)
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""decision":"block""#))
        .stdout(predicate::str::contains("max retries reached").not());

    let alpha_attempts_result = fs::read_to_string(alpha_dir.path().join("attempts.log"));
    assert!(
        alpha_attempts_result.is_ok(),
        "project A should execute command once"
    );
    let beta_attempts_result = fs::read_to_string(beta_dir.path().join("attempts.log"));
    assert!(
        beta_attempts_result.is_ok(),
        "project B should execute command independently"
    );
}

#[test]
fn retry_state_is_isolated_between_rules_and_sessions() {
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
  max_retries: 3
hooks:
  - id: r1
    event: before_command
    command: exit 1
  - id: r2
    event: before_command
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

    let payload_session_a =
        r#"{"hook_event_name":"before_command","session_id":"session_a","cwd":"."}"#;
    let payload_session_b =
        r#"{"hook_event_name":"before_command","session_id":"session_b","cwd":"."}"#;

    for (rule_id, payload) in [
        ("r1", payload_session_a),
        ("r2", payload_session_a),
        ("r1", payload_session_b),
    ] {
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
            .arg(rule_id)
            .write_stdin(payload)
            .assert()
            .success();
    }

    let state_r1_a = retry_state_path("codex", &config_path, "session_a", "r1");
    let state_r2_a = retry_state_path("codex", &config_path, "session_a", "r2");
    let state_r1_b = retry_state_path("codex", &config_path, "session_b", "r1");

    assert!(state_r1_a.exists(), "session_a/r1 retry state should exist");
    assert!(state_r2_a.exists(), "session_a/r2 retry state should exist");
    assert!(state_r1_b.exists(), "session_b/r1 retry state should exist");
    assert_ne!(state_r1_a, state_r2_a);
    assert_ne!(state_r1_a, state_r1_b);
    assert_ne!(state_r2_a, state_r1_b);

    for path in [state_r1_a, state_r2_a, state_r1_b] {
        let cleanup_result = fs::remove_file(&path);
        assert!(
            cleanup_result.is_ok(),
            "retry state fixture should be removable after test"
        );
    }
}

#[test]
fn retry_state_file_is_created_on_failure_and_removed_after_success() {
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
  max_retries: 3
hooks:
  - id: r_reset
    event: before_command
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

    let payload = r#"{"hook_event_name":"before_command","session_id":"reset_session","cwd":"."}"#;
    let state_path = retry_state_path("codex", &config_path, "reset_session", "r_reset");

    let first_run_result = Command::cargo_bin("hook_bridge");
    assert!(
        first_run_result.is_ok(),
        "binary should build for integration tests"
    );
    let Ok(mut first_run) = first_run_result else {
        return;
    };
    first_run
        .current_dir(temp.path())
        .arg("run")
        .arg("--platform")
        .arg("codex")
        .arg("--rule-id")
        .arg("r_reset")
        .write_stdin(payload)
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""decision":"block""#));

    assert!(state_path.exists(), "failing run should create retry state");

    let rewrite_result = fs::write(
        &config_path,
        r"
version: 1
defaults:
  max_retries: 3
hooks:
  - id: r_reset
    event: before_command
    command: echo recovered
",
    );
    assert!(
        rewrite_result.is_ok(),
        "config file should be rewritable between runs"
    );

    let second_run_result = Command::cargo_bin("hook_bridge");
    assert!(
        second_run_result.is_ok(),
        "binary should build for integration tests"
    );
    let Ok(mut second_run) = second_run_result else {
        return;
    };
    second_run
        .current_dir(temp.path())
        .arg("run")
        .arg("--platform")
        .arg("codex")
        .arg("--rule-id")
        .arg("r_reset")
        .write_stdin(payload)
        .assert()
        .success()
        .stdout(predicate::str::is_empty());

    assert!(
        !state_path.exists(),
        "successful run should clear prior retry state"
    );
}

#[test]
fn retry_state_updates_consecutive_failure_count_across_runs() {
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
  max_retries: 3
hooks:
  - id: r_count
    event: before_command
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

    let payload = r#"{"hook_event_name":"before_command","session_id":"count_session","cwd":"."}"#;
    let state_path = retry_state_path("codex", &config_path, "count_session", "r_count");

    for expected_count in [1_u64, 2] {
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
            .arg("r_count")
            .write_stdin(payload)
            .assert()
            .success()
            .stdout(predicate::str::contains(r#""decision":"block""#));

        let state_result = fs::read_to_string(&state_path);
        assert!(
            state_result.is_ok(),
            "retry state should exist after failed run"
        );
        let Ok(state_json) = state_result else {
            return;
        };
        let parsed_result = serde_json::from_str::<serde_json::Value>(&state_json);
        assert!(parsed_result.is_ok(), "retry state should stay valid json");
        let Ok(parsed) = parsed_result else {
            return;
        };

        assert_eq!(
            parsed.get("consecutive_failures"),
            Some(&serde_json::Value::from(expected_count))
        );
        assert_eq!(
            parsed.get("last_error"),
            Some(&serde_json::Value::from(
                "Command failed with exit code 1.\n\nCommand:\nexit 1"
            ))
        );
    }

    let cleanup_result = fs::remove_file(&state_path);
    assert!(
        cleanup_result.is_ok(),
        "retry state fixture should be removable after test"
    );
}

#[test]
fn invalid_structured_output_creates_retry_state_after_protocol_failure() {
    let temp_result = tempfile::tempdir();
    assert!(temp_result.is_ok(), "tempdir creation should succeed");
    let Ok(temp) = temp_result else {
        return;
    };

    let config_path = temp.path().join("hook-bridge.yaml");
    let write_result = fs::write(
        &config_path,
        r#"
version: 1
defaults:
  max_retries: 2
hooks:
  - id: bad_structured
    event: Notification
    command: |
      cat <<'EOF'
      {"hook_bridge":{"kind":"stop","reason":"later"}}
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
        r#"{"hook_event_name":"Notification","session_id":"bad_structured_session","cwd":"."}"#;
    let state_path = retry_state_path(
        "claude",
        &config_path,
        "bad_structured_session",
        "bad_structured",
    );

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
        .arg("bad_structured")
        .write_stdin(payload)
        .assert()
        .failure()
        .code(8)
        .stderr(predicate::str::contains("platform protocol error"));

    let state_result = fs::read_to_string(&state_path);
    assert!(
        state_result.is_ok(),
        "retry state should exist after translate-time protocol failure"
    );
    let Ok(state_json) = state_result else {
        return;
    };
    let parsed_result = serde_json::from_str::<serde_json::Value>(&state_json);
    assert!(parsed_result.is_ok(), "retry state should stay valid json");
    let Ok(parsed) = parsed_result else {
        return;
    };
    assert_eq!(
        parsed.get("consecutive_failures"),
        Some(&serde_json::Value::from(1))
    );
    assert!(
        parsed
            .get("last_error")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|message| message.contains("platform protocol error")),
        "retry state should record the translate-time protocol error"
    );
}
