use std::env;
use std::path::{self, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use duration_str::parse;

#[derive(Parser, Debug)]
#[command(name = "splitter")]
#[command(about = "Split standard input into multiple files based on line count or time intervals")]
pub struct Args {
    #[arg(short = 't', long, value_parser = |arg: &str| parse(arg), help = "How long to wait for new input before creating a new file, e.g., 5s, 1m. If the timeout is reached, even if the number of lines is not reached, the file will be created.")]
    pub interval: Option<Duration>,

    #[arg(short = 'l', long, help = "Maximum number of lines per file")]
    pub lines: Option<usize>,

    #[arg(
        short = 'x',
        long,
        help = "Command to execute after each file is created, the current file path is available in the FILE environment variable"
    )]
    pub command: Option<String>,

    #[arg(short = 'p', long, help = "The prefix for the file name")]
    pub prefix: Option<String>,

    #[arg(short = 's', long, help = "The suffix for the file name")]
    pub suffix: Option<String>,

    #[arg(
        short = 'F',
        long,
        default_value = "%Y%m%d%s.%9f",
        help = "The format for the timestamp in the file name"
    )]
    pub format: String,

    #[arg(
        short = 'o',
        long,
        help = "Output directory, defaults to current directory"
    )]
    pub output: Option<PathBuf>,

    #[arg(
        short = 'b',
        long,
        help = "Maximum number of lines to buffer in memory (unbounded if not set)"
    )]
    pub buffer_size: Option<usize>,

    #[arg(
        short = 'j',
        long,
        default_value = "1",
        help = "Number of parallel workers for command execution"
    )]
    pub jobs: usize,

    #[arg(
        short = 'w',
        long,
        help = "Wait for each command to complete before processing next file"
    )]
    pub wait: bool,
}

impl Args {
    pub fn output_dir(&self) -> Result<PathBuf> {
        let dir = self
            .output
            .clone()
            .unwrap_or_else(|| env::current_dir().expect("Failed to get current directory"));

        path::absolute(dir).context("Failed to resolve output directory")
    }
}
