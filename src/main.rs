use std::io::{BufWriter, Write};
use std::path::{self, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;
use std::{env, io, thread};

use clap::Parser;
use crossbeam_channel::{after, never, select, unbounded};
use duration_str::parse;
use tempfile::NamedTempFile;

#[derive(Parser, Debug)]
struct Args {
    #[arg(short = 't', long, value_parser = |arg: &str| parse(arg))]
    interval: Option<Duration>,

    #[arg(short = 'l', long)]
    lines: Option<usize>,

    #[arg(short = 'x', long)]
    command: Option<String>,

    #[arg(short = 'p', long, default_value = "x")]
    prefix: String,

    #[arg(short = 'F', long, default_value = "%Y%m%d%s%6f")]
    format: String,

    #[arg(short = 'o', long)]
    output: Option<PathBuf>,
}

fn main() {
    let args = Args::parse();

    if let Some(output_dir) = &args.output {
        if !output_dir.exists() {
            std::fs::create_dir_all(&output_dir).unwrap();
        }
    }

    let (s, r) = unbounded();

    thread::spawn(move || {
        for line in io::stdin().lines() {
            s.send(line.unwrap()).unwrap();
        }
    });

    let max_lines = args.lines.unwrap_or(usize::MAX);

    let mut file_lines = if let Some(lines) = args.lines {
        Vec::with_capacity(lines)
    } else {
        Vec::new()
    };

    loop {
        let timeout = args
            .interval
            .map(|duration| after(duration))
            .unwrap_or(never());

        while max_lines > file_lines.len() {
            select! {
                recv(r) -> msg => match msg {
                    Ok(value) => {
                        file_lines.push(value);
                    }
                    Err(_) => continue,
                },
                recv(timeout) -> _ => {
                    eprintln!("timeout");
                    break;
                },
            }
        }

        let timestamp = chrono::Utc::now().format(&args.format).to_string();

        let filename = format!("{}_{}", args.prefix, timestamp);

        if let Some(command) = &args.command {
            unsafe {
                env::set_var("FILE", &filename);
            }

            let mut child =
                Command::new(env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_owned()))
                    .arg("-c")
                    .arg(command)
                    .env("FILE", &filename)
                    .stdin(Stdio::piped())
                    .spawn()
                    .unwrap();

            if let Some(stdin) = child.stdin.as_mut() {
                for line in file_lines.iter() {
                    writeln!(stdin, "{}", line).unwrap();
                }
            }
            let _ = child.wait();
        } else if let Some(output_dir) = &args.output {
            let file = NamedTempFile::new().unwrap();

            let mut writer = BufWriter::new(file.as_file());

            for line in &file_lines {
                writeln!(writer, "{}", line).unwrap();
            }

            writer.flush().unwrap();

            let filename = path::absolute(output_dir.as_path()).unwrap().join(filename);

            std::fs::rename(file.path(), &filename).unwrap();
        }

        file_lines.clear();
    }
}
