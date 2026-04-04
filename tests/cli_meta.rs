use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn help_exits_with_success_and_writes_stdout() -> Result<(), Box<dyn std::error::Error>> {
    let mut command = Command::cargo_bin("hook_bridge")?;

    command
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Bridge for hook-driven workflows"));

    Ok(())
}

#[test]
fn version_exits_with_success_and_writes_stdout() -> Result<(), Box<dyn std::error::Error>> {
    let mut command = Command::cargo_bin("hook_bridge")?;

    command
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("hook_bridge"));

    Ok(())
}
