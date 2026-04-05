use std::fs;
use std::path::Path;

use assert_cmd::Command;

fn temp_dir() -> tempfile::TempDir {
    let temp_result = tempfile::tempdir();
    assert!(temp_result.is_ok(), "tempdir creation should succeed");
    temp_result.unwrap_or_else(|_| unreachable!())
}

fn cargo_bin() -> Command {
    let command_result = Command::cargo_bin("hook_bridge");
    assert!(
        command_result.is_ok(),
        "binary should build for integration tests"
    );
    command_result.unwrap_or_else(|_| unreachable!())
}

fn write_file(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) {
    let write_result = fs::write(path, contents);
    assert!(write_result.is_ok(), "file should be written");
}

fn write_basic_config(temp: &tempfile::TempDir) {
    write_file(
        temp.path().join("hook-bridge.yaml"),
        r"
version: 1
hooks:
  - id: r1
    event: before_command
    command: echo ok
",
    );
}

fn managed_file(source_config: &str) -> String {
    serde_json::json!({
        "_hook_bridge": {
            "managed_by": "hook_bridge",
            "managed_version": 1,
            "source_config": source_config
        },
        "hooks": {}
    })
    .to_string()
}

#[path = "cli_generate/basic.rs"]
mod basic;

#[path = "cli_generate/platform_filter.rs"]
mod platform_filter;

#[path = "cli_generate/events_and_errors.rs"]
mod events_and_errors;
