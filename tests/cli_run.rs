use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use predicates::prelude::*;
use sha2::{Digest, Sha256};

fn retry_state_path(
    platform: &str,
    source_config_path: &Path,
    session_id: &str,
    rule_id: &str,
) -> PathBuf {
    let normalized_source_config =
        fs::canonicalize(source_config_path).unwrap_or_else(|_| source_config_path.to_path_buf());
    let mut hasher = Sha256::new();
    hasher.update(normalized_source_config.to_string_lossy().as_bytes());
    hasher.update(session_id.as_bytes());
    let hash = hex::encode(hasher.finalize());

    std::env::temp_dir()
        .join("hook_bridge")
        .join("retries")
        .join(platform)
        .join(hash)
        .join(format!("{rule_id}.json"))
}

#[test]
fn run_command_executes_rule_and_returns_codex_protocol() {
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
  - id: r1
    event: before_command
    command: cat > payload.json
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

    let payload = r#"{"hook_event_name":"before_command","session_id":"t1","cwd":"."}"#;

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
        .write_stdin(payload)
        .assert()
        .success()
        .stdout(predicate::str::is_empty());

    let payload_result = fs::read_to_string(temp.path().join("payload.json"));
    assert!(
        payload_result.is_ok(),
        "payload file should be created by command"
    );
    let Ok(persisted_payload) = payload_result else {
        return;
    };

    assert_eq!(persisted_payload, payload);
}

#[test]
fn run_command_accepts_native_platform_event_name_from_generated_hook() {
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
hooks:
  - id: r_native
    event: before_command
    command: echo native > native.txt
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

    let payload = r#"{"hook_event_name":"PreToolUse","session_id":"t_native","cwd":"."}"#;

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
        .arg("r_native")
        .write_stdin(payload)
        .assert()
        .success()
        .stdout(predicate::str::is_empty());

    let output_result = fs::read_to_string(temp.path().join("native.txt"));
    assert!(
        output_result.is_ok(),
        "native event mapping should still execute the configured rule"
    );
}

#[test]
fn run_uses_absolute_managed_source_config_across_working_directories() {
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
  - id: r_abs
    event: before_command
    command: echo ok > abs.txt
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

    let nested = temp.path().join("nested");
    let mkdir_result = fs::create_dir_all(&nested);
    assert!(mkdir_result.is_ok(), "nested dir should be creatable");
    let nested_codex = nested.join(".codex");
    let nested_codex_result = fs::create_dir_all(&nested_codex);
    assert!(
        nested_codex_result.is_ok(),
        "nested codex dir should be creatable"
    );
    let copy_hooks_result = fs::copy(
        temp.path().join(".codex/hooks.json"),
        nested_codex.join("hooks.json"),
    );
    assert!(
        copy_hooks_result.is_ok(),
        "managed hooks file should be copied to nested cwd"
    );

    let payload = r#"{"hook_event_name":"before_command","session_id":"abs_thread","cwd":"."}"#;
    let run_result = Command::cargo_bin("hook_bridge");
    assert!(
        run_result.is_ok(),
        "binary should build for integration tests"
    );
    let Ok(mut run_command) = run_result else {
        return;
    };

    run_command
        .current_dir(&nested)
        .arg("run")
        .arg("--platform")
        .arg("codex")
        .arg("--rule-id")
        .arg("r_abs")
        .write_stdin(payload)
        .assert()
        .success();

    let output_result = fs::read_to_string(nested.join("abs.txt"));
    assert!(
        output_result.is_ok(),
        "run should still resolve source config and execute in nested cwd"
    );
}

#[test]
fn non_zero_exit_increments_retry_and_hits_max_retries_guard() {
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
  max_retries: 1
hooks:
  - id: r_fail
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
        .current_dir(temp.path())
        .arg("generate")
        .arg("--config")
        .arg("hook-bridge.yaml")
        .assert()
        .success();

    let thread_suffix = temp.path().display();
    let unique_thread = format!("t_retry_{thread_suffix}");
    let payload = format!(
        "{{\"hook_event_name\":\"before_command\",\"session_id\":\"{unique_thread}\",\"cwd\":\".\"}}"
    );

    let first_run = Command::cargo_bin("hook_bridge");
    assert!(
        first_run.is_ok(),
        "binary should build for integration tests"
    );
    let Ok(mut first) = first_run else {
        return;
    };
    first
        .current_dir(temp.path())
        .arg("run")
        .arg("--platform")
        .arg("codex")
        .arg("--rule-id")
        .arg("r_fail")
        .write_stdin(payload.as_str())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""decision":"block""#));

    let second_run = Command::cargo_bin("hook_bridge");
    assert!(
        second_run.is_ok(),
        "binary should build for integration tests"
    );
    let Ok(mut second) = second_run else {
        return;
    };
    second
        .current_dir(temp.path())
        .arg("run")
        .arg("--platform")
        .arg("codex")
        .arg("--rule-id")
        .arg("r_fail")
        .write_stdin(payload.as_str())
        .assert()
        .success()
        .stdout(predicate::str::contains("max retries reached"));

    let attempts_result = fs::read_to_string(temp.path().join("attempts.log"));
    assert!(attempts_result.is_ok(), "attempt log should be readable");
    let Ok(attempts) = attempts_result else {
        return;
    };
    assert_eq!(attempts.lines().count(), 1);
}

#[test]
fn spawn_failure_still_returns_protocol_json() {
    let temp_result = tempfile::tempdir();
    assert!(temp_result.is_ok(), "tempdir creation should succeed");
    let Ok(temp) = temp_result else {
        return;
    };

    let write_result = fs::write(
        temp.path().join("hook-bridge.yaml"),
        r"
version: 1
defaults:
  shell: command_that_does_not_exist_123
hooks:
  - id: r_spawn_fail
    event: before_command
    command: echo never
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

    let payload = r#"{"hook_event_name":"before_command","session_id":"spawn_fail","cwd":"."}"#;
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
        .arg("r_spawn_fail")
        .write_stdin(payload)
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""decision":"block""#))
        .stdout(predicate::str::contains("failed to spawn process"));
}

#[test]
fn timeout_still_returns_protocol_json() {
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
  - id: r_timeout
    event: before_command
    command: sleep 2
    timeout_sec: 1
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

    let payload = r#"{"hook_event_name":"before_command","session_id":"timeout_case","cwd":"."}"#;
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
        .arg("r_timeout")
        .write_stdin(payload)
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""decision":"block""#))
        .stdout(predicate::str::contains("timeout after 1s"));
}

#[test]
fn run_rejects_managed_version_mismatch() {
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
  - id: r_version
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

    let hooks_path = temp.path().join(".codex/hooks.json");
    let hooks_content_result = fs::read_to_string(&hooks_path);
    assert!(
        hooks_content_result.is_ok(),
        "managed hooks should be readable for mutation"
    );
    let Ok(hooks_content) = hooks_content_result else {
        return;
    };
    let patched = hooks_content.replace("\"managed_version\": 1", "\"managed_version\": 99");
    let write_patched_result = fs::write(&hooks_path, patched);
    assert!(
        write_patched_result.is_ok(),
        "patched managed hooks should be writable"
    );

    let payload = r#"{"hook_event_name":"before_command","session_id":"version_case","cwd":"."}"#;
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
        .arg("r_version")
        .write_stdin(payload)
        .assert()
        .failure()
        .code(8)
        .stderr(predicate::str::contains("unsupported managed_version"));
}

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

    let payload = r#"{"hook_event_name":"before_command","session_id":"claude_s","cwd":"."}"#;
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
