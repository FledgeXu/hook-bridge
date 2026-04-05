use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::platform::Platform;

pub const DEFAULT_CONFIG_PATH: &str = "hook-bridge.yaml";

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
    #[arg(long, default_value = DEFAULT_CONFIG_PATH)]
    pub config: PathBuf,

    #[arg(long)]
    pub platform: Option<Platform>,
}

#[derive(Debug, clap::Args)]
pub struct RunArgs {
    #[arg(long)]
    pub platform: Platform,

    #[arg(long = "rule-id", value_parser = clap::builder::NonEmptyStringValueParser::new())]
    pub rule_id: String,
}
