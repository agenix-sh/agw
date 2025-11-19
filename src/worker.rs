use crate::config::Config;
use crate::error::{AgwError, AgwResult};
use crate::executor;
use crate::plan::Plan;
use crate::resp::RespClient;
use tokio::task::JoinHandle;
use tracing::{debug, error, info};
use uuid::Uuid;

/// AGW Worker
pub struct Worker {
    config: Config,
    id: String,
    name: String,
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

        // Generate or use provided worker name
        let worker_name = config.name.clone().unwrap_or_else(|| {
            // Auto-generate name from worker ID (use "worker-" prefix + first 12 chars)
            // This provides uniqueness while being more readable than full UUID
            let short_id = worker_id.chars().take(18).collect::<String>();
            format!("worker-{}", short_id.replace("agw-", ""))
        });

        info!(
            "Initializing worker with ID: {} (name: {})",
            worker_id, worker_name
        );

        // Connect to AGQ
        let mut client = RespClient::connect(&config.agq_address).await?;

        // Authenticate
        client.authenticate(&config.session_key).await?;

        // Register available tools with AGQ
        let tools = config.tools.clone().unwrap_or_else(|| {
            info!("No tools specified, auto-discovery not yet implemented");
            vec![]
        });

        if !tools.is_empty() {
            client.register_tools(&worker_id, &tools).await?;
        }

        Ok(Self {
            config,
            id: worker_id,
            name: worker_name,
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

        // Setup signal handlers for graceful shutdown
        #[cfg(unix)]
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .map_err(|e| AgwError::Worker(format!("Failed to setup SIGTERM handler: {e}")))?;

        #[cfg(unix)]
        let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
            .map_err(|e| AgwError::Worker(format!("Failed to setup SIGINT handler: {e}")))?;

        // Main loop: fetch jobs and send heartbeats
        let mut heartbeat_interval = tokio::time::interval(self.config.heartbeat_duration());

        // Consume the first tick (which completes immediately) and send initial heartbeat
        heartbeat_interval.tick().await;
        self.send_heartbeat().await?;

        // Track currently executing job (if any)
        let mut current_job: Option<JoinHandle<()>> = None;

        // Shutdown flag (Unix only - Windows doesn't have signal handlers yet)
        #[cfg(unix)]
        let mut shutdown_requested = false;

        loop {
            // Check if shutdown was requested and no job is running (Unix only)
            #[cfg(unix)]
            if shutdown_requested && current_job.is_none() {
                info!("Shutdown complete - no jobs running");
                break;
            }

            // Check if current job is complete (non-blocking)
            // If finished, await the handle to detect panics and ensure cleanup
            if let Some(handle) = current_job.as_mut() {
                if handle.is_finished() {
                    debug!("Job execution task completed");
                    // Await the handle to catch any panics and ensure proper cleanup
                    // This prevents silently ignoring panicked tasks during normal operation
                    if let Err(e) = handle.await {
                        error!("Job execution task panicked: {e}");
                    }
                    current_job = None;
                }
            }

            // Use tokio::select with biased mode to prioritize heartbeats
            // This prevents DoS when jobs are continuously available
            #[cfg(unix)]
            {
                tokio::select! {
                    biased;

                    // Signal handlers - highest priority
                    _ = sigterm.recv() => {
                        info!("Received SIGTERM, initiating graceful shutdown");
                        shutdown_requested = true;
                        if current_job.is_some() {
                            info!("Waiting for current job to complete before shutdown");
                        }
                    }

                    _ = sigint.recv() => {
                        info!("Received SIGINT (Ctrl+C), initiating graceful shutdown");
                        shutdown_requested = true;
                        if current_job.is_some() {
                            info!("Waiting for current job to complete before shutdown");
                        }
                    }

                    // Heartbeat tick
                    _ = heartbeat_interval.tick() => {
                        match self.send_heartbeat().await {
                            Ok(()) => {
                                debug!("Heartbeat sent successfully for worker {}", self.id);
                            }
                            Err(e) => {
                                error!("Failed to send heartbeat: {e}");
                                return Err(e);
                            }
                        }
                    }

                    // Job fetch and preparation
                    job_result = self.fetch_and_prepare_job(), if current_job.is_none() && !shutdown_requested => {
                    match job_result {
                        Ok(Some((job_id, plan, job_id_raw))) => {
                            debug!("Prepared job {} (plan {}) with {} tasks",
                                job_id, plan.plan_id, plan.tasks.len());

                            // Clone client for the spawned task
                            let client = self.client.clone();

                            // Spawn plan execution on a separate task to allow heartbeats to continue
                            let plan_handle = tokio::spawn(Self::handle_plan_execution(job_id, plan, job_id_raw, client));

                            current_job = Some(plan_handle);
                        }
                        Ok(None) => {
                            // Timeout - continue loop
                            debug!("Job fetch timeout, continuing...");
                        }
                        Err(e) => {
                            error!("Failed to fetch and prepare job: {e}");
                            return Err(e);
                        }
                    }
                }
                }
            }

            // Non-Unix platforms (Windows) - no signal handling available yet
            #[cfg(not(unix))]
            {
                tokio::select! {
                    biased;

                    // Heartbeat tick
                    _ = heartbeat_interval.tick() => {
                        match self.send_heartbeat().await {
                            Ok(()) => {
                                debug!("Heartbeat sent successfully for worker {}", self.id);
                            }
                            Err(e) => {
                                error!("Failed to send heartbeat: {e}");
                                return Err(e);
                            }
                        }
                    }

                    // Job fetch and preparation (no shutdown handling on Windows yet)
                    job_result = self.fetch_and_prepare_job(), if current_job.is_none() => {
                        match job_result {
                            Ok(Some((job_id, plan, job_id_raw))) => {
                                debug!("Prepared job {} (plan {}) with {} tasks",
                                    job_id, plan.plan_id, plan.tasks.len());

                                let client = self.client.clone();

                                let plan_handle = tokio::spawn(Self::handle_plan_execution(job_id, plan, job_id_raw, client));

                                current_job = Some(plan_handle);
                            }
                            Ok(None) => {
                                debug!("Job fetch timeout, continuing...");
                            }
                            Err(e) => {
                                error!("Failed to fetch and prepare job: {e}");
                                return Err(e);
                            }
                        }
                    }
                }
            }
        }

        // Graceful shutdown: wait for current job to complete if still running
        if let Some(handle) = current_job {
            if let Some(timeout) = self.config.shutdown_timeout_duration() {
                info!(
                    "Waiting up to {:?} for current job to complete before shutdown",
                    timeout
                );
                match tokio::time::timeout(timeout, handle).await {
                    Ok(Ok(())) => {
                        info!("Job completed successfully before shutdown");
                    }
                    Ok(Err(e)) => {
                        error!("Job execution task panicked during shutdown: {e}");
                    }
                    Err(_) => {
                        error!(
                            "Job did not complete within {:?}, forcing shutdown. \
                             Job results may be incomplete.",
                            timeout
                        );
                    }
                }
            } else {
                info!("Waiting for current job to complete before shutdown (no timeout)");
                if let Err(e) = handle.await {
                    error!("Job execution task panicked during shutdown: {e}");
                }
            }
        }

        info!("Worker {} shutting down gracefully", self.id);
        Ok(())
    }

    /// Fetch and prepare a job for execution
    ///
    /// New workflow (AGQ #46):
    /// 1. Pop job_id from queue (BRPOPLPUSH for reliability)
    /// 2. Fetch job metadata (JOB.GET)
    /// 3. Fetch plan template (PLAN.GET)
    /// 4. Substitute input variables in tasks
    ///
    /// Returns (job_id, plan_with_substituted_inputs, job_json) tuple
    ///
    /// # Errors
    ///
    /// Returns an error if fetching fails, JSON is invalid, or validation fails
    async fn fetch_and_prepare_job(&mut self) -> AgwResult<Option<(String, Plan, String)>> {
        use crate::plan::Job;

        const QUEUE_READY: &str = "queue:ready";
        const QUEUE_PROCESSING: &str = "queue:processing";
        const TIMEOUT: u64 = 5; // 5 second timeout to allow heartbeats

        // Step 1: Pop job_id from queue
        match self
            .client
            .brpoplpush(QUEUE_READY, QUEUE_PROCESSING, TIMEOUT)
            .await?
        {
            Some(job_id_raw) => {
                info!("Received job_id from queue (moved to processing)");

                // Step 2: Get job metadata
                let job_json = self.client.job_get(&job_id_raw).await.map_err(|e| {
                    AgwError::Worker(format!(
                        "Failed to fetch job metadata for '{}': {}",
                        job_id_raw, e
                    ))
                })?;

                let job = Job::from_json(&job_json).map_err(|e| {
                    AgwError::Worker(format!(
                        "Failed to parse job JSON for '{}': {}",
                        job_id_raw, e
                    ))
                })?;

                job.validate().map_err(|e| {
                    AgwError::Worker(format!("Job validation failed for '{}': {}", job.job_id, e))
                })?;

                info!("Fetched job {} (plan_id: {})", job.job_id, job.plan_id);

                // Step 3: Get plan template
                let plan_json = self.client.plan_get(&job.plan_id).await.map_err(|e| {
                    AgwError::Worker(format!(
                        "Failed to fetch plan '{}' for job '{}': {}",
                        job.plan_id, job.job_id, e
                    ))
                })?;

                let mut plan = Plan::from_json(&plan_json).map_err(|e| {
                    AgwError::Worker(format!(
                        "Failed to parse plan JSON for '{}': {}",
                        job.plan_id, e
                    ))
                })?;

                plan.validate().map_err(|e| {
                    AgwError::Worker(format!(
                        "Plan validation failed for '{}': {}",
                        plan.plan_id, e
                    ))
                })?;

                info!(
                    "Fetched plan {} with {} tasks",
                    plan.plan_id,
                    plan.tasks.len()
                );

                // Step 4: Substitute input variables in tasks
                let mut substituted_tasks = Vec::new();
                for task in &plan.tasks {
                    let substituted_task = task.substitute_input(&job.input).map_err(|e| {
                        AgwError::Worker(format!(
                            "Failed to substitute input variables for task {} in job '{}': {}",
                            task.task_number, job.job_id, e
                        ))
                    })?;
                    substituted_tasks.push(substituted_task);
                }

                plan.tasks = substituted_tasks;

                Ok(Some((job.job_id, plan, job_id_raw)))
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

    /// Get the worker name
    #[must_use]
    #[allow(dead_code)]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Handle plan execution (extracted to avoid duplication between Unix/non-Unix code paths)
    ///
    /// This function executes the plan and handles cleanup of the processing queue.
    /// The `job_id_raw` parameter is the raw job_id string used for cleanup via LREM.
    async fn handle_plan_execution(
        job_id: String,
        plan: Plan,
        job_id_raw: String,
        mut client: RespClient,
    ) {
        const QUEUE_PROCESSING: &str = "queue:processing";

        match executor::execute_plan(&job_id, &plan).await {
            Ok(result) => {
                info!(
                    "Plan {} (job {}) completed: {} tasks executed, success={}",
                    result.plan_id,
                    result.job_id,
                    result.task_results.len(),
                    result.success
                );

                // Post result to AGQ (includes partial results if plan failed mid-execution)
                // Note: result.success == false means some tasks failed, but we still have
                // partial output from tasks that completed before the failure
                let status = if result.success {
                    "completed"
                } else {
                    "failed"
                };
                if let Err(e) = client
                    .post_job_result(
                        &result.job_id,
                        &result.combined_stdout(),
                        &result.combined_stderr(),
                        status,
                    )
                    .await
                {
                    error!("Failed to post results for job {}: {e}", result.job_id);
                    // Don't remove from processing queue if we couldn't post results
                    return;
                }

                // Remove job from processing queue after successful result posting
                info!("Job completed successfully, removing from processing queue");
                if let Err(e) = client.lrem(QUEUE_PROCESSING, 1, &job_id_raw).await {
                    error!(
                        "Failed to remove job {} from processing queue: {e}",
                        result.job_id
                    );
                    // Job stays in queue:processing for monitoring/retry
                }
            }
            Err(e) => {
                error!("Failed to execute plan {}: {e}", plan.plan_id);

                // Post error to AGQ with empty results
                // Note: Execution errors occur before any tasks run, so no partial results exist
                let error_msg = format!("Execution error: {e}");
                if let Err(post_err) = client
                    .post_job_result(&job_id, "", &error_msg, "failed")
                    .await
                {
                    error!("Failed to post error for job {}: {post_err}", job_id);
                    // Don't remove from processing queue if we couldn't post results
                    return;
                }

                // Remove job from processing queue even on execution failure
                // (we successfully posted the failure results, so job is complete)
                info!("Job failed but results posted, removing from processing queue");
                if let Err(e) = client.lrem(QUEUE_PROCESSING, 1, &job_id_raw).await {
                    error!("Failed to remove job {} from processing queue: {e}", job_id);
                    // Job stays in queue:processing for monitoring
                }
            }
        }
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
    fn test_worker_name_generation() {
        // Test that auto-generated worker names follow the pattern
        let worker_id = format!("agw-{}", Uuid::new_v4());
        let short_id = worker_id.chars().take(18).collect::<String>();
        let name = format!("worker-{}", short_id.replace("agw-", ""));

        assert!(name.starts_with("worker-"));
        assert!(name.len() > 7); // "worker-" + at least some ID chars

        // Verify it validates correctly
        use crate::config::validate_worker_name;
        assert!(validate_worker_name(&name).is_ok());
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
