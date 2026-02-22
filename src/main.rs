use std::io::{BufWriter, Write};
use std::path::{self, PathBuf};
use std::process::Command;
use std::time::Duration;
use std::{env, io, thread};

use clap::Parser;
use crossbeam_channel::{after, never, select, unbounded};
use duration_str::parse;
use tempfile::NamedTempFile;

#[derive(Parser, Debug)]
#[command(name = "splitter")]
#[command(about = "A tool to split input into files based on line count or timeout")]
struct Args {
    #[arg(short = 't', long, value_parser = |arg: &str| parse(arg), help = "How long to wait for new input before creating a new file, e.g., 5s, 1m. If the timeout is reached, even if the number of lines is not reached, the file will be created.")]
    interval: Option<Duration>,

    #[arg(short = 'l', long, help = "Maximum number of lines per file")]
    lines: Option<usize>,

    #[arg(
        short = 'x',
        long,
        help = "Command to execute after each file is created, the current file path is available in the FILE environment variable"
    )]
    command: Option<String>,

    #[arg(short = 'p', long, help = "The prefix for the file name")]
    prefix: Option<String>,

    #[arg(short = 's', long, help = "The suffix for the file name")]
    suffix: Option<String>,

    #[arg(
        short = 'F',
        long,
        default_value = "%Y%m%d%s%6f",
        help = "The format for the timestamp in the file name"
    )]
    format: String,

    #[arg(
        short = 'o',
        long,
        help = "Output directory, defaults to current directory"
    )]
    output: Option<PathBuf>,
}

fn main() {
    env_logger::init();

    let args = Args::parse();

    let output_dir = path::absolute(
        args.output
            .unwrap_or_else(|| env::current_dir().unwrap_or(".".into())),
    )
    .unwrap();

    let prefix = args.prefix.unwrap_or_else(|| "".to_string());
    let suffix = args.suffix.unwrap_or_else(|| "".to_string());

    if !output_dir.exists() {
        log::debug!("Creating output directory: {}", output_dir.display());

        std::fs::create_dir_all(&output_dir).unwrap();
    }

    let (s, r) = unbounded();

    thread::spawn(move || {
        for line in io::stdin().lines() {
            s.send(line.unwrap()).unwrap();
        }
    });

    let max_lines = args.lines.unwrap_or(usize::MAX);

    log::debug!(
        "Starting splitter: output_dir={}, max_lines={}, interval={:?}",
        output_dir.display(),
        max_lines,
        args.interval
    );

    let mut eof = false;

    while !eof {
        let file = NamedTempFile::new().unwrap();

        let mut writer = BufWriter::new(file.as_file());

        let mut lines = 0;

        let timeout = args
            .interval
            .map(|duration| after(duration))
            .unwrap_or(never());

        while lines < max_lines {
            select! {
                recv(r) -> msg => match msg {
                    Ok(value) => {
                        log::debug!("Received: {}", value);

                        writeln!(writer, "{}", value).unwrap();

                        lines += 1;
                    }
                    Err(_) => {
                        log::debug!("Channel closed (EOF)");

                        eof = true;
                        break;
                    }
                },
                recv(timeout) -> _ => {
                    log::debug!("Timeout reached after {} lines", lines);

                    break;
                },
            }
        }

        // In case of no lines, we skip the file creation
        if lines == 0 {
            log::debug!("No lines received, skipping file creation");
            continue;
        }

        writer.flush().unwrap();

        let timestamp = chrono::Utc::now().format(&args.format).to_string();

        let file_name = format!("{}{}{}", prefix, timestamp, suffix);

        let file_path = output_dir.join(file_name);

        std::fs::rename(file.path(), &file_path).unwrap();

        log::debug!("Created file: {} ({} lines)", file_path.display(), lines);

        if let Some(command) = &args.command {
            log::debug!("Executing command: {}", command);

            let mut child =
                Command::new(env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_owned()))
                    .arg("-c")
                    .arg(command)
                    .env("FILE", &file_path)
                    .spawn()
                    .unwrap();

            child.wait().unwrap();
        }
    }
}
