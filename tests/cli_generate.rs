use std::fs;

use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn generate_command_writes_managed_files() {
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
  timeout_sec: 5
hooks:
  - id: r1
    event: before_command
    command: echo ok
",
    );
    assert!(write_result.is_ok(), "config file should be written");

    let command_result = Command::cargo_bin("hook_bridge");
    assert!(
        command_result.is_ok(),
        "binary should build for integration tests"
    );
    let Ok(mut command) = command_result else {
        return;
    };

    command
        .current_dir(temp.path())
        .arg("generate")
        .arg("--config")
        .arg("hook-bridge.yaml")
        .assert()
        .success();

    let claude = temp.path().join(".claude/settings.json");
    let codex = temp.path().join(".codex/hooks.json");

    assert!(claude.exists(), "claude managed file should be generated");
    assert!(codex.exists(), "codex managed file should be generated");

    let content_result = fs::read_to_string(claude);
    assert!(content_result.is_ok(), "generated file should be readable");
    let Ok(claude_content) = content_result else {
        return;
    };
    let parsed_result = serde_json::from_str::<serde_json::Value>(&claude_content);
    assert!(parsed_result.is_ok(), "generated file should be valid json");
    let Ok(parsed) = parsed_result else {
        return;
    };

    assert!(
        claude_content.contains("\"managed_by\": \"hook_bridge\""),
        "managed metadata must be present"
    );
    assert_eq!(
        parsed
            .get("hooks")
            .and_then(|value| value.get("PreToolUse")),
        Some(&serde_json::json!([{
            "hooks": [{
                "type": "command",
                "id": "r1",
                "command": "hook_bridge run --platform claude --rule-id r1",
                "timeout_sec": 5
            }]
        }]))
    );
    let codex_content_result = fs::read_to_string(codex);
    assert!(
        codex_content_result.is_ok(),
        "codex managed file should be readable"
    );
    let Ok(codex_content) = codex_content_result else {
        return;
    };
    let codex_parsed_result = serde_json::from_str::<serde_json::Value>(&codex_content);
    assert!(
        codex_parsed_result.is_ok(),
        "codex managed file should be valid json"
    );
    let Ok(codex_parsed) = codex_parsed_result else {
        return;
    };
    assert_eq!(
        codex_parsed
            .get("hooks")
            .and_then(|value| value.get("PreToolUse")),
        Some(&serde_json::json!([{
            "hooks": [{
                "type": "command",
                "id": "r1",
                "command": "hook_bridge run --platform codex --rule-id r1",
                "timeout_sec": 5
            }]
        }]))
    );
}

#[test]
fn generate_command_rejects_non_managed_target_files() {
    let temp_result = tempfile::tempdir();
    assert!(temp_result.is_ok(), "tempdir creation should succeed");
    let Ok(temp) = temp_result else {
        return;
    };

    let config_path = temp.path().join("hook-bridge.yaml");
    let write_config_result = fs::write(
        &config_path,
        r"
version: 1
hooks:
  - id: r1
    event: before_command
    command: echo ok
",
    );
    assert!(write_config_result.is_ok(), "config file should be written");

    let create_dir_result = fs::create_dir_all(temp.path().join(".codex"));
    assert!(create_dir_result.is_ok(), "codex dir should be created");
    let write_hooks_result = fs::write(temp.path().join(".codex/hooks.json"), "{}");
    assert!(
        write_hooks_result.is_ok(),
        "manual hooks file should be written"
    );

    let command_result = Command::cargo_bin("hook_bridge");
    assert!(
        command_result.is_ok(),
        "binary should build for integration tests"
    );
    let Ok(mut command) = command_result else {
        return;
    };

    command
        .current_dir(temp.path())
        .arg("generate")
        .arg("--config")
        .arg("hook-bridge.yaml")
        .assert()
        .failure()
        .code(4)
        .stderr(predicate::str::contains("file conflict"));
}

#[test]
fn generate_command_overwrites_managed_target_files() {
    let temp_result = tempfile::tempdir();
    assert!(temp_result.is_ok(), "tempdir creation should succeed");
    let Ok(temp) = temp_result else {
        return;
    };

    let config_path = temp.path().join("hook-bridge.yaml");
    let write_config_result = fs::write(
        &config_path,
        r"
version: 1
hooks:
  - id: r1
    event: before_command
    command: echo ok
",
    );
    assert!(write_config_result.is_ok(), "config file should be written");

    let create_dir_result = fs::create_dir_all(temp.path().join(".codex"));
    assert!(create_dir_result.is_ok(), "codex dir should be created");
    let write_hooks_result = fs::write(
        temp.path().join(".codex/hooks.json"),
        serde_json::json!({
            "_hook_bridge": {
                "managed_by": "hook_bridge",
                "managed_version": 1,
                "source_config": "/tmp/old.yaml"
            },
            "hooks": {}
        })
        .to_string(),
    );
    assert!(
        write_hooks_result.is_ok(),
        "managed hooks file should be written"
    );

    let command_result = Command::cargo_bin("hook_bridge");
    assert!(
        command_result.is_ok(),
        "binary should build for integration tests"
    );
    let Ok(mut command) = command_result else {
        return;
    };

    command
        .current_dir(temp.path())
        .arg("generate")
        .arg("--config")
        .arg("hook-bridge.yaml")
        .assert()
        .success();

    let content_result = fs::read_to_string(temp.path().join(".codex/hooks.json"));
    assert!(
        content_result.is_ok(),
        "regenerated hooks should be readable"
    );
    let Ok(content) = content_result else {
        return;
    };
    assert!(
        content.contains("hook_bridge run --platform codex --rule-id r1"),
        "managed file should be overwritten with regenerated hooks"
    );
}

#[test]
fn generate_command_does_not_leave_partial_output_when_second_target_conflicts() {
    let temp_result = tempfile::tempdir();
    assert!(temp_result.is_ok(), "tempdir creation should succeed");
    let Ok(temp) = temp_result else {
        return;
    };

    let config_path = temp.path().join("hook-bridge.yaml");
    let write_config_result = fs::write(
        &config_path,
        r"
version: 1
hooks:
  - id: r1
    event: before_command
    command: echo ok
",
    );
    assert!(write_config_result.is_ok(), "config file should be written");

    let create_dir_result = fs::create_dir_all(temp.path().join(".codex"));
    assert!(create_dir_result.is_ok(), "codex dir should be created");
    let write_hooks_result = fs::write(temp.path().join(".codex/hooks.json"), "{}");
    assert!(
        write_hooks_result.is_ok(),
        "manual codex hooks file should be written"
    );

    let command_result = Command::cargo_bin("hook_bridge");
    assert!(
        command_result.is_ok(),
        "binary should build for integration tests"
    );
    let Ok(mut command) = command_result else {
        return;
    };

    command
        .current_dir(temp.path())
        .arg("generate")
        .arg("--config")
        .arg("hook-bridge.yaml")
        .assert()
        .failure()
        .code(4);

    assert!(
        !temp.path().join(".claude/settings.json").exists(),
        "claude target should not be written when codex preflight fails"
    );
    let codex_content_result = fs::read_to_string(temp.path().join(".codex/hooks.json"));
    assert!(
        codex_content_result.is_ok(),
        "original conflicting codex file should remain readable"
    );
    let Ok(codex_content) = codex_content_result else {
        return;
    };
    assert_eq!(codex_content, "{}");
}

#[test]
fn generate_command_maps_invalid_config_to_config_exit_code() {
    let temp_result = tempfile::tempdir();
    assert!(temp_result.is_ok(), "tempdir creation should succeed");
    let Ok(temp) = temp_result else {
        return;
    };

    let write_config_result = fs::write(
        temp.path().join("hook-bridge.yaml"),
        r"
version: 2
hooks:
  - id: r1
    event: before_command
    command: echo ok
",
    );
    assert!(write_config_result.is_ok(), "config file should be written");

    let command_result = Command::cargo_bin("hook_bridge");
    assert!(
        command_result.is_ok(),
        "binary should build for integration tests"
    );
    let Ok(mut command) = command_result else {
        return;
    };

    command
        .current_dir(temp.path())
        .arg("generate")
        .arg("--config")
        .arg("hook-bridge.yaml")
        .assert()
        .failure()
        .code(3)
        .stderr(predicate::str::contains("config validation error"));
}
