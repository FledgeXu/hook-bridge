use std::io::{self, Write};
use std::process::ExitCode;

pub const STATUS_MESSAGE: &str = "hook_bridge ready";

/// Writes the bridge status message to the provided writer.
///
/// # Errors
///
/// Returns any I/O error produced while writing the status message.
pub fn run(writer: &mut dyn Write) -> io::Result<()> {
    writeln!(writer, "{STATUS_MESSAGE}")
}

pub fn entrypoint(writer: &mut dyn Write) -> ExitCode {
    if run(writer).is_ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

#[must_use]
pub fn cli_main() -> ExitCode {
    entrypoint(&mut io::stdout())
}
