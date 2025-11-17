use crate::config::Config;
use crate::error::{AgwError, AgwResult};
use crate::executor;
use crate::plan::Plan;
use crate::resp::RespClient;
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// AGW Worker
pub struct Worker {
    config: Config,
    id: String,
    client: RespClient,
}

impl Worker {
    /// Create a new worker instance
    ///
    /// # Errors
    ///
    /// Returns an error if configuration validation fails, connection to AGQ fails,
    /// or authentication fails
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
            id: worker_id,
            client,
        })
    }

    /// Run the worker main loop
    ///
    /// # Errors
    ///
    /// Returns an error if heartbeat fails, job fetch fails, or connection to AGQ is lost
    pub async fn run(mut self) -> AgwResult<()> {
        info!("Worker {} starting main loop", self.id);

        // Main loop: fetch jobs and send heartbeats
        let mut heartbeat_interval = tokio::time::interval(self.config.heartbeat_duration());

        // Consume the first tick (which completes immediately) and send initial heartbeat
        heartbeat_interval.tick().await;
        self.send_heartbeat().await?;

        // Track currently executing job (if any)
        let mut current_job: Option<JoinHandle<()>> = None;

        loop {
            // Check if current job is complete (non-blocking)
            if let Some(handle) = current_job.as_mut() {
                if handle.is_finished() {
                    debug!("Job execution task completed");
                    current_job = None;
                }
            }

            // Use tokio::select with biased mode to prioritize heartbeats
            // This prevents DoS when jobs are continuously available
            tokio::select! {
                biased;

                // Heartbeat tick - checked first to ensure heartbeats are never missed
                _ = heartbeat_interval.tick() => {
                    match self.send_heartbeat().await {
                        Ok(()) => {
                            debug!("Heartbeat sent successfully for worker {}", self.id);
                        }
                        Err(e) => {
                            error!("Failed to send heartbeat: {e}");
                            warn!("Worker {} may need to reconnect", self.id);
                            return Err(e);
                        }
                    }
                }

                // Plan fetch (with 5 second timeout to allow heartbeats)
                // Only fetch if not currently executing a plan
                plan_result = self.fetch_plan(), if current_job.is_none() => {
                    match plan_result {
                        Ok(Some(plan)) => {
                            debug!("Received plan {} (job {}) with {} tasks",
                                plan.plan_id, plan.job_id, plan.tasks.len());

                            // Clone client for the spawned task
                            let mut client = self.client.clone();

                            // Spawn plan execution on a separate task to allow heartbeats to continue
                            let plan_handle = tokio::spawn(async move {
                                match executor::execute_plan(&plan).await {
                                    Ok(result) => {
                                        info!(
                                            "Plan {} (job {}) completed: {} tasks executed, success={}",
                                            result.plan_id,
                                            result.job_id,
                                            result.task_results.len(),
                                            result.success
                                        );

                                        // Post result to AGQ
                                        let status = if result.success { "completed" } else { "failed" };
                                        if let Err(e) = client.post_job_result(
                                            &result.job_id,
                                            &result.combined_stdout(),
                                            &result.combined_stderr(),
                                            status
                                        ).await {
                                            error!("Failed to post results for job {}: {e}", result.job_id);
                                        }
                                    }
                                    Err(e) => {
                                        error!("Failed to execute plan {}: {e}", plan.plan_id);

                                        // Post error to AGQ
                                        let error_msg = format!("Execution error: {e}");
                                        if let Err(post_err) = client.post_job_result(
                                            &plan.job_id,
                                            "",
                                            &error_msg,
                                            "failed"
                                        ).await {
                                            error!("Failed to post error for job {}: {post_err}", plan.job_id);
                                        }
                                    }
                                }
                            });

                            current_job = Some(plan_handle);
                        }
                        Ok(None) => {
                            // Timeout - continue loop
                            debug!("Plan fetch timeout, continuing...");
                        }
                        Err(e) => {
                            error!("Failed to fetch plan: {e}");
                            return Err(e);
                        }
                    }
                }
            }
        }
    }

    /// Fetch a plan from the queue
    ///
    /// # Errors
    ///
    /// Returns an error if BRPOP fails, plan JSON is invalid, or validation fails
    async fn fetch_plan(&mut self) -> AgwResult<Option<Plan>> {
        const QUEUE_NAME: &str = "queue:ready";
        const BRPOP_TIMEOUT: u64 = 5; // 5 second timeout to allow heartbeats

        match self.client.brpop(QUEUE_NAME, BRPOP_TIMEOUT).await? {
            Some(json) => {
                // Parse JSON - sanitize error to avoid information disclosure
                let plan = Plan::from_json(&json)
                    .map_err(|_| AgwError::Worker("Invalid plan JSON format".to_string()))?;

                // Validate plan structure and all tasks for security
                plan.validate()?;

                Ok(Some(plan))
            }
            None => Ok(None),
        }
    }

    /// Send a heartbeat message to AGQ
    async fn send_heartbeat(&mut self) -> AgwResult<()> {
        self.client.heartbeat(&self.id).await
    }

    /// Get the worker ID
    #[must_use]
    #[allow(dead_code)]
    pub fn id(&self) -> &str {
        &self.id
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
