use std::fs;

use predicates::prelude::*;

use super::{cargo_bin, managed_file, temp_dir, write_basic_config, write_file};

#[test]
fn generate_command_with_platform_writes_only_selected_target() {
    let temp = temp_dir();
    write_basic_config(&temp);

    cargo_bin()
        .current_dir(temp.path())
        .arg("generate")
        .arg("--platform")
        .arg("codex")
        .assert()
        .success();

    assert!(!temp.path().join(".claude/settings.json").exists());
    assert!(temp.path().join(".codex/hooks.json").exists());
}

#[test]
fn generate_command_with_platform_ignores_unmanaged_unselected_target() {
    let temp = temp_dir();
    write_basic_config(&temp);
    std::fs::create_dir_all(temp.path().join(".claude")).unwrap_or_else(|_| unreachable!());
    write_file(temp.path().join(".claude/settings.json"), "{}");

    cargo_bin()
        .current_dir(temp.path())
        .arg("generate")
        .arg("--platform")
        .arg("codex")
        .assert()
        .success();

    let claude_content = fs::read_to_string(temp.path().join(".claude/settings.json"))
        .unwrap_or_else(|_| unreachable!());
    assert_eq!(claude_content, "{}");
    assert!(temp.path().join(".codex/hooks.json").exists());
}

#[test]
fn generate_command_with_platform_rejects_non_managed_selected_target() {
    let temp = temp_dir();
    write_basic_config(&temp);
    std::fs::create_dir_all(temp.path().join(".codex")).unwrap_or_else(|_| unreachable!());
    write_file(temp.path().join(".codex/hooks.json"), "{}");

    cargo_bin()
        .current_dir(temp.path())
        .arg("generate")
        .arg("--platform")
        .arg("codex")
        .assert()
        .failure()
        .code(4)
        .stderr(predicate::str::contains("file conflict"));
}

#[test]
fn generate_command_with_platform_overwrites_selected_managed_target_only() {
    let temp = temp_dir();
    write_basic_config(&temp);
    std::fs::create_dir_all(temp.path().join(".codex")).unwrap_or_else(|_| unreachable!());
    write_file(
        temp.path().join(".codex/hooks.json"),
        managed_file("/tmp/old.yaml"),
    );
    std::fs::create_dir_all(temp.path().join(".claude")).unwrap_or_else(|_| unreachable!());
    write_file(
        temp.path().join(".claude/settings.json"),
        serde_json::json!({
            "_hook_bridge": {
                "managed_by": "hook_bridge",
                "managed_version": 1,
                "source_config": "/tmp/claude-old.yaml"
            },
            "hooks": {
                "PreToolUse": [{
                    "hooks": [{
                        "type": "command",
                        "id": "old",
                        "command": "old command",
                        "timeout_sec": 5
                    }]
                }]
            }
        })
        .to_string(),
    );

    cargo_bin()
        .current_dir(temp.path())
        .arg("generate")
        .arg("--platform")
        .arg("codex")
        .assert()
        .success();

    let codex_content = fs::read_to_string(temp.path().join(".codex/hooks.json"))
        .unwrap_or_else(|_| unreachable!());
    assert!(codex_content.contains("hook_bridge run --platform codex --rule-id r1"));

    let claude_content = fs::read_to_string(temp.path().join(".claude/settings.json"))
        .unwrap_or_else(|_| unreachable!());
    assert!(
        claude_content.contains("\"id\":\"old\"") || claude_content.contains("\"id\": \"old\"")
    );
}
