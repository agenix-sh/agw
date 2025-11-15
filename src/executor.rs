// Allow module inception - this is a common Rust pattern for protocol clients
#![allow(clippy::module_name_repetitions)]

use crate::error::{AgwError, AgwResult};
use crate::job::Job;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tracing::{debug, info, warn};

/// Result of step execution
#[derive(Debug, Clone, PartialEq)]
pub struct ExecutionResult {
    /// Job ID that was executed
    pub job_id: String,
    /// Standard output from the command
    pub stdout: String,
    /// Standard error from the command
    pub stderr: String,
    /// Exit code (0 = success)
    pub exit_code: i32,
    /// Whether execution was successful (exit code 0)
    pub success: bool,
}

impl ExecutionResult {
    /// Create a new execution result
    #[must_use]
    pub fn new(job_id: String, stdout: String, stderr: String, exit_code: i32) -> Self {
        Self {
            job_id,
            stdout,
            stderr,
            exit_code,
            success: exit_code == 0,
        }
    }
}

/// Execute a job step as a subprocess
///
/// # Errors
///
/// Returns an error if:
/// - Command spawning fails
/// - IO operations fail while reading stdout/stderr
/// - Timeout is exceeded
/// - Process cannot be killed after timeout
pub async fn execute_step(job: &Job) -> AgwResult<ExecutionResult> {
    info!(
        "Executing job {} - {} step {}",
        job.id, job.tool, job.step_number
    );
    debug!("Command: {} with args: {:?}", job.command, job.args);

    // Validate command is not empty
    if job.command.is_empty() {
        return Err(AgwError::Executor("Command cannot be empty".to_string()));
    }

    // Spawn the process with piped stdout/stderr
    let mut child = Command::new(&job.command)
        .args(&job.args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null()) // No stdin - security measure
        .kill_on_drop(true) // Ensure cleanup on drop
        .spawn()
        .map_err(|e| {
            AgwError::Executor(format!("Failed to spawn command '{}': {}", job.command, e))
        })?;

    // Get stdout and stderr handles
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| AgwError::Executor("Failed to capture stdout".to_string()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| AgwError::Executor("Failed to capture stderr".to_string()))?;

    // Create buffered readers
    let stdout_reader = BufReader::new(stdout);
    let stderr_reader = BufReader::new(stderr);

    // Spawn tasks to read stdout and stderr concurrently
    let stdout_handle = tokio::spawn(read_stream(stdout_reader));
    let stderr_handle = tokio::spawn(read_stream(stderr_reader));

    // Wait for process with optional timeout
    let wait_result = if let Some(timeout_secs) = job.timeout {
        let timeout_duration = std::time::Duration::from_secs(timeout_secs);

        match tokio::time::timeout(timeout_duration, child.wait()).await {
            Ok(Ok(status)) => Ok(status),
            Ok(Err(e)) => Err(AgwError::Executor(format!("Process wait failed: {e}"))),
            Err(_) => {
                // Timeout occurred - kill the process
                warn!(
                    "Job {} exceeded timeout of {}s, killing process",
                    job.id, timeout_secs
                );
                child.kill().await.map_err(|e| {
                    AgwError::Executor(format!("Failed to kill process after timeout: {e}"))
                })?;

                // Wait for process to be reaped
                let status = child.wait().await.map_err(|e| {
                    AgwError::Executor(format!("Failed to wait for killed process: {e}"))
                })?;

                Ok(status)
            }
        }
    } else {
        // No timeout - wait indefinitely
        child
            .wait()
            .await
            .map_err(|e| AgwError::Executor(format!("Process wait failed: {e}")))
    };

    let status = wait_result?;

    // Collect stdout and stderr
    let stdout_output = stdout_handle
        .await
        .map_err(|e| AgwError::Executor(format!("Failed to join stdout task: {e}")))??;

    let stderr_output = stderr_handle
        .await
        .map_err(|e| AgwError::Executor(format!("Failed to join stderr task: {e}")))??;

    // Get exit code
    let exit_code = status.code().unwrap_or(-1);

    info!(
        "Job {} completed with exit code {} ({} bytes stdout, {} bytes stderr)",
        job.id,
        exit_code,
        stdout_output.len(),
        stderr_output.len()
    );

    Ok(ExecutionResult::new(
        job.id.clone(),
        stdout_output,
        stderr_output,
        exit_code,
    ))
}

/// Read all lines from a stream asynchronously
async fn read_stream<R: tokio::io::AsyncRead + Unpin>(reader: BufReader<R>) -> AgwResult<String> {
    let mut lines = reader.lines();
    let mut output = String::new();

    loop {
        match lines.next_line().await {
            Ok(Some(line)) => {
                output.push_str(&line);
                output.push('\n');
            }
            Ok(None) => break,
            Err(e) => return Err(AgwError::Executor(format!("Failed to read line: {e}"))),
        }
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_execute_simple_command() {
        let job = Job {
            id: "test-1".to_string(),
            plan_id: "plan-1".to_string(),
            step_number: 1,
            tool: "echo".to_string(),
            command: "echo".to_string(),
            args: vec!["hello".to_string()],
            timeout: None,
        };

        let result = execute_step(&job).await.unwrap();
        assert_eq!(result.job_id, "test-1");
        assert_eq!(result.stdout.trim(), "hello");
        assert_eq!(result.stderr, "");
        assert_eq!(result.exit_code, 0);
        assert!(result.success);
    }

    #[tokio::test]
    async fn test_execute_command_with_stderr() {
        let job = Job {
            id: "test-2".to_string(),
            plan_id: "plan-1".to_string(),
            step_number: 1,
            tool: "sh".to_string(),
            command: "sh".to_string(),
            args: vec!["-c".to_string(), "echo error >&2".to_string()],
            timeout: None,
        };

        let result = execute_step(&job).await.unwrap();
        assert_eq!(result.job_id, "test-2");
        assert_eq!(result.stdout, "");
        assert_eq!(result.stderr.trim(), "error");
        assert_eq!(result.exit_code, 0);
        assert!(result.success);
    }

    #[tokio::test]
    async fn test_execute_command_with_exit_code() {
        let job = Job {
            id: "test-3".to_string(),
            plan_id: "plan-1".to_string(),
            step_number: 1,
            tool: "sh".to_string(),
            command: "sh".to_string(),
            args: vec!["-c".to_string(), "exit 42".to_string()],
            timeout: None,
        };

        let result = execute_step(&job).await.unwrap();
        assert_eq!(result.job_id, "test-3");
        assert_eq!(result.exit_code, 42);
        assert!(!result.success);
    }

    #[tokio::test]
    async fn test_execute_command_timeout() {
        let job = Job {
            id: "test-4".to_string(),
            plan_id: "plan-1".to_string(),
            step_number: 1,
            tool: "sleep".to_string(),
            command: "sleep".to_string(),
            args: vec!["10".to_string()],
            timeout: Some(1), // 1 second timeout
        };

        let result = execute_step(&job).await.unwrap();
        assert_eq!(result.job_id, "test-4");
        // Process should be killed, exit code will be non-zero
        assert!(!result.success);
    }

    #[tokio::test]
    async fn test_execute_invalid_command() {
        let job = Job {
            id: "test-5".to_string(),
            plan_id: "plan-1".to_string(),
            step_number: 1,
            tool: "nonexistent".to_string(),
            command: "this_command_does_not_exist_12345".to_string(),
            args: vec![],
            timeout: None,
        };

        let result = execute_step(&job).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Failed to spawn"));
    }

    #[tokio::test]
    async fn test_execute_empty_command() {
        let job = Job {
            id: "test-6".to_string(),
            plan_id: "plan-1".to_string(),
            step_number: 1,
            tool: "empty".to_string(),
            command: "".to_string(),
            args: vec![],
            timeout: None,
        };

        let result = execute_step(&job).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Command cannot be empty"));
    }

    #[tokio::test]
    async fn test_execute_multiline_output() {
        let job = Job {
            id: "test-7".to_string(),
            plan_id: "plan-1".to_string(),
            step_number: 1,
            tool: "sh".to_string(),
            command: "sh".to_string(),
            args: vec![
                "-c".to_string(),
                "echo line1; echo line2; echo line3".to_string(),
            ],
            timeout: None,
        };

        let result = execute_step(&job).await.unwrap();
        assert_eq!(result.job_id, "test-7");
        assert!(result.stdout.contains("line1"));
        assert!(result.stdout.contains("line2"));
        assert!(result.stdout.contains("line3"));
        assert_eq!(result.exit_code, 0);
        assert!(result.success);
    }
}
