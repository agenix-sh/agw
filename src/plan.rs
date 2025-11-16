// Allow module inception - this is a common Rust pattern for protocol clients
#![allow(clippy::module_name_repetitions)]

use crate::error::{AgwError, AgwResult};
use serde::{Deserialize, Serialize};

/// Maximum length for job ID
const MAX_JOB_ID_LEN: usize = 128;
/// Maximum length for plan ID
const MAX_PLAN_ID_LEN: usize = 128;
/// Maximum length for plan description
const MAX_PLAN_DESCRIPTION_LEN: usize = 1024;
/// Maximum length for command
const MAX_COMMAND_LEN: usize = 4096;
/// Maximum number of arguments per task
const MAX_ARGS_COUNT: usize = 256;
/// Maximum length for a single argument
const MAX_ARG_LEN: usize = 4096;
/// Maximum number of tasks in a plan
const MAX_TASKS_COUNT: usize = 100;
/// Minimum timeout in seconds
const MIN_TIMEOUT_SECS: u32 = 1;
/// Maximum timeout in seconds (24 hours)
const MAX_TIMEOUT_SECS: u32 = 86400;

/// Dangerous Unicode characters (bidirectional overrides, zero-width)
const DANGEROUS_UNICODE: &[char] = &[
    '\u{202A}', // LEFT-TO-RIGHT EMBEDDING
    '\u{202B}', // RIGHT-TO-LEFT EMBEDDING
    '\u{202C}', // POP DIRECTIONAL FORMATTING
    '\u{202D}', // LEFT-TO-RIGHT OVERRIDE
    '\u{202E}', // RIGHT-TO-LEFT OVERRIDE
    '\u{200B}', // ZERO WIDTH SPACE
    '\u{200C}', // ZERO WIDTH NON-JOINER
    '\u{200D}', // ZERO WIDTH JOINER
    '\u{FEFF}', // ZERO WIDTH NO-BREAK SPACE
];

/// Execution plan containing multiple tasks
///
/// Plans are fetched from AGQ via BRPOP on the `queue:ready` list.
/// Each plan contains an ordered list of tasks to execute sequentially.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[allow(clippy::struct_field_names)] // Field names match schema specification
pub struct Plan {
    /// Unique job identifier for this execution instance
    pub job_id: String,

    /// Stable plan identifier (reused across multiple job executions)
    pub plan_id: String,

    /// Optional description of plan intent
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_description: Option<String>,

    /// Ordered list of tasks to execute
    pub tasks: Vec<Task>,
}

/// A single task within an execution plan
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[allow(clippy::struct_field_names)] // Field names match schema specification
pub struct Task {
    /// 1-based task number (must be contiguous)
    pub task_number: u32,

    /// Command to execute (e.g., "sort", "uniq", "agx-ocr")
    pub command: String,

    /// Command arguments
    #[serde(default)]
    pub args: Vec<String>,

    /// Optional reference to a previous task whose stdout becomes this task's stdin
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_from_task: Option<u32>,

    /// Optional per-task timeout in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u32>,
}

impl Plan {
    /// Parse a plan from JSON string
    ///
    /// # Errors
    ///
    /// Returns an error if the JSON is invalid or doesn't match the Plan schema
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Serialize plan to JSON string
    ///
    /// # Errors
    ///
    /// Returns an error if serialization fails
    #[allow(dead_code)] // Used in tests
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Validate the plan structure and all tasks
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Any field contains dangerous patterns
    /// - Tasks are empty or exceed maximum count
    /// - Task numbers are not contiguous starting at 1
    /// - `input_from_task` references are invalid
    pub fn validate(&self) -> AgwResult<()> {
        // Validate job_id
        validate_string_field(&self.job_id, "job_id", MAX_JOB_ID_LEN, true)?;

        // Validate plan_id
        validate_string_field(&self.plan_id, "plan_id", MAX_PLAN_ID_LEN, true)?;

        // Validate plan_description if present
        if let Some(desc) = &self.plan_description {
            validate_string_field(desc, "plan_description", MAX_PLAN_DESCRIPTION_LEN, false)?;
        }

        // Validate tasks array
        if self.tasks.is_empty() {
            return Err(AgwError::Worker(
                "Plan must contain at least one task".to_string(),
            ));
        }

        if self.tasks.len() > MAX_TASKS_COUNT {
            return Err(AgwError::Worker(format!(
                "Plan exceeds maximum of {MAX_TASKS_COUNT} tasks"
            )));
        }

        // Validate task numbers are contiguous starting at 1
        for (index, task) in self.tasks.iter().enumerate() {
            let expected_task_number = u32::try_from(index + 1)
                .map_err(|_| AgwError::Worker("Task index overflow".to_string()))?;
            if task.task_number != expected_task_number {
                return Err(AgwError::Worker(format!(
                    "Task numbers must be contiguous starting at 1: expected {expected_task_number}, got {}",
                    task.task_number
                )));
            }

            // Validate the task itself
            task.validate()?;

            // Validate input_from_task references
            if let Some(ref_task) = task.input_from_task {
                if ref_task == 0 {
                    return Err(AgwError::Worker("input_from_task must be >= 1".to_string()));
                }
                if ref_task >= task.task_number {
                    return Err(AgwError::Worker(format!(
                        "Task {} has invalid input_from_task {}: cannot reference self or future tasks",
                        task.task_number, ref_task
                    )));
                }
            }
        }

        Ok(())
    }
}

impl Task {
    /// Validate the task fields
    ///
    /// # Errors
    ///
    /// Returns an error if any field contains dangerous patterns or exceeds limits
    pub fn validate(&self) -> AgwResult<()> {
        // Validate command
        validate_string_field(&self.command, "command", MAX_COMMAND_LEN, false)?;
        check_for_dangerous_patterns(&self.command, "command")?;

        // Validate arguments
        if self.args.len() > MAX_ARGS_COUNT {
            return Err(AgwError::Worker(format!(
                "Task {} exceeds maximum of {MAX_ARGS_COUNT} arguments",
                self.task_number
            )));
        }

        for (i, arg) in self.args.iter().enumerate() {
            validate_string_field(arg, &format!("args[{i}]"), MAX_ARG_LEN, false)?;
            check_for_dangerous_patterns(arg, &format!("args[{i}]"))?;
        }

        // Validate timeout if present
        if let Some(timeout) = self.timeout_secs {
            if timeout < MIN_TIMEOUT_SECS {
                return Err(AgwError::Worker(format!(
                    "Task {} timeout must be at least {MIN_TIMEOUT_SECS} seconds",
                    self.task_number
                )));
            }
            if timeout > MAX_TIMEOUT_SECS {
                return Err(AgwError::Worker(format!(
                    "Task {} timeout must not exceed {MAX_TIMEOUT_SECS} seconds",
                    self.task_number
                )));
            }
        }

        Ok(())
    }
}

/// Validate a string field for length and dangerous characters
fn validate_string_field(
    value: &str,
    field_name: &str,
    max_len: usize,
    check_empty: bool,
) -> AgwResult<()> {
    if check_empty && value.is_empty() {
        return Err(AgwError::Worker(format!("{field_name} cannot be empty")));
    }

    if value.len() > max_len {
        return Err(AgwError::Worker(format!(
            "{field_name} exceeds maximum length of {max_len}"
        )));
    }

    // Check for null bytes
    if value.contains('\0') {
        return Err(AgwError::Worker(format!("{field_name} contains null byte")));
    }

    // Check for control characters (except tab and newline which might be legitimate in descriptions)
    for ch in value.chars() {
        if ch.is_control() && ch != '\t' && ch != '\n' {
            return Err(AgwError::Worker(format!(
                "{field_name} contains control character"
            )));
        }
    }

    // Check for dangerous Unicode
    for &dangerous_char in DANGEROUS_UNICODE {
        if value.contains(dangerous_char) {
            return Err(AgwError::Worker(format!(
                "{field_name} contains dangerous Unicode character"
            )));
        }
    }

    Ok(())
}

/// Check for dangerous shell patterns
fn check_for_dangerous_patterns(value: &str, field_name: &str) -> AgwResult<()> {
    let dangerous_chars = ['&', '|', ';', '$', '`', '\n', '\r'];

    for &ch in &dangerous_chars {
        if value.contains(ch) {
            return Err(AgwError::Worker(format!(
                "{field_name} contains dangerous character: '{ch}'"
            )));
        }
    }

    // Path traversal check - precise to avoid false positives like "1..10"
    if value.contains("../") || value.contains("..\\") || value.starts_with("..") {
        return Err(AgwError::Worker(format!(
            "{field_name} contains path traversal sequence"
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plan_creation() {
        let plan = Plan {
            job_id: "job-123".to_string(),
            plan_id: "plan-456".to_string(),
            plan_description: Some("Test plan".to_string()),
            tasks: vec![Task {
                task_number: 1,
                command: "echo".to_string(),
                args: vec!["hello".to_string()],
                input_from_task: None,
                timeout_secs: Some(30),
            }],
        };

        assert_eq!(plan.job_id, "job-123");
        assert_eq!(plan.plan_id, "plan-456");
        assert_eq!(plan.tasks.len(), 1);
    }

    #[test]
    fn test_plan_json_serialization() {
        let plan = Plan {
            job_id: "job-123".to_string(),
            plan_id: "plan-456".to_string(),
            plan_description: None,
            tasks: vec![Task {
                task_number: 1,
                command: "ls".to_string(),
                args: vec!["-la".to_string()],
                input_from_task: None,
                timeout_secs: Some(30),
            }],
        };

        let json = plan.to_json().unwrap();
        let parsed = Plan::from_json(&json).unwrap();
        assert_eq!(plan, parsed);
    }

    #[test]
    fn test_plan_with_multiple_steps() {
        let plan = Plan {
            job_id: "job-123".to_string(),
            plan_id: "plan-456".to_string(),
            plan_description: Some("Multi-step plan".to_string()),
            tasks: vec![
                Task {
                    task_number: 1,
                    command: "sort".to_string(),
                    args: vec!["-r".to_string()],
                    input_from_task: None,
                    timeout_secs: Some(30),
                },
                Task {
                    task_number: 2,
                    command: "uniq".to_string(),
                    args: vec![],
                    input_from_task: Some(1),
                    timeout_secs: Some(30),
                },
            ],
        };

        assert_eq!(plan.tasks.len(), 2);
        assert_eq!(plan.tasks[1].input_from_task, Some(1));
    }

    #[test]
    fn test_plan_validation_success() {
        let plan = Plan {
            job_id: "job-123".to_string(),
            plan_id: "plan-456".to_string(),
            plan_description: Some("Valid plan".to_string()),
            tasks: vec![
                Task {
                    task_number: 1,
                    command: "echo".to_string(),
                    args: vec!["test".to_string()],
                    input_from_task: None,
                    timeout_secs: Some(30),
                },
                Task {
                    task_number: 2,
                    command: "wc".to_string(),
                    args: vec!["-l".to_string()],
                    input_from_task: Some(1),
                    timeout_secs: Some(30),
                },
            ],
        };

        assert!(plan.validate().is_ok());
    }

    #[test]
    fn test_plan_validation_empty_steps() {
        let plan = Plan {
            job_id: "job-123".to_string(),
            plan_id: "plan-456".to_string(),
            plan_description: None,
            tasks: vec![],
        };

        assert!(plan.validate().is_err());
    }

    #[test]
    fn test_plan_validation_non_contiguous_steps() {
        let plan = Plan {
            job_id: "job-123".to_string(),
            plan_id: "plan-456".to_string(),
            plan_description: None,
            tasks: vec![
                Task {
                    task_number: 1,
                    command: "echo".to_string(),
                    args: vec![],
                    input_from_task: None,
                    timeout_secs: None,
                },
                Task {
                    task_number: 3, // Skip 2
                    command: "wc".to_string(),
                    args: vec![],
                    input_from_task: None,
                    timeout_secs: None,
                },
            ],
        };

        assert!(plan.validate().is_err());
    }

    #[test]
    fn test_plan_validation_invalid_input_from_task() {
        let plan = Plan {
            job_id: "job-123".to_string(),
            plan_id: "plan-456".to_string(),
            plan_description: None,
            tasks: vec![
                Task {
                    task_number: 1,
                    command: "echo".to_string(),
                    args: vec![],
                    input_from_task: None,
                    timeout_secs: None,
                },
                Task {
                    task_number: 2,
                    command: "wc".to_string(),
                    args: vec![],
                    input_from_task: Some(2), // Cannot reference self
                    timeout_secs: None,
                },
            ],
        };

        assert!(plan.validate().is_err());
    }

    #[test]
    fn test_step_validation_command_injection() {
        let step = Task {
            task_number: 1,
            command: "ls; rm -rf /".to_string(),
            args: vec![],
            input_from_task: None,
            timeout_secs: None,
        };

        assert!(step.validate().is_err());
    }

    #[test]
    fn test_step_validation_timeout_too_low() {
        let step = Task {
            task_number: 1,
            command: "sleep".to_string(),
            args: vec!["10".to_string()],
            input_from_task: None,
            timeout_secs: Some(0),
        };

        assert!(step.validate().is_err());
    }
}
