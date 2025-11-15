use clap::Parser;
use std::time::Duration;

/// AGW - Agentic Worker for the AGX ecosystem
#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
pub struct Config {
    /// AGQ server address (host:port)
    #[arg(
        short = 'a',
        long,
        env = "AGQ_ADDRESS",
        default_value = "127.0.0.1:6379"
    )]
    pub agq_address: String,

    /// Session key for authentication
    #[arg(short = 'k', long, env = "AGQ_SESSION_KEY")]
    pub session_key: String,

    /// Worker ID (generated if not provided)
    #[arg(short = 'w', long, env = "WORKER_ID")]
    pub worker_id: Option<String>,

    /// Heartbeat interval in seconds
    #[arg(long, env = "HEARTBEAT_INTERVAL", default_value = "30")]
    pub heartbeat_interval: u64,

    /// Connection timeout in seconds
    #[arg(long, env = "CONNECTION_TIMEOUT", default_value = "10")]
    pub connection_timeout: u64,
}

impl Config {
    /// Validate configuration
    pub fn validate(&self) -> anyhow::Result<()> {
        // Validate AGQ address format
        if !self.agq_address.contains(':') {
            anyhow::bail!("AGQ address must be in format host:port");
        }

        // Validate session key
        validate_session_key(&self.session_key)?;

        // Validate worker ID if provided
        if let Some(ref id) = self.worker_id {
            validate_worker_id(id)?;
        }

        // Validate intervals
        if self.heartbeat_interval == 0 {
            anyhow::bail!("Heartbeat interval must be greater than 0");
        }

        if self.connection_timeout == 0 {
            anyhow::bail!("Connection timeout must be greater than 0");
        }

        Ok(())
    }

    /// Get heartbeat interval as Duration
    pub fn heartbeat_duration(&self) -> Duration {
        Duration::from_secs(self.heartbeat_interval)
    }

    /// Get connection timeout as Duration
    #[allow(dead_code)]
    pub fn connection_timeout_duration(&self) -> Duration {
        Duration::from_secs(self.connection_timeout)
    }
}

/// Validate session key format
pub fn validate_session_key(key: &str) -> anyhow::Result<()> {
    if key.is_empty() {
        anyhow::bail!("Session key cannot be empty");
    }

    if key.len() < 8 {
        anyhow::bail!("Session key must be at least 8 characters");
    }

    // Check for control characters (null bytes, etc.)
    if key.chars().any(|c| c.is_control()) {
        anyhow::bail!("Session key contains invalid characters");
    }

    // Check for path traversal attempts
    if key.contains("..") || key.contains('/') || key.contains('\\') {
        anyhow::bail!("Session key contains invalid characters");
    }

    // Check for command injection attempts
    if key.contains(';')
        || key.contains('|')
        || key.contains('&')
        || key.contains('$')
        || key.contains('`')
    {
        anyhow::bail!("Session key contains invalid characters");
    }

    Ok(())
}

/// Validate worker ID format
pub fn validate_worker_id(id: &str) -> anyhow::Result<()> {
    if id.is_empty() {
        anyhow::bail!("Worker ID cannot be empty");
    }

    if id.len() > 64 {
        anyhow::bail!("Worker ID cannot exceed 64 characters");
    }

    // Check for control characters
    if id.chars().any(|c| c.is_control()) {
        anyhow::bail!("Worker ID contains invalid characters");
    }

    // Only allow alphanumeric, hyphens, and underscores
    if !id
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        anyhow::bail!(
            "Worker ID can only contain alphanumeric characters, hyphens, and underscores"
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_session_key_valid() {
        assert!(validate_session_key("valid-session-key-12345").is_ok());
        assert!(validate_session_key("abcdefgh").is_ok());
        assert!(validate_session_key("test_key_123").is_ok());
    }

    #[test]
    fn test_validate_session_key_empty() {
        assert!(validate_session_key("").is_err());
    }

    #[test]
    fn test_validate_session_key_too_short() {
        assert!(validate_session_key("short").is_err());
    }

    #[test]
    fn test_validate_session_key_path_traversal() {
        assert!(validate_session_key("../etc/passwd").is_err());
        assert!(validate_session_key("key/../other").is_err());
        assert!(validate_session_key("/etc/passwd").is_err());
        assert!(validate_session_key("C:\\Windows\\System32").is_err());
    }

    #[test]
    fn test_validate_session_key_command_injection() {
        assert!(validate_session_key("key;rm -rf /").is_err());
        assert!(validate_session_key("key|cat /etc/passwd").is_err());
        assert!(validate_session_key("key&& echo bad").is_err());
        assert!(validate_session_key("key$(whoami)").is_err());
        assert!(validate_session_key("key`whoami`").is_err());
    }

    #[test]
    fn test_validate_worker_id_valid() {
        assert!(validate_worker_id("worker-1").is_ok());
        assert!(validate_worker_id("worker_abc_123").is_ok());
        assert!(validate_worker_id("WORKER123").is_ok());
    }

    #[test]
    fn test_validate_worker_id_empty() {
        assert!(validate_worker_id("").is_err());
    }

    #[test]
    fn test_validate_worker_id_too_long() {
        let long_id = "a".repeat(65);
        assert!(validate_worker_id(&long_id).is_err());
    }

    #[test]
    fn test_validate_worker_id_invalid_chars() {
        assert!(validate_worker_id("worker.1").is_err());
        assert!(validate_worker_id("worker/1").is_err());
        assert!(validate_worker_id("worker;1").is_err());
        assert!(validate_worker_id("worker@1").is_err());
    }
}
