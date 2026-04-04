pub mod app;
pub mod cli;
pub mod config;
pub mod error;
pub mod generate;
pub mod platform;
pub mod run;
pub mod runtime;

use std::process::ExitCode;

use app::App;
use cli::Cli;
use error::{ExitCodeKind, HookBridgeError, exit_code_for_error};
use runtime::RealRuntime;

/// Runs the top-level CLI application flow.
///
/// # Errors
///
/// Returns any domain error produced while executing the selected subcommand.
pub fn run_cli(cli: Cli) -> Result<(), HookBridgeError> {
    let app = App::new(RealRuntime::default());
    app.execute(cli)
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
    use std::process::ExitCode;

    use crate::error::HookBridgeError;

    use super::result_to_exit_code;

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
}
