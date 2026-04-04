use std::io::Write;
use std::process::ExitCode;

use clap::Parser;
use clap::error::ErrorKind;

use hook_bridge::cli::Cli;
use hook_bridge::error::HookBridgeError;

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => match error.kind() {
            ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => {
                let _ = error.print();
                return ExitCode::SUCCESS;
            }
            _ => {
                let _ = error.print();
                let parameter_error = HookBridgeError::Parameter {
                    message: error.to_string(),
                };
                return ExitCode::from(hook_bridge::error::exit_code_for_error(&parameter_error));
            }
        },
    };

    let result = hook_bridge::run_cli(cli);
    if let Err(error) = &result {
        let _ = std::io::stderr()
            .lock()
            .write_all(format!("{error}\n").as_bytes());
    }

    hook_bridge::result_to_exit_code(&result)
}
