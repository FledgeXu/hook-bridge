use super::*;

fn write_config(root: &Path, config: &str) -> PathBuf {
    let config_path = root.join("hook-bridge.yaml");
    let write_result = fs::write(&config_path, config);
    assert!(write_result.is_ok(), "config file should be written");
    config_path
}

fn generate_ok(root: &Path) {
    let generate_result = Command::cargo_bin("hook_bridge");
    assert!(
        generate_result.is_ok(),
        "binary should build for integration tests"
    );
    let Ok(mut generate_command) = generate_result else {
        unreachable!();
    };
    generate_command
        .current_dir(root)
        .arg("generate")
        .arg("--config")
        .arg("hook-bridge.yaml")
        .assert()
        .success();
}

fn generate_fails(root: &Path, needle: &str) {
    let generate_result = Command::cargo_bin("hook_bridge");
    assert!(
        generate_result.is_ok(),
        "binary should build for integration tests"
    );
    let Ok(mut generate_command) = generate_result else {
        unreachable!();
    };
    generate_command
        .current_dir(root)
        .arg("generate")
        .arg("--config")
        .arg("hook-bridge.yaml")
        .assert()
        .failure()
        .code(3)
        .stderr(predicate::str::contains(needle));
}

fn run_codex_rule(root: &Path, rule_id: &str, session_id: &str) -> assert_cmd::assert::Assert {
    let run_result = Command::cargo_bin("hook_bridge");
    assert!(
        run_result.is_ok(),
        "binary should build for integration tests"
    );
    let Ok(mut run_command) = run_result else {
        unreachable!();
    };
    let payload = format!(
        "{{\"hook_event_name\":\"before_command\",\"session_id\":\"{session_id}\",\"cwd\":\".\"}}"
    );
    run_command
        .current_dir(root)
        .arg("run")
        .arg("--platform")
        .arg("codex")
        .arg("--rule-id")
        .arg(rule_id)
        .write_stdin(payload)
        .assert()
}

fn attempts_count(root: &Path) -> usize {
    let attempts_result = fs::read_to_string(root.join("attempts.log"));
    assert!(attempts_result.is_ok(), "attempt log should be readable");
    attempts_result.unwrap_or_default().lines().count()
}

fn read_consecutive_failures(path: &Path) -> Option<u64> {
    let state_json = fs::read_to_string(path).ok()?;
    let parsed = serde_json::from_str::<serde_json::Value>(&state_json).ok()?;
    parsed.get("consecutive_failures")?.as_u64()
}

#[test]
fn block_policy_short_circuits_after_threshold_without_resetting_state() {
    let temp_result = tempfile::tempdir();
    assert!(temp_result.is_ok(), "tempdir creation should succeed");
    let Ok(temp) = temp_result else {
        return;
    };

    let config_path = write_config(
        temp.path(),
        r"
version: 1
defaults:
  max_retries: 1
  on_max_retries: block
hooks:
  - id: r_block
    event: before_command
    command: echo fail >> attempts.log; exit 1
",
    );
    generate_ok(temp.path());

    run_codex_rule(temp.path(), "r_block", "block_session")
        .success()
        .stdout(predicate::str::contains(r#""decision":"block""#))
        .stdout(predicate::str::contains("max retries reached").not());
    run_codex_rule(temp.path(), "r_block", "block_session")
        .success()
        .stdout(predicate::str::contains(r#""decision":"block""#))
        .stdout(predicate::str::contains("max retries reached"));

    let state_path = retry_state_path("codex", &config_path, "block_session", "r_block");
    assert_eq!(attempts_count(temp.path()), 1);
    assert!(
        state_path.exists(),
        "block policy should preserve retry state"
    );
}

#[test]
fn allow_and_reset_policy_succeeds_silently_and_starts_count_over() {
    let temp_result = tempfile::tempdir();
    assert!(temp_result.is_ok(), "tempdir creation should succeed");
    let Ok(temp) = temp_result else {
        return;
    };

    let config_path = write_config(
        temp.path(),
        r"
version: 1
defaults:
  max_retries: 1
  on_max_retries: allow_and_reset
hooks:
  - id: r_allow
    event: before_command
    command: echo fail >> attempts.log; exit 1
",
    );
    generate_ok(temp.path());

    let state_path = retry_state_path("codex", &config_path, "allow_session", "r_allow");
    run_codex_rule(temp.path(), "r_allow", "allow_session")
        .success()
        .stdout(predicate::str::contains(r#""decision":"block""#));
    assert!(state_path.exists(), "failing run should create retry state");

    run_codex_rule(temp.path(), "r_allow", "allow_session")
        .success()
        .stdout(predicate::str::is_empty());
    assert!(
        !state_path.exists(),
        "allow_and_reset should remove retry state after guard success"
    );

    run_codex_rule(temp.path(), "r_allow", "allow_session")
        .success()
        .stdout(predicate::str::contains(r#""decision":"block""#));
    assert_eq!(read_consecutive_failures(&state_path), Some(1));
    assert_eq!(attempts_count(temp.path()), 2);
}

#[test]
fn stop_policy_degrades_to_block_when_event_cannot_stop() {
    let temp_result = tempfile::tempdir();
    assert!(temp_result.is_ok(), "tempdir creation should succeed");
    let Ok(temp) = temp_result else {
        return;
    };

    write_config(
        temp.path(),
        r"
version: 1
defaults:
  max_retries: 1
  on_max_retries: stop
hooks:
  - id: r_stop
    event: before_command
    command: echo fail >> attempts.log; exit 1
",
    );
    generate_ok(temp.path());

    for _ in 0..2 {
        run_codex_rule(temp.path(), "r_stop", "stop_session")
            .success()
            .stdout(predicate::str::contains(r#""decision":"block""#));
    }

    assert_eq!(attempts_count(temp.path()), 1);
}

#[test]
fn block_policy_is_rejected_for_claude_stop_only_events() {
    let temp_result = tempfile::tempdir();
    assert!(temp_result.is_ok(), "tempdir creation should succeed");
    let Ok(temp) = temp_result else {
        return;
    };

    write_config(
        temp.path(),
        r"
version: 1
defaults:
  max_retries: 1
  on_max_retries: block
hooks:
  - id: r1
    event: TaskCompleted
    command: echo ok
    platforms:
      codex:
        enabled: false
",
    );
    generate_fails(
        temp.path(),
        "field 'on_max_retries' value 'block' is not supported for event 'TaskCompleted' on platform 'claude'",
    );
}

#[test]
fn stop_policy_is_rejected_for_claude_side_effect_only_events_when_retries_are_enabled() {
    let temp_result = tempfile::tempdir();
    assert!(temp_result.is_ok(), "tempdir creation should succeed");
    let Ok(temp) = temp_result else {
        return;
    };

    write_config(
        temp.path(),
        r"
version: 1
hooks:
  - id: r1
    event: Notification
    command: echo ok
    max_retries: 1
    platforms:
      codex:
        enabled: false
",
    );
    generate_fails(
        temp.path(),
        "field 'on_max_retries' value 'stop' is not supported for event 'Notification' on platform 'claude'",
    );
}
