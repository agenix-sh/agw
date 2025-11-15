use crate::config::Config;
use crate::error::{AgwError, AgwResult};
use crate::resp::RespClient;
use tracing::{error, info, warn};
use uuid::Uuid;

/// AGW Worker
pub struct Worker {
    config: Config,
    worker_id: String,
    client: RespClient,
}

impl Worker {
    /// Create a new worker instance
    pub async fn new(config: Config) -> AgwResult<Self> {
        // Validate configuration
        config
            .validate()
            .map_err(|e| AgwError::InvalidConfig(e.to_string()))?;

        // Generate or use provided worker ID
        let worker_id = config
            .worker_id
            .clone()
            .unwrap_or_else(|| format!("agw-{}", Uuid::new_v4()));

        info!("Initializing worker with ID: {}", worker_id);

        // Connect to AGQ
        let mut client = RespClient::connect(&config.agq_address).await?;

        // Authenticate
        client.authenticate(&config.session_key).await?;

        Ok(Self {
            config,
            worker_id,
            client,
        })
    }

    /// Run the worker main loop
    pub async fn run(mut self) -> AgwResult<()> {
        info!("Worker {} starting main loop", self.worker_id);

        // Send initial heartbeat
        self.send_heartbeat().await?;

        // Main heartbeat loop
        let mut interval = tokio::time::interval(self.config.heartbeat_duration());

        loop {
            interval.tick().await;

            match self.send_heartbeat().await {
                Ok(_) => {
                    info!("Heartbeat sent successfully for worker {}", self.worker_id);
                }
                Err(e) => {
                    error!("Failed to send heartbeat: {}", e);
                    warn!("Worker {} may need to reconnect", self.worker_id);
                    // In a production version, we'd implement reconnection logic here
                    return Err(e);
                }
            }
        }
    }

    /// Send a heartbeat message to AGQ
    async fn send_heartbeat(&mut self) -> AgwResult<()> {
        self.client.heartbeat(&self.worker_id).await
    }

    /// Get the worker ID
    #[allow(dead_code)]
    pub fn id(&self) -> &str {
        &self.worker_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_worker_id_generation() {
        // Test that generated worker IDs follow the pattern
        let id = format!("agw-{}", Uuid::new_v4());
        assert!(id.starts_with("agw-"));
        assert!(id.len() > 4);
    }

    #[test]
    fn test_worker_id_validation() {
        use crate::config::validate_worker_id;

        // Valid generated IDs
        let id = format!("agw-{}", Uuid::new_v4());
        assert!(validate_worker_id(&id).is_ok());

        // Valid custom IDs
        assert!(validate_worker_id("worker-1").is_ok());
        assert!(validate_worker_id("test_worker").is_ok());
    }
}
