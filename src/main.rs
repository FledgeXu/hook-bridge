#![allow(
    clippy::multiple_crate_versions,
    reason = "transitive dependencies currently require multiple hashbrown versions"
)]

use std::process::ExitCode;

fn main() -> ExitCode {
    let outcome = hook_bridge::run_program(std::env::args_os());
    let _ = outcome.emit();
    outcome.exit_code
}
