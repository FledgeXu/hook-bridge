use std::fs;

use assert_cmd::Command;

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

    assert!(
        claude_content.contains("\"managed_by\": \"hook_bridge\""),
        "managed metadata must be present"
    );
}
