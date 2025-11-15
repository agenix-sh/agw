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
/// Maximum number of arguments per step
const MAX_ARGS_COUNT: usize = 256;
/// Maximum length for a single argument
const MAX_ARG_LEN: usize = 4096;
/// Maximum number of steps in a plan
const MAX_STEPS_COUNT: usize = 100;
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

/// Execution plan containing multiple steps
///
/// Plans are fetched from AGQ via BRPOP on the `queue:ready` list.
/// Each plan contains an ordered list of steps to execute sequentially.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Plan {
    /// Unique job identifier for this execution instance
    pub job_id: String,

    /// Stable plan identifier (reused across multiple job executions)
    pub plan_id: String,

    /// Optional description of plan intent
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_description: Option<String>,

    /// Ordered list of steps to execute
    pub steps: Vec<Step>,
}

/// A single step within an execution plan
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Step {
    /// 1-based step number (must be contiguous)
    pub step_number: u32,

    /// Command to execute (e.g., "sort", "uniq", "agx-ocr")
    pub command: String,

    /// Command arguments
    #[serde(default)]
    pub args: Vec<String>,

    /// Optional reference to a previous step whose stdout becomes this step's stdin
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_from_step: Option<u32>,

    /// Optional per-step timeout in seconds
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

    /// Validate the plan structure and all steps
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Any field contains dangerous patterns
    /// - Steps are empty or exceed maximum count
    /// - Step numbers are not contiguous starting at 1
    /// - input_from_step references are invalid
    pub fn validate(&self) -> AgwResult<()> {
        // Validate job_id
        validate_string_field(&self.job_id, "job_id", MAX_JOB_ID_LEN, true)?;

        // Validate plan_id
        validate_string_field(&self.plan_id, "plan_id", MAX_PLAN_ID_LEN, true)?;

        // Validate plan_description if present
        if let Some(desc) = &self.plan_description {
            validate_string_field(desc, "plan_description", MAX_PLAN_DESCRIPTION_LEN, false)?;
        }

        // Validate steps array
        if self.steps.is_empty() {
            return Err(AgwError::Worker(
                "Plan must contain at least one step".to_string(),
            ));
        }

        if self.steps.len() > MAX_STEPS_COUNT {
            return Err(AgwError::Worker(format!(
                "Plan exceeds maximum of {MAX_STEPS_COUNT} steps"
            )));
        }

        // Validate step numbers are contiguous starting at 1
        for (index, step) in self.steps.iter().enumerate() {
            let expected_step_number = (index + 1) as u32;
            if step.step_number != expected_step_number {
                return Err(AgwError::Worker(format!(
                    "Step numbers must be contiguous starting at 1: expected {expected_step_number}, got {}",
                    step.step_number
                )));
            }

            // Validate the step itself
            step.validate()?;

            // Validate input_from_step references
            if let Some(ref_step) = step.input_from_step {
                if ref_step == 0 {
                    return Err(AgwError::Worker("input_from_step must be >= 1".to_string()));
                }
                if ref_step >= step.step_number {
                    return Err(AgwError::Worker(format!(
                        "Step {} has invalid input_from_step {}: cannot reference self or future steps",
                        step.step_number, ref_step
                    )));
                }
            }
        }

        Ok(())
    }
}

impl Step {
    /// Validate the step fields
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
                "Step {} exceeds maximum of {MAX_ARGS_COUNT} arguments",
                self.step_number
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
                    "Step {} timeout must be at least {MIN_TIMEOUT_SECS} seconds",
                    self.step_number
                )));
            }
            if timeout > MAX_TIMEOUT_SECS {
                return Err(AgwError::Worker(format!(
                    "Step {} timeout must not exceed {MAX_TIMEOUT_SECS} seconds",
                    self.step_number
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
            steps: vec![Step {
                step_number: 1,
                command: "echo".to_string(),
                args: vec!["hello".to_string()],
                input_from_step: None,
                timeout_secs: Some(30),
            }],
        };

        assert_eq!(plan.job_id, "job-123");
        assert_eq!(plan.plan_id, "plan-456");
        assert_eq!(plan.steps.len(), 1);
    }

    #[test]
    fn test_plan_json_serialization() {
        let plan = Plan {
            job_id: "job-123".to_string(),
            plan_id: "plan-456".to_string(),
            plan_description: None,
            steps: vec![Step {
                step_number: 1,
                command: "ls".to_string(),
                args: vec!["-la".to_string()],
                input_from_step: None,
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
            steps: vec![
                Step {
                    step_number: 1,
                    command: "sort".to_string(),
                    args: vec!["-r".to_string()],
                    input_from_step: None,
                    timeout_secs: Some(30),
                },
                Step {
                    step_number: 2,
                    command: "uniq".to_string(),
                    args: vec![],
                    input_from_step: Some(1),
                    timeout_secs: Some(30),
                },
            ],
        };

        assert_eq!(plan.steps.len(), 2);
        assert_eq!(plan.steps[1].input_from_step, Some(1));
    }

    #[test]
    fn test_plan_validation_success() {
        let plan = Plan {
            job_id: "job-123".to_string(),
            plan_id: "plan-456".to_string(),
            plan_description: Some("Valid plan".to_string()),
            steps: vec![
                Step {
                    step_number: 1,
                    command: "echo".to_string(),
                    args: vec!["test".to_string()],
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

        assert!(plan.validate().is_ok());
    }

    #[test]
    fn test_plan_validation_empty_steps() {
        let plan = Plan {
            job_id: "job-123".to_string(),
            plan_id: "plan-456".to_string(),
            plan_description: None,
            steps: vec![],
        };

        assert!(plan.validate().is_err());
    }

    #[test]
    fn test_plan_validation_non_contiguous_steps() {
        let plan = Plan {
            job_id: "job-123".to_string(),
            plan_id: "plan-456".to_string(),
            plan_description: None,
            steps: vec![
                Step {
                    step_number: 1,
                    command: "echo".to_string(),
                    args: vec![],
                    input_from_step: None,
                    timeout_secs: None,
                },
                Step {
                    step_number: 3, // Skip 2
                    command: "wc".to_string(),
                    args: vec![],
                    input_from_step: None,
                    timeout_secs: None,
                },
            ],
        };

        assert!(plan.validate().is_err());
    }

    #[test]
    fn test_plan_validation_invalid_input_from_step() {
        let plan = Plan {
            job_id: "job-123".to_string(),
            plan_id: "plan-456".to_string(),
            plan_description: None,
            steps: vec![
                Step {
                    step_number: 1,
                    command: "echo".to_string(),
                    args: vec![],
                    input_from_step: None,
                    timeout_secs: None,
                },
                Step {
                    step_number: 2,
                    command: "wc".to_string(),
                    args: vec![],
                    input_from_step: Some(2), // Cannot reference self
                    timeout_secs: None,
                },
            ],
        };

        assert!(plan.validate().is_err());
    }

    #[test]
    fn test_step_validation_command_injection() {
        let step = Step {
            step_number: 1,
            command: "ls; rm -rf /".to_string(),
            args: vec![],
            input_from_step: None,
            timeout_secs: None,
        };

        assert!(step.validate().is_err());
    }

    #[test]
    fn test_step_validation_timeout_too_low() {
        let step = Step {
            step_number: 1,
            command: "sleep".to_string(),
            args: vec!["10".to_string()],
            input_from_step: None,
            timeout_secs: Some(0),
        };

        assert!(step.validate().is_err());
    }
}
