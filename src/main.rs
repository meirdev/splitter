mod args;
mod splitter;
mod worker;

use anyhow::Result;
use args::Args;
use clap::Parser;
use splitter::Splitter;

fn main() -> Result<()> {
    env_logger::init();

    let args = Args::parse();

    let splitter = Splitter::new(
        args.output_dir()?,
        args.prefix.unwrap_or_default(),
        args.suffix.unwrap_or_default(),
        args.format,
        args.lines.unwrap_or(usize::MAX),
        args.interval,
        args.command,
        args.wait,
        args.jobs,
    )?;

    splitter.run(args.buffer_size)
}
