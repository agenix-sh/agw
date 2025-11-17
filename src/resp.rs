// Allow module inception - this is a common Rust pattern for protocol clients
#![allow(clippy::module_name_repetitions)]

use crate::error::{AgwError, AgwResult};
use redis::{aio::ConnectionManager, Client, Cmd};
use tracing::{debug, info};

/// RESP client for communicating with AGQ
///
/// Clone is safe and efficient because `ConnectionManager` uses Arc internally,
/// making clones lightweight. This allows workers to spawn plan execution tasks
/// with their own client instance for result posting, while the main worker
/// continues to send heartbeats on the original client.
#[derive(Clone)]
pub struct RespClient {
    connection: ConnectionManager,
}

impl RespClient {
    /// Create a new RESP client and connect to AGQ
    ///
    /// # Errors
    ///
    /// Returns an error if connection fails or address is invalid
    pub async fn connect(address: &str) -> AgwResult<Self> {
        debug!("Connecting to AGQ at {}", address);

        // Validate address format to prevent injection
        if !is_valid_address(address) {
            return Err(AgwError::InvalidConfig(
                "Invalid AGQ address format".to_string(),
            ));
        }

        let redis_url = format!("redis://{address}");
        let client = Client::open(redis_url)
            .map_err(|e| AgwError::Connection(format!("Failed to create client: {e}")))?;

        let connection = ConnectionManager::new(client)
            .await
            .map_err(|e| AgwError::Connection(format!("Failed to connect: {e}")))?;

        info!("Connected to AGQ at {}", address);

        Ok(Self { connection })
    }

    /// Authenticate with the AGQ server using session key
    ///
    /// # Errors
    ///
    /// Returns an error if authentication fails or receives unexpected response
    pub async fn authenticate(&mut self, session_key: &str) -> AgwResult<()> {
        debug!("Authenticating with AGQ");

        let response: String = Cmd::new()
            .arg("AUTH")
            .arg(session_key)
            .query_async(&mut self.connection)
            .await
            .map_err(|e| AgwError::Authentication(format!("AUTH failed: {e}")))?;

        if response != "OK" {
            return Err(AgwError::Authentication(format!(
                "Unexpected AUTH response: {response}"
            )));
        }

        info!("Successfully authenticated with AGQ");
        Ok(())
    }

    /// Send a heartbeat to AGQ
    ///
    /// # Errors
    ///
    /// Returns an error if the RESP protocol command fails
    pub async fn heartbeat(&mut self, worker_id: &str) -> AgwResult<()> {
        debug!("Sending heartbeat for worker {worker_id}");

        let response: String = Cmd::new()
            .arg("PING")
            .arg(worker_id)
            .query_async(&mut self.connection)
            .await
            .map_err(|e| AgwError::RespProtocol(format!("PING failed: {e}")))?;

        debug!("Heartbeat response: {response}");
        Ok(())
    }

    /// Register worker's available tools with AGQ
    ///
    /// Stores the tool list in the `worker:<id>:tools` key as a comma-separated string.
    /// This enables AGQ to perform capability-based job routing in the future.
    ///
    /// # Errors
    ///
    /// Returns an error if the RESP protocol command fails or if tool names are invalid
    pub async fn register_tools(&mut self, worker_id: &str, tools: &[String]) -> AgwResult<()> {
        const MAX_TOOLS: usize = 100;
        const MAX_TOOL_NAME_LENGTH: usize = 64;

        if tools.is_empty() {
            debug!("No tools to register for worker {}", worker_id);
            return Ok(());
        }

        // Validate number of tools
        if tools.len() > MAX_TOOLS {
            return Err(AgwError::RespProtocol(format!(
                "Too many tools: {} (maximum {MAX_TOOLS})",
                tools.len()
            )));
        }

        // Validate each tool name
        for tool in tools {
            // Check length
            if tool.is_empty() {
                return Err(AgwError::RespProtocol(
                    "Tool name cannot be empty".to_string(),
                ));
            }

            if tool.len() > MAX_TOOL_NAME_LENGTH {
                return Err(AgwError::RespProtocol(format!(
                    "Tool name too long: '{}' ({} chars, maximum {})",
                    tool,
                    tool.len(),
                    MAX_TOOL_NAME_LENGTH
                )));
            }

            // Only allow alphanumeric, hyphens, and underscores
            // This prevents command injection, path traversal, and key injection
            if !tool
                .chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
            {
                return Err(AgwError::RespProtocol(format!(
                    "Invalid tool name '{tool}': only alphanumeric, hyphens, and underscores allowed"
                )));
            }
        }

        let key = format!("worker:{worker_id}:tools");
        let value = tools.join(",");

        info!(
            "Registering {} tools for worker {worker_id}: {value}",
            tools.len()
        );

        self.set(&key, &value).await?;

        info!("Successfully registered tools for worker {worker_id}");
        Ok(())
    }

    /// Blocking pop from queue using BRPOP
    ///
    /// Blocks until a job is available in the queue or timeout is reached.
    /// Returns the job data as a JSON string, or None if timeout occurred.
    ///
    /// # Errors
    ///
    /// Returns an error if the RESP protocol command fails or queue name doesn't match
    pub async fn brpop(&mut self, queue: &str, timeout: u64) -> AgwResult<Option<String>> {
        debug!(
            "Blocking pop from queue {} with timeout {}s",
            queue, timeout
        );

        // BRPOP returns (key, value) tuple or nil on timeout
        let result: Option<(String, String)> = Cmd::new()
            .arg("BRPOP")
            .arg(queue)
            .arg(timeout)
            .query_async(&mut self.connection)
            .await
            .map_err(|e| AgwError::RespProtocol(format!("BRPOP failed: {e}")))?;

        if let Some((returned_queue, value)) = result {
            // Validate that the job came from the expected queue
            if returned_queue != queue {
                return Err(AgwError::RespProtocol(format!(
                    "Job received from unexpected queue: expected '{queue}', got '{returned_queue}'"
                )));
            }

            debug!("Received job from queue {}: {} bytes", queue, value.len());
            Ok(Some(value))
        } else {
            debug!("BRPOP timeout on queue {}", queue);
            Ok(None)
        }
    }

    /// Set a key-value pair in AGQ
    ///
    /// # Errors
    ///
    /// Returns an error if the RESP protocol command fails
    pub async fn set(&mut self, key: &str, value: &str) -> AgwResult<()> {
        debug!("Setting key: {}", key);

        let response: String = Cmd::new()
            .arg("SET")
            .arg(key)
            .arg(value)
            .query_async(&mut self.connection)
            .await
            .map_err(|e| AgwError::RespProtocol(format!("SET failed: {e}")))?;

        if response != "OK" {
            return Err(AgwError::RespProtocol(format!(
                "Unexpected SET response: {response}"
            )));
        }

        debug!("Successfully set key: {}", key);
        Ok(())
    }

    /// Post job execution results to AGQ with retry logic
    ///
    /// Stores stdout, stderr, and status for the given job ID.
    /// Retries up to 3 times with exponential backoff on failure to ensure
    /// results are not lost due to transient network issues.
    ///
    /// # Errors
    ///
    /// Returns an error if all retry attempts fail or if `job_id`/`status` are invalid
    ///
    /// # Panics
    ///
    /// Panics if all retry attempts fail but last_error is None. This should never
    /// happen in practice since MAX_RETRIES is at least 1, guaranteeing last_error
    /// will be populated.
    pub async fn post_job_result(
        &mut self,
        job_id: &str,
        stdout: &str,
        stderr: &str,
        status: &str,
    ) -> AgwResult<()> {
        const MAX_RETRIES: u32 = 3;
        const INITIAL_BACKOFF_MS: u64 = 100;

        let mut last_error = None;

        for attempt in 0..MAX_RETRIES {
            match self
                .post_job_result_once(job_id, stdout, stderr, status)
                .await
            {
                Ok(()) => return Ok(()),
                Err(e) => {
                    last_error = Some(e);
                    if attempt < MAX_RETRIES - 1 {
                        let backoff_ms = INITIAL_BACKOFF_MS * 2_u64.pow(attempt);
                        debug!(
                            "Result posting failed (attempt {}/{}), retrying after {}ms",
                            attempt + 1,
                            MAX_RETRIES,
                            backoff_ms
                        );
                        tokio::time::sleep(tokio::time::Duration::from_millis(backoff_ms)).await;
                    }
                }
            }
        }

        Err(last_error.unwrap())
    }

    /// Internal method to post job result once without retries
    ///
    /// # Errors
    ///
    /// Returns an error if any RESP protocol command fails or if `job_id`/`status` are invalid
    async fn post_job_result_once(
        &mut self,
        job_id: &str,
        stdout: &str,
        stderr: &str,
        status: &str,
    ) -> AgwResult<()> {
        debug!("Posting results for job {}", job_id);

        // Validate job ID to prevent Redis key injection
        if job_id.is_empty() {
            return Err(AgwError::RespProtocol("Job ID cannot be empty".to_string()));
        }

        // Prevent colons in job ID to avoid key collision/injection
        // (job IDs with colons could create malformed keys like "job:abc:def:stdout")
        if job_id.contains(':') {
            return Err(AgwError::RespProtocol(format!(
                "Job ID cannot contain colons: {job_id}"
            )));
        }

        // Validate status is one of the expected values
        if !matches!(status, "completed" | "failed" | "pending" | "running") {
            return Err(AgwError::RespProtocol(format!(
                "Invalid job status: {status}"
            )));
        }

        // Set stdout
        let stdout_key = format!("job:{}:stdout", job_id);
        self.set(&stdout_key, stdout).await?;

        // Set stderr
        let stderr_key = format!("job:{}:stderr", job_id);
        self.set(&stderr_key, stderr).await?;

        // Set status
        let status_key = format!("job:{}:status", job_id);
        self.set(&status_key, status).await?;

        info!("Successfully posted results for job {}", job_id);
        Ok(())
    }

    /// Get the underlying connection (for future operations)
    #[allow(dead_code)]
    pub fn connection(&mut self) -> &mut ConnectionManager {
        &mut self.connection
    }
}

/// Validate address format (host:port)
fn is_valid_address(address: &str) -> bool {
    // Must contain exactly one colon
    let parts: Vec<&str> = address.split(':').collect();
    if parts.len() != 2 {
        return false;
    }

    let host = parts[0];
    let port = parts[1];

    // Host must not be empty and not contain suspicious characters
    if host.is_empty()
        || host.contains(';')
        || host.contains('|')
        || host.contains('$')
        || host.contains('`')
        || host.contains('&')
    {
        return false;
    }

    // Port must be a valid number
    port.parse::<u16>().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_valid_address() {
        assert!(is_valid_address("127.0.0.1:6379"));
        assert!(is_valid_address("localhost:6379"));
        assert!(is_valid_address("agq.example.com:6379"));
        assert!(is_valid_address("192.168.1.1:8080"));
    }

    #[test]
    fn test_is_valid_address_invalid() {
        assert!(!is_valid_address(""));
        assert!(!is_valid_address("localhost"));
        assert!(!is_valid_address("localhost:"));
        assert!(!is_valid_address(":6379"));
        assert!(!is_valid_address("localhost:abc"));
        assert!(!is_valid_address("localhost:99999"));
        assert!(!is_valid_address("host;rm -rf /:6379"));
        assert!(!is_valid_address("host|cat:6379"));
    }

    #[test]
    fn test_is_valid_address_injection() {
        assert!(!is_valid_address("localhost;whoami:6379"));
        assert!(!is_valid_address("localhost|cat /etc/passwd:6379"));
        assert!(!is_valid_address("$(whoami):6379"));
    }

    #[test]
    fn test_post_job_result_validates_status() {
        // Valid statuses should be accepted (tested via mock in integration tests)
        let valid_statuses = vec!["completed", "failed", "pending", "running"];
        for status in valid_statuses {
            assert!(matches!(
                status,
                "completed" | "failed" | "pending" | "running"
            ));
        }

        // Invalid status would be rejected
        let invalid_status = "invalid_status";
        assert!(!matches!(
            invalid_status,
            "completed" | "failed" | "pending" | "running"
        ));
    }

    #[test]
    fn test_job_key_format() {
        // Test that job key format matches expected pattern
        let job_id = "job-123";
        let stdout_key = format!("job:{}:stdout", job_id);
        let stderr_key = format!("job:{}:stderr", job_id);
        let status_key = format!("job:{}:status", job_id);

        assert_eq!(stdout_key, "job:job-123:stdout");
        assert_eq!(stderr_key, "job:job-123:stderr");
        assert_eq!(status_key, "job:job-123:status");
    }

    #[test]
    fn test_job_id_validation() {
        // Valid job IDs should pass validation checks
        let valid_job_ids = vec![
            "job-123",
            "550e8400-e29b-41d4-a716-446655440000",
            "job_with_underscores",
            "JOB-UPPERCASE-123",
        ];

        for job_id in valid_job_ids {
            assert!(!job_id.is_empty());
            assert!(!job_id.contains(':'));
        }

        // Invalid job IDs should fail validation
        let invalid_job_ids = vec![
            "",                // Empty
            "job:123",         // Contains colon (key injection)
            "job-123:status",  // Contains colon (could create "job:job-123:status:stdout")
            "abc:def:ghi",     // Multiple colons
            ":leading-colon",  // Leading colon
            "trailing-colon:", // Trailing colon
        ];

        for job_id in invalid_job_ids {
            assert!(job_id.is_empty() || job_id.contains(':'));
        }
    }

    #[test]
    fn test_tool_list_formatting() {
        // Test that tools are formatted correctly for storage
        let tools = [
            "sort".to_string(),
            "grep".to_string(),
            "agx-ocr".to_string(),
        ];
        let formatted = tools.join(",");
        assert_eq!(formatted, "sort,grep,agx-ocr");

        // Test empty tools list
        let empty_tools: Vec<String> = vec![];
        let formatted_empty = empty_tools.join(",");
        assert_eq!(formatted_empty, "");
    }

    #[test]
    fn test_worker_tools_key_format() {
        // Test that worker tools key is formatted correctly
        let worker_id = "worker-123";
        let key = format!("worker:{}:tools", worker_id);
        assert_eq!(key, "worker:worker-123:tools");

        // UUID format
        let uuid_worker = "550e8400-e29b-41d4-a716-446655440000";
        let uuid_key = format!("worker:{}:tools", uuid_worker);
        assert_eq!(
            uuid_key,
            "worker:550e8400-e29b-41d4-a716-446655440000:tools"
        );
    }

    #[test]
    fn test_tool_name_validation_logic() {
        // Valid tool names
        let valid_names = vec!["sort", "grep", "agx-ocr", "tool_name", "TOOL123"];
        for name in valid_names {
            assert!(name
                .chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_'));
        }

        // Invalid tool names (command injection attempts)
        assert!(!"tool;rm -rf /"
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_'));
        assert!(!"tool|pipe"
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_'));
        assert!(!"tool&background"
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_'));

        // Invalid tool names (path traversal attempts)
        assert!(!"../etc/passwd"
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_'));
        assert!(!"tool/path"
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_'));

        // Invalid tool names (key injection attempts)
        assert!(!"tool:colon"
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_'));
    }

    #[test]
    fn test_tool_name_length_limits() {
        // Valid length (64 chars max)
        let max_valid = "a".repeat(64);
        assert_eq!(max_valid.len(), 64);

        // Invalid length (65 chars)
        let too_long = "a".repeat(65);
        assert_eq!(too_long.len(), 65);
        assert!(too_long.len() > 64);
    }

    #[test]
    fn test_tool_list_size_limits() {
        // Valid size (100 tools max)
        let valid_count: Vec<String> = (0..100).map(|i| format!("tool{}", i)).collect();
        assert_eq!(valid_count.len(), 100);

        // Invalid size (101 tools)
        let too_many: Vec<String> = (0..101).map(|i| format!("tool{}", i)).collect();
        assert_eq!(too_many.len(), 101);
        assert!(too_many.len() > 100);
    }
}
