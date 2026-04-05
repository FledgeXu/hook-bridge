use assert_cmd::Command;
use clap::Parser;
use predicates::prelude::*;

use hook_bridge::cli::{Cli, Command as CliCommand, DEFAULT_CONFIG_PATH};
use hook_bridge::platform::Platform;

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
fn generate_uses_default_config_argument_when_omitted() {
    let parse_result = Cli::try_parse_from(["hook_bridge", "generate"]);
    assert!(
        parse_result.is_ok(),
        "generate should parse without --config"
    );
    let Ok(cli) = parse_result else {
        return;
    };

    let CliCommand::Generate(args) = cli.command else {
        return;
    };
    assert_eq!(args.config, std::path::PathBuf::from(DEFAULT_CONFIG_PATH));
    assert_eq!(args.platform, None);
}

#[test]
fn generate_parses_optional_platform_argument() {
    let parse_result = Cli::try_parse_from(["hook_bridge", "generate", "--platform", "codex"]);
    assert!(
        parse_result.is_ok(),
        "generate should parse with --platform codex"
    );
    let Ok(cli) = parse_result else {
        return;
    };

    let CliCommand::Generate(args) = cli.command else {
        return;
    };
    assert_eq!(args.config, std::path::PathBuf::from(DEFAULT_CONFIG_PATH));
    assert_eq!(args.platform, Some(Platform::Codex));
}

#[test]
fn generate_rejects_invalid_platform_argument() -> Result<(), Box<dyn std::error::Error>> {
    let mut command = Command::cargo_bin("hook_bridge")?;

    command
        .arg("generate")
        .arg("--platform")
        .arg("invalid")
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::starts_with(
            "error: invalid value 'invalid' for '--platform <PLATFORM>'",
        ));

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
