use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::time::Duration;
use std::{io, thread};

use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, after, bounded, never, select, unbounded};
use tempfile::NamedTempFile;

use crate::worker::{WorkerPool, run_command};

pub struct Splitter {
    output_dir: PathBuf,
    prefix: String,
    suffix: String,
    format: String,
    max_lines: usize,
    interval: Option<Duration>,
    command: Option<String>,
    worker_pool: Option<WorkerPool>,
}

impl Splitter {
    pub fn new(
        output_dir: PathBuf,
        prefix: String,
        suffix: String,
        format: String,
        max_lines: usize,
        interval: Option<Duration>,
        command: Option<String>,
        wait: bool,
        jobs: usize,
    ) -> Result<Self> {
        if !output_dir.exists() {
            log::debug!("Creating output directory: {}", output_dir.display());
            std::fs::create_dir_all(&output_dir).context("Failed to create output directory")?;
        }

        let worker_pool = if let Some(ref cmd) = command {
            if !wait {
                Some(WorkerPool::new(cmd, jobs))
            } else {
                None
            }
        } else {
            None
        };

        Ok(Self {
            output_dir,
            prefix,
            suffix,
            format,
            max_lines,
            interval,
            command,
            worker_pool,
        })
    }

    pub fn run(&self, buffer_size: Option<usize>) -> Result<()> {
        let (s, r) = match buffer_size {
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

        log::debug!(
            "Starting splitter: output_dir={}, max_lines={}, interval={:?}",
            self.output_dir.display(),
            self.max_lines,
            self.interval
        );

        self.process_loop(r)
    }

    fn process_loop(&self, receiver: Receiver<String>) -> Result<()> {
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

            let timeout = self
                .interval
                .map(|duration| after(duration))
                .unwrap_or(never());

            while lines < self.max_lines {
                select! {
                    recv(receiver) -> msg => match msg {
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

            if lines == 0 {
                log::debug!("No lines received, skipping file creation");
                continue;
            }

            if let Err(e) = writer.flush() {
                log::error!("Failed to flush writer: {}", e);
                continue;
            }

            let file_path = match self.create_output_file(&file) {
                Some(path) => path,
                None => continue,
            };

            log::debug!("Created file: {} ({} lines)", file_path.display(), lines);

            self.execute_command(file_path);
        }

        Ok(())
    }

    fn create_output_file(&self, temp_file: &NamedTempFile) -> Option<PathBuf> {
        let timestamp = chrono::Utc::now().format(&self.format).to_string();
        let file_name = format!("{}{}{}", self.prefix, timestamp, self.suffix);
        let file_path = self.output_dir.join(file_name);

        if file_path.exists() {
            log::warn!("File already exists, skipping: {}", file_path.display());
            return None;
        }

        if let Err(e) = std::fs::rename(temp_file.path(), &file_path) {
            log::error!(
                "Failed to rename temp file to {}: {}",
                file_path.display(),
                e
            );
            return None;
        }

        Some(file_path)
    }

    fn execute_command(&self, file_path: PathBuf) {
        if let Some(ref pool) = self.worker_pool {
            pool.submit(file_path);
        } else if let Some(ref command) = self.command {
            run_command(command, &file_path);
        }
    }
}
