use std::io::{BufWriter, Write};
use std::path::{self, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;
use std::{env, io, thread};

use anyhow::{Context, Result};
use clap::Parser;
use crossbeam_channel::{after, bounded, never, select, unbounded};
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

    #[arg(
        short = 'b',
        long,
        help = "Maximum number of lines to buffer in memory (unbounded if not set)"
    )]
    buffer_size: Option<usize>,

    #[arg(
        short = 'j',
        long,
        default_value = "1",
        help = "Number of parallel workers for command execution"
    )]
    jobs: usize,

    #[arg(
        short = 'w',
        long,
        help = "Wait for each command to complete before processing next file"
    )]
    wait: bool,
}

fn main() -> Result<()> {
    env_logger::init();

    let args = Args::parse();

    let output_dir = path::absolute(
        args.output
            .unwrap_or_else(|| env::current_dir().unwrap_or(".".into())),
    )
    .context("Failed to resolve output directory")?;

    let prefix = args.prefix.unwrap_or_default();
    let suffix = args.suffix.unwrap_or_default();

    if !output_dir.exists() {
        log::debug!("Creating output directory: {}", output_dir.display());

        std::fs::create_dir_all(&output_dir).context("Failed to create output directory")?;
    }

    let (s, r) = match args.buffer_size {
        Some(size) => bounded(size),
        None => unbounded(),
    };

    thread::spawn(move || {
        for line in io::stdin().lines() {
            match line {
                Ok(line) => {
                    if s.send(line).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    log::error!("Failed to read from stdin: {}", e);
                    break;
                }
            }
        }
    });

    let max_lines = args.lines.unwrap_or(usize::MAX);

    log::debug!(
        "Starting splitter: output_dir={}, max_lines={}, interval={:?}",
        output_dir.display(),
        max_lines,
        args.interval
    );

    let command_sender = if let Some(ref cmd) = args.command {
        if !args.wait {
            let (cmd_tx, cmd_rx) = bounded::<PathBuf>(args.jobs);
            let cmd = Arc::new(cmd.clone());

            for worker_id in 0..args.jobs {
                let cmd_rx = cmd_rx.clone();
                let cmd = Arc::clone(&cmd);

                thread::spawn(move || {
                    while let Ok(file_path) = cmd_rx.recv() {
                        log::debug!(
                            "Worker {} executing command for: {}",
                            worker_id,
                            file_path.display()
                        );

                        let result = Command::new(
                            env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_owned()),
                        )
                        .arg("-c")
                        .arg(cmd.as_str())
                        .env("FILE", &file_path)
                        .spawn();

                        match result {
                            Ok(mut child) => {
                                if let Err(e) = child.wait() {
                                    log::error!("Failed to wait for command: {}", e);
                                }
                            }
                            Err(e) => {
                                log::error!("Failed to spawn command '{}': {}", cmd, e);
                            }
                        }
                    }
                });
            }

            Some(cmd_tx)
        } else {
            None
        }
    } else {
        None
    };

    let mut eof = false;

    while !eof {
        let file = match NamedTempFile::new() {
            Ok(f) => f,
            Err(e) => {
                log::error!("Failed to create temp file: {}", e);
                continue;
            }
        };

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

                        if let Err(e) = writeln!(writer, "{}", value) {
                            log::error!("Failed to write to temp file: {}", e);
                            break;
                        }

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

        // Skip file creation if no lines were written
        if lines == 0 {
            log::debug!("No lines received, skipping file creation");
            continue;
        }

        if let Err(e) = writer.flush() {
            log::error!("Failed to flush writer: {}", e);
            continue;
        }

        let timestamp = chrono::Utc::now().format(&args.format).to_string();

        let file_name = format!("{}{}{}", prefix, timestamp, suffix);

        let file_path = output_dir.join(file_name);

        if file_path.exists() {
            log::warn!("File already exists, skipping: {}", file_path.display());
            continue;
        }

        if let Err(e) = std::fs::rename(file.path(), &file_path) {
            log::error!(
                "Failed to rename temp file to {}: {}",
                file_path.display(),
                e
            );
            continue;
        }

        log::debug!("Created file: {} ({} lines)", file_path.display(), lines);

        if let Some(ref sender) = command_sender {
            if let Err(e) = sender.send(file_path) {
                log::error!("Failed to send to worker pool: {}", e);
            }
        } else if let Some(command) = &args.command {
            log::debug!("Executing command: {}", command);

            let result = Command::new(env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_owned()))
                .arg("-c")
                .arg(command)
                .env("FILE", &file_path)
                .spawn();

            match result {
                Ok(mut child) => {
                    if let Err(e) = child.wait() {
                        log::error!("Failed to wait for command: {}", e);
                    }
                }
                Err(e) => {
                    log::error!("Failed to spawn command '{}': {}", command, e);
                }
            }
        }
    }

    Ok(())
}
