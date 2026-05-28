use std::fs;
use std::io;
use std::io::ErrorKind;
use std::path::Path;
use std::process::{Child, Command, ExitStatus};
#[cfg(windows)]
use std::process::Stdio;
use std::thread;
use std::time::{Duration, Instant};

use crate::{ExecutionProgress, ExecutionProgressReporter};

const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(50);
const EXECUTION_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug)]
pub(crate) struct CommandExecutionOutput {
    pub(crate) output: Vec<u8>,
    pub(crate) status: ExitStatus,
}

#[derive(Debug)]
pub(crate) struct CommandExecutionError {
    pub(crate) output: Vec<u8>,
    pub(crate) error: io::Error,
    pub(crate) exit_status: Option<ExitStatus>,
}

pub(crate) fn execute_command_with_timeout(
    command: &mut Command,
    command_label: &str,
    timeout: Duration,
    log_path: &Path,
    log_preamble: &[u8],
    fallback_log_path: Option<&Path>,
    fallback_log_label: Option<&str>,
    reporter: &mut dyn ExecutionProgressReporter,
) -> Result<CommandExecutionOutput, CommandExecutionError> {
    let mut child = command.spawn().map_err(|error| CommandExecutionError {
        output: Vec::new(),
        error: io::Error::other(format!("spawn {command_label}: {error}")),
        exit_status: None,
    })?;

    let (status, timed_out) =
        match wait_for_child(&mut child, command_label, timeout, log_path, reporter) {
            Ok(result) => result,
            Err(error) => {
                return Err(CommandExecutionError {
                    output: read_command_log(
                        command_label,
                        log_path,
                        log_preamble,
                        fallback_log_path,
                        fallback_log_label,
                    ),
                    error,
                    exit_status: None,
                });
            }
        };
    let output = read_command_log(
        command_label,
        log_path,
        log_preamble,
        fallback_log_path,
        fallback_log_label,
    );
    if timed_out {
        return Err(CommandExecutionError {
            output,
            error: io::Error::new(
                ErrorKind::TimedOut,
                format!("{command_label} exceeded {}s timeout", timeout.as_secs()),
            ),
            exit_status: Some(status),
        });
    }

    Ok(CommandExecutionOutput { output, status })
}

fn read_command_log(
    command_label: &str,
    log_path: &Path,
    log_preamble: &[u8],
    fallback_log_path: Option<&Path>,
    fallback_log_label: Option<&str>,
) -> Vec<u8> {
    let mut output = Vec::new();
    output.extend_from_slice(log_preamble);

    match fs::read(log_path) {
        Ok(contents) if contains_visible_text(&contents) => {
            output.extend_from_slice(&contents);
            return output;
        }
        Ok(_) | Err(_) => {
            output.extend_from_slice(
                format!(
                    "execution log file {} was not written by {command_label}\n",
                    log_path.display()
                )
                .as_bytes(),
            );
        }
    }

    if let Some(fallback_log_path) = fallback_log_path {
        let fallback_log_label = fallback_log_label.unwrap_or("fallback log");
        output.extend_from_slice(
            format!(
                "attempting fallback {fallback_log_label} at {}\n",
                fallback_log_path.display()
            )
            .as_bytes(),
        );
        if let Some(tail) = read_log_tail(fallback_log_path, 32 * 1024) {
            if contains_visible_text(&tail) {
                output
                    .extend_from_slice(format!("\n--- {fallback_log_label} tail ---\n").as_bytes());
                output.extend_from_slice(&tail);
                return output;
            }
        }
    }

    output
}

fn contains_visible_text(contents: &[u8]) -> bool {
    contents.iter().any(|byte| !byte.is_ascii_whitespace())
}

fn read_log_tail(path: &Path, max_bytes: usize) -> Option<Vec<u8>> {
    let contents = fs::read(path).ok()?;
    if contents.len() <= max_bytes {
        return Some(contents);
    }

    Some(contents[contents.len() - max_bytes..].to_vec())
}

fn last_meaningful_log_line(path: &Path, max_bytes: usize) -> Option<String> {
    let contents = read_log_tail(path, max_bytes)?;
    String::from_utf8_lossy(&contents)
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(normalize_log_line)
}

fn normalize_log_line(raw_line: &str) -> String {
    raw_line
        .split_whitespace()
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn wait_for_child(
    child: &mut Child,
    command_label: &str,
    timeout: Duration,
    log_path: &Path,
    reporter: &mut dyn ExecutionProgressReporter,
) -> io::Result<(ExitStatus, bool)> {
    let started_at = Instant::now();
    let mut last_heartbeat_at = Instant::now() - EXECUTION_HEARTBEAT_INTERVAL;
    let mut last_message = String::new();
    let mut last_log_size = None;

    loop {
        if let Some(status) = child.try_wait()? {
            return Ok((status, false));
        }

        if last_heartbeat_at.elapsed() >= EXECUTION_HEARTBEAT_INTERVAL {
            let (message, log_size) = match fs::metadata(log_path) {
                Ok(metadata) if metadata.is_file() && metadata.len() > 0 => {
                    let log_size = metadata.len();
                    let message = last_meaningful_log_line(log_path, 4 * 1024)
                        .filter(|line| !line.is_empty())
                        .map(|line| format!("{command_label} log: {line}"))
                        .unwrap_or_else(|| {
                            format!("{command_label} running; log size {log_size} bytes")
                        });
                    (message, Some(log_size))
                }
                Ok(_) | Err(_) => (
                    format!(
                        "{command_label} running for {}s; waiting for log output",
                        started_at.elapsed().as_secs()
                    ),
                    None,
                ),
            };

            if message != last_message || log_size != last_log_size {
                reporter.heartbeat(ExecutionProgress {
                    message: message.clone(),
                });
                last_message = message;
                last_log_size = log_size;
            }

            last_heartbeat_at = Instant::now();
        }

        if started_at.elapsed() >= timeout {
            terminate_child_process(child, command_label)?;

            return child.wait().map(|status| (status, true));
        }

        thread::sleep(PROCESS_POLL_INTERVAL);
    }
}

fn terminate_child_process(child: &mut Child, command_label: &str) -> io::Result<()> {
    #[cfg(windows)]
    {
        match Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
        {
            Ok(status) if status.success() => return Ok(()),
            Ok(_) | Err(_) => {}
        }
    }

    match child.kill() {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::InvalidInput => Ok(()),
        Err(error) => Err(io::Error::other(format!(
            "terminate timed out {command_label}: {error}"
        ))),
    }
}
