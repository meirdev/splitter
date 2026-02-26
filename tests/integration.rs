use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn splitter_binary() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("target");
    path.push(if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    });
    path.push("splitter");
    path
}

fn create_temp_dir() -> tempfile::TempDir {
    tempfile::tempdir().expect("Failed to create temp dir")
}

#[test]
fn test_split_by_lines() {
    let temp_dir = create_temp_dir();
    let output_dir = temp_dir.path();

    let mut child = Command::new(splitter_binary())
        .args([
            "--lines",
            "3",
            "--output",
            output_dir.to_str().unwrap(),
            "--prefix",
            "test_",
            "--suffix",
            ".txt",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to start splitter");

    {
        let stdin = child.stdin.as_mut().expect("Failed to open stdin");
        for i in 1..=10 {
            writeln!(stdin, "line {}", i).expect("Failed to write to stdin");
        }
    }

    let output = child.wait_with_output().expect("Failed to wait on child");
    assert!(output.status.success(), "Splitter failed: {:?}", output);

    let files: Vec<_> = fs::read_dir(output_dir)
        .expect("Failed to read output dir")
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_str()
                .map(|n| n.starts_with("test_") && n.ends_with(".txt"))
                .unwrap_or(false)
        })
        .collect();

    assert_eq!(
        files.len(),
        4,
        "Expected 4 files (3+3+3+1 lines), got {}",
        files.len()
    );

    let mut total_lines = 0;
    for file in &files {
        let content = fs::read_to_string(file.path()).expect("Failed to read file");
        let lines: Vec<_> = content.lines().collect();
        assert!(
            lines.len() <= 3,
            "File {} has {} lines, expected <= 3",
            file.path().display(),
            lines.len()
        );
        total_lines += lines.len();
    }
    assert_eq!(
        total_lines, 10,
        "Expected 10 total lines, got {}",
        total_lines
    );
}

#[test]
fn test_split_by_timeout() {
    let temp_dir = create_temp_dir();
    let output_dir = temp_dir.path();

    let mut child = Command::new(splitter_binary())
        .args([
            "--interval",
            "300ms",
            "--output",
            output_dir.to_str().unwrap(),
            "--prefix",
            "timeout_",
            "--suffix",
            ".txt",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to start splitter");

    let start = Instant::now();

    {
        let mut stdin = child.stdin.take().expect("Failed to open stdin");

        writeln!(stdin, "first batch line 1").unwrap();
        writeln!(stdin, "first batch line 2").unwrap();
        stdin.flush().unwrap();

        std::thread::sleep(Duration::from_millis(500));

        let _ = writeln!(stdin, "second batch line 1");
        let _ = writeln!(stdin, "second batch line 2");
        let _ = stdin.flush();
    }

    let output = child.wait_with_output().expect("Failed to wait on child");
    let elapsed = start.elapsed();

    assert!(output.status.success(), "Splitter failed: {:?}", output);

    let files: Vec<_> = fs::read_dir(output_dir)
        .expect("Failed to read output dir")
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_str()
                .map(|n| n.starts_with("timeout_") && n.ends_with(".txt"))
                .unwrap_or(false)
        })
        .collect();

    assert!(
        files.len() >= 2,
        "Expected at least 2 files due to timeout, got {}. Elapsed: {:?}",
        files.len(),
        elapsed
    );

    let mut total_lines = 0;
    for file in &files {
        let content = fs::read_to_string(file.path()).expect("Failed to read file");
        total_lines += content.lines().count();
    }
    assert!(
        total_lines >= 2 && total_lines <= 4,
        "Expected 2-4 total lines, got {}",
        total_lines
    );
}

#[test]
fn test_calling_command() {
    let temp_dir = create_temp_dir();
    let output_dir = temp_dir.path();
    let marker_file = output_dir.join("command_executed.txt");

    let command = format!(
        "echo \"processed: $FILE\" >> {}",
        marker_file.to_str().unwrap()
    );

    let mut child = Command::new(splitter_binary())
        .args([
            "--lines",
            "2",
            "--output",
            output_dir.to_str().unwrap(),
            "--prefix",
            "cmd_",
            "--suffix",
            ".txt",
            "--command",
            &command,
            "--wait",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to start splitter");

    {
        let stdin = child.stdin.as_mut().expect("Failed to open stdin");
        for i in 1..=6 {
            writeln!(stdin, "line {}", i).expect("Failed to write to stdin");
        }
    }

    let output = child.wait_with_output().expect("Failed to wait on child");
    assert!(output.status.success(), "Splitter failed: {:?}", output);

    assert!(
        marker_file.exists(),
        "Command marker file was not created - command was not executed"
    );

    let marker_content = fs::read_to_string(&marker_file).expect("Failed to read marker file");
    let executions: Vec<_> = marker_content.lines().collect();

    assert_eq!(
        executions.len(),
        3,
        "Expected command to be called 3 times (6 lines / 2 per file), got {}",
        executions.len()
    );

    for line in &executions {
        assert!(
            line.starts_with("processed:") && line.contains("cmd_"),
            "Command output doesn't contain expected file path: {}",
            line
        );
    }
}

#[test]
fn test_command_receives_file_env_variable() {
    let temp_dir = create_temp_dir();
    let output_dir = temp_dir.path();
    let env_dump_file = output_dir.join("file_env.txt");

    let command = format!("echo $FILE > {}", env_dump_file.to_str().unwrap());

    let mut child = Command::new(splitter_binary())
        .args([
            "--lines",
            "1",
            "--output",
            output_dir.to_str().unwrap(),
            "--prefix",
            "env_",
            "--suffix",
            ".txt",
            "--command",
            &command,
            "--wait",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to start splitter");

    {
        let stdin = child.stdin.as_mut().expect("Failed to open stdin");
        writeln!(stdin, "single line").expect("Failed to write to stdin");
    }

    let output = child.wait_with_output().expect("Failed to wait on child");
    assert!(output.status.success(), "Splitter failed: {:?}", output);

    let file_env = fs::read_to_string(&env_dump_file)
        .expect("Failed to read env dump file")
        .trim()
        .to_string();

    assert!(
        file_env.contains("env_") && file_env.contains(".txt"),
        "FILE env variable doesn't contain expected file path: {}",
        file_env
    );

    assert!(
        PathBuf::from(&file_env).exists(),
        "FILE env variable points to non-existent file: {}",
        file_env
    );
}
