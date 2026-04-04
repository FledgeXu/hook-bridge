use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::platform::Platform;

#[derive(Debug, Parser)]
#[command(
    name = "hook_bridge",
    version,
    about = "Bridge for hook-driven workflows"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Generate(GenerateArgs),
    Run(RunArgs),
}

#[derive(Debug, clap::Args)]
pub struct GenerateArgs {
    #[arg(long)]
    pub config: PathBuf,
}

#[derive(Debug, clap::Args)]
pub struct RunArgs {
    #[arg(long)]
    pub platform: Platform,

    #[arg(long = "rule-id", value_parser = clap::builder::NonEmptyStringValueParser::new())]
    pub rule_id: String,
}
