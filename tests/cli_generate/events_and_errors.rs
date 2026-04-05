use std::fs;

use predicates::prelude::*;

use super::{cargo_bin, temp_dir, write_file};

#[test]
fn generate_command_maps_invalid_config_to_config_exit_code() {
    let temp = temp_dir();
    write_file(
        temp.path().join("hook-bridge.yaml"),
        r"
version: 2
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
        .failure()
        .code(3)
        .stderr(predicate::str::contains("config validation error"));
}

#[test]
fn generate_command_maps_invalid_yaml_syntax_to_config_exit_code() {
    let temp = temp_dir();
    write_file(temp.path().join("hook-bridge.yaml"), "version: [\n");

    cargo_bin()
        .current_dir(temp.path())
        .arg("generate")
        .arg("--config")
        .arg("hook-bridge.yaml")
        .assert()
        .failure()
        .code(3)
        .stderr(predicate::str::contains("config validation error"));
}

#[test]
fn generate_command_uses_native_event_names_for_extended_events() {
    let temp = temp_dir();
    write_file(
        temp.path().join("hook-bridge.yaml"),
        r"
version: 1
hooks:
  - id: s1
    event: session_start
    command: echo start
    matcher: resume
  - id: c1
    event: Notification
    command: echo note
    platforms:
      codex:
        enabled: false
",
    );

    cargo_bin()
        .current_dir(temp.path())
        .arg("generate")
        .arg("--config")
        .arg("hook-bridge.yaml")
        .assert()
        .success();

    let claude_content = fs::read_to_string(temp.path().join(".claude/settings.json"))
        .unwrap_or_else(|_| unreachable!());
    let claude_parsed: serde_json::Value =
        serde_json::from_str(&claude_content).unwrap_or_else(|_| unreachable!());
    assert_eq!(
        claude_parsed
            .get("hooks")
            .and_then(|value| value.get("SessionStart")),
        Some(&serde_json::json!([{
            "matcher": "resume",
            "hooks": [{
                "type": "command",
                "id": "s1",
                "command": "hook_bridge run --platform claude --rule-id s1",
                "timeout_sec": 30
            }]
        }]))
    );
    assert!(
        claude_parsed
            .get("hooks")
            .and_then(|value| value.get("Notification"))
            .is_some()
    );

    let codex_content = fs::read_to_string(temp.path().join(".codex/hooks.json"))
        .unwrap_or_else(|_| unreachable!());
    let codex_parsed: serde_json::Value =
        serde_json::from_str(&codex_content).unwrap_or_else(|_| unreachable!());
    assert_eq!(
        codex_parsed
            .get("hooks")
            .and_then(|value| value.get("SessionStart")),
        Some(&serde_json::json!([{
            "matcher": "resume",
            "hooks": [{
                "type": "command",
                "id": "s1",
                "command": "hook_bridge run --platform codex --rule-id s1",
                "timeout_sec": 30
            }]
        }]))
    );
    assert!(
        codex_parsed
            .get("hooks")
            .and_then(|value| value.get("Notification"))
            .is_none()
    );
}
