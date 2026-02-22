use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::{env, thread};

use crossbeam_channel::{Sender, bounded};

pub struct WorkerPool {
    sender: Sender<PathBuf>,
}

impl WorkerPool {
    pub fn new(command: &str, jobs: usize) -> Self {
        let (tx, rx) = bounded::<PathBuf>(jobs);
        let cmd = Arc::new(command.to_string());

        for worker_id in 0..jobs {
            let rx = rx.clone();
            let cmd = Arc::clone(&cmd);

            thread::spawn(move || {
                while let Ok(file_path) = rx.recv() {
                    log::debug!(
                        "Worker {} executing command for: {}",
                        worker_id,
                        file_path.display()
                    );

                    run_command(&cmd, &file_path);
                }
            });
        }

        Self { sender: tx }
    }

    pub fn submit(&self, file_path: PathBuf) {
        if let Err(e) = self.sender.send(file_path) {
            log::error!("Failed to send to worker pool: {}", e);
        }
    }
}

pub fn run_command(command: &str, file_path: &PathBuf) {
    log::debug!("Executing command: {}", command);

    let result = Command::new(env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_owned()))
        .arg("-c")
        .arg(command)
        .env("FILE", file_path)
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
