// Allow module inception - this is a common Rust pattern for protocol clients
#![allow(clippy::module_name_repetitions)]

use crate::error::{AgwError, AgwResult};
use crate::plan::{Plan, Step};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tracing::{debug, error, info, warn};

/// Result of a single step execution
#[derive(Debug, Clone, PartialEq)]
pub struct StepResult {
    /// Step number that was executed
    pub step_number: u32,
    /// Standard output from the command
    pub stdout: String,
    /// Standard error from the command
    pub stderr: String,
    /// Exit code (0 = success)
    pub exit_code: i32,
    /// Whether execution was successful (exit code 0)
    pub success: bool,
}

/// Result of entire plan execution
#[derive(Debug, Clone, PartialEq)]
pub struct PlanResult {
    /// Job ID that was executed
    pub job_id: String,
    /// Plan ID
    pub plan_id: String,
    /// Results from each step that was executed
    pub step_results: Vec<StepResult>,
    /// Whether all steps succeeded
    pub success: bool,
}

impl StepResult {
    /// Create a new step result
    #[must_use]
    pub fn new(step_number: u32, stdout: String, stderr: String, exit_code: i32) -> Self {
        Self {
            step_number,
            stdout,
            stderr,
            exit_code,
            success: exit_code == 0,
        }
    }
}

impl PlanResult {
    /// Create a new plan result
    #[must_use]
    pub fn new(job_id: String, plan_id: String, step_results: Vec<StepResult>) -> Self {
        let success = step_results.iter().all(|r| r.success);
        Self {
            job_id,
            plan_id,
            step_results,
            success,
        }
    }
}

/// Execute an entire plan sequentially
///
/// # Errors
///
/// Returns an error if:
/// - Command spawning fails
/// - IO operations fail while reading/writing stdout/stderr
/// - Timeout is exceeded
/// - Process cannot be killed after timeout
///
/// # Panics
///
/// This function will not panic under normal conditions. The unwrap at line 111
/// is safe because `step_results` is guaranteed to be non-empty when we check success.
///
/// Note: This function will halt on first failure and return partial results
pub async fn execute_plan(plan: &Plan) -> AgwResult<PlanResult> {
    info!(
        "Executing plan {} (job {}) with {} steps",
        plan.plan_id,
        plan.job_id,
        plan.steps.len()
    );

    let mut step_results = Vec::new();
    let mut previous_outputs: std::collections::HashMap<u32, String> =
        std::collections::HashMap::new();

    for step in &plan.steps {
        info!("Executing step {}: {}", step.step_number, step.command);

        // Get input from previous step if specified
        let input = step
            .input_from_step
            .and_then(|step_num| previous_outputs.get(&step_num).cloned());

        match execute_single_step(step, input.as_deref()).await {
            Ok(result) => {
                // Store stdout for potential use by later steps
                previous_outputs.insert(step.step_number, result.stdout.clone());

                let success = result.success;
                step_results.push(result);

                // Halt on first failure
                if !success {
                    warn!(
                        "Step {} failed with exit code {}, halting plan execution",
                        step.step_number,
                        step_results.last().unwrap().exit_code
                    );
                    break;
                }
            }
            Err(e) => {
                error!("Step {} execution failed: {e}", step.step_number);
                return Err(e);
            }
        }
    }

    let plan_result = PlanResult::new(plan.job_id.clone(), plan.plan_id.clone(), step_results);

    info!(
        "Plan {} completed: {} steps executed, success={}",
        plan.plan_id,
        plan_result.step_results.len(),
        plan_result.success
    );

    Ok(plan_result)
}

/// Execute a single step as a subprocess
///
/// # Errors
///
/// Returns an error if:
/// - Command spawning fails
/// - IO operations fail while reading stdout/stderr
/// - Timeout is exceeded
/// - Process cannot be killed after timeout
async fn execute_single_step(step: &Step, stdin_input: Option<&str>) -> AgwResult<StepResult> {
    debug!("Command: {} with args: {:?}", step.command, step.args);

    // Validate command is not empty
    if step.command.is_empty() {
        return Err(AgwError::Executor("Command cannot be empty".to_string()));
    }

    // Spawn the process with piped stdout/stderr
    let mut child = Command::new(&step.command)
        .args(&step.args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(if stdin_input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| {
            AgwError::Executor(format!("Failed to spawn command '{}': {}", step.command, e))
        })?;

    // Write stdin if provided
    if let Some(input) = stdin_input {
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(input.as_bytes())
                .await
                .map_err(|e| AgwError::Executor(format!("Failed to write stdin: {e}")))?;
            stdin
                .shutdown()
                .await
                .map_err(|e| AgwError::Executor(format!("Failed to close stdin: {e}")))?;
        }
    }

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
    let wait_result = if let Some(timeout_secs) = step.timeout_secs {
        let timeout_duration = std::time::Duration::from_secs(u64::from(timeout_secs));

        match tokio::time::timeout(timeout_duration, child.wait()).await {
            Ok(Ok(status)) => Ok(status),
            Ok(Err(e)) => Err(AgwError::Executor(format!("Process wait failed: {e}"))),
            Err(_) => {
                // Timeout occurred - kill the process
                warn!(
                    "Step {} exceeded timeout of {}s, killing process",
                    step.step_number, timeout_secs
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
        "Step {} completed with exit code {} ({} bytes stdout, {} bytes stderr)",
        step.step_number,
        exit_code,
        stdout_output.len(),
        stderr_output.len()
    );

    Ok(StepResult::new(
        step.step_number,
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
    async fn test_execute_single_step_plan() {
        let plan = Plan {
            job_id: "job-123".to_string(),
            plan_id: "plan-456".to_string(),
            plan_description: None,
            steps: vec![Step {
                step_number: 1,
                command: "echo".to_string(),
                args: vec!["hello".to_string()],
                input_from_step: None,
                timeout_secs: Some(30),
            }],
        };

        let result = execute_plan(&plan).await.unwrap();
        assert_eq!(result.job_id, "job-123");
        assert_eq!(result.plan_id, "plan-456");
        assert_eq!(result.step_results.len(), 1);
        assert_eq!(result.step_results[0].stdout.trim(), "hello");
        assert_eq!(result.step_results[0].exit_code, 0);
        assert!(result.success);
    }

    #[tokio::test]
    async fn test_execute_multi_step_plan() {
        let plan = Plan {
            job_id: "job-123".to_string(),
            plan_id: "plan-456".to_string(),
            plan_description: Some("Multi-step test".to_string()),
            steps: vec![
                Step {
                    step_number: 1,
                    command: "echo".to_string(),
                    args: vec!["line1\nline2\nline3".to_string()],
                    input_from_step: None,
                    timeout_secs: Some(30),
                },
                Step {
                    step_number: 2,
                    command: "wc".to_string(),
                    args: vec!["-l".to_string()],
                    input_from_step: Some(1),
                    timeout_secs: Some(30),
                },
            ],
        };

        let result = execute_plan(&plan).await.unwrap();
        assert_eq!(result.step_results.len(), 2);
        assert!(result.step_results[0].success);
        assert!(result.step_results[1].success);
        assert!(result.success);
    }

    #[tokio::test]
    async fn test_execute_plan_with_failure() {
        let plan = Plan {
            job_id: "job-123".to_string(),
            plan_id: "plan-456".to_string(),
            plan_description: None,
            steps: vec![
                Step {
                    step_number: 1,
                    command: "sh".to_string(),
                    args: vec!["-c".to_string(), "exit 42".to_string()],
                    input_from_step: None,
                    timeout_secs: Some(30),
                },
                Step {
                    step_number: 2,
                    command: "echo".to_string(),
                    args: vec!["should not run".to_string()],
                    input_from_step: None,
                    timeout_secs: Some(30),
                },
            ],
        };

        let result = execute_plan(&plan).await.unwrap();
        // Should only execute first step
        assert_eq!(result.step_results.len(), 1);
        assert_eq!(result.step_results[0].exit_code, 42);
        assert!(!result.step_results[0].success);
        assert!(!result.success);
    }

    #[tokio::test]
    async fn test_execute_plan_with_timeout() {
        let plan = Plan {
            job_id: "job-123".to_string(),
            plan_id: "plan-456".to_string(),
            plan_description: None,
            steps: vec![Step {
                step_number: 1,
                command: "sleep".to_string(),
                args: vec!["10".to_string()],
                input_from_step: None,
                timeout_secs: Some(1),
            }],
        };

        let result = execute_plan(&plan).await.unwrap();
        assert_eq!(result.step_results.len(), 1);
        assert!(!result.step_results[0].success);
        assert!(!result.success);
    }

    #[tokio::test]
    async fn test_execute_plan_with_stdin_piping() {
        let plan = Plan {
            job_id: "job-123".to_string(),
            plan_id: "plan-456".to_string(),
            plan_description: None,
            steps: vec![
                Step {
                    step_number: 1,
                    command: "echo".to_string(),
                    args: vec!["foo\nbar\nfoo".to_string()],
                    input_from_step: None,
                    timeout_secs: Some(30),
                },
                Step {
                    step_number: 2,
                    command: "sort".to_string(),
                    args: vec![],
                    input_from_step: Some(1),
                    timeout_secs: Some(30),
                },
                Step {
                    step_number: 3,
                    command: "uniq".to_string(),
                    args: vec![],
                    input_from_step: Some(2),
                    timeout_secs: Some(30),
                },
            ],
        };

        let result = execute_plan(&plan).await.unwrap();
        assert_eq!(result.step_results.len(), 3);
        assert!(result.success);

        // Final output should be sorted and unique
        let final_output = result.step_results[2].stdout.trim();
        assert!(final_output.contains("bar"));
        assert!(final_output.contains("foo"));
    }

    #[tokio::test]
    async fn test_execute_invalid_command() {
        let plan = Plan {
            job_id: "job-123".to_string(),
            plan_id: "plan-456".to_string(),
            plan_description: None,
            steps: vec![Step {
                step_number: 1,
                command: "this_command_does_not_exist_12345".to_string(),
                args: vec![],
                input_from_step: None,
                timeout_secs: None,
            }],
        };

        let result = execute_plan(&plan).await;
        assert!(result.is_err());
    }
}
