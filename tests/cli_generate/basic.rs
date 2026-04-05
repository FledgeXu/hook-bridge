use std::fs;

use predicates::prelude::*;

use super::{cargo_bin, managed_file, temp_dir, write_basic_config, write_file};

#[test]
fn generate_command_writes_managed_files() {
    let temp = temp_dir();
    write_file(
        temp.path().join("hook-bridge.yaml"),
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

    cargo_bin()
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

    let claude_content = fs::read_to_string(&claude).unwrap_or_else(|_| unreachable!());
    let parsed: serde_json::Value =
        serde_json::from_str(&claude_content).unwrap_or_else(|_| unreachable!());
    assert!(claude_content.contains("\"managed_by\": \"hook_bridge\""));
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

    let codex_content = fs::read_to_string(codex).unwrap_or_else(|_| unreachable!());
    let codex_parsed: serde_json::Value =
        serde_json::from_str(&codex_content).unwrap_or_else(|_| unreachable!());
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
fn generate_command_uses_default_config_path() {
    let temp = temp_dir();
    write_basic_config(&temp);

    cargo_bin()
        .current_dir(temp.path())
        .arg("generate")
        .assert()
        .success();

    assert!(temp.path().join(".claude/settings.json").exists());
    assert!(temp.path().join(".codex/hooks.json").exists());
}

#[test]
fn generate_command_without_default_config_reports_missing_file() {
    let temp = temp_dir();

    cargo_bin()
        .current_dir(temp.path())
        .arg("generate")
        .assert()
        .failure()
        .code(10)
        .stderr(predicate::str::contains("hook-bridge.yaml"));
}

#[test]
fn generate_command_rejects_non_managed_target_files() {
    let temp = temp_dir();
    write_basic_config(&temp);
    std::fs::create_dir_all(temp.path().join(".codex")).unwrap_or_else(|_| unreachable!());
    write_file(temp.path().join(".codex/hooks.json"), "{}");

    cargo_bin()
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
    let temp = temp_dir();
    write_basic_config(&temp);
    std::fs::create_dir_all(temp.path().join(".codex")).unwrap_or_else(|_| unreachable!());
    write_file(
        temp.path().join(".codex/hooks.json"),
        managed_file("/tmp/old.yaml"),
    );

    cargo_bin()
        .current_dir(temp.path())
        .arg("generate")
        .arg("--config")
        .arg("hook-bridge.yaml")
        .assert()
        .success();

    let content = fs::read_to_string(temp.path().join(".codex/hooks.json"))
        .unwrap_or_else(|_| unreachable!());
    assert!(content.contains("hook_bridge run --platform codex --rule-id r1"));
}

#[test]
fn generate_command_does_not_leave_partial_output_when_second_target_conflicts() {
    let temp = temp_dir();
    write_basic_config(&temp);
    std::fs::create_dir_all(temp.path().join(".codex")).unwrap_or_else(|_| unreachable!());
    write_file(temp.path().join(".codex/hooks.json"), "{}");

    cargo_bin()
        .current_dir(temp.path())
        .arg("generate")
        .arg("--config")
        .arg("hook-bridge.yaml")
        .assert()
        .failure()
        .code(4);

    assert!(!temp.path().join(".claude/settings.json").exists());
    let codex_content = fs::read_to_string(temp.path().join(".codex/hooks.json"))
        .unwrap_or_else(|_| unreachable!());
    assert_eq!(codex_content, "{}");
}
