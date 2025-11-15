// Allow module inception - this is a common Rust pattern for protocol clients
#![allow(clippy::module_name_repetitions)]

use crate::error::{AgwError, AgwResult};
use redis::{aio::ConnectionManager, Client, Cmd};
use tracing::{debug, info};

/// RESP client for communicating with AGQ
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

    /// Blocking pop from queue using BRPOP
    ///
    /// Blocks until a job is available in the queue or timeout is reached.
    /// Returns the job data as a JSON string, or None if timeout occurred.
    ///
    /// # Errors
    ///
    /// Returns an error if the RESP protocol command fails
    pub async fn brpop(&mut self, queue: &str, timeout: u64) -> AgwResult<Option<String>> {
        debug!("Blocking pop from queue {} with timeout {}s", queue, timeout);

        // BRPOP returns (key, value) tuple or nil on timeout
        let result: Option<(String, String)> = Cmd::new()
            .arg("BRPOP")
            .arg(queue)
            .arg(timeout)
            .query_async(&mut self.connection)
            .await
            .map_err(|e| AgwError::RespProtocol(format!("BRPOP failed: {e}")))?;

        if let Some((_key, value)) = result {
            debug!("Received job from queue {}: {} bytes", queue, value.len());
            Ok(Some(value))
        } else {
            debug!("BRPOP timeout on queue {}", queue);
            Ok(None)
        }
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
}
