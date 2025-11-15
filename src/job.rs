use serde::{Deserialize, Serialize};

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
}
