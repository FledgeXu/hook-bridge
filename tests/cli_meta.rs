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

#[test]
fn invalid_flag_exits_with_parameter_error() -> Result<(), Box<dyn std::error::Error>> {
    let mut command = Command::cargo_bin("hook_bridge")?;

    command
        .arg("--definitely-invalid")
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("unexpected argument"));

    Ok(())
}

#[test]
fn generate_requires_config_argument() -> Result<(), Box<dyn std::error::Error>> {
    let mut command = Command::cargo_bin("hook_bridge")?;

    command
        .arg("generate")
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("--config <CONFIG>"));

    Ok(())
}

#[test]
fn run_requires_rule_id_argument() -> Result<(), Box<dyn std::error::Error>> {
    let mut command = Command::cargo_bin("hook_bridge")?;

    command
        .arg("run")
        .arg("--platform")
        .arg("codex")
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("--rule-id <RULE_ID>"));

    Ok(())
}

#[test]
fn run_rejects_invalid_platform_argument() -> Result<(), Box<dyn std::error::Error>> {
    let mut command = Command::cargo_bin("hook_bridge")?;

    command
        .arg("run")
        .arg("--platform")
        .arg("invalid")
        .arg("--rule-id")
        .arg("r1")
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::starts_with(
            "error: invalid value 'invalid' for '--platform <PLATFORM>'",
        ));

    Ok(())
}
