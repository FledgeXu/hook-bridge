#![allow(
    clippy::multiple_crate_versions,
    reason = "transitive dependencies currently require multiple hashbrown versions"
)]

pub mod app;
pub mod cli;
pub mod config;
pub mod error;
pub mod generate;
pub mod platform;
pub mod run;
pub mod runtime;

use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

use clap::Parser;
use clap::error::ErrorKind;

use app::App;
use cli::Cli;
use error::{ExitCodeKind, HookBridgeError, exit_code_for_error};
use runtime::RealRuntime;

#[cfg(test)]
pub(crate) static CWD_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Runs the top-level CLI application flow.
///
/// # Errors
///
/// Returns any domain error produced while executing the selected subcommand.
pub fn run_cli(cli: Cli) -> Result<u8, HookBridgeError> {
    let app = App::new(RealRuntime::default());
    app.execute(cli)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputStream {
    Stdout,
    Stderr,
}

#[derive(Debug, PartialEq)]
pub struct ProgramOutcome {
    pub exit_code: ExitCode,
    pub stream: Option<OutputStream>,
    pub message: Option<String>,
}

impl ProgramOutcome {
    #[must_use]
    pub fn success(exit_code: u8) -> Self {
        Self {
            exit_code: ExitCode::from(exit_code),
            stream: None,
            message: None,
        }
    }

    #[must_use]
    pub fn clap_error(error: &clap::Error) -> Self {
        let kind = error.kind();
        let message = error.to_string();
        match kind {
            ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => Self {
                exit_code: ExitCode::SUCCESS,
                stream: Some(OutputStream::Stdout),
                message: Some(message),
            },
            _ => Self {
                exit_code: ExitCode::from(ExitCodeKind::Parameter as u8),
                stream: Some(OutputStream::Stderr),
                message: Some(message),
            },
        }
    }

    #[must_use]
    pub fn domain_error(error: &HookBridgeError) -> Self {
        Self {
            exit_code: ExitCode::from(exit_code_for_error(error)),
            stream: Some(OutputStream::Stderr),
            message: Some(format!("{error}\n")),
        }
    }

    /// Writes any buffered program message to the selected stdio stream.
    ///
    /// # Errors
    ///
    /// Returns any stdio write failure encountered while emitting the buffered message.
    pub fn emit(&self) -> std::io::Result<()> {
        let Some(message) = &self.message else {
            return Ok(());
        };

        match self.stream {
            Some(OutputStream::Stdout) => std::io::stdout().lock().write_all(message.as_bytes()),
            Some(OutputStream::Stderr) => std::io::stderr().lock().write_all(message.as_bytes()),
            None => Ok(()),
        }
    }
}

#[must_use]
pub fn run_program<I, T>(args: I) -> ProgramOutcome
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error) => return ProgramOutcome::clap_error(&error),
    };

    let app = App::new(RealRuntime::default());
    match app.execute(cli) {
        Ok(exit_code) => ProgramOutcome::success(exit_code),
        Err(error) => ProgramOutcome::domain_error(&error),
    }
}

#[must_use]
pub fn result_to_exit_code(result: &Result<(), HookBridgeError>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::from(ExitCodeKind::Success as u8),
        Err(error) => ExitCode::from(exit_code_for_error(error)),
    }
}

#[cfg(test)]
mod tests {
    use std::io::ErrorKind;
    use std::path::PathBuf;
    use std::process::ExitCode;

    use clap::Parser;

    use crate::app::App;
    use crate::error::HookBridgeError;
    use crate::runtime::Runtime;
    use crate::runtime::clock::{Clock, FixedClock};
    use crate::runtime::fs::{FakeFileSystem, FileSystem};
    use crate::runtime::io::{FakeIo, Io};
    use crate::runtime::process::{FakeProcessRunner, ProcessRunner};

    use super::{OutputStream, ProgramOutcome, result_to_exit_code, run_cli};

    struct TestRuntime {
        fs: FakeFileSystem,
        clock: FixedClock,
        process: FakeProcessRunner,
        io: FakeIo,
    }

    impl Runtime for TestRuntime {
        fn fs(&self) -> &dyn FileSystem {
            &self.fs
        }

        fn clock(&self) -> &dyn Clock {
            &self.clock
        }

        fn process_runner(&self) -> &dyn ProcessRunner {
            &self.process
        }

        fn io(&self) -> &dyn Io {
            &self.io
        }

        fn temp_dir(&self) -> PathBuf {
            std::env::temp_dir()
        }
    }

    #[test]
    fn result_to_exit_code_maps_success() {
        let result: Result<(), HookBridgeError> = Ok(());
        assert_eq!(result_to_exit_code(&result), ExitCode::from(0));
    }

    #[test]
    fn result_to_exit_code_maps_error() {
        let result: Result<(), HookBridgeError> = Err(HookBridgeError::NotImplemented {
            feature: "generate",
        });
        assert_eq!(result_to_exit_code(&result), ExitCode::from(9));
    }

    #[test]
    fn clap_help_maps_to_success_stdout() {
        let parse_result = crate::cli::Cli::try_parse_from(["hook_bridge", "--help"]);
        assert!(parse_result.is_err(), "help should early-exit");
        let Err(error) = parse_result else {
            return;
        };
        let outcome = ProgramOutcome::clap_error(&error);

        assert_eq!(outcome.exit_code, ExitCode::SUCCESS);
        assert_eq!(outcome.stream, Some(OutputStream::Stdout));
        assert!(
            outcome
                .message
                .as_deref()
                .is_some_and(|message| message.contains("Bridge for hook-driven workflows")),
            "help output should be preserved"
        );
    }

    #[test]
    fn clap_parameter_error_maps_to_stderr_and_exit_code_2() {
        let parse_result =
            crate::cli::Cli::try_parse_from(["hook_bridge", "run", "--platform", "nope"]);
        assert!(parse_result.is_err(), "invalid platform should fail");
        let Err(error) = parse_result else {
            return;
        };
        let outcome = ProgramOutcome::clap_error(&error);

        assert_eq!(outcome.exit_code, ExitCode::from(2));
        assert_eq!(outcome.stream, Some(OutputStream::Stderr));
        assert!(
            outcome
                .message
                .as_deref()
                .is_some_and(|message| message.starts_with("error: invalid value 'nope'")),
            "clap error should preserve raw diagnostics"
        );
    }

    #[test]
    fn domain_error_outcome_adds_newline_and_uses_stderr() {
        let outcome = ProgramOutcome::domain_error(&HookBridgeError::ConfigValidation {
            message: "bad yaml".to_string(),
        });

        assert_eq!(outcome.exit_code, ExitCode::from(3));
        assert_eq!(outcome.stream, Some(OutputStream::Stderr));
        assert_eq!(
            outcome.message,
            Some("config validation error: bad yaml\n".to_string())
        );
    }

    #[test]
    fn domain_error_outcome_covers_all_error_variants() {
        let cases = [
            (
                HookBridgeError::Parameter {
                    message: "bad flag".to_string(),
                },
                ExitCode::from(2),
            ),
            (
                HookBridgeError::ConfigValidation {
                    message: "invalid yaml".to_string(),
                },
                ExitCode::from(3),
            ),
            (
                HookBridgeError::FileConflict {
                    path: PathBuf::from("managed.json"),
                },
                ExitCode::from(4),
            ),
            (
                HookBridgeError::JsonParse {
                    message: "invalid json".to_string(),
                },
                ExitCode::from(5),
            ),
            (
                HookBridgeError::Process {
                    message: "spawn failed".to_string(),
                },
                ExitCode::from(6),
            ),
            (
                HookBridgeError::Timeout { timeout_sec: 5 },
                ExitCode::from(7),
            ),
            (
                HookBridgeError::PlatformProtocol {
                    message: "bad event".to_string(),
                },
                ExitCode::from(8),
            ),
            (
                HookBridgeError::NotImplemented { feature: "run" },
                ExitCode::from(9),
            ),
            (
                HookBridgeError::Io {
                    operation: "read",
                    path: PathBuf::from("<stdin>"),
                    kind: ErrorKind::UnexpectedEof,
                },
                ExitCode::from(10),
            ),
        ];

        for (error, expected_exit_code) in cases {
            let outcome = ProgramOutcome::domain_error(&error);
            assert_eq!(outcome.exit_code, expected_exit_code);
            assert_eq!(outcome.stream, Some(OutputStream::Stderr));
            assert!(
                outcome
                    .message
                    .as_deref()
                    .is_some_and(|message| message.ends_with('\n')),
                "domain errors should render as line-based stderr output"
            );
        }
    }

    #[test]
    fn app_can_be_composed_under_program_outcome_flow() {
        let app = App::new(TestRuntime {
            fs: FakeFileSystem::default(),
            clock: FixedClock::new(std::time::SystemTime::UNIX_EPOCH),
            process: FakeProcessRunner::success(0),
            io: FakeIo::default(),
        });
        let cli = crate::cli::Cli {
            command: crate::cli::Command::Generate(crate::cli::GenerateArgs {
                config: "missing.yaml".into(),
                platform: None,
                force: false,
                yes: false,
            }),
        };

        let result = app.execute(cli);
        let outcome = match result {
            Ok(exit_code) => ProgramOutcome::success(exit_code),
            Err(error) => ProgramOutcome::domain_error(&error),
        };

        assert_eq!(outcome.exit_code, ExitCode::from(3));
        assert_eq!(outcome.stream, Some(OutputStream::Stderr));
    }

    #[test]
    fn run_cli_surfaces_run_command_errors() {
        let lock_result = crate::CWD_LOCK.lock();
        assert!(lock_result.is_ok(), "cwd lock should not be poisoned");
        let Ok(_lock) = lock_result else {
            return;
        };
        let temp_result = tempfile::tempdir();
        assert!(temp_result.is_ok(), "tempdir creation should succeed");
        let Ok(temp) = temp_result else {
            return;
        };
        let original_result = std::env::current_dir();
        assert!(original_result.is_ok(), "cwd lookup should succeed");
        let Ok(original) = original_result else {
            return;
        };
        let switch_result = std::env::set_current_dir(temp.path());
        assert!(switch_result.is_ok(), "cwd switch should succeed");
        let Ok(()) = switch_result else {
            return;
        };

        let result = run_cli(crate::cli::Cli {
            command: crate::cli::Command::Run(crate::cli::RunArgs {
                platform: crate::platform::Platform::Codex,
                rule_id: "r1".to_string(),
            }),
        });

        let restore_result = std::env::set_current_dir(&original);
        assert!(restore_result.is_ok(), "cwd restore should succeed");
        assert!(matches!(result, Err(HookBridgeError::Io { .. })));
    }
}
