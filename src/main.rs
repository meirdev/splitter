use std::io::{BufWriter, Write};
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

    #[arg(short = 'o', long)]
    output: String, 
}

fn main() {
    let args = Args::parse();

    let (s, r) = unbounded();

    thread::spawn(move || {
        for line_result in io::stdin().lines() {
            match line_result {
                Ok(line) => {
                    s.send(line).unwrap();
                }
                Err(error) => {
                    eprintln!("Error reading line: {}", error);
                }
            }
        }
    });

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

        loop {
            select! {
                recv(r) -> msg => match msg {
                    Ok(value) => {
                        file_lines.push(value);

                        if let Some(lines) = args.lines && lines == file_lines.len() {
                            break;
                        }
                    }
                    Err(_) => continue,
                },
                recv(timeout) -> _ => {
                    eprintln!("timeout");
                    break;
                },
            }
        }

        if let Some(command) = &args.command {
            unsafe {
                env::set_var("FILE", "");
            }

            let shell_command =
                Command::new(env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_owned()))
                    .arg("-c")
                    .arg(command)
                    .stdin(Stdio::piped())
                    .spawn();

            match shell_command {
                Ok(mut child) => {
                    if let Some(stdin) = child.stdin.as_mut() {
                        for line in file_lines.iter() {
                            writeln!(stdin, "{}", line).unwrap();
                        }
                    }
                    let _ = child.wait();
                }
                Err(e) => eprintln!("Failed to execute command: {}", e),
            }
        } else {
            let file = NamedTempFile::new().unwrap();

            let mut writer = BufWriter::new(file);

            for line in &file_lines {
                writeln!(writer, "{}", line).unwrap();
            }

            writer.flush().unwrap();
        }

        file_lines.clear();
    }
}
